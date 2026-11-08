//! The reference side of the synthetic gate and of the probe: runs one shared scenario
//! (`mega_evm2_scenarios`) on the Satin implementation under review, plain and with a recording
//! inspector, over a recording database and SALT/Oracle environments, and renders the outcome
//! record (`mega_evm2_scenarios::record`).

use alloy_evm::Evm as _;
use alloy_op_evm::OpTx;
use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use mega_evm::{
    EvmTxRuntimeLimits, ExternalEnvFactory, ExternalEnvs, LimitCheck, MegaContext, MegaEvm,
    MegaSpecId, MegaTransactionOutcome, SaltEnv, TestExternalEnvs,
    test_utils::{
        LedgerPools, MemoryDatabase, RecordingDatabase, RecordingEnvFactory, WitnessRecord,
        op_transaction, record_start_ledger_settlements, take_start_ledger_settlements,
    },
};
use mega_evm2_scenarios::{self as scenarios, Scenario, record, record::hex};
use revm::{
    Database, DatabaseCommit, Inspector,
    context::{BlockEnv, TxEnv},
    context_interface::ContextTr,
    context_interface::result::ExecutionResult,
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, InstructionResult,
        interpreter::EthInterpreter,
    },
};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc};

fn status_of(r: InstructionResult) -> String {
    if r.is_ok() {
        record::SUCCESS.into()
    } else if r.is_revert() {
        record::REVERT.into()
    } else {
        record::HALT.into()
    }
}

#[derive(Default)]
struct Frames {
    stack: Vec<usize>,
    frames: Vec<Value>,
}

impl Frames {
    fn open(&mut self, kind: &str, gas_limit: u64, reservoir: u64, value: U256) {
        let level = self.stack.len();
        self.stack.push(self.frames.len());
        self.frames.push(json!({
            "kind": kind, "level": level, "gas_limit": gas_limit, "reservoir": reservoir,
            "value": value.to_string(),
        }));
    }
    fn close(&mut self, r: InstructionResult, output: &Bytes, gas_left: u64, reservoir_left: u64) {
        let i = self.stack.pop().expect("open frame");
        let f = self.frames[i].as_object_mut().unwrap();
        f.insert("status".into(), json!(status_of(r)));
        f.insert("output".into(), json!(hex(output)));
        f.insert("gas_left".into(), json!(gas_left));
        f.insert("reservoir_left".into(), json!(reservoir_left));
    }
}

impl<DB: Database> Inspector<MegaContext<DB, Envs>, EthInterpreter> for Frames {
    fn call(
        &mut self,
        _: &mut MegaContext<DB, Envs>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        let kind = format!("{:?}", inputs.scheme);
        self.open(&kind, inputs.gas_limit, inputs.reservoir, inputs.call_value());
        None
    }
    fn call_end(
        &mut self,
        _: &mut MegaContext<DB, Envs>,
        _: &CallInputs,
        outcome: &mut CallOutcome,
    ) {
        let g = outcome.result.gas;
        self.close(outcome.result.result, &outcome.result.output, g.remaining(), g.reservoir());
    }
    fn create(
        &mut self,
        _: &mut MegaContext<DB, Envs>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        self.open("Create", inputs.gas_limit(), inputs.reservoir(), inputs.value());
        None
    }
    fn create_end(
        &mut self,
        _: &mut MegaContext<DB, Envs>,
        _: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        let g = outcome.result.gas;
        self.close(outcome.result.result, &outcome.result.output, g.remaining(), g.reservoir());
    }
}

type Test = TestExternalEnvs<String>;
type Envs =
    (mega_evm::test_utils::RecordingSaltEnv<Test>, mega_evm::test_utils::RecordingOracleEnv<Test>);

#[derive(Clone)]
struct Fixed(Test);

impl ExternalEnvFactory for Fixed {
    type EnvTypes = (Test, Test);
    fn external_envs(&self, _block: u64) -> ExternalEnvs<Self::EnvTypes> {
        ExternalEnvs { salt_env: self.0.clone(), oracle_env: self.0.clone() }
    }
}

fn db(s: &Scenario) -> MemoryDatabase {
    let mut db = MemoryDatabase::default();
    for a in &s.accounts {
        if a.balance > U256::ZERO {
            db = db.account_balance(a.address, a.balance);
        }
        if !a.code.is_empty() {
            db = db.account_code(a.address, Bytes::from(a.code.clone()));
        }
        if a.nonce > 0 {
            db = db.account_nonce(a.address, a.nonce);
        }
        for (k, v) in &a.storage {
            db = db.account_storage(a.address, U256::from(*k), U256::from(*v));
        }
    }
    // The L1 block info: slot 1 is the L1 base fee, slot 3 holds the Ecotone base fee scalar in
    // bits 96..128. Written after the accounts' own storage, so `l1_fee` decides both slots.
    if let Some((base_fee, scalar, _)) = &s.l1_fee {
        db = db.account_storage(scenarios::L1_BLOCK, U256::from(1), U256::from(*base_fee));
        db = db.account_storage(scenarios::L1_BLOCK, U256::from(3), U256::from(*scalar) << 96);
    }
    // A 23-byte `0xef0100 || address` is installed as an EIP-7702 delegation (the builder's
    // `account_code` makes legacy code of it), as on a chain whose authority was delegated.
    for a in &s.accounts {
        if a.code.len() == 23 && a.code[..3] == [0xef, 0x01, 0x00] {
            let code = revm::state::Bytecode::new_eip7702(Address::from_slice(&a.code[3..]));
            let info = revm::state::AccountInfo::new(a.balance, a.nonce, code.hash_slow(), code);
            db.insert_account_info(a.address, info);
        }
    }
    db
}

fn limits(s: &Scenario) -> EvmTxRuntimeLimits {
    let mut l = EvmTxRuntimeLimits::default();
    if let Some(v) = s.limits.data {
        l.tx_data_size_limit = v;
    }
    if let Some(v) = s.limits.kv {
        l.tx_kv_update_limit = v;
    }
    if let Some(v) = s.limits.growth {
        l.tx_state_growth_limit = v;
    }
    if let Some(v) = s.limits.block_env_cap {
        l.block_env_access_compute_gas_limit = v;
    }
    if let Some(v) = s.limits.oracle_cap {
        l.oracle_access_compute_gas_limit = v;
    }
    l
}

// The scenarios' SALT unit is the reference's minimum bucket.
const _: () = assert!(scenarios::MIN_BUCKET == mega_evm::MIN_BUCKET_SIZE as u64);

fn envs(s: &Scenario) -> Test {
    let mut env = Test::new();
    for (a, k, m) in &s.salt {
        let id = match k {
            Some(k) => <Test as SaltEnv>::bucket_id_for_slot(*a, U256::from(*k)),
            None => <Test as SaltEnv>::bucket_id_for_account(*a),
        };
        let capacity = scenarios::salt_capacity(*m)
            .unwrap_or_else(|| panic!("{}: {m} minimum buckets is not a u64 capacity", s.name));
        env = env.with_bucket_capacity(id, capacity);
    }
    for (a, k, capacity) in &s.salt_capacity {
        let id = match k {
            Some(k) => <Test as SaltEnv>::bucket_id_for_slot(*a, U256::from(*k)),
            None => <Test as SaltEnv>::bucket_id_for_account(*a),
        };
        env = env.with_bucket_capacity(id, *capacity);
    }
    for (k, v) in &s.oracle {
        env = env.with_oracle_storage(U256::from(*k), U256::from(*v));
    }
    env
}

fn context(
    s: &Scenario,
    record: Rc<RefCell<WitnessRecord>>,
    env: Test,
) -> MegaContext<RecordingDatabase<MemoryDatabase>, Envs> {
    let factory = RecordingEnvFactory::new(Fixed(env), record.clone());
    let ext = factory.external_envs(1);
    let ctx = MegaContext::new_with_external_envs(
        RecordingDatabase::new(db(s), record),
        MegaSpecId::SATIN,
        ext,
    );
    // The node's simulation switches (`Scenario::skip_nonce_check`, `skip_eip3607`).
    let ctx = if s.skip_nonce_check || s.skip_eip3607 {
        let mut cfg = ctx.mega_cfg().clone();
        cfg.disable_nonce_check = s.skip_nonce_check;
        cfg.disable_eip3607 = s.skip_eip3607;
        ctx.with_cfg(cfg)
    } else {
        ctx
    };
    ctx.with_block(BlockEnv {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: scenarios::BLOCK_GAS_LIMIT,
        slot_num: 42,
        beneficiary: s.beneficiary,
        ..Default::default()
    })
    .with_tx_runtime_limits(limits(s))
}

fn tx(s: &Scenario) -> mega_evm::MegaTransaction {
    if !s.auths.is_empty() {
        // A type-4 transaction with pre-recovered authorities (Satin's own scenario format).
        use alloy_eips::eip7702::{Authorization, RecoveredAuthority, RecoveredAuthorization};
        let auths: Vec<RecoveredAuthorization> = s
            .auths
            .iter()
            .map(|a| {
                RecoveredAuthorization::new_unchecked(
                    Authorization {
                        chain_id: U256::from(a.chain_id),
                        address: a.address,
                        nonce: a.nonce,
                    },
                    a.authority.map_or(RecoveredAuthority::Invalid, RecoveredAuthority::Valid),
                )
            })
            .collect();
        let env = TxEnv::builder()
            .tx_type(Some(4))
            .caller(s.sender)
            .kind(TxKind::Call(s.to))
            .data(Bytes::from(s.data.clone()))
            .value(s.value)
            .gas_limit(s.gas_limit)
            .gas_price(s.gas_price as u128)
            .gas_priority_fee(Some(s.gas_price as u128))
            .chain_id(Some(1))
            .nonce(s.nonce)
            .authorization_list_recovered(auths)
            .build_fill();
        let mut t = OpTx(op_transaction(env));
        t.0.enveloped_tx = Some(Bytes::new());
        return t;
    }
    let kind = if s.create { TxKind::Create } else { TxKind::Call(s.to) };
    if s.tx_type != 0 {
        // An EIP-2930 or EIP-1559 transaction: chain id 1, nonce 0, no access list.
        let env = TxEnv::builder()
            .tx_type(Some(s.tx_type))
            .caller(s.sender)
            .kind(kind)
            .data(Bytes::from(s.data.clone()))
            .value(s.value)
            .gas_limit(s.gas_limit)
            .gas_price(s.gas_price as u128)
            .gas_priority_fee((s.tx_type == 2).then_some(s.gas_price as u128))
            .chain_id(Some(1))
            .nonce(s.nonce)
            .build_fill();
        let mut t = OpTx(op_transaction(env));
        t.0.enveloped_tx = Some(Bytes::new());
        return t;
    }
    let mut t = OpTx(op_transaction(TxEnv {
        caller: s.sender,
        nonce: if s.deposit.is_some() { 0 } else { s.nonce },
        kind,
        data: Bytes::from(s.data.clone()),
        value: s.value,
        gas_limit: s.gas_limit,
        gas_price: s.gas_price as u128,
        chain_id: if s.deposit.is_some() { Some(1) } else { s.chain_id },
        ..Default::default()
    }));
    if let Some((_, _, bytes)) = &s.l1_fee {
        t.0.enveloped_tx = Some(Bytes::from(bytes.clone()));
    }
    if let Some(mint) = s.deposit {
        t.0.deposit.source_hash = B256::with_last_byte(0x42);
        t.0.deposit.is_system_transaction = false;
        t.0.deposit.mint = Some(mint as u128);
        t.0.enveloped_tx = None;
    }
    t
}

fn pools(p: &LedgerPools) -> Value {
    let nums: Vec<u64> = format!("{:?}", p.remaining)
        .split(|c: char| !c.is_ascii_digit())
        .filter(|t| !t.is_empty())
        .map(|t| t.parse().unwrap())
        .collect();
    json!({ "remaining": nums, "net": p.net })
}

/// The changed accounts and slots of an outcome, as the record writes them: every touched account
/// whose info changed (a destroyed one as zero) and every slot whose value changed.
fn changed(o: &MegaTransactionOutcome) -> (serde_json::Map<String, Value>, serde_json::Map<String, Value>) {
    let mut accounts = serde_json::Map::new();
    let mut storage = serde_json::Map::new();
    for (a, acc) in o.state.iter() {
        let orig = acc.original_info();
        let changed = orig.balance != acc.info.balance ||
            orig.nonce != acc.info.nonce ||
            orig.code_hash != acc.info.code_hash ||
            acc.is_selfdestructed();
        if acc.is_touched() && changed {
            let code_hash = if acc.is_selfdestructed() { B256::ZERO } else { acc.info.code_hash };
            let (b, n) = if acc.is_selfdestructed() {
                (U256::ZERO, 0)
            } else {
                (acc.info.balance, acc.info.nonce)
            };
            accounts.insert(format!("{a}"), json!(record::account(b, n, code_hash)));
        }
        for (k, slot) in acc.storage.iter() {
            if slot.present_value != slot.original_value && !acc.is_selfdestructed() {
                storage.insert(
                    record::slot_key(*a, *k),
                    json!(record::slot_value(slot.present_value)),
                );
            }
        }
    }
    (accounts, storage)
}

fn outcome_json(s: &Scenario, o: &MegaTransactionOutcome, rec: &WitnessRecord) -> Value {
    let (status, output) = match &o.result {
        ExecutionResult::Success { output, .. } => {
            (record::SUCCESS.to_string(), output.data().clone())
        }
        ExecutionResult::Revert { output, .. } => (record::REVERT.to_string(), output.clone()),
        ExecutionResult::Halt { .. } => (record::HALT.to_string(), Bytes::new()),
    };
    let (accounts, storage) = changed(o);
    let logs: Vec<Value> = o
        .result
        .logs()
        .iter()
        .map(|l| json!({"address": format!("{}", l.address), "topics": l.topics().iter().map(|t| format!("{t}")).collect::<Vec<_>>(), "data": hex(&l.data.data)}))
        .collect();
    let limit = match o.limit_exceeded {
        Some(LimitCheck::ExceedsLimit { kind, limit, .. }) => {
            json!(format!("{}:{limit}", kind as u8))
        }
        _ => Value::Null,
    };
    // What the database served is the first transaction of a block's alone (`Scenario::prior`):
    // the engine's state keeps what an earlier transaction loaded, so a later transaction asks its
    // database for less there than here. The SALT buckets and the Oracle reads are per
    // transaction on both sides.
    let served = s.prior.is_empty();
    let reads = json!({
        "accounts": rec.accounts.keys().filter(|_| served).map(|a| format!("{a}")).collect::<Vec<_>>(),
        "storage": rec.storage.keys().filter(|_| served).map(|(a, k)| record::slot_key(*a, *k)).collect::<Vec<_>>(),
        "codes": rec.codes.keys().filter(|_| served).map(|h| format!("{h}")).collect::<Vec<_>>(),
        "block_hashes": rec.block_hashes.keys().filter(|_| served).collect::<Vec<_>>(),
        "buckets": rec.buckets.keys().collect::<Vec<_>>(),
        "oracle_reads": rec.oracle_reads.len(),
    });
    json!({
        "name": s.name,
        "group": s.group,
        "status": status,
        "halt": match &o.result { ExecutionResult::Halt { reason, .. } => json!(format!("{reason:?}")), _ => Value::Null },
        "output": hex(&output),
        "gas_used": o.result.tx_gas_used(),
        "regular": o.gas.regular,
        "state": o.gas.state,
        "history": o.gas.history,
        "history_bytes": o.gas.history_bytes,
        "reservoir_remaining": o.gas.reservoir_remaining,
        "data_size": o.usage.data_size,
        "kv": o.usage.write_records,
        "growth": o.usage.state_growth,
        "limit_exceeded": limit,
        "logs": logs,
        "accounts": accounts,
        "storage": storage,
        "reads": reads,
    })
}

fn run_once(s: &Scenario, inspected: bool) -> (Value, Vec<Value>) {
    let record = Rc::new(RefCell::new(WitnessRecord::default()));
    let env = envs(s);
    let mut evm =
        MegaEvm::new(context(s, record.clone(), env.clone())).with_inspector(Frames::default());
    evm.set_inspector_enabled(inspected);
    record_start_ledger_settlements(evm.ctx_mut());
    // The transactions of the block that ran before `s`: each runs on this EVM and its outcome is
    // committed; a refused one leaves nothing. What they changed is carried into the record.
    let mut block_accounts = serde_json::Map::new();
    let mut block_storage = serde_json::Map::new();
    for p in &s.prior {
        if let Ok(o) = evm.execute_transaction(tx(p)) {
            let (a, st) = changed(&o);
            block_accounts.extend(a);
            block_storage.extend(st);
            evm.ctx_mut().db_mut().commit(o.state.clone());
        }
        take_start_ledger_settlements(evm.ctx_mut());
        *evm.inspector_mut() = Frames::default();
    }
    let hints_before = env.recorded_hints().len();
    // What the transaction reads is its own: the record starts empty.
    *record.borrow_mut() = WitnessRecord::default();
    let result = evm.execute_transaction(tx(s));
    let frames = evm.inspector().frames.clone();
    let ledger: Vec<Value> = take_start_ledger_settlements(evm.ctx_mut()).iter()
        .map(|l| json!({"depth":l.depth,"frame":pools(&l.frame),"caller":pools(&l.caller),"refund":l.refund})).collect();
    let mut out = match &result {
        Ok(o) => outcome_json(s, o, &record.borrow()),
        Err(e) => json!({"name":s.name,"group":s.group,"error":format!("{e:?}")}),
    };
    if out.get("error").is_none() {
        let hints: Vec<Value> = env
            .recorded_hints()
            .iter()
            .skip(hints_before)
            .map(|h| json!([format!("{}", h.from), format!("{}", h.topic), hex(&h.data)]))
            .collect();
        out.as_object_mut().unwrap().insert("hints".into(), json!(hints));
        out.as_object_mut().unwrap().insert("ledger".into(), json!(ledger));
        if !s.prior.is_empty() {
            let (a, st) = changed(result.as_ref().unwrap());
            block_accounts.extend(a);
            block_storage.extend(st);
            out.as_object_mut().unwrap().insert("block_accounts".into(), block_accounts.into());
            out.as_object_mut().unwrap().insert("block_storage".into(), block_storage.into());
        }
    }
    (out, frames)
}

/// Runs `s` plain and inspected and renders its outcome record: the plain run's, with
/// `inspected_equal`, the inspected run's record when the two differ, and the inspected run's
/// frames.
pub fn run(s: &Scenario) -> Value {
    let (mut plain, _) = run_once(s, false);
    let (inspected, frames) = run_once(s, true);
    let same = plain == inspected;
    let o = plain.as_object_mut().unwrap();
    o.insert("inspected_equal".into(), json!(same));
    if !same {
        o.insert("inspected".into(), inspected);
    }
    o.insert("frames".into(), json!(frames));
    if let Err(e) = record::check_fields(record::Harness::Reference, o.keys().map(String::as_str)) {
        panic!("{}: {e}", s.name);
    }
    plain
}
