//! The harness: a Satin transaction ([`Case`]) run through the engine's factory with any
//! inspector attached, what the run left ([`Ran`]: the result, the state and the engine's
//! ledgers), and every tracer and inspector of the inspector crate ([`Tracer`]) built into the JSON
//! a node returns for it.

use crate::common;
use alloy_consensus::{SignableTransaction, TxEip7702, TxLegacy, transaction::Recovered};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256, keccak256, map::HashSet};
use alloy_rpc_types_trace::{
    geth::{GethDebugTracingOptions, GethTrace},
    parity::TraceType,
};
use evm2::{
    Evm, TxResultWithState,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    inspector::Inspector,
    interpreter::op,
};
use evm2_inspectors::{
    access_list::AccessListInspector,
    opcode::OpcodeGasInspector,
    otterscan::InternalOperationsInspector,
    storage::StorageInspector,
    tracing::{DebugInspector, TracingInspector, TracingInspectorConfig, TransactionContext},
    transfer::TransferInspector,
};
use mega_evm2::{
    MegaTypes, TxLimits,
    factory::Simulation,
    ledger::Lite,
    oracle::{OracleAnswers, OracleServices, RecordingOracle},
};
use op_evm2::{OpTxEnvelope, TxDeposit};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

/// No per-transaction limit and no detention: the limits a test runs under when it names none of
/// its own, as the Satin implementation's tracer tests run (`EvmTxRuntimeLimits::no_limits`).
pub const NO_LIMITS: TxLimits = TxLimits { tx: [u64::MAX; 3], cap: u64::MAX, oracle_cap: u64::MAX };

/// The block the tests run in: block 1 at timestamp 1,700,000,000, gas limit 10,000,000,000, no
/// base fee, as the Satin implementation's tracer tests have it.
pub fn block() -> BlockEnv<MegaTypes> {
    BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_700_000_000u64),
        gas_limit: U256::from(10_000_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    }
}

/// An account holding `code`, nonce 0, no balance, as the Satin implementation's test database
/// installs one.
pub fn code(code: &[u8]) -> AccountInfo {
    AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code)))
}

/// An account holding `balance` and nothing else.
pub fn balance(balance: U256) -> AccountInfo {
    AccountInfo::default().with_balance(balance)
}

/// An in-memory state of `accounts`.
pub fn db(accounts: impl IntoIterator<Item = (Address, AccountInfo)>) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    for (address, info) in accounts {
        db.insert_account_info(&address, info);
    }
    db
}

/// A legacy transaction from `from` of kind `to` moving `value` with `input`, gas limit
/// `gas_limit`, gas price 0 and nonce `nonce`, signed for chain id 1.
pub fn legacy(
    from: Address,
    to: TxKind,
    value: U256,
    input: Bytes,
    gas_limit: u64,
    nonce: u64,
) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy { chain_id: Some(1), nonce, gas_price: 0, gas_limit, to, value, input };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), from)
}

/// A call from `from` to `to`, nonce 0.
pub fn call(
    from: Address,
    to: Address,
    value: U256,
    input: Bytes,
    gas_limit: u64,
) -> Recovered<OpTxEnvelope> {
    legacy(from, TxKind::Call(to), value, input, gas_limit, 0)
}

/// A creation from `from` running `init`, nonce 0.
pub fn create(from: Address, init: Bytes, gas_limit: u64) -> Recovered<OpTxEnvelope> {
    legacy(from, TxKind::Create, U256::ZERO, init, gas_limit, 0)
}

/// A deposit from `from` of kind `to` minting `mint` and moving `value`.
pub fn deposit(
    from: Address,
    to: TxKind,
    mint: u128,
    value: U256,
    input: Bytes,
    gas_limit: u64,
) -> Recovered<OpTxEnvelope> {
    let tx = TxDeposit {
        source_hash: B256::repeat_byte(0x42),
        from,
        to,
        mint,
        value,
        gas_limit,
        is_system_transaction: false,
        input,
    };
    Recovered::new_unchecked(OpTxEnvelope::deposit(tx), from)
}

/// An EIP-7702 transaction from `from` to `to` carrying `auths`, each `(delegate, authority,
/// nonce)` with its authority recovered already, chain id 1, no fee.
pub fn eip7702(
    from: Address,
    to: Address,
    input: Bytes,
    gas_limit: u64,
    auths: &[(Address, Address, u64)],
) -> Recovered<OpTxEnvelope> {
    use evm2::ethereum::LazyTxEip7702;
    let list = auths
        .iter()
        .map(|(delegate, _, nonce)| {
            let inner =
                Authorization { chain_id: U256::from(1), address: *delegate, nonce: *nonce };
            SignedAuthorization::new_unchecked(inner, 0, U256::from(1), U256::from(1))
        })
        .collect();
    let tx = TxEip7702 {
        chain_id: 1,
        nonce: 0,
        gas_limit,
        max_fee_per_gas: 0,
        max_priority_fee_per_gas: 0,
        to,
        value: U256::ZERO,
        access_list: Default::default(),
        authorization_list: list,
        input,
    };
    let authorities = auths.iter().map(|(_, authority, _)| Some(*authority)).collect();
    let lazy = LazyTxEip7702::from_cached_recovered_authorizations(tx, authorities);
    Recovered::new_unchecked(OpTxEnvelope::new(lazy, Some(Bytes::new())), from)
}

/// One Satin transaction to trace: the state it runs on, the transaction, the limits and the block.
#[derive(Clone)]
pub struct Case {
    pub name: &'static str,
    pub db: InMemoryDB,
    pub tx: Recovered<OpTxEnvelope>,
    pub limits: TxLimits,
    pub block: BlockEnv<MegaTypes>,
    /// The oracle service's answers, by slot, when the EVM is connected to a service that records
    /// the hints it is handed; `None`: no service, every Oracle read pushes the chain's value.
    pub oracle: Option<Vec<(U256, U256)>>,
}

/// What a run left: the result and its state, and what the run left of the engine's own record.
pub struct Ran {
    pub result: TxResultWithState<MegaTypes>,
    /// The receipt, the state changes, every gas figure and the engine's ledgers, rendered.
    pub outcome: Value,
}

impl Ran {
    /// Whether the transaction succeeded.
    pub const fn success(&self) -> bool {
        self.result.result.status
    }
}

impl Case {
    /// The case `name`: `tx` over `db`, with no limits, in [`block`].
    pub fn new(name: &'static str, db: InMemoryDB, tx: Recovered<OpTxEnvelope>) -> Self {
        Self { name, db, tx, limits: NO_LIMITS, block: block(), oracle: None }
    }

    /// The case under `limits` instead.
    pub const fn limits(mut self, limits: TxLimits) -> Self {
        self.limits = limits;
        self
    }

    /// The case with an oracle service answering `answers`.
    pub fn oracle(mut self, answers: Vec<(U256, U256)>) -> Self {
        self.oracle = Some(answers);
        self
    }

    /// A fresh Satin EVM over the case's state, from the engine's factory: the minimum SALT bucket
    /// everywhere, the case's limits (a test's counterfactual limits, a simulation's switch).
    pub fn evm(&self) -> Evm<'static, MegaTypes> {
        let simulation = Simulation { limits: Some(self.limits), ..Simulation::default() };
        let oracle = match &self.oracle {
            None => OracleServices::default(),
            Some(answers) => {
                let answers: OracleAnswers = answers.iter().copied().collect();
                OracleServices::service(Arc::new(RecordingOracle::new(answers)))
            }
        };
        common::factory()
            .simulated_evm(
                self.block,
                Db::new(self.db.clone()),
                common::minimum_salt(B256::ZERO),
                oracle,
                &simulation,
            )
            .expect("a Satin timestamp")
    }

    /// The pre-state, for the tracers that read it.
    pub fn pre_db(&self) -> Db<InMemoryDB> {
        Db::new(self.db.clone())
    }

    /// Runs the transaction, with `inspector` attached when given (borrowed, as a node attaches a
    /// tracer to one transaction), and detaches its state.
    pub fn run(&self, inspector: Option<&mut (dyn Inspector<MegaTypes> + '_)>) -> Ran {
        let mut evm = self.evm();
        let executed = match inspector {
            Some(inspector) => evm.transact_with_inspector(&self.tx, inspector),
            None => evm.transact(&self.tx),
        };
        let result = executed
            .map(evm2::evm::ExecutedTx::detach)
            .unwrap_or_else(|error| panic!("{}: the transaction is refused: {error:?}", self.name));
        let outcome = outcome(&result, evm.ext());
        Ran { result, outcome }
    }

    /// The run with no inspector.
    pub fn plain(&self) -> Ran {
        self.run(None)
    }

    /// The run with `inspector` attached, held to the plain run: the same receipt, state, gas and
    /// ledgers.
    pub fn traced(&self, inspector: &mut (dyn Inspector<MegaTypes> + '_), what: &str) -> Ran {
        let ran = self.run(Some(inspector));
        assert_same_outcome(
            &self.plain().outcome,
            &ran.outcome,
            &format!("{} under {what}", self.name),
        );
        ran
    }

    /// What a node passes a tracer about the transaction: its block's hash, its index and its
    /// hash (fixed values: the tests run outside a chain).
    pub fn context(&self) -> TransactionContext {
        TransactionContext::default()
            .with_block_hash(B256::repeat_byte(0xbb))
            .with_tx_index(0)
            .with_tx_hash(keccak256(self.name))
    }

    /// The transaction's sender.
    pub fn sender(&self) -> Address {
        self.tx.signer()
    }
}

/// `inspected` and `plain` are the same run, field by field.
pub fn assert_same_outcome(plain: &Value, inspected: &Value, what: &str) {
    if plain == inspected {
        return;
    }
    let (Value::Object(p), Value::Object(q)) = (plain, inspected) else { unreachable!() };
    let differ: Vec<_> = p.keys().filter(|k| p.get(*k) != q.get(*k)).collect();
    panic!(
        "{what}: the inspected run differs from the plain one in {differ:?}\nplain: {plain:#}\ninspected: {inspected:#}"
    );
}

fn hex(bytes: &[u8]) -> String {
    format!("0x{}", alloy_primitives::hex::encode(bytes))
}

/// The receipt, the state changes, every gas figure and the engine's ledgers of a run.
pub fn outcome(r: &TxResultWithState<MegaTypes>, ext: &Lite) -> Value {
    let res = &r.result;
    let logs: Vec<Value> = res
        .logs
        .iter()
        .map(|l| {
            json!({
                "address": l.address.to_string(),
                "topics": l.topics().iter().map(ToString::to_string).collect::<Vec<_>>(),
                "data": hex(&l.data.data),
            })
        })
        .collect();
    let mut accounts = BTreeMap::new();
    for c in r.pending_state.account_changes() {
        let info = |i: Option<&AccountInfo>| {
            i.map(|i| json!([i.balance.to_string(), i.nonce, i.code_hash.to_string()]))
        };
        let mut slots: Vec<(U256, U256, U256)> =
            c.storage.changed_slots().map(|(k, s)| (*k, s.original, s.current)).collect();
        slots.sort();
        accounts.insert(
            c.address.to_string(),
            json!({
                "original": info(c.original),
                "current": info(c.current),
                "created": c.created,
                "selfdestructed": c.selfdestructed,
                "touched": c.touched,
                "code": c.code.map(|(h, _)| h.to_string()),
                "slots": slots.iter().map(|(k, o, v)| [k.to_string(), o.to_string(), v.to_string()]).collect::<Vec<_>>(),
            }),
        );
    }
    json!({
        "receipt": {
            "status": res.status,
            "stop": format!("{:?}", res.stop),
            "output": hex(&res.output),
            "gas_used": res.tx_gas_used(),
            "created_address": res.created_address.map(|a| a.to_string()),
            "logs": logs,
            "ext": format!("{:?}", res.ext),
        },
        "gas": {
            "total_gas_spent": res.total_gas_spent,
            "state_gas_spent": res.state_gas_spent,
            "refunded": res.refunded,
            "floor_gas": res.floor_gas,
        },
        "state": accounts,
        "ledgers": {
            "usage": ext.usage(res.status),
            "history": ext.history(res.status),
            "reservoir_left": ext.reservoir_left,
            "top_stop": ext.top_stop,
            "stops": ext.stops,
            "detention": format!("{:?}", ext.detention),
            "mechanisms": format!("{:?}", ext.mechanisms),
            "tx_net": ext.tx_net,
            "tx_growth": ext.tx_growth,
            "tx_start_charge": ext.tx_start_charge,
            "body_history": ext.body_history,
            "failed_deposit": ext.failed_deposit,
            "oracle_reads": ext.oracle_reads,
            "buckets": format!("{:?}", ext.salt.lookups()),
            "created_prices": format!("{:?}", ext.created_prices),
            "destroyed": format!("{:?}", ext.destroyed),
        },
    })
}

/// A JavaScript tracer, as a node's caller writes one: it records every frame it enters and
/// leaves, every step's opcode, depth, gas, cost and refund, every fault, and in `result` the
/// transaction's context and a few reads of the state through `db`.
pub const JS_TRACER: &str = r#"{
    frames: [],
    steps: [],
    faults: [],
    step: function(log, db) {
        this.steps.push([log.op.toString(), log.getDepth(), log.getPC(), log.getGas(),
            log.getCost(), log.getRefund(), log.stack.length()]);
    },
    fault: function(log, db) {
        this.faults.push([log.op.toString(), log.getDepth(), log.getError()]);
    },
    enter: function(frame) {
        this.frames.push(["enter", frame.getType(), toHex(frame.getFrom()), toHex(frame.getTo()),
            toHex(frame.getInput()), frame.getGas(), String(frame.getValue())]);
    },
    exit: function(res) {
        this.frames.push(["exit", res.getGasUsed(), toHex(res.getOutput()), res.getError()]);
    },
    result: function(ctx, db) {
        return {
            type: ctx.type,
            from: toHex(ctx.from),
            to: toHex(ctx.to),
            input: toHex(ctx.input),
            gas: ctx.gas,
            gasUsed: ctx.gasUsed,
            gasPrice: String(ctx.gasPrice),
            value: String(ctx.value),
            block: ctx.block,
            output: toHex(ctx.output),
            error: ctx.error,
            senderBalance: String(db.getBalance(ctx.from)),
            senderNonce: db.getNonce(ctx.from),
            toExists: db.exists(ctx.to),
            toCode: toHex(db.getCode(ctx.to)),
            frames: this.frames,
            steps: this.steps,
            faults: this.faults,
        };
    }
}"#;

/// Every tracer and inspector of the inspector crate, as a node runs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tracer {
    /// The Geth struct (opcode) logger, with the options a node uses when the call names none:
    /// the stack and the storage an `SLOAD` or `SSTORE` touched, no memory, no return data.
    StructLog,
    /// The Geth struct logger with memory and return data as well.
    StructLogFull,
    /// The Geth call tracer, with logs.
    Call,
    /// The Geth prestate tracer, default mode.
    Prestate,
    /// The Geth prestate tracer, diff mode.
    PrestateDiff,
    /// The Geth flat call tracer.
    FlatCall,
    /// The Geth four-byte tracer.
    FourByte,
    /// The Geth mux tracer of the call tracer with logs, the prestate diff, the flat call tracer,
    /// the four-byte tracer and the JavaScript tracer.
    Mux,
    /// The Geth no-op tracer.
    Noop,
    /// The EIP-8037 state-gas summary tracer.
    StateGas,
    /// The ERC-7562 tracer.
    Erc7562,
    /// The keccak-256 preimage tracer.
    KeccakPreimage,
    /// The JavaScript tracer, running [`JS_TRACER`].
    Js,
    /// Parity's `trace_replayTransaction` with the trace, the state diff and the VM trace.
    Parity,
    /// The access-list inspector, as `eth_createAccessList` runs it: the sender, the recipient
    /// and the precompiles left out.
    AccessList,
    /// The transfer inspector, every transfer and the logs.
    Transfer,
    /// The Otterscan internal-operations inspector.
    Otterscan,
    /// The opcode counter and gas inspector.
    OpcodeGas,
    /// The storage-access inspector.
    Storage,
}

impl Tracer {
    pub const ALL: [Self; 19] = [
        Self::StructLog,
        Self::StructLogFull,
        Self::Call,
        Self::Prestate,
        Self::PrestateDiff,
        Self::FlatCall,
        Self::FourByte,
        Self::Mux,
        Self::Noop,
        Self::StateGas,
        Self::Erc7562,
        Self::KeccakPreimage,
        Self::Js,
        Self::Parity,
        Self::AccessList,
        Self::Transfer,
        Self::Otterscan,
        Self::OpcodeGas,
        Self::Storage,
    ];

    /// The tracer's key in a golden file.
    pub const fn name(self) -> &'static str {
        match self {
            Self::StructLog => "struct_log",
            Self::StructLogFull => "struct_log_full",
            Self::Call => "call",
            Self::Prestate => "prestate",
            Self::PrestateDiff => "prestate_diff",
            Self::FlatCall => "flat_call",
            Self::FourByte => "four_byte",
            Self::Mux => "mux",
            Self::Noop => "noop",
            Self::StateGas => "state_gas",
            Self::Erc7562 => "erc7562",
            Self::KeccakPreimage => "keccak_preimage",
            Self::Js => "js",
            Self::Parity => "parity",
            Self::AccessList => "access_list",
            Self::Transfer => "transfer",
            Self::Otterscan => "otterscan",
            Self::OpcodeGas => "opcode_gas",
            Self::Storage => "storage",
        }
    }

    /// The `debug_traceTransaction` options of a Geth tracer, as a node's caller sends them.
    pub fn geth_options(self) -> Option<GethDebugTracingOptions> {
        let options = match self {
            Self::StructLog => json!({}),
            Self::StructLogFull => json!({"enableMemory": true, "enableReturnData": true}),
            Self::Call => json!({"tracer": "callTracer", "tracerConfig": {"withLog": true}}),
            Self::Prestate => json!({"tracer": "prestateTracer"}),
            Self::PrestateDiff => {
                json!({"tracer": "prestateTracer", "tracerConfig": {"diffMode": true}})
            }
            Self::FlatCall => json!({"tracer": "flatCallTracer"}),
            Self::FourByte => json!({"tracer": "4byteTracer"}),
            Self::Mux => json!({"tracer": "muxTracer", "tracerConfig": {
                "callTracer": {"withLog": true},
                "prestateTracer": {"diffMode": true},
                "flatCallTracer": {},
                "4byteTracer": {},
                JS_TRACER: {},
            }}),
            Self::Noop => json!({"tracer": "noopTracer"}),
            Self::StateGas => json!({"tracer": "stateGasTracer"}),
            Self::Erc7562 => json!({"tracer": "erc7562Tracer"}),
            Self::KeccakPreimage => json!({"tracer": "keccak256PreimageTracer"}),
            Self::Js => json!({"tracer": JS_TRACER}),
            _ => return None,
        };
        Some(serde_json::from_value(options).expect("valid tracing options"))
    }

    /// Runs `case` under the tracer, held to the plain run, and returns the tracer's output as a
    /// node returns it, with the run.
    pub fn trace(self, case: &Case) -> (Value, Ran) {
        if let Some(options) = self.geth_options() {
            let (trace, ran) = geth_trace(case, options, self.name());
            return (serde_json::to_value(trace).expect("a trace is JSON"), ran);
        }
        let what = self.name();
        match self {
            Self::Parity => {
                let types: HashSet<TraceType> =
                    [TraceType::Trace, TraceType::StateDiff, TraceType::VmTrace]
                        .into_iter()
                        .collect();
                let mut inspector =
                    TracingInspector::new(TracingInspectorConfig::from_parity_config(&types));
                let ran = case.traced(&mut inspector, what);
                let results = inspector
                    .into_parity_builder()
                    .into_trace_results_with_state(&ran.result, &types, &mut case.pre_db())
                    .expect("the pre-state serves every read");
                (serde_json::to_value(results).unwrap(), ran)
            }
            Self::AccessList => {
                let tx: &OpTxEnvelope = &case.tx;
                let mut inspector = AccessListInspector::default()
                    .with_excluded_from_tx(tx, common::CHAIN_ID)
                    .with_excluded(precompile_addresses());
                let ran = case.traced(&mut inspector, what);
                (serde_json::to_value(inspector.into_access_list()).unwrap(), ran)
            }
            Self::Transfer => {
                let mut inspector = TransferInspector::new(false).with_logs(true);
                let ran = case.traced(&mut inspector, what);
                let transfers: Vec<Value> = inspector
                    .transfers()
                    .iter()
                    .map(|t| {
                        json!({
                            "kind": format!("{:?}", t.kind),
                            "from": format!("{:#x}", t.from),
                            "to": format!("{:#x}", t.to),
                            "value": t.value.to_string(),
                        })
                    })
                    .collect();
                let logs: Vec<Value> = inspector
                    .logs()
                    .iter()
                    .map(|l| {
                        json!({
                            "address": format!("{:#x}", l.address),
                            "topics": l.topics().iter().map(ToString::to_string).collect::<Vec<_>>(),
                            "data": hex(&l.data.data),
                        })
                    })
                    .collect();
                (json!({"transfers": transfers, "logs": logs}), ran)
            }
            Self::Otterscan => {
                let mut inspector = InternalOperationsInspector::default();
                let ran = case.traced(&mut inspector, what);
                (serde_json::to_value(inspector.into_operations()).unwrap(), ran)
            }
            Self::OpcodeGas => {
                let mut inspector = OpcodeGasInspector::new();
                let ran = case.traced(&mut inspector, what);
                let mut ops: Vec<(String, u64, u64)> = inspector
                    .opcode_iter()
                    .map(|(op, (count, gas))| (op.to_string(), count, gas))
                    .collect();
                ops.sort();
                (json!(ops), ran)
            }
            Self::Storage => {
                let mut inspector = StorageInspector::new();
                let ran = case.traced(&mut inspector, what);
                let mut slots: Vec<(String, String, u64)> = inspector
                    .accessed_slots()
                    .iter()
                    .flat_map(|(a, s)| {
                        s.iter().map(move |(k, n)| (a.to_string(), k.to_string(), *n))
                    })
                    .collect();
                slots.sort();
                let summary = json!({
                    "slots": slots,
                    "unique_loads": inspector.unique_loads(),
                    "warm_loads": inspector.warm_loads(),
                });
                (summary, ran)
            }
            _ => unreachable!("a Geth tracer"),
        }
    }
}

/// The engine's precompiles' addresses, which `eth_createAccessList` leaves out of the list.
pub fn precompile_addresses() -> Vec<Address> {
    mega_evm2::precompile_map().addresses().collect()
}

/// `case` under the Geth tracer of `options`, held to the plain run: the trace
/// `DebugInspector::get_result` builds, as a node's `debug_traceTransaction` returns it.
pub fn geth_trace(case: &Case, options: GethDebugTracingOptions, what: &str) -> (GethTrace, Ran) {
    let mut inspector = DebugInspector::new(options).expect("a tracer the crate provides");
    let ran = case.traced(&mut inspector, what);
    let trace = inspector
        .get_result(Some(case.context()), &case.tx, &case.block, &ran.result, &mut case.pre_db())
        .expect("the trace builds");
    (trace, ran)
}

/// Every tracer's output for `case`, by the tracer's name.
pub fn all_traces(case: &Case) -> BTreeMap<&'static str, Value> {
    Tracer::ALL.iter().map(|t| (t.name(), t.trace(case).0)).collect()
}

/// Every tracer's output for a case, and its plain run.
pub struct Traces {
    pub plain: Ran,
    pub by: BTreeMap<&'static str, Value>,
}

impl std::ops::Index<&str> for Traces {
    type Output = Value;
    fn index(&self, tracer: &str) -> &Value {
        self.by.get(tracer).unwrap_or_else(|| panic!("no tracer {tracer}"))
    }
}

/// Runs `case` under every tracer, each run held to the plain run, and holds their output to the
/// case's golden file, with the receipt's status, gas used and output beside it.
pub fn pin(case: &Case) -> Traces {
    let plain = case.plain();
    let by = all_traces(case);
    let mut golden = serde_json::Map::new();
    let receipt = &plain.outcome["receipt"];
    golden.insert(
        "outcome".into(),
        json!({"status": receipt["status"], "gas_used": receipt["gas_used"], "output": receipt["output"]}),
    );
    for (tracer, output) in &by {
        golden.insert((*tracer).into(), output.clone());
    }
    crate::golden::check(case.name, &Value::Object(golden));
    Traces { plain, by }
}

/// A bytecode builder that assembles code byte for byte as the Satin implementation's test
/// builder (`BytecodeBuilder`) does, so the scenarios ported from its tracer tests run the same
/// program counters and opcodes.
#[derive(Clone, Debug, Default)]
pub struct Asm(Vec<u8>);

impl Asm {
    pub fn op(mut self, op: u8) -> Self {
        self.0.push(op);
        self
    }

    pub fn ops(mut self, ops: impl IntoIterator<Item = u8>) -> Self {
        self.0.extend(ops);
        self
    }

    /// `PUSHn` of `bytes`, n their length (`PUSH0` for none).
    pub fn push(mut self, bytes: &[u8]) -> Self {
        assert!(bytes.len() <= 32);
        self.0.push(op::PUSH0 + bytes.len() as u8);
        self.0.extend_from_slice(bytes);
        self
    }

    pub fn push_u8(self, v: u8) -> Self {
        self.push(&[v])
    }

    pub fn push_u16(self, v: u16) -> Self {
        self.push(&v.to_be_bytes())
    }

    pub fn push_u64(self, v: u64) -> Self {
        self.push(&v.to_be_bytes())
    }

    pub fn push_u256(self, v: U256) -> Self {
        self.push(&v.to_be_bytes::<32>())
    }

    pub fn push_address(self, a: Address) -> Self {
        self.push(a.as_slice())
    }

    /// `bytes`, right-padded to whole words, stored word by word from `offset`: a `PUSH32` of the
    /// word, a `PUSH8` of its offset and an `MSTORE` each.
    pub fn mstore(mut self, offset: usize, bytes: &[u8]) -> Self {
        let mut padded = bytes.to_vec();
        padded.resize(bytes.len().div_ceil(32) * 32, 0);
        for (i, word) in padded.chunks(32).enumerate() {
            self = self.push(word).push_u64((offset + i * 32) as u64).op(op::MSTORE);
        }
        self
    }

    /// `SSTORE` of `value` at `slot`: two `PUSH32` and the `SSTORE`.
    pub fn sstore(self, slot: U256, value: U256) -> Self {
        self.push_u256(value).push_u256(slot).op(op::SSTORE)
    }

    /// A `CALL` of `target` carrying `value` with all the gas left, no input and no return area.
    pub fn call(self, target: Address, value: U256) -> Self {
        self.ops([op::PUSH0; 4]).push_u256(value).push_address(target).op(op::GAS).op(op::CALL)
    }

    pub fn revert(self) -> Self {
        self.ops([op::PUSH0, op::PUSH0, op::REVERT])
    }

    pub fn return_empty(self) -> Self {
        self.ops([op::PUSH0, op::PUSH0, op::RETURN])
    }

    pub fn stop(self) -> Self {
        self.op(op::STOP)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn build(self) -> Bytes {
        Bytes::from(self.0)
    }
}

impl Asm {
    /// A `PUSHn` of `v` in as few bytes as it takes (`PUSH0` for zero).
    pub fn push_min(self, v: u64) -> Self {
        let bytes = v.to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        self.push(&bytes[skip..])
    }

    /// `kind` (`CALL`, `CALLCODE`, `DELEGATECALL` or `STATICCALL`) of `target` with `input`
    /// stored at memory offset 0, `value` (for `CALL` and `CALLCODE`), all the gas left, and one
    /// word of return area at offset 0; the success flag is left on the stack.
    pub fn call_input(self, kind: u8, target: Address, value: u64, input: &[u8]) -> Self {
        let mut a = self
            .mstore(0, input)
            .push_u8(32)
            .op(op::PUSH0)
            .push_min(input.len() as u64)
            .op(op::PUSH0);
        if matches!(kind, op::CALL | op::CALLCODE) {
            a = a.push_min(value);
        }
        a.push_address(target).op(op::GAS).op(kind)
    }

    /// `kind` of `target` with no input and all the gas left, the success flag left on the stack.
    pub fn call_plain(self, kind: u8, target: Address, value: u64) -> Self {
        self.call_input(kind, target, value, &[])
    }
}
