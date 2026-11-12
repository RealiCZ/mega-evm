//! The engine's per-transaction ledger: the start ledger with the three resource pools, the signed
//! history ledger, the write records, the SALT cache (`salt.rs`) and what a transaction reports.
//! History is state gas on evm2's shared spill plus a signed per-frame ledger, with the LOG
//! allowance; a history give-back follows the version's give-back rule, into a reservoir alone
//! under Satin's (`SATIN-HIST-012`).

use crate::{MegaTypes, detention::DetentionState};
use alloy_primitives::{Address, B256, U256, keccak256, map::HashMap};
use core::cmp::min;
use evm2::{
    Evm,
    interpreter::{Gas, InstrStop, InterpreterState, Result},
    version::GasId,
};

pub const CPSB: u64 = 1_530;
pub const CPHB: u64 = 88;
pub const WRITE_RECORD_SIZE: u64 = 40;
pub const TX_BODY_SIZE: u64 = 310;
pub const TRANSFER_LOG_SIZE: u64 = 160;
pub const HISTORY_ALLOWANCE_BYTES: u64 = 160;
pub const EXECUTION_CAP: u64 = 200_000_000;
/// The share of its remaining data-size, KV-update and state-growth budgets a frame passes to a
/// child frame: 98/100.
pub const FRAME_SHARE: (u64, u64) = (98, 100);
/// The pre-Satin price of a fresh slot's `SSTORE`, the legacy share's base for a slot.
pub const PRE_FORK_SSTORE_SET_COST: u64 = 20_000;
/// The pre-Satin price of a new account, the legacy share's base for an account.
pub const PRE_FORK_NEW_ACCOUNT_COST: u64 = 25_000;
/// The pre-Satin price of a `CREATE`, the legacy share's base for a created account.
pub const PRE_FORK_CREATE_COST: u64 = 32_000;
pub const MIN_BUCKET_SIZE: u64 = 256;
/// Limit kinds by pool index: data size, KV updates, state growth.
pub const KIND: [u8; 3] = [0, 1, 3];
/// The Oracle, and the `SequencerRegistry` with its `_currentSystemAddress` slot, which a block,
/// or a single transaction's EVM, reads once to resolve its live system address.
pub const ORACLE: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01]);
pub const SEQUENCER_REGISTRY: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x06]);
/// The registry slot that holds `_currentSystemAddress`: slot 0, fixed by the protocol.
pub const REGISTRY_SLOT: U256 = U256::ZERO;

/// What the node supplies beside the SALT snapshot (`Lite::salt`), persisting across
/// transactions: the Oracle service's answers.
#[derive(Clone, Debug, Default)]
pub struct Cfg {
    /// The oracle service's answers by slot (Satin `OracleEnv::get_oracle_storage`).
    pub oracle: HashMap<U256, U256>,
}

/// The per-transaction limits and gas detention's caps a transaction runs under (Satin
/// `ProtocolLimits::for_spec(SATIN).tx_runtime_limits`). They are protocol values, not
/// configuration: [`TxLimits::SATIN`] is the default, and the block executor installs it whatever
/// an embedder set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxLimits {
    /// The transaction limits by pool index: data size, KV updates, state growth.
    pub tx: [u64; 3],
    /// Detention's block-environment cap (`u64::MAX`: off).
    pub cap: u64,
    /// Detention's Oracle cap (`u64::MAX`: off).
    pub oracle_cap: u64,
}

impl TxLimits {
    /// Satin's: a transaction keeps at most 13,107,200 bytes of data size, 500,000 write records
    /// and 1,000 new accounts and slots; a read of volatile data caps its compute at the compute at
    /// the read plus 20,000,000, for the block environment and the Oracle alike.
    pub const SATIN: Self =
        Self { tx: [13_107_200, 500_000, 1_000], cap: 20_000_000, oracle_cap: 20_000_000 };
}

impl Default for TxLimits {
    fn default() -> Self {
        Self::SATIN
    }
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
/// traces).
#[derive(Clone, Debug, Default)]
pub struct Lite {
    pub cfg: Cfg,
    /// The live system address every transaction is classified against, resolved once and kept
    /// across transactions (`SATIN-REG-002`): by a block's pre-block sequence, after its other
    /// steps; by the factory for a single transaction's EVM, off the state it runs on. `None`: no
    /// address is the system address. No transaction reads the registry for its classification.
    pub system_address: Option<Address>,
    /// The limits every transaction runs under: Satin's ([`TxLimits::SATIN`]), which the block
    /// executor installs. A tool or a test that runs under other values sets them here; what it
    /// computes is then a counterfactual, not the chain's result.
    pub limits: TxLimits,
    pub l1: op_evm2::L1BlockInfo,
    pub sender: Address,
    /// Whether the transaction pays history gas: every transaction, deposits and system-originated
    /// ones included, but a system call (`SATIN-HIST-016`).
    pub prices_history: bool,
    /// The transaction is a failed deposit: it reports no history gas, and its body and calldata
    /// as history bytes (`SATIN-DEP-005`, `SATIN-HIST-022`).
    pub failed_deposit: bool,
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
    /// SALT: the snapshot capacities are read from, and the multipliers of the running attempt,
    /// whose keys are its valid lookups.
    pub salt: crate::salt::SaltCache,
    /// The block hashes `BLOCKHASH` was served in the running attempt, recorded where they are
    /// served, above every cache (`SATIN-WIT-014`).
    pub block_hashes: crate::block_hashes::BlockHashReads,
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
    pub is_system: bool,
    pub deposit_created_caller: bool,
    pub top_started: bool,
    pub oracle_reads: u64,
    pub tx_start_pending: Option<[i64; 3]>,
    pub deposit_loan: DepositLoan,
    /// The protocol's own work (a promoted system-address transaction, a pre-block system
    /// call): held to no per-transaction limit (still counted), not detained, the minimum SALT
    /// bucket (Satin `AdditionalLimit::is_exempt`, `is_system_originated`); a system call also
    /// pays no history gas, a system-originated transaction does (`prices_history`).
    pub exempt: bool,
    /// The state of the volatile-access control, hint, authority and keyless mechanisms
    /// (`satin_ext.rs`).
    pub mechanisms: crate::satin_ext::MechanismState,
    /// Comparison only: the rules of the implementation under review that the engine has changed
    /// and that implementation has not yet, to run under instead of the engine's; the gates run a
    /// case again under them to check the accepted class of that difference. Kept across
    /// transactions.
    #[cfg(feature = "compare")]
    pub reference_rules: Vec<ReferenceRule>,
    /// Comparison only: the regular-gas invariant of Satin's give-back rule, checked as the
    /// transaction runs (`crate::regular_check`).
    #[cfg(feature = "compare")]
    pub regular_check: crate::regular_check::RegularGasCheck,
}

/// A rule of the implementation under review that the engine has changed by a decision that
/// implementation has not made yet (`Lite::reference_rules`, comparison only; without the
/// `compare` feature the engine never runs under one).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceRule {
    /// An EIP-7702 list too short to cross the transaction's KV or state-growth limit is applied
    /// without the simulation, which then loads none of its authorities.
    ShortListUnsimulated,
    /// ModExp checks EIP-7823's bound before its formula price: an input over the bound costs the
    /// 500 minimum and fails, and is priced at that minimum.
    ModexpBoundFirst,
    /// EIP-8037's pinned give-back rule and no legacy share: a give-back of state or history gas
    /// credits the frame's execution gas first, up to the state gas that gas paid; a failed
    /// frame's spill returns to its execution gas; a returned reservoir unwinds its caller's
    /// spill; every state charge draws the reservoir first; and gas detention, the
    /// `remainingComputeGas()` answer and a keyless deployment's `gasUsed` read that rule's
    /// outstanding spill. The version runs without `EIP8037_RESERVOIR_REFILLS` and with every
    /// legacy share zero ([`crate::satin_version_under`]), so the rule combines with any of the
    /// others, which `Lite::reference_rules` holds as well.
    Eip8037GiveBack,
    /// A deposit and a system-originated transaction pay no history gas: no body, calldata, log,
    /// record or deposited code is priced, and a failed deposit reports no history bytes.
    DepositHistoryExempt,
    /// An Oracle `SLOAD` charges its warm base first, as the opcode's static gas; one whose
    /// regular gas cannot then pay the rest of the cold access runs out of gas before the slot is
    /// loaded, the service asked and the read marked; a read is marked once both are paid.
    OracleColdSkip,
    /// A refused volatile read is refused after the opcode's static gas, an account opcode's at
    /// its load once its body's charges are made, and the opcode's operands checked first.
    RefusalAfterStaticGas,
    /// A Mega System Transaction is not held to the block's data-size, KV-update and
    /// state-growth budgets.
    MstPackingExempt,
    /// The pre-block sequence deploys no system contract: neither the five flat contracts nor the
    /// SequencerRegistry are installed, replaced or migrated, whatever their addresses hold.
    NoPreBlockDeployment,
    /// Every transaction reads the registry's account and slot 0 as it is classified, both warm
    /// for the rest of the transaction, and is classified against what it read.
    RegistryReadPerTransaction,
}

/// Withheld gas a code deposit drew (its state spill, its history), which an admission refusal
/// gives back: the refusal restores the frame's gas from before the deposit, which does not hold
/// the draw. Lent by the deposit's draws, settled by its admission, and dropped unsettled when the
/// creation's frame ends (a deposit that failed before admission ran), so a loan never outlives
/// its deposit.
#[derive(Clone, Copy, Debug, Default)]
pub struct DepositLoan(u64);

impl DepositLoan {
    /// Lends `drawn` more to the running deposit.
    pub fn lend(&mut self, drawn: u64) {
        self.0 += drawn;
    }

    /// The admission's settlement: what the deposit drew, the loan ended.
    pub fn settle(&mut self) -> u64 {
        core::mem::take(&mut self.0)
    }

    /// Ends a loan its admission did not settle.
    pub fn drop_unsettled(&mut self) {
        self.0 = 0;
    }
}

/// Whether the signed usage `total` is over `limit`, compared over the whole `u64` range of the
/// limit (a limit at or above 2^63 does not narrow to a negative `i64`).
#[inline(always)]
pub fn over(total: i64, limit: u64) -> bool {
    if crate::off("wide_limits") {
        return total > limit as i64;
    }
    i128::from(total) > i128::from(limit)
}

/// The room `limit` leaves above the signed usage `used`: none when the usage is over it.
#[inline(always)]
pub fn room(limit: u64, used: i64) -> u64 {
    if crate::off("wide_limits") {
        return (limit as i64 - used).max(0) as u64;
    }
    (i128::from(limit) - i128::from(used)).clamp(0, i128::from(u64::MAX)) as u64
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
        #[cfg(feature = "compare")]
        self.regular_check.reset();
        self.stops = 0;
        self.detention.reset(self.limits.cap, self.limits.oracle_cap);
        self.tx_start_charge = 0;
        self.tx_start_bytes = 0;
        self.last_caller_remaining = 0;
        self.reservoir_left = 0;
        self.salt.begin_attempt();
        self.block_hashes.begin_attempt();
        self.created_prices.clear();
        self.slot_records.clear();
        self.slot_undo.clear();
        self.destroyed.clear();
        self.tx_growth = 0;
        self.is_system = false;
        self.deposit_created_caller = false;
        self.failed_deposit = false;
        self.top_started = false;
        self.oracle_reads = 0;
        self.tx_start_pending = None;
        self.deposit_loan = DepositLoan::default();
        self.exempt = false;
        self.mechanisms.reset();
        self.body_bytes = if prices_history { body_bytes } else { 0 };
        self.body_history = self.body_bytes * CPHB;
        self.tx_net = [body_bytes as i64, 0, 0];
        let limits = self.limits.tx;
        let budget =
            core::array::from_fn(|i| limits[i].saturating_sub(self.tx_net[i].max(0) as u64));
        self.frames.push(Entry { budget, ..Entry::default() });
        if sender == evm2::evm::SYSTEM_ADDRESS && !crate::off("protocol_origin") {
            self.set_system_originated();
        }
    }

    /// Resets the ledger for a pre-block system call: the protocol's own work, and a system call
    /// rather than a transaction (the Oracle's storage reads the chain and asks no service).
    pub fn begin_system_call(&mut self) {
        self.begin_tx(evm2::evm::SYSTEM_ADDRESS, 0, false, false);
        self.set_system_originated();
        self.mechanisms.system_call = true;
    }

    /// Marks the running execution as the protocol's own (see `exempt`). A system-originated
    /// transaction still pays history gas as any transaction does (`SATIN-MST-008`); a system call
    /// is begun without it ([`Self::begin_system_call`]).
    pub fn set_system_originated(&mut self) {
        self.exempt = true;
        self.frames[0].budget = [u64::MAX; 3];
        self.is_system = true;
        if crate::under_reference_rule(self, crate::ReferenceRule::DepositHistoryExempt) {
            self.prices_history = false;
            self.body_history = 0;
            self.body_bytes = 0;
        }
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
            if over(total + delta[i], self.limits.tx[i]) {
                return Some((KIND[i], self.limits.tx[i]));
            }
        }
        let top = self.frames.last_mut().unwrap();
        for (i, d) in delta.into_iter().enumerate() {
            top.net[i] += d;
            self.frames_net[i] += d;
        }
        None
    }

    /// Draws the transaction's own frame's decided start, `delta`, held to the limits (Satin's
    /// order at one site): per dimension in order — data size, KV updates, state growth — the
    /// frame budget, then the transaction limit, each also for a dimension the start adds nothing
    /// to, so usage over a limit before the start (a deposit's created caller) is the crossing
    /// there. A built frame and a frame answered with a success are held alike. The protocol's own
    /// work is counted and held to nothing.
    pub fn draw_first_frame(&mut self, delta: [i64; 3]) -> Option<(u8, u64)> {
        let top = self.frames.last().expect("a frame");
        // With `hold_start` off a dimension the start adds nothing to is not held.
        let hold = !crate::off("hold_start");
        for (i, kind) in KIND.into_iter().enumerate() {
            if self.exempt {
                break;
            }
            let draws = delta[i] > 0;
            let room = if crate::off("unclamped_draw") {
                top.remaining(i)
            } else {
                top.reported_remaining(i)
            };
            if (draws && delta[i] as u64 > room) || (hold && over(top.net[i], top.budget[i])) {
                return Some((kind, top.budget[i]));
            }
            let total: i64 =
                self.tx_net[i] + if i == 2 { self.tx_growth } else { 0 } + self.frames_net[i];
            if (draws || hold) && over(total + delta[i].max(0), self.limits.tx[i]) {
                return Some((kind, self.limits.tx[i]));
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
            if over(top.net[i], top.budget[i]) {
                return Some((kind, top.budget[i]));
            }
            let total =
                self.tx_net[i] + if i == 2 { self.tx_growth } else { 0 } + self.frames_net[i];
            if over(total, self.limits.tx[i]) {
                return Some((kind, self.limits.tx[i]));
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
        let (share, of) = FRAME_SHARE;
        let budget = core::array::from_fn(|i| {
            (u128::from(parent.remaining(i)) * u128::from(share) / u128::from(of)) as u64
        });
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

    /// (history gas, history bytes) the transaction reports. A failed deposit reports no history
    /// gas, and the body and calldata it keeps as its history bytes.
    pub fn history(&self, success: bool) -> (u64, u64) {
        if self.failed_deposit {
            return (0, self.body_bytes);
        }
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

/// Charges `cost` of history as state gas, its spill past the reservoir funded W first (the rule
/// evm2's state charges follow through the gas reserve).
pub fn spend_history_withheld(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    cost: u64,
) -> Result {
    if !crate::off("lend") {
        let drawn = crate::detention::draw_withheld(
            state.host().ext_mut(),
            cost.saturating_sub(gas.reservoir()),
        );
        gas.erase_cost(drawn);
    }
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
        spend_history_withheld(gas, state, amount)?;
    }
    state.host().ext_mut().book_history(amount as i64, bytes as i64);
    Ok(())
}

/// Gives `bytes` of history back to the running frame, by the version's give-back rule: into its
/// reservoir alone under Satin's (`SATIN-HIST-012`).
pub fn refill_history(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>, bytes: u64) {
    if !state.host().ext().prices_history || bytes == 0 {
        return;
    }
    evm2::interpreter::instructions::give_back_state_gas(gas, state, bytes * CPHB);
    state.host().ext_mut().book_history(-((bytes * CPHB) as i64), -(bytes as i64));
}
