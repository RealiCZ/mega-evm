//! The engine's per-transaction ledger: the start ledger with the three resource pools, the signed
//! history ledger, the write records, the SALT multipliers and what a transaction reports.
//! History is state gas on evm2's shared spill plus a signed per-frame ledger, with the LOG
//! allowance.

use crate::{MegaTypes, detention::DetentionState};
use alloy_primitives::{Address, B256, U256, keccak256, map::HashMap};
use core::cmp::min;
use evm2::{
    Evm,
    interpreter::{Gas, InstrStop, InterpreterState, Result},
    version::{GasId, StateGasSite},
};
#[cfg(feature = "witness")]
use std::collections::BTreeSet;

pub const CPSB: u64 = 1_530;
pub const CPHB: u64 = 88;
pub const WRITE_RECORD_SIZE: u64 = 40;
pub const TX_BODY_SIZE: u64 = 310;
pub const TRANSFER_LOG_SIZE: u64 = 160;
pub const HISTORY_ALLOWANCE_BYTES: u64 = 160;
pub const EXECUTION_CAP: u64 = 200_000_000;
pub const MIN_BUCKET_SIZE: u64 = 256;
/// Limit kinds by pool index: data size, KV updates, state growth.
pub const KIND: [u8; 3] = [0, 1, 3];
/// `SequencerRegistry` and its `_currentSystemAddress` slot, read by every transaction.
pub const ORACLE: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01]);
pub const SEQUENCER_REGISTRY: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x06]);

/// What the node configures (persists across transactions).
#[derive(Clone, Debug, Default)]
pub struct Cfg {
    pub limits: [u64; 3],
    /// Detention's block-environment cap (`u64::MAX`: off).
    pub cap: u64,
    /// SALT bucket capacities by bucket id; missing = the minimum bucket.
    pub buckets: HashMap<u64, u64>,
    /// The registry slot that holds `_currentSystemAddress`.
    pub registry_slot: U256,
    /// Detention's Oracle cap (`u64::MAX`: off).
    pub oracle_cap: u64,
    /// The oracle service's answers by slot (Satin `OracleEnv::get_oracle_storage`).
    pub oracle: HashMap<U256, U256>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Entry {
    pub budget: [u64; 3],
    pub net: [i64; 3],
    /// History gas this frame keeps (signed: refills are negative) and its bytes.
    pub hist: i64,
    pub hbytes: i64,
    /// The history allowance of a value call's frame (gas), drawn only by LOGs.
    pub allowance: u64,
    /// History the caller paid for the frame's own start records; given back on failure.
    pub start_charge: u64,
    pub kept_reservoir: u64,
    pub takebacks: u64,
    pub start: Option<StartRecords>,
    pub depth: usize,
    /// Where this frame's entries begin in the transaction's undo logs (slot records and
    /// destroyed accounts); a failed frame rolls the logs back to these marks.
    pub slot_mark: usize,
    pub destroyed_mark: usize,
}

/// A start's accounts (a call's caller and target, or a creation's created account) as they stood
/// before the start, its state growth and its transfer log. Inline (at most two accounts),
/// so a start allocates nothing.
#[derive(Clone, Copy, Debug, Default)]
pub struct StartRecords {
    pub accounts: [(Address, bool); 2],
    pub n: u8,
    pub growth: i64,
    pub transfer_log: bool,
}

impl StartRecords {
    pub fn one(a: (Address, bool), growth: i64, transfer_log: bool) -> Self {
        Self { accounts: [a, Default::default()], n: 1, growth, transfer_log }
    }
    pub fn two(a: (Address, bool), b: (Address, bool), growth: i64, transfer_log: bool) -> Self {
        Self { accounts: [a, b], n: 2, growth, transfer_log }
    }
    pub fn accounts(&self) -> &[(Address, bool)] {
        &self.accounts[..self.n as usize]
    }
}

impl Entry {
    /// What the start ledger reports: budget − net, not clamped to the budget.
    pub fn reported_remaining(&self, i: usize) -> u64 {
        self.budget[i].saturating_add_signed(self.net[i].saturating_neg())
    }

    pub fn remaining(&self, i: usize) -> u64 {
        let rem = self.budget[i].saturating_add_signed(self.net[i].saturating_neg());
        min(self.budget[i], rem)
    }
}

#[derive(Clone, Debug)]
pub struct Settlement {
    pub depth: usize,
    pub frame_remaining: [u64; 3],
    pub frame_net: [i64; 3],
    pub caller_remaining: [u64; 3],
    pub caller_net: [i64; 3],
    pub refund: u64,
}

/// The engine state in `Evm::ext_mut`.
///
/// One value per EVM, cleared (not rebuilt) per transaction, so its collections keep their
/// capacity. The slot records and the destroyed accounts are undone per frame through undo logs
/// (O(changes)), not snapshotted per frame. What exists only for the comparison is behind the
/// `compare` feature (settlements, precompile pricing report, negative-control switches, debug
/// traces); the read-set recording a witness producer needs (bucket reads) is behind `witness`.
#[derive(Clone, Debug, Default)]
pub struct Lite {
    pub cfg: Cfg,
    pub l1: op_evm2::L1BlockInfo,
    pub sender: Address,
    /// Deposits pay no history (`prices_history`).
    pub prices_history: bool,
    pub is_deposit: bool,
    pub tx_net: [i64; 3],
    pub frames: Vec<Entry>,
    /// Σ `frames[..].net`, kept as the frames draw, merge and roll back (instead of a sum over the
    /// frames at every draw).
    pub frames_net: [i64; 3],
    pub top_stop: Option<(u8, u64)>,
    #[cfg(feature = "compare")]
    pub settlements: Vec<Settlement>,
    pub stops: u64,
    pub detention: DetentionState,
    pub tx_start_charge: u64,
    pub tx_start_bytes: u64,
    pub body_history: u64,
    pub body_bytes: u64,
    pub last_caller_remaining: u64,
    pub reservoir_left: u64,
    /// SALT: multipliers read this transaction.
    pub salt_cache: HashMap<u64, u64>,
    /// SALT: every bucket read (read set; a witness producer needs it, a validator does not).
    #[cfg(feature = "witness")]
    pub bucket_reads: BTreeSet<u64>,
    /// The price an account-level creation charge (`CreateState`, `CodeDepositState`) was
    /// made at, for the destruction table; a slot's charge is re-priced through the transaction's
    /// SALT cache instead (Satin re-prices through `state_gas_price`). Was every state charge.
    pub created_prices: Vec<(Address, GasId, u64)>,
    /// Slot records live in the transaction: (address, key) -> made non-zero from zero.
    pub slot_records: HashMap<(Address, U256), bool>,
    /// Undo log of `slot_records` (the key and the value it replaced), below depth 0 only.
    pub slot_undo: Vec<((Address, U256), Option<bool>)>,
    /// Accounts the destruction table gave back (count nothing more); a stack, so a failed
    /// frame truncates it to its mark. Almost always empty.
    pub destroyed: Vec<Address>,
    /// Growth counted on the transaction (a deposit's created caller), survives failure.
    pub tx_growth: i64,
    pub intercept_before: bool,
    pub is_system: bool,
    pub deposit_created_caller: bool,
    pub top_started: bool,
    pub oracle_reads: u64,
    pub tx_start_pending: Option<[i64; 3]>,
    pub deposit_loan: u64,
    /// The protocol's own work (a promoted system-address transaction, a pre-block system
    /// call): held to no per-transaction limit (still counted), not detained, no history, the
    /// minimum SALT bucket (Satin `AdditionalLimit::is_exempt`, `is_system_originated`).
    pub exempt: bool,
    /// The state of the volatile-access control, hint, authority and keyless mechanisms
    /// (`satin_ext.rs`).
    pub mechanisms: crate::satin_ext::MechanismState,
}

pub fn limit_exceeded_output(kind: u8, limit: u64) -> Vec<u8> {
    let sel = keccak256("MegaLimitExceeded(uint8,uint64)");
    let mut out = sel[..4].to_vec();
    out.extend_from_slice(&B256::from(U256::from(kind)).0);
    out.extend_from_slice(&B256::from(U256::from(limit)).0);
    out
}

impl Lite {
    /// Resets the per-transaction state, keeping the configuration and the L1 info cache.
    /// Cleared in place; every collection keeps its capacity.
    pub fn begin_tx(
        &mut self,
        sender: Address,
        body_bytes: u64,
        prices_history: bool,
        is_deposit: bool,
    ) {
        self.sender = sender;
        self.prices_history = prices_history;
        self.is_deposit = is_deposit;
        self.frames.clear();
        self.frames_net = [0; 3];
        self.top_stop = None;
        #[cfg(feature = "compare")]
        self.settlements.clear();
        self.stops = 0;
        self.detention.reset(self.cfg.cap, self.cfg.oracle_cap);
        self.tx_start_charge = 0;
        self.tx_start_bytes = 0;
        self.last_caller_remaining = 0;
        self.reservoir_left = 0;
        self.salt_cache.clear();
        #[cfg(feature = "witness")]
        self.bucket_reads.clear();
        self.created_prices.clear();
        self.slot_records.clear();
        self.slot_undo.clear();
        self.destroyed.clear();
        self.tx_growth = 0;
        self.intercept_before = true;
        self.is_system = false;
        self.deposit_created_caller = false;
        self.top_started = false;
        self.oracle_reads = 0;
        self.tx_start_pending = None;
        self.deposit_loan = 0;
        self.exempt = false;
        self.mechanisms.reset();
        self.body_bytes = if prices_history { body_bytes } else { 0 };
        self.body_history = self.body_bytes * CPHB;
        self.tx_net = [body_bytes as i64, 0, 0];
        let limits = self.cfg.limits;
        let budget =
            core::array::from_fn(|i| limits[i].saturating_sub(self.tx_net[i].max(0) as u64));
        self.frames.push(Entry { budget, ..Entry::default() });
    }

    /// Marks the running execution as the protocol's own (see `exempt`).
    pub fn set_system_originated(&mut self) {
        self.exempt = true;
        self.frames[0].budget = [u64::MAX; 3];
        self.is_system = true;
        self.prices_history = false;
        self.body_history = 0;
        self.body_bytes = 0;
        self.detention.cap = u64::MAX;
        self.detention.oracle_cap = u64::MAX;
    }

    pub fn draw(&mut self, delta: [i64; 3]) -> Option<(u8, u64)> {
        let top = self.frames.last().expect("a frame");
        for i in 0..3 {
            if self.exempt {
                break;
            }
            if delta[i] <= 0 {
                continue;
            }
            // The frame's own draws see `budget − net` unclamped (a restore gives room back);
            // only the share a child takes is clamped to the budget (`push_child`).
            let room = if crate::off("unclamped_draw") {
                top.remaining(i)
            } else {
                top.reported_remaining(i)
            };
            if delta[i] as u64 > room {
                return Some((KIND[i], top.budget[i]));
            }
            let total: i64 =
                self.tx_net[i] + if i == 2 { self.tx_growth } else { 0 } + self.frames_net[i];
            if total + delta[i] > self.cfg.limits[i] as i64 {
                return Some((KIND[i], self.cfg.limits[i]));
            }
        }
        let top = self.frames.last_mut().unwrap();
        for (i, d) in delta.into_iter().enumerate() {
            top.net[i] += d;
            self.frames_net[i] += d;
        }
        None
    }

    /// The running frame and the transaction held to their limits with nothing drawn.
    pub fn hold_check(&self) -> Option<(u8, u64)> {
        if self.exempt {
            return None;
        }
        let top = self.frames.last()?;
        for (i, kind) in KIND.into_iter().enumerate() {
            if top.net[i] > top.budget[i] as i64 {
                return Some((kind, top.budget[i]));
            }
            let total =
                self.tx_net[i] + if i == 2 { self.tx_growth } else { 0 } + self.frames_net[i];
            if total > self.cfg.limits[i] as i64 {
                return Some((kind, self.cfg.limits[i]));
            }
        }
        None
    }

    pub fn push_child(
        &mut self,
        start_charge: u64,
        kept_reservoir: u64,
        start: Option<StartRecords>,
        allowance: u64,
    ) {
        let parent = self.frames.last().expect("a caller");
        let budget = core::array::from_fn(|i| (u128::from(parent.remaining(i)) * 98 / 100) as u64);
        let depth = self.frames.len();
        let (slot_mark, destroyed_mark) = (self.slot_undo.len(), self.destroyed.len());
        self.frames.push(Entry {
            budget,
            start_charge,
            kept_reservoir,
            start,
            depth,
            allowance,
            slot_mark,
            destroyed_mark,
            ..Entry::default()
        });
    }

    pub fn pop_child(&mut self, success: bool) -> Entry {
        let child = self.frames.pop().expect("a child");
        if success {
            let parent = self.frames.last_mut().expect("a caller");
            for i in 0..3 {
                parent.net[i] += child.net[i];
            }
            parent.hist += child.hist;
            parent.hbytes += child.hbytes;
        } else {
            // The frame's draws go with it, and its slot records and destructions are undone.
            for i in 0..3 {
                self.frames_net[i] -= child.net[i];
            }
            while self.slot_undo.len() > child.slot_mark {
                let (key, before) = self.slot_undo.pop().unwrap();
                match before {
                    Some(v) => {
                        self.slot_records.insert(key, v);
                    }
                    None => {
                        self.slot_records.remove(&key);
                    }
                }
            }
            self.destroyed.truncate(child.destroyed_mark);
        }
        #[cfg(feature = "compare")]
        {
            let parent = self.frames.last().unwrap();
            self.settlements.push(Settlement {
                depth: child.depth,
                frame_remaining: core::array::from_fn(|i| child.reported_remaining(i)),
                frame_net: child.net,
                caller_remaining: core::array::from_fn(|i| parent.reported_remaining(i)),
                caller_net: parent.net,
                refund: if success { 0 } else { child.start_charge },
            });
        }
        child
    }

    /// Sets (`Some`) or removes (`None`) a slot record, logging the old value for a frame that
    /// may still fail (none at depth 0: nothing rolls the transaction's own frame back).
    pub fn set_slot_record(&mut self, key: (Address, U256), value: Option<bool>) {
        let before = match value {
            Some(v) => self.slot_records.insert(key, v),
            None => self.slot_records.remove(&key),
        };
        if self.frames.len() > 1 && before != value {
            self.slot_undo.push((key, before));
        }
    }

    pub fn is_destroyed(&self, a: &Address) -> bool {
        self.destroyed.contains(a)
    }

    pub fn note_stop(&mut self, stop: (u8, u64)) {
        self.stops += 1;
        if self.frames.len() == 1 {
            self.top_stop = Some(stop);
        }
    }

    /// Usage the transaction reports: what sits on no frame plus what its own frame kept.
    pub fn usage(&self, success: bool) -> [u64; 3] {
        core::array::from_fn(|i| {
            let kept = if success { self.frames.first().map_or(0, |f| f.net[i]) } else { 0 };
            let tx = self.tx_net[i] + if i == 2 { self.tx_growth } else { 0 };
            (tx + kept).max(0) as u64
        })
    }

    /// (history gas, history bytes) the transaction reports.
    pub fn history(&self, success: bool) -> (u64, u64) {
        let (h, b) = if success {
            let f = &self.frames[0];
            (self.tx_start_charge as i64 + f.hist, self.tx_start_bytes as i64 + f.hbytes)
        } else {
            (0, 0)
        };
        ((self.body_history as i64 + h).max(0) as u64, (self.body_bytes as i64 + b).max(0) as u64)
    }

    /// Books history gas `amount` and `bytes` on the running frame.
    pub fn book_history(&mut self, amount: i64, bytes: i64) {
        let f = self.frames.last_mut().expect("a frame");
        f.hist += amount;
        f.hbytes += bytes;
    }
}

/// Whether `a` differs from its original in the transaction (its write record exists).
pub fn changed(evm: &mut Evm<'_, MegaTypes>, a: Address) -> bool {
    if a == evm.ext().sender || evm.ext().is_destroyed(&a) {
        return false;
    }
    // The transaction's baseline (the fork's transaction-original account info), exact even
    // when the first observation follows a change.
    let Ok(acc) = evm.state_mut().account(&a) else { return false };
    let orig = acc.original().cloned().unwrap_or_default();
    let cur = acc.get().cloned().unwrap_or_default();
    (orig.nonce, orig.balance, orig.code_hash) != (cur.nonce, cur.balance, cur.code_hash)
}

pub fn stop_frame(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    stop: (u8, u64),
) -> Result {
    state.host().ext_mut().note_stop(stop);
    revert_with(gas, state, &limit_exceeded_output(stop.0, stop.1))
}

pub fn revert_with(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    payload: &[u8],
) -> Result {
    let offset = state.memory().len();
    let mut scratch = Gas::new(u64::MAX / 2);
    state.resize_memory(&mut scratch, offset, payload.len())?;
    state.memory().set(offset, payload);
    state.set_output(offset as u32..(offset + payload.len()) as u32);
    let _ = gas;
    Err(InstrStop::Revert)
}

/// The share of its caller's EIP-8037 reservoir a frame starts with: all but the caller's two per
/// cent of it, rounded down. The kept part is taken in `u128`: a transaction's gas limit is
/// bounded by the block's alone, so twice a reservoir need not fit in a `u64`.
pub const fn reservoir_share(reservoir: u64) -> u64 {
    reservoir - (reservoir as u128 * 2 / 100) as u64
}

/// Funds a state spill from the consumer's withheld pool before the base debit (W first).
pub fn lend_state(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>, cost: u64) {
    if crate::off("lend") {
        return;
    }
    let spill = cost.saturating_sub(gas.reservoir());
    if let Some(frame) = state.host().ext_mut().detention.frames.last_mut() {
        let loan = spill.min(frame.withheld);
        frame.withheld -= loan;
        gas.erase_cost(loan);
    }
}

pub fn spend_state_withheld(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    cost: u64,
) -> Result {
    lend_state(gas, state, cost);
    gas.spend_state(cost)
}

/// Charges `bytes` of history on the running frame (W-first spill); a LOG draws the frame's
/// allowance first. Exempt transactions charge and count nothing.
pub fn charge_history(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    bytes: u64,
    from_allowance: bool,
) -> Result {
    if !state.host().ext().prices_history || bytes == 0 {
        return Ok(());
    }
    let mut amount = bytes * CPHB;
    if from_allowance {
        let f = state.host().ext_mut().frames.last_mut().unwrap();
        let paid = amount.min(f.allowance);
        f.allowance -= paid;
        amount -= paid;
    }
    if amount > 0 {
        spend_state_withheld(gas, state, amount)?;
    }
    state.host().ext_mut().book_history(amount as i64, bytes as i64);
    Ok(())
}

pub fn refill_history(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>, bytes: u64) {
    if !state.host().ext().prices_history || bytes == 0 {
        return;
    }
    gas.refill_reservoir(bytes * CPHB);
    state.host().ext_mut().book_history(-((bytes * CPHB) as i64), -(bytes as i64));
}

/// SALT: the bucket id of `key`: the production hasher under `salt-ahash` (the measured build,
/// as `mega-evme`'s replay uses), else Satin's test hasher.
#[cfg(feature = "salt-ahash")]
#[inline(always)]
pub fn bucket_id(key: &[u8]) -> u64 {
    crate::salt_hash::bucket_id(key)
}

/// SALT: FNV-1a bucket ids of Satin's `TestExternalEnvs` (`SimpleBucketHasher`).
#[cfg(not(feature = "salt-ahash"))]
pub fn bucket_id(key: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in key {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    const NUM_BUCKETS: u64 = 1 << 24;
    const NUM_META_BUCKETS: u64 = NUM_BUCKETS / 256;
    hash % (NUM_BUCKETS - NUM_META_BUCKETS) + NUM_META_BUCKETS
}

pub fn bucket_for(site: StateGasSite) -> u64 {
    match site {
        StateGasSite::Account(a) => bucket_id(a.as_slice()),
        StateGasSite::Storage(a, k) => {
            // The 52-byte key on the stack (not a heap `Vec` per lookup).
            let mut key = [0u8; 52];
            key[..20].copy_from_slice(a.as_slice());
            key[20..].copy_from_slice(&k.to_be_bytes::<32>());
            bucket_id(&key)
        }
    }
}

/// The SALT multiplier of `site`, read once per transaction and recorded in the read set.
pub fn multiplier(ext: &mut Lite, site: StateGasSite) -> u64 {
    let id = bucket_for(site);
    if let Some(m) = ext.salt_cache.get(&id) {
        return *m;
    }
    #[cfg(feature = "witness")]
    ext.bucket_reads.insert(id);
    let m = ext.cfg.buckets.get(&id).copied().unwrap_or(MIN_BUCKET_SIZE) / MIN_BUCKET_SIZE;
    ext.salt_cache.insert(id, m);
    m
}
