//! A block session of the engine, the counterpart of Satin's `MegaBlockExecutor`
//! (`crates/mega-evm/src/block/executor.rs`, `limit.rs` @ 5e3f30ad). A session is made only by
//! [`SatinFactory`](crate::factory::SatinFactory), which installs the fixed protocol parameters and
//! derives the block's activation facts from the chain configuration; it holds its EVM privately,
//! so nothing can change a protocol value under it.
//!
//! - **pre-block**, before the session is returned: the EIP-2935 and EIP-4788 system calls (block
//!   zero makes neither, and its parent beacon block root must be zero), the `SequencerRegistry`'s
//!   due-change read and its `applyPendingChanges()` call when due, each on evm2's
//!   `Evm::system_call` as the protocol's own work, then the read of the L1 block info slots
//!   (read-only, as Satin's `read_l1_block_info`). Each step is committed on its own;
//! - **per candidate, before it runs** ([`BlockSession::execute_candidate`]): the activation
//!   block's deposit-only rule, the Mega System Transaction decision off the block's state
//!   (`live_system_address` + the system shape), `BlockLimiter::pre_execution_check` (the block's
//!   gas limit, the builder's size limits, the three packing budgets: data size, KV updates, state
//!   growth; a deposit is held to the gas limit alone, a Mega System Transaction to all of them as
//!   any other non-deposit transaction), the DA footprint against the block's gas limit, the
//!   sender's account read; then `Evm::transact` and `ExecutedTx::detach` (nothing committed);
//! - **commit** ([`BlockSession::commit_candidate`]): the candidate's block, the room check and the
//!   sender check again, the block's counters, the receipt (status, cumulative gas, logs; a
//!   deposit's nonce and receipt version), the state into the EVM's overlay and the block's change
//!   set (`BlockStateAccumulator`, the counterpart of revm's bundle).
//!
//! The limits are Satin's `ProtocolLimits::for_spec(SATIN)`; the builder's policy is a
//! validator's (`BlockLimits::default()`: no limit). Failures are typed ([`crate::error`]): a
//! refusal names its rule; a failure that decides the block (an internal error or an invalid
//! block) is kept, and the session refuses every later candidate and cannot finish.

use crate::{
    MegaTypes, SEQUENCER_REGISTRY, TxLimits,
    chain::Activation,
    error::{
        BlockError, BlockIdentity, Budget, CandidateError, ErrorContext, Phase, Refusal, TxContext,
    },
    tx::ORACLE,
};
use alloy_consensus::{Transaction, transaction::Recovered};
use alloy_eips::Typed2718;
use alloy_primitives::{Address, B256, Bytes, Log, TxKind, U256, keccak256};
use evm2::{
    Evm, TxResult,
    evm::{BlockStateAccumulator, PendingState, StateChangeSource, SystemTx, TxResultWithState},
};
use op_evm2::{
    L1BlockInfo, OpTxEnvelope, constants::L1_BLOCK_CONTRACT, estimate_tx_compressed_size,
};

/// Satin's `ProtocolLimits::for_spec(SATIN)` block budgets: data size, KV updates, state growth.
pub const BLOCK_BUDGETS: [u64; 3] = [13_107_200, 500_000, 1_000];
const COMPRESSED_SIZE_SCALE: u64 = 1_000_000;
/// The L1 block slots the transactions can be priced against (Satin `L1_BLOCK_INFO_SLOTS`).
const L1_BLOCK_INFO_SLOTS: [u64; 5] = [1, 3, 5, 7, 8];
/// The registry's pending-change slots: (pending system address, its activation block) and
/// (pending sequencer, its activation block).
const REGISTRY_PENDING: [(u64, u64); 2] = [(7, 8), (9, 10)];
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
/// ran for.
#[derive(Debug)]
pub struct Candidate {
    pub ty: u8,
    pub gas_limit: u64,
    pub tx_size: u64,
    pub da_size: u64,
    pub da_footprint: u64,
    pub is_deposit: bool,
    pub is_system_transaction: bool,
    pub sender: Address,
    pub sender_nonce: u64,
    pub sender_balance: U256,
    pub result: TxResult<MegaTypes>,
    pub pending: PendingState,
    /// The engine's usage (data size, KV, growth) and ledgers (execution, state, history gas,
    /// history bytes), read when the transaction ended.
    pub usage: [u64; 3],
    pub gas: [u64; 4],
    /// Receipt gas (`max(spent − refund, floor)`).
    pub gas_used: u64,
    block: BlockIdentity,
}

impl Candidate {
    /// The block the candidate ran for.
    pub const fn block(&self) -> BlockIdentity {
        self.block
    }
}

/// What a finished block produced.
#[derive(Debug)]
pub struct BlockOutput {
    pub receipts: Vec<Receipt>,
    /// The block's change set, pre-block states included.
    pub changes: BlockStateAccumulator,
    /// Whole pre-block states in commit order, including read-only accounts and slots.
    pub pre_block_states: Vec<PendingState>,
    pub limiter: BlockLimiter,
    /// Valid SALT lookups across every execution in the block, including discarded candidates.
    #[cfg(feature = "witness")]
    pub bucket_reads: std::collections::BTreeSet<u64>,
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
    /// Valid SALT lookups across every execution in this block, including discarded candidates.
    #[cfg(feature = "witness")]
    bucket_reads: std::collections::BTreeSet<u64>,
    identity: BlockIdentity,
    parent_beacon_block_root: Option<B256>,
    activation: Activation,
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
        parent_beacon_block_root: Option<B256>,
        activation: Activation,
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
            #[cfg(feature = "witness")]
            bucket_reads: Default::default(),
            identity,
            parent_beacon_block_root,
            activation,
            limits,
            simulation,
            fault: None,
        };
        session.apply_pre_execution_changes()?;
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

    /// Valid SALT lookups across every execution in this block, including discarded candidates
    /// and a candidate whose later lookup failed.
    #[cfg(feature = "witness")]
    pub const fn bucket_reads(&self) -> &std::collections::BTreeSet<u64> {
        &self.bucket_reads
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
            limiter: self.limiter,
            #[cfg(feature = "witness")]
            bucket_reads: self.bucket_reads,
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

    /// Whether the registry has a role change due in this block (Satin
    /// `is_apply_pending_changes_due`), read off the block's state: each role's pending value, and
    /// its activation block when one is pending. A read the database cannot serve is an internal
    /// error.
    pub fn registry_due(&mut self) -> Result<bool, BlockError> {
        self.read_registry_due().map(|(due, _)| due)
    }

    fn read_registry_due(&mut self) -> Result<(bool, PendingState), BlockError> {
        let number = self.evm.block().number;
        let state = self.evm.state_mut();
        let failed = |slot: Option<U256>| {
            move |e: evm2::DatabaseError| {
                let context = ErrorContext { slot, ..ErrorContext::account(SEQUENCER_REGISTRY) };
                BlockError::read(Phase::DueChangeRead, e, context)
            }
        };
        let info = state.account_info_untracked(&SEQUENCER_REGISTRY).map_err(failed(None))?;
        let mut reads = PendingState::default();
        reads.insert_account(SEQUENCER_REGISTRY, info.clone(), info.clone());
        let mut due = false;
        if info.is_some() {
            for (p, a) in REGISTRY_PENDING {
                let key = U256::from(p);
                let pending = state
                    .storage_slot_untracked(&SEQUENCER_REGISTRY, &key)
                    .map_err(failed(Some(key)))?;
                reads.insert_storage(SEQUENCER_REGISTRY, key, pending, pending);
                if pending.is_zero() {
                    continue;
                }
                let key = U256::from(a);
                let activation = state
                    .storage_slot_untracked(&SEQUENCER_REGISTRY, &key)
                    .map_err(failed(Some(key)))?;
                reads.insert_storage(SEQUENCER_REGISTRY, key, activation, activation);
                due |= number >= activation;
            }
        }
        Ok((due, reads))
    }

    /// Satin `apply_pre_execution_changes` (the activation block's factory check and the system
    /// contracts' deployment are not made). A block without a parent beacon block root is invalid
    /// at the EIP-4788 step; block zero makes neither EIP call (`SATIN-ACT-012`).
    fn apply_pre_execution_changes(&mut self) -> Result<(), BlockError> {
        if self.activation.block_zero() {
            // Block zero has no parent: no EIP-2935 or EIP-4788 call, and its parent beacon block
            // root must be zero.
            match self.parent_beacon_block_root {
                Some(root) if root.is_zero() => {}
                Some(root) => return Err(BlockError::genesis_parent_beacon_root(root)),
                None => return Err(BlockError::missing_parent_beacon_root(BEACON_ROOTS)),
            }
        } else {
            let parent = Bytes::from(self.identity.parent_hash.0.to_vec());
            let state = self.system_call(HISTORY_STORAGE, parent)?;
            self.commit_pre_block_state(state);
            // A block without a parent beacon block root is invalid (EIP-4788).
            let root = self
                .parent_beacon_block_root
                .ok_or_else(|| BlockError::missing_parent_beacon_root(BEACON_ROOTS))?;
            let state = self.system_call(BEACON_ROOTS, Bytes::from(root.0.to_vec()))?;
            self.commit_pre_block_state(state);
        }
        let (due, reads) = self.read_registry_due()?;
        self.commit_pre_block_state(reads);
        if due {
            let state = self.system_call(
                SEQUENCER_REGISTRY,
                Bytes::from(keccak256("applyPendingChanges()")[..4].to_vec()),
            )?;
            self.commit_pre_block_state(state);
        }
        // Satin `read_l1_block_info`: the account, then its slots when it exists. A read the
        // database cannot serve is an internal error.
        let state = self.evm.state_mut();
        let failed = |slot: Option<U256>| {
            move |e: evm2::DatabaseError| {
                let context = ErrorContext { slot, ..ErrorContext::account(L1_BLOCK_CONTRACT) };
                BlockError::read(Phase::L1BlockInfoRead, e, context)
            }
        };
        let info = state.account_info_untracked(&L1_BLOCK_CONTRACT).map_err(failed(None))?;
        let mut reads = PendingState::default();
        reads.insert_account(L1_BLOCK_CONTRACT, info.clone(), info.clone());
        if info.is_some() {
            for slot in L1_BLOCK_INFO_SLOTS {
                let key = U256::from(slot);
                let value = state
                    .storage_slot_untracked(&L1_BLOCK_CONTRACT, &key)
                    .map_err(failed(Some(key)))?;
                reads.insert_storage(L1_BLOCK_CONTRACT, key, value, value);
            }
        }
        self.commit_pre_block_state(reads);
        Ok(())
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

    /// Whether `tx` is a Mega System Transaction: sent from the live system address, which every
    /// transaction reads off the block's state, with the system shape (a transaction of another
    /// shape from that address is refused when the engine validates it). A read the database
    /// cannot serve is an internal error.
    fn is_system_transaction(
        &mut self,
        tx: &Recovered<OpTxEnvelope>,
        context: TxContext,
    ) -> Result<bool, BlockError> {
        let live = crate::tx::live_system_address(&mut self.evm).map_err(|e| {
            let context = ErrorContext::account(SEQUENCER_REGISTRY).with_transaction(context);
            BlockError::read(Phase::SystemAddressResolution, e, context)
        })?;
        let shape = tx.inner().ty() == 0 && tx.inner().kind() == TxKind::Call(ORACLE);
        Ok(shape && live == Some(tx.signer()))
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
        let is_system_transaction = !is_deposit && self.is_system_transaction(tx, context)?;
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
        #[cfg(feature = "witness")]
        self.evm.ext_mut().bucket_reads.clear();
        let executed = self.evm.transact(tx).map(|executed| executed.detach());
        // Capture successful capacity answers even if a later lookup aborts the transaction.
        // Committing an outcome executed elsewhere deliberately contributes no reads here.
        #[cfg(feature = "witness")]
        self.bucket_reads.extend(self.evm.ext().bucket_reads.iter().copied());
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
            block: self.identity,
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
