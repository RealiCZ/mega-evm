//! Runs a shared scenario through `Evm::transact` on the Satin engine and renders the outcome
//! in the oracle's JSON shape, with the read set recorded below the journal.

#[path = "../shared/scenarios.rs"]
pub mod scenarios;

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use evm2::{
    Evm, ExecutionConfig, TxResult,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Database, InMemoryDB},
    inspector::Inspector,
    interpreter::{InstrStop, Interpreter, Message, MessageKind, MessageResult, Word},
};
use mega_evm2::{MegaSpec, MegaTypes, ledger::*};
use op_evm2::{OpTxEnvelope, TxDeposit};
use scenarios::Scenario;
use serde_json::{Map, Value, json};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

pub const DEFAULT_LIMITS: [u64; 3] = [13_107_200, 500_000, 1_000];
pub const DEFAULT_DETENTION_CAP: u64 = 20_000_000;

fn hex(b: &[u8]) -> String {
    let mut s = String::from("0x");
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

pub fn status_of(stop: InstrStop) -> String {
    if stop.is_success() {
        "success".into()
    } else if stop.is_frame_revert() {
        "revert".into()
    } else {
        "halt".into()
    }
}

/// The `status` and `halt` fields of an outcome. op-revm reports every failed deposit as
/// `Halt(FailedDeposit)`.
pub fn status_halt(r: &TxResult<MegaTypes>) -> (Value, Value) {
    let status = if r.ext.failed_deposit { json!("halt") } else { json!(status_of(r.stop)) };
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
            accounts.insert(format!("{}", c.address), json!(format!("{b}/n{n}/{h}")));
        }
        if !c.selfdestructed {
            for (k, v) in &c.slots {
                storage.insert(format!("{}:{k:#x}", c.address), json!(format!("{v:#x}")));
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
    if let Some((_, scalar, _)) = &s.l1_fee {
        db.insert_account_storage(
            &scenarios::L1_BLOCK,
            &U256::from(3),
            &(U256::from(*scalar) << 96),
        );
    }
    db
}

pub fn evm(s: &Scenario, reads: Rc<RefCell<Reads>>) -> Evm<'static, MegaTypes> {
    let config = ExecutionConfig::<MegaTypes>::for_spec_and_version(
        MegaSpec::Satin,
        mega_evm2::satin_version(),
    );
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(scenarios::BLOCK_GAS_LIMIT),
        slot_num: U256::from(42),
        beneficiary: s.beneficiary,
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        mega_evm2::tx::registry(),
        evm2::evm::Db::new(RecordingDb { inner: db(s), reads }),
        mega_evm2::precompiles(),
    );
    // Read storage of an account the database lacks, as revm does.
    evm.state_mut().set_read_absent_storage(!mega_evm2::off("absent_storage_read"));
    let ext = evm.ext_mut();
    ext.cfg.limits = [
        s.limits.data.unwrap_or(DEFAULT_LIMITS[0]),
        s.limits.kv.unwrap_or(DEFAULT_LIMITS[1]),
        s.limits.growth.unwrap_or(DEFAULT_LIMITS[2]),
    ];
    ext.cfg.cap = s.limits.block_env_cap.unwrap_or(DEFAULT_DETENTION_CAP);
    ext.cfg.oracle_cap = s.limits.oracle_cap.unwrap_or(DEFAULT_DETENTION_CAP);
    for (k, v) in &s.oracle {
        ext.cfg.oracle.insert(U256::from(*k), U256::from(*v));
    }
    for (a, k, m) in &s.salt {
        let id = match k {
            Some(k) => bucket_for(evm2::version::StateGasSite::Storage(*a, U256::from(*k))),
            None => bucket_for(evm2::version::StateGasSite::Account(*a)),
        };
        ext.cfg.buckets.insert(id, MIN_BUCKET_SIZE * m);
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
                nonce: 0,
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
        None => OpTxEnvelope::new(
            TxLegacy {
                chain_id: None,
                nonce: 0,
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

/// Runs `s` (plain, or with the frame-recording inspector) and renders the outcome.
pub fn run(s: &Scenario, inspected: bool) -> Value {
    let reads = Rc::new(RefCell::new(Reads::default()));
    let mut evm = evm(s, reads.clone());
    if inspected {
        evm.set_inspector(Frames::default());
    }
    let tx = tx(s);
    let (r, changes) = {
        let executed = match evm.transact(&tx) {
            Ok(e) => e,
            Err(e) => return json!({"name": s.name, "group": s.group, "error": format!("{e:?}")}),
        };
        let changes = collect_changes(&executed);
        (executed.commit(), changes)
    };
    let frames = if inspected {
        evm.clear_inspector_as::<Frames>().map(|f| f.frames.clone()).unwrap_or_default()
    } else {
        Vec::new()
    };
    let ext: &Lite = evm.ext();
    let mut out = render_outcome(&r, ext, changed_json(&changes));
    let rd = reads.borrow();
    let read_set = json!({
        "accounts": rd.accounts.iter().map(|a| format!("{a}")).collect::<Vec<_>>(),
        "storage": rd.storage.iter().map(|(a, k)| format!("{a}:{k:#x}")).collect::<Vec<_>>(),
        "codes": rd.codes.iter().map(|h| format!("{h}")).collect::<Vec<_>>(),
        "block_hashes": rd.block_hashes.iter().collect::<Vec<_>>(),
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
    Value::Object(out)
}
