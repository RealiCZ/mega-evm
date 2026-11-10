//! Runs recorded MegaETH blocks on the Satin engine, transaction by transaction through
//! `Evm::transact`, over the shared block state (`shared/blockdb.rs`), and renders each outcome in
//! the Satin replay's JSON shape.
//!
//! Correctness runs build a fresh `Evm` for every transaction (so the database reads of a run are
//! that transaction's read set, as on the Satin side) and commit the state diff given to them.

#[path = "../shared/blockdb.rs"]
pub mod blockdb;

use crate::{
    driver::{Frames, changed_json, collect_changes, render_outcome},
    invariant,
};
use alloy_consensus::transaction::Recovered;
use alloy_eips::Decodable2718;
use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use blockdb::{BlockDb, EMPTY_CODE_HASH, Header, addr, b256, bytes, u64v, u256};
use evm2::{
    Evm,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Database},
    interpreter::Word,
};
use mega_evm2::{
    MegaTypes,
    chain::{BootstrapInput, ChainConfig},
    factory::{BlockHeader, BlockInput, ParentContext, SatinFactory, Simulation},
    ledger::*,
};
use op_evm2::{OpTxEnvelope, TxDeposit};
use serde_json::{Map, Value, json};
use std::{cell::RefCell, collections::HashMap, convert::Infallible, rc::Rc};

/// The block state as an evm2 `Database`. Bytecode is kept per hash, analysed once (as a node's
/// code cache keeps it), so neither the jump-table analysis nor the hashing is in a run.
pub struct Evm2Db {
    pub db: Rc<RefCell<BlockDb>>,
    pub code_cache: Rc<RefCell<HashMap<B256, Bytecode>>>,
}

impl core::fmt::Debug for Evm2Db {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Evm2Db")
    }
}

impl Evm2Db {
    fn bytecode(&self, h: B256, code: Bytes) -> Bytecode {
        self.code_cache
            .borrow_mut()
            .entry(h)
            .or_insert_with(|| {
                let bc = Bytecode::new_raw(code);
                let _ = bc.legacy_jump_table();
                bc
            })
            .clone()
    }
}

impl Database for Evm2Db {
    type Error = Infallible;
    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Infallible> {
        let a = self.db.borrow_mut().account(*address);
        Ok(a.map(|(balance, nonce, code_hash, code)| {
            let mut info = AccountInfo::default().with_balance(balance);
            info.nonce = nonce;
            if code_hash != EMPTY_CODE_HASH {
                info.code = Some(self.bytecode(code_hash, code));
                info.code_hash = code_hash;
            }
            info
        }))
    }
    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Infallible> {
        let code = self.db.borrow_mut().code(*code_hash);
        Ok(self.bytecode(*code_hash, code))
    }
    fn get_storage(&mut self, address: &Address, key: &Word) -> Result<Word, Infallible> {
        Ok(self.db.borrow_mut().storage(*address, *key))
    }
    fn get_block_hash(&mut self, number: &Word) -> Result<B256, Infallible> {
        Ok(self.db.borrow_mut().block_hash(number.to::<u64>()))
    }
}

/// The block environment, as the Satin replay builds it (slot number 0; the blob base fee of
/// excess blob gas 0 is the minimum).
pub fn block_env(h: &Header) -> BlockEnv<MegaTypes> {
    assert_eq!(h.excess_blob_gas, 0, "only blocks without excess blob gas are replayed");
    BlockEnv::<MegaTypes> {
        number: U256::from(h.number),
        beneficiary: h.beneficiary,
        timestamp: U256::from(h.timestamp),
        gas_limit: U256::from(h.gas_limit),
        basefee: U256::from(h.base_fee),
        difficulty: h.difficulty,
        prevrandao: U256::from_be_bytes(h.mix_hash.0),
        blob_basefee: U256::from(1),
        slot_num: U256::ZERO,
        ..BlockEnv::<MegaTypes>::default()
    }
}

pub fn new_evm(h: &Header, chain_id: u64, db: Evm2Db) -> Evm<'static, MegaTypes> {
    new_evm_under(h, chain_id, db, &[])
}

/// The chain the gates execute the recorded blocks on: Satin and every fork below it active from
/// genesis (so every recorded block is a Satin block in which nothing activates), the recorded
/// block's chain id, and placeholder registry bootstrap values (no recorded block deploys the
/// registry).
pub fn gate_chain(chain_id: u64) -> ChainConfig {
    let bootstrap = BootstrapInput {
        initial_sequencer: Some(Address::repeat_byte(0x5e)),
        initial_admin: Some(Address::repeat_byte(0xad)),
        min_rotation_delay: Some(1),
    };
    ChainConfig::satin_from_genesis(chain_id, bootstrap).expect("the gate chain is valid")
}

/// The engine's factory on [`gate_chain`].
pub fn factory(chain_id: u64) -> SatinFactory {
    SatinFactory::new(gate_chain(chain_id))
}

/// The simulation of the reference's `rules` (`Lite::reference_rules`), `None` for Satin's.
pub fn simulation_under(rules: &[ReferenceRule]) -> Option<Simulation> {
    (!rules.is_empty())
        .then(|| Simulation { reference_rules: rules.to_vec(), ..Simulation::default() })
}

/// The recorded block's input: its header and its parent. The recording does not carry the
/// parent's timestamp; on [`gate_chain`], where every fork is active from genesis, no parent
/// timestamp changes what the block's activation is, and the second before the block is used.
pub fn block_input(h: &Header) -> BlockInput {
    BlockInput {
        header: BlockHeader {
            number: h.number,
            timestamp: h.timestamp,
            beneficiary: h.beneficiary,
            gas_limit: h.gas_limit,
            base_fee: h.base_fee,
            difficulty: h.difficulty,
            prevrandao: h.mix_hash,
            excess_blob_gas: h.excess_blob_gas,
            parent_hash: h.parent_hash,
            parent_beacon_block_root: h.parent_beacon_block_root,
        },
        parent: ParentContext::Parent {
            number: h.number - 1,
            hash: h.parent_hash,
            timestamp: h.timestamp.saturating_sub(1),
        },
    }
}

/// [`new_evm`] with the engine under the reference's `rules` (`Lite::reference_rules`), whose
/// version [`mega_evm2::satin_version_under`] builds: the factory's simulation of them, Satin's
/// own EVM when there are none.
pub fn new_evm_under(
    h: &Header,
    chain_id: u64,
    db: Evm2Db,
    rules: &[ReferenceRule],
) -> Evm<'static, MegaTypes> {
    let factory = factory(chain_id);
    let db = evm2::evm::Db::new(db);
    match simulation_under(rules) {
        None => factory.evm(block_env(h), db),
        Some(simulation) => factory.simulated_evm(block_env(h), db, &simulation),
    }
    .expect("a Satin block")
}

/// The transaction the Satin side recorded: its 2718 bytes for every non-deposit (an Ethereum
/// envelope, with the bytes kept for the L1 fee), the RPC fields for a deposit.
pub fn decode(raw: &Bytes, t: &Value) -> Result<Recovered<OpTxEnvelope>, String> {
    let sender = addr(&t["from"]);
    let ty = u64v(&t["type"]);
    let env = if ty == 0x7e {
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: b256(&t["sourceHash"]),
            from: sender,
            to: match t.get("to") {
                Some(to) if !to.is_null() => TxKind::Call(addr(to)),
                _ => TxKind::Create,
            },
            mint: u256(&t["mint"]).to(),
            value: u256(&t["value"]),
            gas_limit: u64v(&t["gas"]),
            is_system_transaction: t.get("isSystemTx").and_then(Value::as_bool).unwrap_or(false),
            input: bytes(&t["input"]),
        })
    } else {
        let e = alloy_consensus::EthereumTxEnvelope::<alloy_consensus::TxEip4844>::decode_2718(
            &mut raw.as_ref(),
        )
        .map_err(|e| format!("decode: {e}"))?;
        OpTxEnvelope::try_from(e).map_err(|e| format!("{e}"))?
    };
    Ok(Recovered::new_unchecked(env, sender))
}

/// Runs one transaction on a fresh `Evm` and renders it as the Satin replay does.
pub fn run_tx(
    h: &Header,
    chain_id: u64,
    db: &Rc<RefCell<BlockDb>>,
    code_cache: &Rc<RefCell<HashMap<B256, Bytecode>>>,
    tx: &Recovered<OpTxEnvelope>,
    inspected: bool,
) -> (Value, Vec<Value>) {
    run_tx_under(h, chain_id, db, code_cache, tx, inspected, &[])
}

/// [`run_tx`] with the engine under the reference's `rules` (`Lite::reference_rules`): the run a
/// replay transaction of a class of a change the reference has not made yet is checked with.
/// The fields of a replayed transaction's record ([`run_tx_under`]): an executed one's, the
/// outcome's ([`render_outcome`]) then the read set, the database misses and the changed accounts'
/// flags.
pub const RECORD_FIELDS: [&str; 20] = [
    "status",
    "halt",
    "output",
    "gas_used",
    "regular",
    "state",
    "history",
    "history_bytes",
    "reservoir_remaining",
    "data_size",
    "kv",
    "growth",
    "limit_exceeded",
    "logs",
    "accounts",
    "storage",
    "ledger",
    "reads",
    "misses",
    "account_flags",
];

/// The fields of a refused transaction's record ([`run_tx_under`]).
pub const REFUSED_FIELDS: [&str; 3] = ["error", "reads", "misses"];

/// Whether `record` has exactly the fields [`run_tx_under`] renders, [`RECORD_FIELDS`] or, with
/// `error`, [`REFUSED_FIELDS`]; `Err` names the missing and the unexpected fields (a regular-gas
/// invariant violation, [`crate::invariant::KEY`], among them).
pub fn check_record_fields(record: &Value) -> Result<(), String> {
    let o = record.as_object().ok_or("record fields: not an object")?;
    let want: &[&str] = if o.contains_key("error") { &REFUSED_FIELDS } else { &RECORD_FIELDS };
    let missing: Vec<_> = want.iter().filter(|k| !o.contains_key(**k)).collect();
    let unexpected: Vec<_> = o.keys().filter(|k| !want.contains(&k.as_str())).collect();
    if missing.is_empty() && unexpected.is_empty() {
        Ok(())
    } else {
        Err(format!("record fields: missing {missing:?}, unexpected {unexpected:?}"))
    }
}

pub fn run_tx_under(
    h: &Header,
    chain_id: u64,
    db: &Rc<RefCell<BlockDb>>,
    code_cache: &Rc<RefCell<HashMap<B256, Bytecode>>>,
    tx: &Recovered<OpTxEnvelope>,
    inspected: bool,
    rules: &[ReferenceRule],
) -> (Value, Vec<Value>) {
    db.borrow_mut().begin_run();
    let evm2db = Evm2Db { db: db.clone(), code_cache: code_cache.clone() };
    let mut evm = new_evm_under(h, chain_id, evm2db, rules);
    if inspected {
        evm.set_inspector(Frames::default());
    }
    // A run under Satin's rule is checked for the regular-gas invariant; a run under a
    // reference's rule is not (`invariant`).
    let checked = rules.is_empty();
    if checked {
        invariant::ran();
    }
    let (r, changes) = {
        let executed = match evm.transact(tx) {
            Ok(e) => e,
            Err(e) => {
                let d = db.borrow();
                return (
                    json!({"error": format!("{e:?}"), "reads": d.reads.to_json(), "misses": d.run_misses}),
                    Vec::new(),
                );
            }
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
    let (accounts, storage) = changed_json(&changes);
    let mut out = render_outcome(&r, ext, (accounts.clone(), storage));
    let d = db.borrow();
    let mut reads = d.reads.to_json();
    let ro = reads.as_object_mut().unwrap();
    ro.insert("buckets".into(), json!(ext.bucket_reads.iter().collect::<Vec<_>>()));
    ro.insert("oracle_reads".into(), json!(ext.oracle_reads));
    let flags: Map<String, Value> = changes
        .iter()
        .filter(|c| {
            c.touched &&
                (c.selfdestructed ||
                    c.created ||
                    accounts.contains_key(&format!("{}", c.address)) ||
                    !c.slots.is_empty())
        })
        .map(|c| {
            (format!("{}", c.address), json!({"created":c.created,"destroyed":c.selfdestructed}))
        })
        .collect();
    for (k, v) in
        [("reads", reads), ("misses", json!(d.run_misses)), ("account_flags", Value::Object(flags))]
    {
        out.insert(k.into(), v);
    }
    if checked {
        invariant::note(&mut out, invariant::violations(ext));
    }
    (Value::Object(out), frames)
}

/// The pre-block system calls (EIP-2935, EIP-4788, and the registry's due change), each on a
/// fresh `Evm` as the protocol's own work; each compared against the Satin call of the same name.
pub fn run_system_call(
    h: &Header,
    chain_id: u64,
    db: &Rc<RefCell<BlockDb>>,
    code_cache: &Rc<RefCell<HashMap<B256, Bytecode>>>,
    target: Address,
    data: Bytes,
    inspected: bool,
) -> Value {
    db.borrow_mut().begin_run();
    let mut evm = new_evm(h, chain_id, Evm2Db { db: db.clone(), code_cache: code_cache.clone() });
    if inspected {
        evm.set_inspector(Frames::default());
    }
    evm.ext_mut().begin_system_call();
    let tx = evm2::evm::SystemTx::new(target, data);
    let (r, changes) = match evm.system_call(tx) {
        Ok(executed) => {
            let changes = collect_changes(&executed);
            (executed.commit(), changes)
        }
        Err(e) => return json!({"error": format!("{e:?}")}),
    };
    let (accounts, storage) = changed_json(&changes);
    let d = db.borrow();
    json!({
        "status": if r.status { "success" } else { "fail" },
        "gas_used": r.tx_gas_used(),
        "accounts": accounts, "storage": storage,
        "reads": d.reads.to_json(), "misses": d.run_misses,
    })
}

/// Whether the registry has a role change due at `number` (Satin `is_apply_pending_changes_due`:
/// slots 7/8 pending system address and its activation block, 9/10 pending sequencer).
pub fn registry_due(db: &mut BlockDb, number: u64, slots: [(U256, U256); 2]) -> bool {
    if db.account(SEQUENCER_REGISTRY).is_none() {
        return false;
    }
    slots.iter().any(|(pending, activation)| {
        !db.storage(SEQUENCER_REGISTRY, *pending).is_zero() &&
            U256::from(number) >= db.storage(SEQUENCER_REGISTRY, *activation)
    })
}
