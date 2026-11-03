//! Reference side of the engine replay gate: replays the recorded blocks on the Satin
//! implementation under review (transaction by transaction, plain and inspected) and writes one
//! JSON line per transaction and one for the pre-block calls to
//! `$MEGA_EVM2_REPLAY_OUT/<set>/<block>.jsonl`.
//!
//! Environment: `MEGA_EVM2_BLOCK_DIR` (recorded blocks), `MEGA_EVM2_REPLAY_OUT`,
//! optionally `MEGA_EVM2_SETS` / `MEGA_EVM2_BLOCKS` and `MEGA_EVM2_EXPECT_TXS`.

#![allow(unreachable_pub, deprecated, missing_docs, clippy::all)]

use alloy_evm::Evm as _;
use mega_evm::{
    MegaEvm, MegaHardforkConfig,
    test_utils::{WitnessRecord, record_start_ledger_settlements},
};
use mega_evm2_reference::{
    Envs, Frames, SatinDb,
    blockdb::{self, Block, BlockDb, addr, bytes, hex, u64v},
    context, diff_of, envelope, ledger_json, mega_tx, outcome_json, system_json,
};
use revm::context_interface::result::ResultAndState;
use serde_json::{Value, json};
use std::{cell::RefCell, io::Write, rc::Rc, time::Instant};

fn run_block(b: Block, out: &mut impl Write) -> (usize, usize) {
    let number = b.number;
    let db = Rc::new(RefCell::new(BlockDb { record: true, ..BlockDb::default() }));
    {
        let mut d = db.borrow_mut();
        let src = b.db;
        d.accounts = src.accounts;
        d.codes = src.codes;
        d.block_hashes = src.block_hashes;
    }
    let b = Block { db: BlockDb::default(), ..b };
    let hf = MegaHardforkConfig::default().with_all_activated();

    // Pre-block calls, each on a fresh EVM, each committed.
    let mut pre = Vec::new();
    let mut step = |name: &str,
                    f: &mut dyn FnMut(
        &mut MegaEvm<SatinDb, Frames, Envs>,
    ) -> Option<ResultAndState<op_revm::OpHaltReason>>| {
        let mut results = Vec::new();
        for inspected in [false, true] {
            db.borrow_mut().begin_run();
            let record = Rc::new(RefCell::new(WitnessRecord::default()));
            let mut evm = MegaEvm::new(context(&b, &db, record)).with_inspector(Frames::default());
            evm.set_inspector_enabled(inspected);
            let r = f(&mut evm);
            drop(evm);
            results.push(r.map(|r| {
                let mut j = system_json(&r, &db.borrow());
                j["diff"] = diff_of(&r.state);
                j
            }));
        }
        assert_eq!(results[0], results[1], "pre-block reference inspector {number}:{name}");
        if let Some(mut j) = results.remove(0) {
            j["name"] = json!(name);
            j["inspected_equal"] = json!(true);
            db.borrow_mut().apply_diff(&j["diff"]);
            pre.push(j);
        }
    };
    let parent = b.header.parent_hash;
    let root = b.header.parent_beacon_block_root;
    step("eip2935", &mut |evm| {
        mega_evm::transact_blockhashes_contract_call(hf.clone(), parent, evm).expect("2935")
    });
    step("eip4788", &mut |evm| {
        mega_evm::transact_beacon_root_contract_call(hf.clone(), root, evm).expect("4788")
    });
    let due = {
        let mut sdb = SatinDb(db.clone());
        db.borrow_mut().begin_run();
        let (due, _) =
            mega_evm::system::is_apply_pending_changes_due(&mut sdb, number).expect("registry");
        due
    };
    if due {
        step("apply_pending_changes", &mut |evm| {
            Some(mega_evm::system::transact_apply_pending_changes(evm).expect("registry call"))
        });
    }
    writeln!(out, "{}", json!({"kind": "pre_block", "set": b.set, "block": number, "registry_due": due, "calls": pre})).unwrap();

    let mut errors = 0;
    for (i, t) in b.txs.iter().enumerate() {
        let hash = t["hash"].as_str().unwrap().to_string();
        let sender = addr(&t["from"]);
        let ty = u64v(&t["type"]);
        let mut line = json!({"kind": "tx", "set": b.set, "block": number, "index": i, "hash": hash, "type": ty, "from": format!("{sender}"),
            "to": t.get("to").filter(|x| !x.is_null()).map(|x| x.as_str().unwrap().to_lowercase()), "input_len": bytes(&t["input"]).len(),
            "chain_status": b.receipts.get(i).map(|r| r["status"].clone()), "chain_gas": b.receipts.get(i).map(|r| u64v(&r["gasUsed"]))});
        let (env, raw) = match envelope(t) {
            Ok(x) => x,
            Err(e) => {
                errors += 1;
                line.as_object_mut().unwrap().insert("envelope_error".into(), json!(e));
                writeln!(out, "{line}").unwrap();
                continue;
            }
        };
        line.as_object_mut().unwrap().insert("raw".into(), json!(hex(&raw)));

        // Plain.
        db.borrow_mut().begin_run();
        let record = Rc::new(RefCell::new(WitnessRecord::default()));
        let mut evm = MegaEvm::new(context(&b, &db, record.clone()));
        record_start_ledger_settlements(evm.ctx_mut());
        let t0 = Instant::now();
        let plain = evm.execute_transaction(mega_tx(&env, &raw, sender));
        let micros = t0.elapsed().as_micros() as u64;
        let plain_ledger = ledger_json(&mut evm);
        drop(evm);
        let (mut plain_json, diff) = match &plain {
            Ok(o) => (outcome_json(o, &db.borrow(), &record.borrow()), Some(diff_of(&o.state))),
            Err(e) => (
                json!({"error": format!("{e}"), "reads": db.borrow().reads.to_json(), "misses": db.borrow().run_misses}),
                None,
            ),
        };

        if plain.is_ok() {
            plain_json["ledger"] = plain_ledger;
        }

        // Inspected, on the same pre-state.
        db.borrow_mut().begin_run();
        let record2 = Rc::new(RefCell::new(WitnessRecord::default()));
        let mut evm =
            MegaEvm::new(context(&b, &db, record2.clone())).with_inspector(Frames::default());
        record_start_ledger_settlements(evm.ctx_mut());
        let inspected = evm.execute_transaction(mega_tx(&env, &raw, sender));
        let inspected_ledger = ledger_json(&mut evm);
        let frames = evm.inspector().frames.clone();
        drop(evm);
        let mut inspected_json = match &inspected {
            Ok(o) => outcome_json(o, &db.borrow(), &record2.borrow()),
            Err(e) => {
                json!({"error": format!("{e}"), "reads": db.borrow().reads.to_json(), "misses": db.borrow().run_misses})
            }
        };
        if inspected.is_ok() {
            inspected_json["ledger"] = inspected_ledger;
        }
        let same = plain_json == inspected_json;
        assert!(same, "reference plain/inspected differ {number}:{i}");
        let o = line.as_object_mut().unwrap();
        o.insert("satin".into(), plain_json);
        o.insert("inspected_equal".into(), json!(same));
        if !same {
            o.insert("satin_inspected".into(), inspected_json);
        }
        o.insert("frames".into(), Value::Array(frames));
        o.insert("micros".into(), json!(micros));
        if let Some(diff) = diff {
            db.borrow_mut().apply_diff(&diff);
            o.insert("diff".into(), diff);
        }
        writeln!(out, "{line}").unwrap();
    }
    let misses = db.borrow().misses.len();
    writeln!(out, "{}", json!({"kind": "block_end", "set": b.set, "block": number, "txs": b.txs.len(), "envelope_errors": errors, "misses_total": misses})).unwrap();
    (b.txs.len(), errors)
}

#[test]
fn test_reference_replay() {
    let out_dir = std::env::var("MEGA_EVM2_REPLAY_OUT").expect("MEGA_EVM2_REPLAY_OUT");
    let list = blockdb::block_list();
    let t0 = Instant::now();
    let mut total = 0;
    for (set, path) in list {
        let b = blockdb::load_block(&path, &set);
        let n = b.number;
        std::fs::create_dir_all(format!("{out_dir}/{set}")).unwrap();
        let mut f = std::io::BufWriter::new(
            std::fs::File::create(format!("{out_dir}/{set}/{n}.jsonl")).unwrap(),
        );
        let (txs, errors) = run_block(b, &mut f);
        total += txs;
        println!(
            "{set}:{n} txs={txs} envelope_errors={errors} elapsed={:.1}s",
            t0.elapsed().as_secs_f64()
        );
    }
    println!("total txs {total}");
    assert!(total > 0);
    if let Ok(n) = std::env::var("MEGA_EVM2_EXPECT_TXS") {
        assert_eq!(total, n.parse::<usize>().unwrap());
    }
}
