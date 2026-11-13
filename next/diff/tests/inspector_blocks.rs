//! The recorded blocks of the block gate under a heavy combination of the inspector crate's
//! tracers, against the same blocks run plain: observing a block must not change what it does.
//!
//! Each block runs twice through the engine's block session: plain, and with an inspector that
//! observes the whole block, its pre-block calls included (`BlockServices::inspector`): the Geth
//! struct logger (the options a node uses when the call names none) and the Geth mux of the call
//! tracer with logs, the flat call tracer and the four-byte tracer, side by side. As a node traces
//! a block, each transaction's traces are built after it runs and the inspector is fused before
//! the next. The two runs must agree on every transaction's outcome, receipt, gas, usage and state
//! changes, the block counters and the post-state; and each traced transaction's call trace on
//! the receipt's gas used and status.
//!
//! Environment: `MEGA_EVM2_BLOCK_DIR` (recorded blocks) and `MEGA_EVM2_REPLAY_REF` (the
//! per-transaction reference lines, for each transaction's raw bytes), as the block gate has them;
//! `diff/scripts/inspector-blocks.sh` sets them for the sample. `RUST_MIN_STACK=8388608` is
//! required. Output: `INSPECTED blocks=<b> txs=<t> traced=<n> equal=<b>`.

use alloy_primitives::{Address, Log, U256};
use evm2::{
    InspectorConfig, TxResultWithState,
    evm::{Db, InMemoryDB},
    inspector::Inspector,
    interpreter::{Interpreter, Message, MessageResult},
};
use evm2_inspectors::tracing::DebugInspector;
use mega_evm2::{MegaTypes, block::encode_receipt, factory::BlockServices};
use mega_evm2_diff::replay::{self, Evm2Db, blockdb};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::HashMap, io::BufRead, rc::Rc};

/// The struct logger and the mux, observing one block side by side.
type Heavy = (DebugInspector, DebugInspector);

fn heavy() -> Heavy {
    let options = |v: Value| serde_json::from_value(v).expect("tracing options");
    (
        DebugInspector::new(options(json!({}))).unwrap(),
        DebugInspector::new(options(json!({"tracer": "muxTracer", "tracerConfig": {
            "callTracer": {"withLog": true},
            "flatCallTracer": {},
            "4byteTracer": {},
        }})))
        .unwrap(),
    )
}

/// The block session owns its inspector; the test keeps a handle on it to build each
/// transaction's traces and fuse it.
struct Shared(Rc<RefCell<Heavy>>);

impl Inspector<MegaTypes> for Shared {
    fn inspector_config(&self) -> InspectorConfig {
        Inspector::<MegaTypes>::inspector_config(&*self.0.borrow())
    }
    fn initialize_interp(&mut self, i: &mut Interpreter<'_, '_, MegaTypes>) {
        Inspector::<MegaTypes>::initialize_interp(&mut *self.0.borrow_mut(), i);
    }
    fn step(&mut self, i: &mut Interpreter<'_, '_, MegaTypes>) {
        Inspector::<MegaTypes>::step(&mut *self.0.borrow_mut(), i);
    }
    fn step_end(&mut self, i: &mut Interpreter<'_, '_, MegaTypes>) {
        Inspector::<MegaTypes>::step_end(&mut *self.0.borrow_mut(), i);
    }
    fn log(&mut self, log: &Log, host: &mut evm2::Evm<'_, MegaTypes>) {
        Inspector::<MegaTypes>::log(&mut *self.0.borrow_mut(), log, host);
    }
    fn call(
        &mut self,
        i: &mut Interpreter<'_, '_, MegaTypes>,
        m: &mut Message<MegaTypes>,
    ) -> Option<MessageResult<MegaTypes>> {
        Inspector::<MegaTypes>::call(&mut *self.0.borrow_mut(), i, m)
    }
    fn call_end(
        &mut self,
        i: &mut Interpreter<'_, '_, MegaTypes>,
        m: &Message<MegaTypes>,
        r: &mut MessageResult<MegaTypes>,
    ) {
        Inspector::<MegaTypes>::call_end(&mut *self.0.borrow_mut(), i, m, r);
    }
    fn create(
        &mut self,
        i: &mut Interpreter<'_, '_, MegaTypes>,
        m: &mut Message<MegaTypes>,
    ) -> Option<MessageResult<MegaTypes>> {
        Inspector::<MegaTypes>::create(&mut *self.0.borrow_mut(), i, m)
    }
    fn create_end(
        &mut self,
        i: &mut Interpreter<'_, '_, MegaTypes>,
        m: &Message<MegaTypes>,
        r: &mut MessageResult<MegaTypes>,
    ) {
        Inspector::<MegaTypes>::create_end(&mut *self.0.borrow_mut(), i, m, r);
    }
    fn selfdestruct(
        &mut self,
        c: &Address,
        t: &Address,
        v: &U256,
        host: &mut evm2::Evm<'_, MegaTypes>,
    ) {
        Inspector::<MegaTypes>::selfdestruct(&mut *self.0.borrow_mut(), c, t, v, host);
    }
}

fn read_lines(path: &str) -> Vec<Value> {
    let f = std::fs::File::open(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    std::io::BufReader::new(f).lines().map(|l| serde_json::from_str(&l.unwrap()).unwrap()).collect()
}

/// What one run of a block did: one row per transaction, the block counters and the post-state;
/// and, under the inspector, the transactions it traced.
struct BlockRun {
    rows: Vec<Value>,
    traced: u64,
}

fn run_block(
    set: &str,
    path: &str,
    raws: &[Value],
    inspector: Option<Rc<RefCell<Heavy>>>,
) -> BlockRun {
    let b = blockdb::load_block(path, set);
    let database = Db::new(Evm2Db {
        db: Rc::new(RefCell::new(b.db)),
        code_cache: Rc::new(RefCell::new(HashMap::new())),
    });
    let services = BlockServices {
        inspector: inspector.clone().map(|i| Box::new(Shared(i)) as Box<dyn Inspector<MegaTypes>>),
        ..replay::services(&b.header)
    };
    let mut session = replay::factory(b.chain_id)
        .start_block(&replay::block_input(&b.header), database, services)
        .expect("pre-block");
    if let Some(i) = &inspector {
        // The pre-block calls were observed; a node traces the transactions after them.
        let (structs, mux) = &mut *i.borrow_mut();
        structs.fuse().unwrap();
        mux.fuse().unwrap();
    }
    let mut rows = Vec::new();
    let mut traced = 0;
    for (index, t) in b.txs.iter().enumerate() {
        let raw = blockdb::bytes(&raws[index]["raw"]);
        let tx = replay::decode(&raw, t).expect("decode");
        let mut row = json!({"index": index});
        match session.execute_candidate(&tx, &raw) {
            Err(e) => row["refused"] = json!(e.to_string()),
            Ok(out) => {
                let r = out.result();
                row["result"] = json!(format!("{r:?}"));
                row["gas"] = json!(out.gas());
                row["usage"] = json!(out.usage());
                row["gas_used"] = json!(out.gas_used());
                let mut changes: Vec<String> = out
                    .pending()
                    .account_changes()
                    .map(|c| {
                        let mut slots: Vec<_> =
                            c.storage.changed_slots().map(|(k, s)| (*k, s.current)).collect();
                        slots.sort();
                        format!(
                            "{} {:?} {:?} {} {} {} {slots:?}",
                            c.address,
                            c.original,
                            c.current,
                            c.created,
                            c.selfdestructed,
                            c.touched
                        )
                    })
                    .collect();
                changes.sort();
                row["changes"] = json!(changes);
                if let Some(i) = &inspector {
                    let result = TxResultWithState::<MegaTypes> {
                        result: r.clone(),
                        pending_state: out.pending().clone(),
                        ..Default::default()
                    };
                    let block = *session.evm().block();
                    let mut db = Db::new(InMemoryDB::default());
                    let (structs, mux) = &mut *i.borrow_mut();
                    let s = structs.get_result(None, &tx, &block, &result, &mut db).unwrap();
                    let m = mux.get_result(None, &tx, &block, &result, &mut db).unwrap();
                    let (s, m) =
                        (serde_json::to_value(s).unwrap(), serde_json::to_value(m).unwrap());
                    let call = &m["callTracer"];
                    let used = |v: &Value| {
                        v.as_str()
                            .map(|h| u64::from_str_radix(h.trim_start_matches("0x"), 16).unwrap())
                    };
                    assert_eq!(
                        used(&call["gasUsed"]),
                        Some(r.tx_gas_used()),
                        "{set}:{}:{index}: the call trace's gasUsed",
                        b.number
                    );
                    assert_eq!(
                        call["error"].is_null(),
                        r.status,
                        "{set}:{}:{index}: the call trace's status",
                        b.number
                    );
                    assert_eq!(
                        s["failed"],
                        json!(!r.status),
                        "{set}:{}:{index}: the struct logs' status",
                        b.number
                    );
                    assert!(m["flatCallTracer"].as_array().is_some_and(|f| !f.is_empty()));
                    structs.fuse().unwrap();
                    mux.fuse().unwrap();
                    traced += 1;
                }
                match session.commit_candidate(out) {
                    Err(e) => row["refused_at_commit"] = json!(e.to_string()),
                    Ok(_) => {
                        let receipt = session.receipts().last().unwrap();
                        row["receipt"] = json!(blockdb::hex(&encode_receipt(receipt)));
                    }
                }
            }
        }
        rows.push(row);
    }
    let l = session.limiter();
    rows.push(json!({"block_gas_used": l.block_gas_used, "da_footprint_used": l.block_da_footprint_used, "usage": l.usage, "gas": l.gas}));
    let mut accounts: Vec<String> =
        session.changes().accounts().map(|(a, t)| format!("{a} {t:?}")).collect();
    accounts.sort();
    let mut storage: Vec<String> = session
        .changes()
        .storage()
        .map(|(k, t)| format!("{}:{:#x} {t:?}", k.address(), k.key()))
        .collect();
    storage.sort();
    rows.push(json!({"accounts": accounts, "storage": storage}));
    BlockRun { rows, traced }
}

#[test]
fn inspector_plain_recorded_blocks_under_the_struct_logger_and_the_mux() {
    let reference = std::env::var("MEGA_EVM2_REPLAY_REF").expect("MEGA_EVM2_REPLAY_REF");
    let (mut blocks, mut txs, mut traced, mut equal) = (0u64, 0u64, 0u64, 0u64);
    for (set, path) in blockdb::block_list() {
        let n = blockdb::load_block(&path, &set).number;
        let raws: Vec<Value> = read_lines(&format!("{reference}/{set}/{n}.jsonl"))
            .into_iter()
            .filter(|l| l["kind"] == "tx")
            .collect();
        let plain = run_block(&set, &path, &raws, None);
        let inspected = run_block(&set, &path, &raws, Some(Rc::new(RefCell::new(heavy()))));
        assert_eq!(plain.rows.len(), inspected.rows.len());
        for (p, q) in plain.rows.iter().zip(&inspected.rows) {
            assert_eq!(p, q, "{set}:{n}: the inspected run differs from the plain one");
        }
        blocks += 1;
        txs += raws.len() as u64;
        traced += inspected.traced;
        equal += 1;
    }
    assert!(blocks > 0);
    println!("INSPECTED blocks={blocks} txs={txs} traced={traced} equal={equal}");
}
