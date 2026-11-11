//! A block session of the engine, the counterpart of Satin's `MegaBlockExecutor`
//! (`crates/mega-evm/src/block/executor.rs`, `limit.rs` @ 5e3f30ad). A session is made only by
//! [`SatinFactory`](crate::factory::SatinFactory), which installs the fixed protocol parameters and
//! derives the block's activation facts from the chain configuration; it holds its EVM privately,
//! so nothing can change a protocol value under it.
//!
//! - **pre-block**, before the session is returned (`pre_block`): the EIP-2935 and EIP-4788 system
//!   calls (block zero makes neither, and its parent beacon block root must be zero), the EIP-7997
//!   factory's check in the block in which Satin activates, the five flat system contracts'
//!   deployment, the `SequencerRegistry`'s due-change read and its `applyPendingChanges()` call
//!   when due, the registry's deployment, the read of the L1 block info slots, and the resolution
//!   of the block's live system address, each committed on its own in that order;
//! - **per candidate, before it runs** ([`BlockSession::execute_candidate`]): the activation
//!   block's deposit-only rule, the Mega System Transaction decision against the block's live
//!   system address (+ the system shape), `BlockLimiter::pre_execution_check` (the block's gas
//!   limit, the builder's size limits, the three packing budgets: data size, KV updates, state
//!   growth; a deposit is held to the gas limit alone, a Mega System Transaction to all of them as
//!   any other non-deposit transaction), the DA footprint against the block's gas limit, the
//!   sender's account read; then `Evm::transact` and `ExecutedTx::detach` (nothing committed);
//! - **commit** ([`BlockSession::commit_candidate`]): the candidate's block, the room check and the
//!   sender check again, the block's counters, the receipt (status, cumulative gas, logs; a
//!   deposit's nonce and receipt version), the state into the EVM's overlay and the block's change
//!   set (`BlockStateAccumulator`, the counterpart of revm's bundle).
//!
//! SALT (`SATIN-WIT-013`): every attempt begins with an empty multiplier cache, and its valid
//! lookups join the block's union ([`BlockSession::bucket_reads`]) whatever becomes of it — a
//! candidate dropped, one whose later lookup failed — and travel with its candidate
//! ([`Candidate::bucket_reads`]). Committing a candidate adds nothing to the union: a session that
//! executed candidates for another one hands its union over with
//! [`BlockSession::drain_bucket_reads`], which that session takes with
//! [`BlockSession::merge_bucket_reads`]. The pre-block calls price at `m = 1` and read no bucket.
//!
//! The limits are Satin's `ProtocolLimits::for_spec(SATIN)`; the builder's policy is a
//! validator's (`BlockLimits::default()`: no limit). Failures are typed ([`crate::error`]): a
//! refusal names its rule; a failure that decides the block (an internal error or an invalid
//! block) is kept, and the session refuses every later candidate and cannot finish.

mod pre_block;

use crate::{
    MegaTypes, TxLimits,
    chain::{Activation, RegistryBootstrap},
    error::{
        BlockError, BlockIdentity, Budget, CandidateError, ErrorContext, Phase, Refusal, TxContext,
    },
    salt::{BucketId, ForeignSaltExport, SaltExport, SnapshotId},
    tx::ORACLE,
};
use alloy_consensus::{Transaction, transaction::Recovered};
use alloy_eips::Typed2718;
use alloy_primitives::{Address, B256, Bytes, Log, TxKind, U256};
use evm2::{
    Evm, TxResult,
    evm::{BlockStateAccumulator, PendingState, StateChangeSource, SystemTx, TxResultWithState},
};
use op_evm2::{
    L1BlockInfo, OpTxEnvelope, constants::L1_BLOCK_CONTRACT, estimate_tx_compressed_size,
};
use std::{boxed::Box, collections::BTreeSet, vec::Vec};

/// Satin's `ProtocolLimits::for_spec(SATIN)` block budgets: data size, KV updates, state growth.
pub const BLOCK_BUDGETS: [u64; 3] = [13_107_200, 500_000, 1_000];
const COMPRESSED_SIZE_SCALE: u64 = 1_000_000;
pub const HISTORY_STORAGE: Address =
    alloy_primitives::address!("0x0000F90827F1C53a10cb7A02335B175320002935");
pub const BEACON_ROOTS: Address =
    alloy_primitives::address!("0x000F3df6D732807Ef1319fB7B8bB8522d0Beac02");

/// Satin's `estimated_da_size`: the FastLZ-based compressed size, in bytes.
pub fn estimated_da_size(encoded: &[u8]) -> u64 {
    estimate_tx_compressed_size(encoded) / COMPRESSED_SIZE_SCALE
}

/// What the block has used (Satin `BlockLimiter`, with a validator's policy: no builder limit).
#[derive(Clone, Debug, Default)]
pub struct BlockLimiter {
    pub block_gas_limit: u64,
    pub block_gas_used: u64,
    /// Data size, write records, state growth the committed transactions kept.
    pub usage: [u64; 3],
    pub block_tx_size_used: u64,
    pub block_da_size_used: u64,
    pub block_da_footprint_used: u64,
    /// Execution (regular, at least the floor), state, history gas and history bytes.
    pub gas: [u64; 4],
}

impl BlockLimiter {
    /// Satin `pre_execution_check` under `BlockLimits::default()`: the builder's per-transaction
    /// and size limits are `u64::MAX`, so the gas limit and the three packing budgets decide.
    pub fn pre_execution_check(&self, gas_limit: u64, packing_exempt: bool) -> Result<(), Refusal> {
        if self.block_gas_used.saturating_add(gas_limit) > self.block_gas_limit {
            return Err(Refusal::BlockGasLimit {
                gas_limit,
                available: self.block_gas_limit.saturating_sub(self.block_gas_used),
            });
        }
        if packing_exempt {
            return Ok(());
        }
        for (i, budget) in Budget::ALL.into_iter().enumerate() {
            if self.usage[i] >= BLOCK_BUDGETS[i] {
                return Err(Refusal::Packing {
                    budget,
                    used: self.usage[i],
                    limit: BLOCK_BUDGETS[i],
                });
            }
        }
        Ok(())
    }

    pub fn check_da_footprint(&self, footprint: u64) -> Result<(), Refusal> {
        let available = self.block_gas_limit.saturating_sub(self.block_da_footprint_used);
        if footprint > available {
            return Err(Refusal::DaFootprint { footprint, available });
        }
        Ok(())
    }
}

/// A receipt as the block keeps it (OP: a deposit's carries its nonce and receipt version).
#[derive(Clone, Debug)]
pub struct Receipt {
    pub ty: u8,
    pub status: bool,
    pub cumulative_gas_used: u64,
    pub logs: Vec<Log>,
    /// The logs bloom, computed when the receipt is built (as alloy-op-evm's receipt builder
    /// builds `ReceiptWithBloom` in Satin's commit; a header needs it either way).
    pub bloom: alloy_primitives::Bloom,
    pub deposit_nonce: Option<u64>,
}

/// An executed, not yet committed transaction (Satin `MegaBlockTxResult`), bound to the block it
/// ran for and to how it ran (under consensus rules or a simulation's). Read-only: a commit takes
/// the candidate as its session made it.
#[derive(Debug)]
pub struct Candidate {
    ty: u8,
    gas_limit: u64,
    tx_size: u64,
    da_size: u64,
    da_footprint: u64,
    is_deposit: bool,
    is_system_transaction: bool,
    sender: Address,
    sender_nonce: u64,
    sender_balance: U256,
    result: TxResult<MegaTypes>,
    pending: PendingState,
    usage: [u64; 3],
    gas: [u64; 4],
    gas_used: u64,
    bucket_reads: Vec<BucketId>,
    block: BlockIdentity,
    simulated: bool,
}

impl Candidate {
    /// The transaction's EIP-2718 type.
    pub const fn ty(&self) -> u8 {
        self.ty
    }
    pub const fn gas_limit(&self) -> u64 {
        self.gas_limit
    }
    /// The transaction's encoded size and its estimated data-availability size, in bytes.
    pub const fn tx_size(&self) -> u64 {
        self.tx_size
    }
    pub const fn da_size(&self) -> u64 {
        self.da_size
    }
    /// The data-availability footprint it counts against the block's gas limit.
    pub const fn da_footprint(&self) -> u64 {
        self.da_footprint
    }
    pub const fn is_deposit(&self) -> bool {
        self.is_deposit
    }
    pub const fn is_system_transaction(&self) -> bool {
        self.is_system_transaction
    }
    pub const fn sender(&self) -> Address {
        self.sender
    }
    /// The sender's nonce and balance before it ran, which the commit checks again.
    pub const fn sender_nonce(&self) -> u64 {
        self.sender_nonce
    }
    pub const fn sender_balance(&self) -> U256 {
        self.sender_balance
    }
    pub const fn result(&self) -> &TxResult<MegaTypes> {
        &self.result
    }
    /// The state it leaves, its read-only entries included.
    pub const fn pending(&self) -> &PendingState {
        &self.pending
    }
    /// The engine's usage (data size, KV, growth), read when the transaction ended.
    pub const fn usage(&self) -> [u64; 3] {
        self.usage
    }
    /// The engine's ledgers (execution, state, history gas, history bytes).
    pub const fn gas(&self) -> [u64; 4] {
        self.gas
    }
    /// Receipt gas (`max(spent − refund, floor)`).
    pub const fn gas_used(&self) -> u64 {
        self.gas_used
    }
    /// The SALT buckets its execution read a valid capacity of, ascending: its attempt's export.
    pub fn bucket_reads(&self) -> &[BucketId] {
        &self.bucket_reads
    }
    /// The block the candidate ran for.
    pub const fn block(&self) -> BlockIdentity {
        self.block
    }
    /// Whether it ran under a simulation's switches.
    pub const fn is_simulated(&self) -> bool {
        self.simulated
    }
}

/// What a finished block produced.
#[derive(Debug)]
pub struct BlockOutput {
    pub receipts: Vec<Receipt>,
    /// The block's change set, pre-block states included.
    pub changes: BlockStateAccumulator,
    /// Whole pre-block states in commit order, including read-only accounts and slots. The change
    /// set is the block's state transition: a registry seeded over an account without code clears
    /// that account's earlier storage there, which its pre-block state does not mark.
    pub pre_block_states: Vec<PendingState>,
    /// The live system address the block's transactions were classified against: always resolved
    /// under consensus rules (`None` only in a comparison run under the reference's
    /// per-transaction classification, where the registry named none).
    pub system_address: Option<Address>,
    pub limiter: BlockLimiter,
    /// Valid SALT lookups of every attempt of the session, discarded candidates included, and of
    /// every export merged into it.
    pub bucket_reads: BTreeSet<BucketId>,
    /// The SALT snapshot the block read its capacities from.
    pub salt_snapshot: SnapshotId,
}

/// One block's execution over one `Evm` for the whole block.
pub struct BlockSession<'a> {
    evm: Evm<'a, MegaTypes>,
    limiter: BlockLimiter,
    receipts: Vec<Receipt>,
    /// The block's change set (revm's bundle counterpart).
    changes: BlockStateAccumulator,
    /// Whole pre-block states in commit order, including read-only accounts and slots.
    pre_block_states: Vec<PendingState>,
    /// Valid SALT lookups of every attempt of the session, discarded candidates included, and of
    /// every export merged into it; emptied by a drain.
    bucket_reads: BTreeSet<BucketId>,
    /// The SALT snapshot the session reads, of the block's parent state.
    salt_snapshot: SnapshotId,
    identity: BlockIdentity,
    parent_beacon_block_root: Option<B256>,
    activation: Activation,
    /// The configuration's registry bootstrap values, which a new registry is seeded with.
    bootstrap: RegistryBootstrap,
    /// The live system address, resolved by the pre-block sequence.
    system_address: Option<Address>,
    /// The limits every candidate runs under: Satin's, or a simulation's.
    limits: TxLimits,
    simulation: bool,
    /// The failure that decided the block, once one did.
    fault: Option<BlockError>,
}

impl<'a> BlockSession<'a> {
    /// A session over `evm`, which the factory configured, its pre-block sequence run.
    pub(crate) fn start(
        evm: Evm<'a, MegaTypes>,
        identity: BlockIdentity,
        salt_snapshot: SnapshotId,
        parent_beacon_block_root: Option<B256>,
        activation: Activation,
        bootstrap: RegistryBootstrap,
        limits: TxLimits,
        simulation: bool,
    ) -> Result<Self, BlockError> {
        let block_gas_limit = evm.block().gas_limit.saturating_to();
        let mut session = Self {
            evm,
            limiter: BlockLimiter { block_gas_limit, ..Default::default() },
            receipts: Vec::new(),
            changes: BlockStateAccumulator::new(),
            pre_block_states: Vec::new(),
            bucket_reads: BTreeSet::new(),
            salt_snapshot,
            identity,
            parent_beacon_block_root,
            activation,
            bootstrap,
            system_address: None,
            limits,
            simulation,
            fault: None,
        };
        session.run_pre_block_sequence()?;
        Ok(session)
    }

    /// The session's EVM, read-only.
    pub const fn evm(&self) -> &Evm<'a, MegaTypes> {
        &self.evm
    }

    pub const fn limiter(&self) -> &BlockLimiter {
        &self.limiter
    }

    pub fn receipts(&self) -> &[Receipt] {
        &self.receipts
    }

    pub const fn changes(&self) -> &BlockStateAccumulator {
        &self.changes
    }

    pub fn pre_block_states(&self) -> &[PendingState] {
        &self.pre_block_states
    }

    /// Valid SALT lookups of every attempt of the session — discarded candidates, a candidate
    /// whose later lookup failed — and of every export merged into it, since the session started
    /// or was last drained (`SATIN-WIT-013`). Committing a candidate adds nothing to it.
    pub const fn bucket_reads(&self) -> &BTreeSet<BucketId> {
        &self.bucket_reads
    }

    /// The live system address the pre-block sequence resolved, which every transaction of the
    /// block is classified against (`SATIN-REG-002`): always resolved under consensus rules, the
    /// block invalid otherwise (`None` only in a comparison run under the reference's
    /// per-transaction classification, where the registry named none).
    pub const fn system_address(&self) -> Option<Address> {
        self.system_address
    }

    /// The valid SALT lookups of the session's last attempt, ascending: after a candidate the
    /// candidate's own, after a failed one the lookups it made before it failed.
    pub fn last_attempt_bucket_reads(&self) -> Vec<BucketId> {
        self.evm.ext().salt.lookups()
    }

    /// The SALT snapshot the session reads.
    pub const fn salt_snapshot(&self) -> SnapshotId {
        self.salt_snapshot
    }

    /// Hands the session's lookups over to the session that builds the block (a worker's to its
    /// coordinator's, or a session that empties its set between transactions), and empties them.
    /// Available after a failure too: the lookups made before it are still the node's.
    pub fn drain_bucket_reads(&mut self) -> SaltExport {
        SaltExport {
            block: self.identity,
            snapshot: self.salt_snapshot,
            simulated: self.simulation,
            buckets: core::mem::take(&mut self.bucket_reads),
        }
    }

    /// Takes the lookups another session of this block made into this one's
    /// ([`Self::drain_bucket_reads`]). An export of another block, another snapshot or other
    /// rules (a simulation's into a consensus session, or the reverse) is refused and handed back
    /// (boxed: a refusal is rare, and the result stays small).
    pub fn merge_bucket_reads(&mut self, export: SaltExport) -> Result<(), Box<ForeignSaltExport>> {
        if export.block != self.identity ||
            export.snapshot != self.salt_snapshot ||
            export.simulated != self.simulation
        {
            return Err(Box::new(ForeignSaltExport {
                block: self.identity,
                snapshot: self.salt_snapshot,
                simulated: self.simulation,
                export,
            }));
        }
        self.bucket_reads.extend(export.buckets);
        Ok(())
    }

    /// What the schedule decided for the block.
    pub const fn activation(&self) -> &Activation {
        &self.activation
    }

    pub const fn identity(&self) -> BlockIdentity {
        self.identity
    }

    /// Whether the session runs under a simulation's switches rather than consensus rules.
    pub const fn is_simulation(&self) -> bool {
        self.simulation
    }

    /// The failure that decided the block, once one did.
    pub const fn fault(&self) -> Option<&BlockError> {
        self.fault.as_ref()
    }

    /// The block's output; an error when a failure decided the block.
    pub fn finish(self) -> Result<BlockOutput, BlockError> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        Ok(BlockOutput {
            receipts: self.receipts,
            changes: self.changes,
            pre_block_states: self.pre_block_states,
            system_address: self.system_address,
            limiter: self.limiter,
            bucket_reads: self.bucket_reads,
            salt_snapshot: self.salt_snapshot,
        })
    }

    fn system_call(&mut self, target: Address, data: Bytes) -> Result<PendingState, BlockError> {
        self.evm.ext_mut().begin_system_call();
        let executed = self
            .evm
            .system_call(SystemTx::new(target, data))
            .map_err(|e| BlockError::system_call(target, e))?;
        let TxResultWithState { result: r, pending_state, .. } = executed.detach();
        if !r.status {
            return Err(BlockError::system_call_failed(target, r.stop));
        }
        Ok(pending_state)
    }

    fn commit_pre_block_state(&mut self, state: PendingState) {
        self.evm.commit_source(&state);
        let _ = state.visit(&mut self.changes);
        self.pre_block_states.push(state);
    }

    /// Whether the registry has a role change due in this block (`SATIN-PBLK-004`), read off the
    /// block's state as it stands: each role's pending value, and its activation block when one is
    /// pending. A read the database cannot serve is an internal error.
    pub fn registry_due(&mut self) -> Result<bool, BlockError> {
        match self.read_registry_due() {
            Ok((due, _)) => Ok(due),
            Err(e) => {
                // A read that fails decides the block, as during the pre-block sequence.
                self.fault.get_or_insert_with(|| e.clone());
                Err(e)
            }
        }
    }

    /// Whether a transaction escapes the three packing budgets: a deposit does (`SATIN-BLK-006`); a
    /// Mega System Transaction is held to them as any other non-deposit transaction is
    /// (`SATIN-BLK-007`), except under the reference's rule (comparison only).
    fn packing_exempt(&self, is_deposit: bool, is_system_transaction: bool) -> bool {
        is_deposit ||
            is_system_transaction &&
                crate::under_reference_rule(
                    self.evm.ext(),
                    crate::ReferenceRule::MstPackingExempt,
                )
    }

    /// Whether `tx` is a Mega System Transaction: sent from the block's live system address, which
    /// the pre-block sequence resolved, with the system shape (a transaction of another shape from
    /// that address is refused when the engine validates it). Nothing is read.
    fn is_system_transaction(&self, tx: &Recovered<OpTxEnvelope>) -> bool {
        let shape = tx.inner().ty() == 0 && tx.inner().kind() == TxKind::Call(ORACLE);
        shape && self.system_address == Some(tx.signer())
    }

    /// The kept failure, if one decided the block.
    fn check_fault(&self) -> Result<(), CandidateError> {
        match &self.fault {
            Some(fault) => Err(CandidateError::Block(fault.clone())),
            None => Ok(()),
        }
    }

    /// Keeps a failure that decides the block (the first one) and hands the error back.
    fn decide(&mut self, error: CandidateError) -> CandidateError {
        if let CandidateError::Block(e) = &error &&
            self.fault.is_none()
        {
            self.fault = Some(e.clone());
        }
        error
    }

    /// Runs `tx` (its 2718 bytes `encoded`) against the block's rules; nothing is committed.
    pub fn execute_candidate(
        &mut self,
        tx: &Recovered<OpTxEnvelope>,
        encoded: &[u8],
    ) -> Result<Candidate, CandidateError> {
        self.check_fault()?;
        self.run(tx, encoded).map_err(|e| self.decide(e))
    }

    fn run(
        &mut self,
        tx: &Recovered<OpTxEnvelope>,
        encoded: &[u8],
    ) -> Result<Candidate, CandidateError> {
        // An attempt reads its buckets afresh (`SATIN-STATE-006`), and exports only its own
        // lookups, also when it is refused before it runs.
        self.evm.ext_mut().salt.begin_attempt();
        let env = tx.inner();
        let ty = env.ty();
        let is_deposit = env.is_deposit();
        let sender = tx.signer();
        let context = TxContext { index: self.receipts.len(), sender, ty };
        if self.activation.deposits_only() && !is_deposit {
            return Err(Refusal::OnlyDeposits.into());
        }
        // Satin's limits (a simulation's when the session is one), whatever ran before.
        self.evm.ext_mut().limits = self.limits;
        let tx_size = encoded.len() as u64;
        let da_size = estimated_da_size(encoded);
        let gas_limit = env.gas_limit();
        let is_system_transaction = !is_deposit && self.is_system_transaction(tx);
        self.limiter.pre_execution_check(
            gas_limit,
            self.packing_exempt(is_deposit, is_system_transaction),
        )?;
        let da_footprint = if is_deposit {
            0
        } else {
            let scalar =
                L1BlockInfo::fetch_da_footprint_gas_scalar(self.evm.state_mut()).map_err(|e| {
                    let context =
                        ErrorContext::account(L1_BLOCK_CONTRACT).with_transaction(context);
                    BlockError::read(Phase::Bookkeeping, e, context)
                })?;
            let footprint = da_size.saturating_mul(scalar as u64);
            self.limiter.check_da_footprint(footprint)?;
            footprint
        };
        let before = self
            .evm
            .read_account_info(&sender)
            .map_err(|e| {
                let context = ErrorContext::account(sender).with_transaction(context);
                BlockError::read(Phase::Bookkeeping, e, context)
            })?
            .unwrap_or_default();
        let executed = self.evm.transact(tx).map(|executed| executed.detach());
        // The attempt's valid lookups join the block's union whatever became of it: kept when a
        // later lookup failed, and when the candidate is dropped (`SATIN-WIT-013`). Committing an
        // outcome executed elsewhere adds no lookup here.
        let bucket_reads = self.evm.ext().salt.lookups();
        self.bucket_reads.extend(bucket_reads.iter().copied());
        let TxResultWithState { result, pending_state, .. } =
            executed.map_err(|e| CandidateError::from_execution(e, context))?;
        let ext = self.evm.ext();
        let usage = ext.usage(result.status);
        let (history, history_bytes) = ext.history(result.status);
        let state = result.state_gas_spent.saturating_sub(history);
        let gas = [result.execution_gas_spent(), state, history, history_bytes];
        let gas_used = result.tx_gas_used();
        Ok(Candidate {
            ty,
            gas_limit,
            tx_size,
            da_size,
            da_footprint,
            is_deposit,
            is_system_transaction,
            sender,
            sender_nonce: before.nonce,
            sender_balance: before.balance,
            result,
            pending: pending_state,
            usage,
            gas,
            gas_used,
            bucket_reads,
            block: self.identity,
            simulated: self.simulation,
        })
    }

    /// Re-checks the candidate's block, the block's room and the sender, then commits `out` (Satin
    /// `commit_transaction_outcome`). Returns the receipt's gas.
    pub fn commit_candidate(&mut self, out: Candidate) -> Result<u64, CandidateError> {
        self.check_fault()?;
        self.commit(out).map_err(|e| self.decide(e))
    }

    fn commit(&mut self, out: Candidate) -> Result<u64, CandidateError> {
        if out.block != self.identity {
            return Err(CandidateError::Foreign { expected: self.identity, found: out.block });
        }
        if out.simulated != self.simulation {
            return Err(CandidateError::OtherRules { simulated: out.simulated });
        }
        // The activation policy holds at the commit too, whichever session ran the candidate.
        if self.activation.deposits_only() && !out.is_deposit {
            return Err(Refusal::OnlyDeposits.into());
        }
        let exempt = self.packing_exempt(out.is_deposit, out.is_system_transaction);
        self.limiter.pre_execution_check(out.gas_limit, exempt)?;
        self.limiter.check_da_footprint(out.da_footprint)?;
        let now = self
            .evm
            .read_account_info(&out.sender)
            .map_err(|e| {
                let context = ErrorContext::account(out.sender).with_transaction(TxContext {
                    index: self.receipts.len(),
                    sender: out.sender,
                    ty: out.ty,
                });
                BlockError::read(Phase::Commit, e, context)
            })?
            .unwrap_or_default();
        if (now.nonce, now.balance) != (out.sender_nonce, out.sender_balance) {
            return Err(CandidateError::Stale { sender: out.sender });
        }
        let l = &mut self.limiter;
        l.block_gas_used = l.block_gas_used.saturating_add(out.gas_used);
        for i in 0..3 {
            l.usage[i] = l.usage[i].saturating_add(out.usage[i]);
        }
        for i in 0..4 {
            l.gas[i] = l.gas[i].saturating_add(out.gas[i]);
        }
        l.block_tx_size_used = l.block_tx_size_used.saturating_add(out.tx_size);
        if !out.is_deposit {
            l.block_da_size_used = l.block_da_size_used.saturating_add(out.da_size);
            l.block_da_footprint_used = l.block_da_footprint_used.saturating_add(out.da_footprint);
        }
        let Candidate { ty, is_deposit, sender_nonce, result, pending, gas_used, .. } = out;
        let status = result.status && !result.ext.failed_deposit;
        let bloom = alloy_primitives::logs_bloom(result.logs.iter());
        self.receipts.push(Receipt {
            ty,
            status,
            cumulative_gas_used: self.limiter.block_gas_used,
            bloom,
            logs: result.logs,
            deposit_nonce: is_deposit.then_some(sender_nonce),
        });
        self.evm.commit_source(&pending);
        let _ = pending.visit(&mut self.changes);
        Ok(gas_used)
    }
}

/// A receipt's EIP-2718 encoding, as op-alloy's `OpReceiptEnvelope` encodes it: the type byte
/// (none for legacy) and the RLP list of status, cumulative gas, logs bloom and logs, plus a
/// deposit's nonce and receipt version (1, Canyon).
pub fn encode_receipt(r: &Receipt) -> Vec<u8> {
    use alloy_rlp::Encodable;
    let bloom = r.bloom;
    let mut payload = Vec::new();
    r.status.encode(&mut payload);
    r.cumulative_gas_used.encode(&mut payload);
    bloom.encode(&mut payload);
    r.logs.encode(&mut payload);
    if let Some(n) = r.deposit_nonce {
        n.encode(&mut payload);
        1u64.encode(&mut payload);
    }
    let mut out = Vec::new();
    if r.ty != 0 {
        out.push(r.ty);
    }
    alloy_rlp::Header { list: true, payload_length: payload.len() }.encode(&mut out);
    out.extend_from_slice(&payload);
    out
}
