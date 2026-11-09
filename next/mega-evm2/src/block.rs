//! A block executor for the engine, the counterpart of Satin's `MegaBlockExecutor`
//! (`crates/mega-evm/src/block/executor.rs`, `limit.rs` @ 5e3f30ad) for what a validator runs:
//!
//! - **pre-block**: the EIP-2935 and EIP-4788 system calls, the `SequencerRegistry`'s due-change
//!   read and its `applyPendingChanges()` call when due, each on evm2's `Evm::system_call` as the
//!   protocol's own work, then the read of the L1 block info slots (read-only, as Satin's
//!   `read_l1_block_info`). Each step is committed on its own;
//! - **per transaction, before it runs** (`run_transaction`): the activation-block rule, the Mega
//!   System Transaction decision off the block's state (`live_system_address` + the system shape),
//!   `BlockLimiter::pre_execution_check` (the block's gas limit, the builder's size limits, the
//!   three packing budgets: data size, KV updates, state growth; a deposit is held to the gas limit
//!   alone, a Mega System Transaction to all of them as any other non-deposit transaction), the DA
//!   footprint against the block's gas limit, the sender's account read; then `Evm::transact` and
//!   `ExecutedTx::detach` (nothing committed);
//! - **commit** (`commit_transaction_outcome`): the room check and the sender check again, the
//!   block's counters, the receipt (status, cumulative gas, logs; a deposit's nonce and receipt
//!   version), the state into the EVM's overlay and the block's change set
//!   (`BlockStateAccumulator`, the counterpart of revm's bundle).
//!
//! The limits are Satin's `ProtocolLimits::for_spec(SATIN)`; the builder's policy is a
//! validator's (`BlockLimits::default()`: no limit). Errors are strings: a refusal names its rule.

use crate::{MegaTypes, SEQUENCER_REGISTRY, tx::ORACLE};
use alloy_consensus::{Transaction, transaction::Recovered};
use alloy_eips::Typed2718;
use alloy_primitives::{Address, Bytes, Log, TxKind, U256, keccak256};
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
    pub fn pre_execution_check(&self, gas_limit: u64, packing_exempt: bool) -> Result<(), String> {
        if self.block_gas_used.saturating_add(gas_limit) > self.block_gas_limit {
            return Err(format!(
                "transaction gas limit {gas_limit} is more than blocks available gas {}",
                self.block_gas_limit.saturating_sub(self.block_gas_used)
            ));
        }
        if packing_exempt {
            return Ok(());
        }
        const NAMES: [&str; 3] =
            ["Block transactions data limit", "Block KV update limit", "Block state growth limit"];
        for i in 0..3 {
            if self.usage[i] >= BLOCK_BUDGETS[i] {
                return Err(format!(
                    "{} reached: block_used={} >= limit={}",
                    NAMES[i], self.usage[i], BLOCK_BUDGETS[i]
                ));
            }
        }
        Ok(())
    }

    pub fn check_da_footprint(&self, footprint: u64) -> Result<(), String> {
        let available = self.block_gas_limit.saturating_sub(self.block_da_footprint_used);
        if footprint > available {
            return Err(format!(
                "transaction DA footprint {footprint} above the block's available {available}"
            ));
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

/// An executed, not yet committed transaction (Satin `MegaBlockTxResult`).
#[derive(Debug)]
pub struct Executed {
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
}

/// The block executor over one `Evm` for the whole block.
pub struct BlockExecutor<'a> {
    pub evm: Evm<'a, MegaTypes>,
    pub limiter: BlockLimiter,
    pub receipts: Vec<Receipt>,
    /// The block's change set (revm's bundle counterpart).
    pub changes: BlockStateAccumulator,
    /// Whole pre-block states in commit order, including read-only accounts and slots.
    pub pre_block_states: Vec<PendingState>,
    /// Valid SALT lookups across every execution in this block, including discarded candidates.
    #[cfg(feature = "witness")]
    pub bucket_reads: std::collections::BTreeSet<u64>,
    pub parent_hash: alloy_primitives::B256,
    pub parent_beacon_block_root: Option<alloy_primitives::B256>,
    /// An activation block admits only deposits (false for every replayed block).
    pub only_deposits: bool,
}

impl<'a> BlockExecutor<'a> {
    /// An executor over `evm`. Its transactions run under Satin's limits and detention caps
    /// ([`crate::TxLimits::SATIN`]) whatever the EVM was set up with ([`Self::run_transaction`]):
    /// they are block validity.
    pub fn new(
        evm: Evm<'a, MegaTypes>,
        parent_hash: alloy_primitives::B256,
        parent_beacon_block_root: Option<alloy_primitives::B256>,
    ) -> Self {
        let block_gas_limit = evm.block().gas_limit.saturating_to();
        Self {
            evm,
            limiter: BlockLimiter { block_gas_limit, ..Default::default() },
            receipts: Vec::new(),
            changes: BlockStateAccumulator::new(),
            pre_block_states: Vec::new(),
            #[cfg(feature = "witness")]
            bucket_reads: Default::default(),
            parent_hash,
            parent_beacon_block_root,
            only_deposits: false,
        }
    }

    fn system_call(&mut self, target: Address, data: Bytes) -> Result<PendingState, String> {
        self.evm.ext_mut().begin_system_call();
        let executed = self
            .evm
            .system_call(SystemTx::new(target, data))
            .map_err(|e| format!("system call {target}: {e:?}"))?;
        let TxResultWithState { result: r, pending_state, .. } = executed.detach();
        if !r.status {
            return Err(format!("system call {target} failed: {:?}", r.stop));
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
    /// its activation block when one is pending. A read the database cannot serve is an error.
    pub fn registry_due(&mut self) -> Result<bool, String> {
        self.read_registry_due().map(|(due, _)| due)
    }

    fn read_registry_due(&mut self) -> Result<(bool, PendingState), String> {
        let number = self.evm.block().number;
        let state = self.evm.state_mut();
        let failed = |e: evm2::DatabaseError| format!("registry read: {e:?}");
        let info = state.account_info_untracked(&SEQUENCER_REGISTRY).map_err(failed)?;
        let mut reads = PendingState::default();
        reads.insert_account(SEQUENCER_REGISTRY, info.clone(), info.clone());
        let mut due = false;
        if info.is_some() {
            for (p, a) in REGISTRY_PENDING {
                let key = U256::from(p);
                let pending =
                    state.storage_slot_untracked(&SEQUENCER_REGISTRY, &key).map_err(failed)?;
                reads.insert_storage(SEQUENCER_REGISTRY, key, pending, pending);
                if pending.is_zero() {
                    continue;
                }
                let key = U256::from(a);
                let activation =
                    state.storage_slot_untracked(&SEQUENCER_REGISTRY, &key).map_err(failed)?;
                reads.insert_storage(SEQUENCER_REGISTRY, key, activation, activation);
                due |= number >= activation;
            }
        }
        Ok((due, reads))
    }

    /// Satin `apply_pre_execution_changes` (no activation block: the predeploy check is not made).
    /// A block without a parent beacon block root is refused at the EIP-4788 step.
    pub fn apply_pre_execution_changes(&mut self) -> Result<(), String> {
        let parent = Bytes::from(self.parent_hash.0.to_vec());
        let state = self.system_call(HISTORY_STORAGE, parent)?;
        self.commit_pre_block_state(state);
        // A block without a parent beacon block root is invalid (EIP-4788).
        let root = self
            .parent_beacon_block_root
            .ok_or_else(|| "missing parent beacon block root (EIP-4788)".to_owned())?;
        let state = self.system_call(BEACON_ROOTS, Bytes::from(root.0.to_vec()))?;
        self.commit_pre_block_state(state);
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
        // database cannot serve fails the block.
        let state = self.evm.state_mut();
        let failed = |e: evm2::DatabaseError| format!("L1 block info read: {e:?}");
        let info = state.account_info_untracked(&L1_BLOCK_CONTRACT).map_err(failed)?;
        let mut reads = PendingState::default();
        reads.insert_account(L1_BLOCK_CONTRACT, info.clone(), info.clone());
        if info.is_some() {
            for slot in L1_BLOCK_INFO_SLOTS {
                let key = U256::from(slot);
                let value =
                    state.storage_slot_untracked(&L1_BLOCK_CONTRACT, &key).map_err(failed)?;
                reads.insert_storage(L1_BLOCK_CONTRACT, key, value, value);
            }
        }
        self.commit_pre_block_state(reads);
        Ok(())
    }

    /// Whether `tx` is a Mega System Transaction: sent from the live system address, which every
    /// transaction reads off the block's state, with the system shape (a transaction of another
    /// shape from that address is refused when the engine validates it). A read the database
    /// cannot serve is an error.
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

    fn is_system_transaction(&mut self, tx: &Recovered<OpTxEnvelope>) -> Result<bool, String> {
        let live = crate::tx::live_system_address(&mut self.evm)
            .map_err(|e| format!("registry read: {e:?}"))?;
        let shape = tx.inner().ty() == 0 && tx.inner().kind() == TxKind::Call(ORACLE);
        Ok(shape && live == Some(tx.signer()))
    }

    /// Runs `tx` (its 2718 bytes `encoded`) against the block's rules; nothing is committed.
    pub fn run_transaction(
        &mut self,
        tx: &Recovered<OpTxEnvelope>,
        encoded: &[u8],
    ) -> Result<Executed, String> {
        let env = tx.inner();
        let ty = env.ty();
        let is_deposit = env.is_deposit();
        if self.only_deposits && !is_deposit {
            return Err("unexpected non-deposit transaction in an activation block".into());
        }
        // Satin's limits, whatever was set on the EVM.
        self.evm.ext_mut().limits = crate::TxLimits::SATIN;
        let tx_size = encoded.len() as u64;
        let da_size = estimated_da_size(encoded);
        let gas_limit = env.gas_limit();
        let is_system_transaction = !is_deposit && self.is_system_transaction(tx)?;
        self.limiter.pre_execution_check(
            gas_limit,
            self.packing_exempt(is_deposit, is_system_transaction),
        )?;
        let da_footprint = if is_deposit {
            0
        } else {
            let scalar = L1BlockInfo::fetch_da_footprint_gas_scalar(self.evm.state_mut())
                .map_err(|e| format!("{e:?}"))?;
            let footprint = da_size.saturating_mul(scalar as u64);
            self.limiter.check_da_footprint(footprint)?;
            footprint
        };
        let sender = tx.signer();
        let before =
            self.evm.read_account_info(&sender).map_err(|e| format!("{e:?}"))?.unwrap_or_default();
        #[cfg(feature = "witness")]
        self.evm.ext_mut().bucket_reads.clear();
        let executed = self.evm.transact(tx).map(|executed| executed.detach());
        // Capture successful capacity answers even if a later lookup aborts the transaction.
        // Committing an outcome executed elsewhere deliberately contributes no reads here.
        #[cfg(feature = "witness")]
        self.bucket_reads.extend(self.evm.ext().bucket_reads.iter().copied());
        let TxResultWithState { result, pending_state, .. } =
            executed.map_err(|e| format!("EVM reported invalid transaction: {e:?}"))?;
        let ext = self.evm.ext();
        let usage = ext.usage(result.status);
        let (history, history_bytes) = ext.history(result.status);
        let state = result.state_gas_spent.saturating_sub(history);
        let gas = [result.execution_gas_spent(), state, history, history_bytes];
        let gas_used = result.tx_gas_used();
        Ok(Executed {
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
        })
    }

    /// Re-checks the block's room and the sender, then commits `out` (Satin
    /// `commit_transaction_outcome`). Returns the receipt's gas.
    pub fn commit_transaction_outcome(&mut self, out: Executed) -> Result<u64, String> {
        let exempt = self.packing_exempt(out.is_deposit, out.is_system_transaction);
        self.limiter.pre_execution_check(out.gas_limit, exempt)?;
        self.limiter.check_da_footprint(out.da_footprint)?;
        let now = self
            .evm
            .read_account_info(&out.sender)
            .map_err(|e| format!("{e:?}"))?
            .unwrap_or_default();
        if (now.nonce, now.balance) != (out.sender_nonce, out.sender_balance) {
            return Err(format!("stale outcome: sender {}", out.sender));
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
        let Executed { ty, is_deposit, sender_nonce, result, pending, gas_used, .. } = out;
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
