//! Runs a shared scenario through `Evm::transact` on the Satin engine and renders the outcome
//! in the oracle's JSON shape, with the read set recorded below the journal.

pub use mega_evm2_scenarios as scenarios;

use crate::invariant;
use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use evm2::{
    Evm, TxResult,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Database, InMemoryDB},
    inspector::Inspector,
    interpreter::{InstrStop, Interpreter, Message, MessageKind, MessageResult, Word},
};
use mega_evm2::{MegaTypes, ledger::*};
use op_evm2::{OpTxEnvelope, TxDeposit};
use scenarios::{Scenario, record, record::hex};

// The scenarios' SALT unit is the engine's minimum bucket.
const _: () = assert!(scenarios::MIN_BUCKET == MIN_BUCKET_SIZE);
use serde_json::{Map, Value, json};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

/// Satin's transaction limits and detention cap, which a scenario's own `limits` override.
pub const DEFAULT_LIMITS: [u64; 3] = mega_evm2::TxLimits::SATIN.tx;
pub const DEFAULT_DETENTION_CAP: u64 = mega_evm2::TxLimits::SATIN.cap;

pub fn status_of(stop: InstrStop) -> String {
    if stop.is_success() {
        record::SUCCESS.into()
    } else if stop.is_frame_revert() {
        record::REVERT.into()
    } else {
        record::HALT.into()
    }
}

/// The `status` and `halt` fields of an outcome. op-revm reports every failed deposit as
/// `Halt(FailedDeposit)`.
pub fn status_halt(r: &TxResult<MegaTypes>) -> (Value, Value) {
    let status = if r.ext.failed_deposit { json!(record::HALT) } else { json!(status_of(r.stop)) };
    let halt = if r.ext.failed_deposit {
        json!("FailedDeposit")
    } else if r.stop.is_halt() {
        json!(format!("{:?}", r.stop))
    } else {
        Value::Null
    };
    (status, halt)
}

/// A changed account of an executed transaction: its state before and after, its lifecycle flags
/// and the slots it changed (key, current value).
pub struct Change {
    pub address: Address,
    pub original: Option<AccountInfo>,
    pub current: Option<AccountInfo>,
    pub touched: bool,
    pub selfdestructed: bool,
    pub created: bool,
    pub slots: Vec<(U256, U256)>,
}

/// The account changes of an executed transaction, taken before it is committed.
pub fn collect_changes(executed: &evm2::evm::ExecutedTx<'_, '_, MegaTypes>) -> Vec<Change> {
    executed
        .account_changes()
        .map(|c| Change {
            address: c.address,
            original: c.original.cloned(),
            current: c.current.cloned(),
            touched: c.touched,
            selfdestructed: c.selfdestructed,
            created: c.created,
            slots: c.storage.changed_slots().map(|(k, s)| (*k, s.current)).collect(),
        })
        .collect()
}

/// The `accounts` and `storage` fields: every touched account whose balance, nonce or code hash
/// changed (destroyed ones rendered as zero, as the reference does), and every changed slot of an
/// account that was not destroyed.
pub fn changed_json(changes: &[Change]) -> (Map<String, Value>, Map<String, Value>) {
    let mut accounts = Map::new();
    let mut storage = Map::new();
    for c in changes {
        let orig = c.original.clone().unwrap_or_default();
        let cur = c.current.clone().unwrap_or_default();
        let changed = (orig.balance, orig.nonce, orig.code_hash) !=
            (cur.balance, cur.nonce, cur.code_hash) ||
            c.selfdestructed;
        if c.touched && changed {
            let (b, n, h) = if c.selfdestructed || c.current.is_none() {
                (U256::ZERO, 0, B256::ZERO)
            } else {
                (cur.balance, cur.nonce, cur.code_hash)
            };
            accounts.insert(format!("{}", c.address), json!(record::account(b, n, h)));
        }
        if !c.selfdestructed {
            for (k, v) in &c.slots {
                storage.insert(record::slot_key(c.address, *k), json!(record::slot_value(*v)));
            }
        }
    }
    (accounts, storage)
}

/// The fields a synthetic run and a replay run report identically, in the reference's JSON shape.
pub fn render_outcome(
    r: &TxResult<MegaTypes>,
    ext: &Lite,
    changed: (Map<String, Value>, Map<String, Value>),
) -> Map<String, Value> {
    let usage = ext.usage(r.status);
    let (history, history_bytes) = ext.history(r.status);
    let limit = match ext.top_stop {
        Some((k, l))
            if !r.status && r.output.as_ref() == limit_exceeded_output(k, l).as_slice() =>
        {
            json!(format!("{k}:{l}"))
        }
        _ => Value::Null,
    };
    let total_state = r.state_gas_spent;
    let state = total_state.saturating_sub(history);
    let regular = r.total_gas_spent.saturating_sub(total_state);
    let (accounts, storage) = changed;
    let (status, halt) = status_halt(r);
    let mut o = Map::new();
    for (k, v) in [
        ("status", status),
        ("halt", halt),
        ("output", json!(hex(&r.output))),
        ("gas_used", json!(r.tx_gas_used())),
        ("regular", json!(regular)),
        ("state", json!(state)),
        ("history", json!(history)),
        ("history_bytes", json!(history_bytes)),
        ("reservoir_remaining", json!(ext.reservoir_left)),
        ("data_size", json!(usage[0])),
        ("kv", json!(usage[1])),
        ("growth", json!(usage[2])),
        ("limit_exceeded", limit),
        ("logs", json!(r.logs.iter().map(|l| json!({"address": format!("{}", l.address), "topics": l.topics().iter().map(|t| format!("{t}")).collect::<Vec<_>>(), "data": hex(&l.data.data)})).collect::<Vec<_>>())),
        ("accounts", Value::Object(accounts)),
        ("storage", Value::Object(storage)),
        ("ledger", json!(ext.settlements.iter().map(|l| json!({"depth": l.depth, "frame": {"remaining": l.frame_remaining, "net": l.frame_net}, "caller": {"remaining": l.caller_remaining, "net": l.caller_net}, "refund": l.refund})).collect::<Vec<_>>())),
    ] {
        o.insert(k.into(), v);
    }
    o
}

/// What the database served (Satin's `RecordingDatabase` keys).
#[derive(Default, Debug, Clone)]
pub struct Reads {
    pub accounts: BTreeSet<Address>,
    pub storage: BTreeSet<(Address, U256)>,
    pub codes: BTreeSet<B256>,
    pub block_hashes: BTreeSet<u64>,
}

#[derive(Debug)]
pub struct RecordingDb {
    pub inner: InMemoryDB,
    pub reads: Rc<RefCell<Reads>>,
}

impl Database for RecordingDb {
    type Error = <InMemoryDB as Database>::Error;
    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.reads.borrow_mut().accounts.insert(*address);
        let info = self.inner.get_account(address)?;
        // Satin's recorder serves code lazily: every code the engine loads is a code read.
        Ok(info)
    }
    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        self.reads.borrow_mut().codes.insert(*code_hash);
        self.inner.get_code_by_hash(code_hash)
    }
    fn get_storage(&mut self, address: &Address, key: &Word) -> Result<Word, Self::Error> {
        self.reads.borrow_mut().storage.insert((*address, *key));
        self.inner.get_storage(address, key)
    }
    fn get_block_hash(&mut self, number: &Word) -> Result<B256, Self::Error> {
        self.reads.borrow_mut().block_hashes.insert(number.to::<u64>());
        self.inner.get_block_hash(number)
    }
}

/// Records every message the way the oracle's inspector does.
#[derive(Default, Debug)]
pub struct Frames {
    stack: Vec<usize>,
    pub frames: Vec<Value>,
}

impl Frames {
    fn open(&mut self, m: &Message<MegaTypes>) {
        let kind = match m.kind {
            MessageKind::Call => "Call",
            MessageKind::CallCode => "CallCode",
            MessageKind::DelegateCall => "DelegateCall",
            MessageKind::StaticCall => "StaticCall",
            _ => "Create",
        };
        let level = self.stack.len();
        self.stack.push(self.frames.len());
        self.frames.push(json!({
            "kind": kind, "level": level, "gas_limit": m.gas_limit, "reservoir": m.reservoir,
            "value": m.value.to_string(),
        }));
    }
    fn close(&mut self, r: &MessageResult<MegaTypes>) {
        let i = self.stack.pop().expect("open frame");
        let f = self.frames[i].as_object_mut().unwrap();
        f.insert("status".into(), json!(status_of(r.stop)));
        f.insert("output".into(), json!(hex(&r.output)));
        f.insert("gas_left".into(), json!(r.gas.remaining()));
        f.insert("reservoir_left".into(), json!(r.gas.reservoir()));
    }
}

impl Inspector<MegaTypes> for Frames {
    fn call(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        m: &mut Message<MegaTypes>,
    ) -> Option<MessageResult<MegaTypes>> {
        self.open(m);
        None
    }
    fn call_end(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        _: &Message<MegaTypes>,
        r: &mut MessageResult<MegaTypes>,
    ) {
        self.close(r);
    }
    fn create(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        m: &mut Message<MegaTypes>,
    ) -> Option<MessageResult<MegaTypes>> {
        self.open(m);
        None
    }
    fn create_end(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        _: &Message<MegaTypes>,
        r: &mut MessageResult<MegaTypes>,
    ) {
        self.close(r);
    }
}

pub fn db(s: &Scenario) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    for a in &s.accounts {
        // The reference's `MemoryDatabase` builder writes nothing for an account with no balance,
        // no code, nonce 0 and no storage: such an account is absent there, so it is here
        // too.
        if a.balance.is_zero() &&
            a.code.is_empty() &&
            a.nonce == 0 &&
            a.storage.is_empty() &&
            !mega_evm2::off("empty_fixture_account")
        {
            continue;
        }
        let mut info = AccountInfo::default().with_balance(a.balance);
        info.nonce = a.nonce;
        if !a.code.is_empty() {
            // A 23-byte `0xef0100 || address` is an EIP-7702 delegation, as revm's
            // `Bytecode::new_raw` (Satin's `MemoryDatabase`) reads it.
            let code = if a.code.len() == 23 &&
                a.code[..3] == [0xef, 0x01, 0x00] &&
                !mega_evm2::off("fixture_delegation")
            {
                Bytecode::new_eip7702(alloy_primitives::Address::from_slice(&a.code[3..]))
            } else {
                Bytecode::new_legacy(Bytes::from(a.code.clone()))
            };
            info = info.with_code(code);
        }
        db.insert_account_info(&a.address, info);
        for (k, v) in &a.storage {
            db.insert_account_storage(&a.address, &U256::from(*k), &U256::from(*v));
        }
    }
    // The L1 block info: slot 1 is the L1 base fee, slot 3 holds the Ecotone base fee scalar in
    // bits 96..128. Written after the accounts' own storage, so `l1_fee` decides both slots.
    if let Some((base_fee, scalar, _)) = &s.l1_fee {
        db.insert_account_storage(&scenarios::L1_BLOCK, &U256::from(1), &U256::from(*base_fee));
        db.insert_account_storage(
            &scenarios::L1_BLOCK,
            &U256::from(3),
            &(U256::from(*scalar) << 96),
        );
    }
    db
}

pub fn evm(s: &Scenario, reads: Rc<RefCell<Reads>>) -> Evm<'static, MegaTypes> {
    evm_under(s, reads, &[])
}

/// [`evm`] with the engine under the reference's `rules`: the version
/// [`mega_evm2::satin_version_under`] builds (the give-back rule is the version's), and
/// `Lite::reference_rules` all of them.
fn evm_under(
    s: &Scenario,
    reads: Rc<RefCell<Reads>>,
    rules: &[ReferenceRule],
) -> Evm<'static, MegaTypes> {
    // The node's simulation switches (`Scenario::skip_nonce_check`, `skip_eip3607`), and a
    // scenario's own limits: a test's explicit override of Satin's (`TxLimits::SATIN`), as the
    // reference's harness overrides its spec's limits.
    let simulation = mega_evm2::factory::Simulation {
        skip_nonce_check: s.skip_nonce_check,
        skip_eip3607: s.skip_eip3607,
        limits: Some(mega_evm2::TxLimits {
            tx: [
                s.limits.data.unwrap_or(DEFAULT_LIMITS[0]),
                s.limits.kv.unwrap_or(DEFAULT_LIMITS[1]),
                s.limits.growth.unwrap_or(DEFAULT_LIMITS[2]),
            ],
            cap: s.limits.block_env_cap.unwrap_or(DEFAULT_DETENTION_CAP),
            oracle_cap: s.limits.oracle_cap.unwrap_or(mega_evm2::TxLimits::SATIN.oracle_cap),
        }),
        reference_rules: rules.to_vec(),
    };
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(scenarios::BLOCK_GAS_LIMIT),
        slot_num: U256::from(42),
        beneficiary: s.beneficiary,
        ..BlockEnv::<MegaTypes>::default()
    };
    let database = evm2::evm::Db::new(RecordingDb { inner: db(s), reads });
    // The scenarios sign for the chain id of Satin's version as the engine defines it.
    let mut evm = crate::replay::factory(mega_evm2::satin_version().chain_id)
        .simulated_evm(block, database, &simulation)
        .expect("a Satin block");
    // The harness's fixture switch: off, evm2 skips the storage read of an account the database
    // lacks (the factory has it read, as revm does).
    evm.state_mut().set_read_absent_storage(!mega_evm2::off("absent_storage_read"));
    let ext = evm.ext_mut();
    for (k, v) in &s.oracle {
        ext.cfg.oracle.insert(U256::from(*k), U256::from(*v));
    }
    for (a, k, m) in &s.salt {
        let id = match k {
            Some(k) => bucket_for(evm2::version::StateGasSite::Storage(*a, U256::from(*k))),
            None => bucket_for(evm2::version::StateGasSite::Account(*a)),
        };
        let capacity = scenarios::salt_capacity(*m)
            .unwrap_or_else(|| panic!("{}: {m} minimum buckets is not a u64 capacity", s.name));
        ext.cfg.buckets.insert(id, capacity);
    }
    for (a, k, capacity) in &s.salt_capacity {
        let id = match k {
            Some(k) => bucket_for(evm2::version::StateGasSite::Storage(*a, U256::from(*k))),
            None => bucket_for(evm2::version::StateGasSite::Account(*a)),
        };
        ext.cfg.buckets.insert(id, *capacity);
    }
    evm
}

pub fn tx(s: &Scenario) -> Recovered<OpTxEnvelope> {
    let to = if s.create { TxKind::Create } else { TxKind::Call(s.to) };
    let env = match s.deposit {
        Some(mint) => OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: s.sender,
            to,
            mint: mint as u128,
            value: s.value,
            gas_limit: s.gas_limit,
            is_system_transaction: false,
            input: Bytes::from(s.data.clone()),
        }),
        None if !s.auths.is_empty() => {
            use evm2::ethereum::LazyTxEip7702;
            let auths = s
                .auths
                .iter()
                .map(|a| {
                    let inner = alloy_eips::eip7702::Authorization {
                        chain_id: U256::from(a.chain_id),
                        address: a.address,
                        nonce: a.nonce,
                    };
                    alloy_eips::eip7702::SignedAuthorization::new_unchecked(
                        inner,
                        0,
                        U256::ZERO,
                        U256::ZERO,
                    )
                })
                .collect();
            let tx = alloy_consensus::TxEip7702 {
                chain_id: 1,
                nonce: s.nonce,
                gas_limit: s.gas_limit,
                max_fee_per_gas: s.gas_price as u128,
                max_priority_fee_per_gas: s.gas_price as u128,
                to: s.to,
                value: s.value,
                access_list: Default::default(),
                authorization_list: auths,
                input: Bytes::from(s.data.clone()),
            };
            OpTxEnvelope::new(
                LazyTxEip7702::from_cached_recovered_authorizations(
                    tx,
                    s.auths.iter().map(|a| a.authority).collect(),
                ),
                Some(Bytes::new()),
            )
        }
        None if s.tx_type == 1 => OpTxEnvelope::new(
            alloy_consensus::TxEip2930 {
                chain_id: 1,
                nonce: s.nonce,
                gas_price: s.gas_price as u128,
                gas_limit: s.gas_limit,
                to,
                value: s.value,
                access_list: Default::default(),
                input: Bytes::from(s.data.clone()),
            },
            Some(Bytes::new()),
        ),
        None if s.tx_type == 2 => OpTxEnvelope::new(
            alloy_consensus::TxEip1559 {
                chain_id: 1,
                nonce: s.nonce,
                gas_limit: s.gas_limit,
                max_fee_per_gas: s.gas_price as u128,
                max_priority_fee_per_gas: s.gas_price as u128,
                to,
                value: s.value,
                access_list: Default::default(),
                input: Bytes::from(s.data.clone()),
            },
            Some(Bytes::new()),
        ),
        None => OpTxEnvelope::new(
            TxLegacy {
                chain_id: s.chain_id,
                nonce: s.nonce,
                gas_price: s.gas_price as u128,
                gas_limit: s.gas_limit,
                to,
                value: s.value,
                input: Bytes::from(s.data.clone()),
            },
            Some(s.l1_fee.as_ref().map_or(Bytes::new(), |(_, _, b)| Bytes::from(b.clone()))),
        ),
    };
    Recovered::new_unchecked(env, s.sender)
}

/// Runs `s` (plain, or with the frame-recording inspector) and renders the outcome. The
/// transactions of its block that ran before it (`Scenario::prior`) run first on the same `Evm`,
/// each committed; the record is the last transaction's, with the block's changes so far.
pub fn run(s: &Scenario, inspected: bool) -> Value {
    run_with(s, inspected, &[])
}

/// Both runs of `s`, plain then inspected, with the engine under the reference's `rules`
/// (`Lite::reference_rules`): the runs a class of a change the reference has not made yet is
/// checked with (`gate::scenario_verdict`).
pub fn runs_under(s: &Scenario, rules: &[ReferenceRule]) -> (Value, Value) {
    (run_with(s, false, rules), run_with(s, true, rules))
}

fn run_with(s: &Scenario, inspected: bool, rules: &[ReferenceRule]) -> Value {
    let reads = Rc::new(RefCell::new(Reads::default()));
    let mut evm = evm_under(s, reads.clone(), rules);
    let mut block_accounts = Map::new();
    let mut block_storage = Map::new();
    // A run under Satin's rule is checked for the regular-gas invariant, every transaction of it;
    // a run under a reference's rule is not (`invariant`).
    let checked = rules.is_empty();
    let mut violations = Vec::new();
    for p in &s.prior {
        if inspected {
            evm.set_inspector(Frames::default());
        }
        let tx = tx(p);
        if let Ok(executed) = evm.transact(&tx) {
            let changes = collect_changes(&executed);
            let _ = executed.commit();
            let (a, st) = changed_json(&changes);
            block_accounts.extend(a);
            block_storage.extend(st);
        }
        if checked {
            violations.extend(invariant::violations(evm.ext()));
        }
    }
    if checked {
        invariant::ran();
    }
    if inspected {
        evm.set_inspector(Frames::default());
    }
    // What the transaction reads is its own: the read set starts empty.
    *reads.borrow_mut() = Reads::default();
    let tx = tx(s);
    let (r, changes) = {
        let executed = match evm.transact(&tx) {
            Ok(e) => e,
            Err(e) => {
                let mut out = json!({"name": s.name, "group": s.group, "error": format!("{e:?}")});
                invariant::note(out.as_object_mut().unwrap(), violations);
                return out;
            }
        };
        let changes = collect_changes(&executed);
        (executed.commit(), changes)
    };
    if checked {
        violations.extend(invariant::violations(evm.ext()));
    }
    let frames = if inspected {
        evm.clear_inspector_as::<Frames>().map(|f| f.frames.clone()).unwrap_or_default()
    } else {
        Vec::new()
    };
    let ext: &Lite = evm.ext();
    let (accounts, storage) = changed_json(&changes);
    let mut out = render_outcome(&r, ext, (accounts.clone(), storage.clone()));
    let rd = reads.borrow();
    // What the database served is the first transaction of a block's alone (`Scenario::prior`): a
    // later transaction finds in the engine's state what an earlier one loaded.
    let served = s.prior.is_empty();
    let read_set = json!({
        "accounts": rd.accounts.iter().filter(|_| served).map(|a| format!("{a}")).collect::<Vec<_>>(),
        "storage": rd.storage.iter().filter(|_| served).map(|(a, k)| record::slot_key(*a, *k)).collect::<Vec<_>>(),
        "codes": rd.codes.iter().filter(|_| served).map(|h| format!("{h}")).collect::<Vec<_>>(),
        "block_hashes": rd.block_hashes.iter().filter(|_| served).collect::<Vec<_>>(),
        "buckets": ext.bucket_reads.iter().collect::<Vec<_>>(),
        "oracle_reads": ext.oracle_reads,
    });
    drop(rd);
    for (k, v) in [
        ("name", json!(s.name)),
        ("group", json!(s.group)),
        ("reads", read_set),
        ("frames", json!(frames)),
        ("priced", json!(ext.detention.priced.iter().map(|p| json!(p)).collect::<Vec<_>>())),
        (
            "hints",
            json!(
                ext.mechanisms
                    .hints
                    .iter()
                    .map(|(f, t, d)| json!([format!("{f}"), format!("{t}"), hex(d)]))
                    .collect::<Vec<_>>()
            ),
        ),
    ] {
        out.insert(k.into(), v);
    }
    if !s.prior.is_empty() {
        block_accounts.extend(accounts);
        block_storage.extend(storage);
        out.insert("block_accounts".into(), Value::Object(block_accounts));
        out.insert("block_storage".into(), Value::Object(block_storage));
    }
    invariant::note(&mut out, violations);
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::{run, scenarios};
    use serde_json::json;
    use std::collections::BTreeSet;

    /// The L1 fee vault's balance after `q45_l1_fee_nonzero` with an L1 base fee of `base_fee`: the
    /// fixture writes `l1_fee`'s base fee into the L1Block's slot 1.
    fn l1_fee_paid(base_fee: u64) -> u128 {
        let mut s = scenarios::by_name("q45_l1_fee_nonzero").expect("q45");
        s.l1_fee.as_mut().expect("an L1 fee").0 = base_fee;
        scenarios::add_chain_accounts(&mut s);
        let o = run(&s, false);
        o["accounts"].get("0x420000000000000000000000000000000000001A").map_or(0, |a| {
            a.as_str().unwrap().split('/').next().unwrap().parse().expect("a balance")
        })
    }

    /// The L1 fee follows the L1 base fee `l1_fee` states: 1,600 times it for this transaction's
    /// 100 enveloped bytes at a scalar of 1,000,000, nothing at 0.
    #[test]
    fn test_the_l1_base_fee_sets_the_l1_fee() {
        assert_eq!(l1_fee_paid(0), 0);
        assert_eq!(l1_fee_paid(1_000_000_000), 1_600_000_000_000);
        assert_eq!(l1_fee_paid(1_000_000_000_000), 1_600_000_000_000_000);
    }

    /// Fuzz mode 7 reaches its class: of the depth-0 precompile calls whose gas limit the
    /// generator draws around what the call needs plus the price (not one of its ample limits),
    /// some run the precompile and some run out of gas in it, for several precompiles.
    #[test]
    fn test_precompile_mode_straddles_the_price() {
        let ample = [100_000u64, scenarios::BELOW_CAP, 100_000_000];
        let (mut ran, mut ran_out) = (BTreeSet::new(), BTreeSet::new());
        for s in scenarios::fz::generate(7, 1, 300) {
            if ample.contains(&s.gas_limit) || s.deposit.is_some() {
                continue;
            }
            let o = run(&s, false);
            if o.get("error").is_some() || !o["limit_exceeded"].is_null() {
                continue;
            }
            if o["status"] == "success" {
                ran.insert(s.to);
            } else if crate::gate::halt(&o["halt"]) == json!("PrecompileOOG") {
                ran_out.insert(s.to);
            }
        }
        assert!(ran.len() >= 5 && ran_out.len() >= 5, "ran {ran:?}, ran out {ran_out:?}");
    }
}
