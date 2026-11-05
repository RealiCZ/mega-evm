//! Reference harness for the block gates: the Satin implementation under review, run on recorded
//! blocks and rendered in the JSON shape the gates in `next/diff` read.
//!
//! Replays recorded MegaETH blocks on the Satin engine (mega-evm at the pinned revision),
//! transaction by transaction through `MegaEvm::execute_transaction` (the entry the block executor
//! and the synthetic oracle use), plain and with a recording inspector, each on a fresh EVM over
//! the block's state, then commits the plain run's state diff before the next transaction.
//!
//! Pre-block: the EIP-2935 and EIP-4788 calls and the `SequencerRegistry`'s due changes, through
//! the engine's public helpers, each committed. Block-level packing rules are not applied here:
//! this is an engine-level comparison; the block level is `tests/block.rs`.

#![allow(unreachable_pub, deprecated, missing_docs, clippy::all)]

#[path = "../../diff/shared/blockdb.rs"]
pub mod blockdb;
pub mod synthetic;

use alloy_consensus::{Signed, TxEip1559, TxEip2930, TxEip7702, TxLegacy};
use alloy_eips::{
    Encodable2718,
    eip2930::{AccessList, AccessListItem},
    eip7702::{Authorization, SignedAuthorization},
};
use alloy_op_evm::OpTx;
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256};
use blockdb::{Block, BlockDb, addr, b256, bytes, hex, u64v, u256};
use mega_evm::{
    ExternalEnvFactory, ExternalEnvs, LimitCheck, MegaContext, MegaEvm, MegaSpecId,
    MegaTransactionOutcome, TestExternalEnvs,
    test_utils::{LedgerPools, RecordingEnvFactory, WitnessRecord, take_start_ledger_settlements},
};
use op_alloy_consensus::{OpTxEnvelope, TxDeposit};
use revm::{
    Database, Inspector,
    context::{BlockEnv, CfgEnv},
    context_interface::result::{ExecutionResult, ResultAndState},
    interpreter::{
        CallInputs, CallOutcome, CreateInputs, CreateOutcome, InstructionResult,
        interpreter::EthInterpreter,
    },
    primitives::eip4844,
    state::{AccountInfo, Bytecode, EvmState},
};
use serde_json::{Map, Value, json};
use std::{cell::RefCell, convert::Infallible, rc::Rc};

// ---------------------------------------------------------------------------------------------
// Database over the shared block state.

#[derive(Clone)]
pub struct SatinDb(pub Rc<RefCell<BlockDb>>);

impl std::fmt::Debug for SatinDb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SatinDb")
    }
}

impl Database for SatinDb {
    type Error = Infallible;
    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Infallible> {
        Ok(self.0.borrow_mut().account(address).map(|(balance, nonce, code_hash, code)| {
            AccountInfo::new(balance, nonce, code_hash, Bytecode::new_raw(code))
        }))
    }
    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Infallible> {
        Ok(Bytecode::new_raw(self.0.borrow_mut().code(code_hash)))
    }
    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Infallible> {
        Ok(self.0.borrow_mut().storage(address, index))
    }
    fn block_hash(&mut self, number: u64) -> Result<B256, Infallible> {
        Ok(self.0.borrow_mut().block_hash(number))
    }
}

// ---------------------------------------------------------------------------------------------
// Inspector: every frame, with its target and, for the 0x6342… system contracts, its selector.

pub fn status_of(r: InstructionResult) -> String {
    if r.is_ok() {
        "success".into()
    } else if r.is_revert() {
        "revert".into()
    } else {
        "halt".into()
    }
}

#[derive(Default)]
pub struct Frames {
    stack: Vec<usize>,
    pub frames: Vec<Value>,
}

impl Frames {
    fn open(
        &mut self,
        kind: &str,
        target: Address,
        selector: Option<String>,
        gas_limit: u64,
        reservoir: u64,
        value: U256,
    ) {
        let level = self.stack.len();
        self.stack.push(self.frames.len());
        self.frames.push(json!({
            "kind": kind, "level": level, "target": format!("{target}"), "selector": selector,
            "gas_limit": gas_limit, "reservoir": reservoir, "value": value.to_string(),
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

const SYSTEM_PREFIX: [u8; 2] = [0x63, 0x42];

impl<DB: Database, E: mega_evm::ExternalEnvTypes> Inspector<MegaContext<DB, E>, EthInterpreter>
    for Frames
{
    fn call(
        &mut self,
        ctx: &mut MegaContext<DB, E>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        let kind = format!("{:?}", inputs.scheme);
        let selector = (inputs.target_address.as_slice()[..2] == SYSTEM_PREFIX).then(|| {
            let input = inputs.input.bytes(ctx);
            hex(&input[..input.len().min(4)])
        });
        self.open(
            &kind,
            inputs.target_address,
            selector,
            inputs.gas_limit,
            inputs.reservoir,
            inputs.call_value(),
        );
        None
    }
    fn call_end(&mut self, _: &mut MegaContext<DB, E>, _: &CallInputs, outcome: &mut CallOutcome) {
        let g = outcome.result.gas;
        self.close(outcome.result.result, &outcome.result.output, g.remaining(), g.reservoir());
    }
    fn create(
        &mut self,
        _: &mut MegaContext<DB, E>,
        inputs: &mut CreateInputs,
    ) -> Option<CreateOutcome> {
        self.open(
            "Create",
            Address::ZERO,
            None,
            inputs.gas_limit(),
            inputs.reservoir(),
            inputs.value(),
        );
        None
    }
    fn create_end(
        &mut self,
        _: &mut MegaContext<DB, E>,
        _: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        let g = outcome.result.gas;
        self.close(outcome.result.result, &outcome.result.output, g.remaining(), g.reservoir());
    }
}

// ---------------------------------------------------------------------------------------------
// Environments: SALT capacities were not captured (every bucket at the minimum), no Oracle
// service answers. The FNV-1a test hasher names the buckets, as in the synthetic oracle.

pub type Test = TestExternalEnvs<String>;
pub type Envs =
    (mega_evm::test_utils::RecordingSaltEnv<Test>, mega_evm::test_utils::RecordingOracleEnv<Test>);

#[derive(Clone)]
pub struct Fixed(pub Test);

impl ExternalEnvFactory for Fixed {
    type EnvTypes = (Test, Test);
    fn external_envs(&self, _block: u64) -> ExternalEnvs<Self::EnvTypes> {
        ExternalEnvs { salt_env: self.0.clone(), oracle_env: self.0.clone() }
    }
}

pub fn block_env(b: &Block) -> BlockEnv {
    let h = &b.header;
    let mut env = BlockEnv {
        number: U256::from(h.number),
        beneficiary: h.beneficiary,
        timestamp: U256::from(h.timestamp),
        gas_limit: h.gas_limit,
        basefee: h.base_fee,
        difficulty: h.difficulty,
        prevrandao: Some(h.mix_hash),
        blob_excess_gas_and_price: None,
        slot_num: 0,
    };
    env.set_blob_excess_gas_and_price(
        h.excess_blob_gas,
        eip4844::BLOB_BASE_FEE_UPDATE_FRACTION_CANCUN,
    );
    env
}

pub fn context(
    b: &Block,
    db: &Rc<RefCell<BlockDb>>,
    record: Rc<RefCell<WitnessRecord>>,
) -> MegaContext<SatinDb, Envs> {
    let factory = RecordingEnvFactory::new(Fixed(Test::new()), record);
    let ext = factory.external_envs(b.number);
    let mut cfg = CfgEnv::new_with_spec(MegaSpecId::SATIN);
    cfg.chain_id = b.chain_id;
    MegaContext::new_with_external_envs(SatinDb(db.clone()), MegaSpecId::SATIN, ext)
        .with_cfg(cfg)
        .with_block(block_env(b))
}

// ---------------------------------------------------------------------------------------------
// Envelopes, rebuilt from the RPC objects; the encoding must hash to the chain's hash.

pub fn kind(v: &Value) -> TxKind {
    match v.get("to") {
        Some(to) if !to.is_null() => TxKind::Call(addr(to)),
        _ => TxKind::Create,
    }
}

pub fn access_list(v: &Value) -> AccessList {
    AccessList(
        v.get("accessList")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|i| AccessListItem {
                        address: addr(&i["address"]),
                        storage_keys: i["storageKeys"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(b256)
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    )
}

pub fn parity(v: &Value) -> bool {
    match v.get("yParity").filter(|p| !p.is_null()) {
        Some(p) => u64v(p) == 1,
        None => u64v(&v["v"]) == 1,
    }
}

pub fn envelope(t: &Value) -> Result<(OpTxEnvelope, Bytes), String> {
    let ty = u64v(&t["type"]);
    let hash = b256(&t["hash"]);
    let input = bytes(&t["input"]);
    let sig = |par: bool| Signature::new(u256(&t["r"]), u256(&t["s"]), par);
    let env = match ty {
        0 => {
            let v = u64v(&t["v"]);
            let (chain_id, par) =
                if v >= 35 { (Some((v - 35) / 2), (v - 35) % 2 == 1) } else { (None, v == 28) };
            let tx = TxLegacy {
                chain_id,
                nonce: u64v(&t["nonce"]),
                gas_price: u256(&t["gasPrice"]).to(),
                gas_limit: u64v(&t["gas"]),
                to: kind(t),
                value: u256(&t["value"]),
                input,
            };
            OpTxEnvelope::Legacy(Signed::new_unchecked(tx, sig(par), hash))
        }
        1 => {
            let tx = TxEip2930 {
                chain_id: u64v(&t["chainId"]),
                nonce: u64v(&t["nonce"]),
                gas_price: u256(&t["gasPrice"]).to(),
                gas_limit: u64v(&t["gas"]),
                to: kind(t),
                value: u256(&t["value"]),
                access_list: access_list(t),
                input,
            };
            OpTxEnvelope::Eip2930(Signed::new_unchecked(tx, sig(parity(t)), hash))
        }
        2 => {
            let tx = TxEip1559 {
                chain_id: u64v(&t["chainId"]),
                nonce: u64v(&t["nonce"]),
                gas_limit: u64v(&t["gas"]),
                max_fee_per_gas: u256(&t["maxFeePerGas"]).to(),
                max_priority_fee_per_gas: u256(&t["maxPriorityFeePerGas"]).to(),
                to: kind(t),
                value: u256(&t["value"]),
                access_list: access_list(t),
                input,
            };
            OpTxEnvelope::Eip1559(Signed::new_unchecked(tx, sig(parity(t)), hash))
        }
        4 => {
            let auths = t["authorizationList"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    SignedAuthorization::new_unchecked(
                        Authorization {
                            chain_id: u256(&a["chainId"]),
                            address: addr(&a["address"]),
                            nonce: u64v(&a["nonce"]),
                        },
                        u64v(&a["yParity"]) as u8,
                        u256(&a["r"]),
                        u256(&a["s"]),
                    )
                })
                .collect();
            let tx = TxEip7702 {
                chain_id: u64v(&t["chainId"]),
                nonce: u64v(&t["nonce"]),
                gas_limit: u64v(&t["gas"]),
                max_fee_per_gas: u256(&t["maxFeePerGas"]).to(),
                max_priority_fee_per_gas: u256(&t["maxPriorityFeePerGas"]).to(),
                to: addr(&t["to"]),
                value: u256(&t["value"]),
                access_list: access_list(t),
                authorization_list: auths,
                input,
            };
            OpTxEnvelope::Eip7702(Signed::new_unchecked(tx, sig(parity(t)), hash))
        }
        0x7e => {
            let tx = TxDeposit {
                source_hash: b256(&t["sourceHash"]),
                from: addr(&t["from"]),
                to: kind(t),
                mint: u256(&t["mint"]).to(),
                value: u256(&t["value"]),
                gas_limit: u64v(&t["gas"]),
                is_system_transaction: t
                    .get("isSystemTx")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                input,
            };
            OpTxEnvelope::Deposit(alloy_primitives::Sealed::new_unchecked(tx, hash))
        }
        other => return Err(format!("unsupported tx type {other:#x}")),
    };
    let raw = Bytes::from(env.encoded_2718());
    let computed = alloy_primitives::keccak256(&raw);
    if computed != hash {
        return Err(format!("re-encoding hashes to {computed}, chain says {hash}"));
    }
    Ok((env, raw))
}

pub fn mega_tx(env: &OpTxEnvelope, raw: &Bytes, sender: Address) -> mega_evm::MegaTransaction {
    <OpTx as alloy_evm::FromTxWithEncoded<OpTxEnvelope>>::from_encoded_tx(env, sender, raw.clone())
}

// ---------------------------------------------------------------------------------------------
// Outcome rendering and the full state diff.

pub fn diff_of(state: &EvmState) -> Value {
    let mut accounts = Map::new();
    for (a, acc) in state.iter() {
        if !acc.is_touched() {
            continue;
        }
        let orig = acc.original_info();
        let destroyed = acc.is_selfdestructed();
        let created = acc.is_created();
        let mut storage = Map::new();
        for (k, slot) in acc.storage.iter() {
            if slot.present_value != slot.original_value ||
                (created && !slot.present_value.is_zero())
            {
                storage.insert(format!("{k:#x}"), json!(format!("{:#x}", slot.present_value)));
            }
        }
        let info_changed = orig.balance != acc.info.balance ||
            orig.nonce != acc.info.nonce ||
            orig.code_hash != acc.info.code_hash;
        if !destroyed && !created && !info_changed && storage.is_empty() {
            continue;
        }
        let code = if acc.info.code_hash != orig.code_hash || created {
            acc.info.code.as_ref().map(|c| json!(hex(c.original_bytes().as_ref())))
        } else {
            None
        };
        accounts.insert(
            format!("{a}"),
            json!({
                "balance": format!("{:#x}", acc.info.balance), "nonce": acc.info.nonce,
                "code_hash": format!("{}", acc.info.code_hash), "code": code,
                "created": created, "destroyed": destroyed, "storage": storage,
            }),
        );
    }
    json!({ "accounts": accounts })
}

pub fn changed_json(state: &EvmState) -> (Map<String, Value>, Map<String, Value>) {
    let mut accounts = Map::new();
    let mut storage = Map::new();
    for (a, acc) in state.iter() {
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
            accounts.insert(format!("{a}"), json!(format!("{b}/n{n}/{code_hash}")));
        }
        for (k, slot) in acc.storage.iter() {
            if slot.present_value != slot.original_value && !acc.is_selfdestructed() {
                storage.insert(format!("{a}:{k:#x}"), json!(format!("{:#x}", slot.present_value)));
            }
        }
    }
    (accounts, storage)
}

pub fn ledger_json<DB, I, E>(evm: &mut MegaEvm<DB, I, E>) -> Value
where
    DB: Database + std::fmt::Debug,
    E: mega_evm::ExternalEnvTypes,
{
    let pools = |p: &LedgerPools| {
        let nums: Vec<u64> = format!("{:?}", p.remaining)
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .map(|s| s.parse().unwrap())
            .collect();
        json!({"remaining":nums,"net":p.net})
    };
    json!(take_start_ledger_settlements(evm.ctx_mut()).iter().map(|l|json!({"depth":l.depth,"frame":pools(&l.frame),"caller":pools(&l.caller),"refund":l.refund})).collect::<Vec<_>>())
}

pub fn outcome_json(o: &MegaTransactionOutcome, db: &BlockDb, rec: &WitnessRecord) -> Value {
    let (status, output, reason) = match &o.result {
        ExecutionResult::Success { output, .. } => {
            ("success".to_string(), output.data().clone(), Value::Null)
        }
        ExecutionResult::Revert { output, .. } => {
            ("revert".to_string(), output.clone(), Value::Null)
        }
        ExecutionResult::Halt { reason, .. } => {
            ("halt".to_string(), Bytes::new(), json!(format!("{reason:?}")))
        }
    };
    let (accounts, storage) = changed_json(&o.state);
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
    let mut reads = db.reads.to_json();
    let r = reads.as_object_mut().unwrap();
    r.insert("buckets".into(), json!(rec.buckets.keys().collect::<Vec<_>>()));
    r.insert("oracle_reads".into(), json!(rec.oracle_reads.len()));
    json!({
        "status": status,
        "halt": reason,
        "account_flags": diff_of(&o.state)["accounts"].as_object().unwrap().iter().map(|(a,v)|(a.clone(),json!({"created":v["created"],"destroyed":v["destroyed"]}))).collect::<Map<String,Value>>(),
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
        "misses": db.run_misses,
    })
}

pub fn system_json(r: &ResultAndState<op_revm::OpHaltReason>, db: &BlockDb) -> Value {
    let (accounts, storage) = changed_json(&r.state);
    json!({
        "status": if r.result.is_success() { "success" } else { "fail" },
        "gas_used": r.result.tx_gas_used(),
        "accounts": accounts, "storage": storage,
        "reads": db.reads.to_json(), "misses": db.run_misses,
    })
}

// ---------------------------------------------------------------------------------------------
