//! Block executor gate: every recorded block through the Satin engine's `BlockExecutor`, plain and
//! inspected, against the reference's block-level output. Each side commits its own state.
//! All deliberate normalizations/exclusions are in `src/gate.rs`.
//!
//! Environment: `MEGA_EVM2_BLOCK_DIR` (recorded blocks), `MEGA_EVM2_REPLAY_REF` (per-transaction
//! reference lines), `MEGA_EVM2_BLOCK_REF` (block-level reference lines), the optional
//! `MEGA_EVM2_EXPECT` count check of `src/gate.rs`; `MEGA_EVM2_OUT` additionally writes every
//! comparison.
//! `RUST_MIN_STACK=8388608` is required. Output: the `GATE` summary line.
use alloy_primitives::B256;
use evm2::evm::AccountInfo;
use mega_evm2::{
    ReferenceRule,
    block::{BlockExecutor, encode_receipt},
};
use mega_evm2_diff::{
    driver::{self, Frames},
    gate,
    replay::{self, Evm2Db, blockdb},
};
use serde_json::{Map, Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    io::{BufRead, Write},
    rc::Rc,
};

fn read_lines(path: &str) -> Vec<Value> {
    let f = std::fs::File::open(path).unwrap_or_else(|e| panic!("oracle {path}: {e}"));
    std::io::BufReader::new(f).lines().map(|l| serde_json::from_str(&l.unwrap()).unwrap()).collect()
}
fn render_info(i: Option<&AccountInfo>) -> Value {
    match i {
        Some(i)
            if !(i.balance.is_zero() &&
                i.nonce == 0 &&
                (i.code_hash == alloy_primitives::KECCAK256_EMPTY ||
                    i.code_hash == B256::ZERO)) =>
        {
            json!(format!(
                "{}/n{}/{}",
                i.balance,
                i.nonce,
                if i.code_hash == B256::ZERO {
                    alloy_primitives::KECCAK256_EMPTY
                } else {
                    i.code_hash
                }
            ))
        }
        _ => json!("absent"),
    }
}
fn pending_diff(p: &evm2::evm::PendingState) -> Value {
    let mut accounts = Map::new();
    for c in p.account_changes() {
        if !c.touched {
            continue;
        }
        let orig = c.original.cloned().unwrap_or_default();
        let cur = c.current.cloned().unwrap_or_default();
        let storage: Map<String, Value> = c
            .storage
            .changed_slots()
            .map(|(k, s)| (format!("{k:#x}"), json!(format!("{:#x}", s.current))))
            .collect();
        let changed =
            (orig.balance, orig.nonce, orig.code_hash) != (cur.balance, cur.nonce, cur.code_hash);
        if !c.selfdestructed && !c.created && !changed && storage.is_empty() {
            continue;
        }
        let code = if cur.code_hash != orig.code_hash || c.created {
            cur.code.as_ref().map(|b| json!(blockdb::hex(&b.original_bytes())))
        } else {
            None
        };
        accounts.insert(format!("{}",c.address),json!({"balance":format!("{:#x}",cur.balance),"nonce":cur.nonce,"code_hash":format!("{}",cur.code_hash),"code":code,"created":c.created,"destroyed":c.selfdestructed,"storage":storage}));
    }
    json!({"accounts":accounts})
}
fn run_block(
    set: &str,
    path: &str,
    st: &[Value],
    inspected: bool,
    rule: Option<ReferenceRule>,
) -> Vec<Value> {
    let b = blockdb::load_block(path, set);
    let n = b.number;
    let txlines: Vec<_> = st.iter().filter(|l| l["kind"] == "tx").collect();
    assert_eq!(txlines.len(), b.txs.len(), "engine oracle count {set}:{n}");
    let db = Rc::new(RefCell::new(b.db));
    let mut evm = replay::new_evm_under(
        &b.header,
        b.chain_id,
        Evm2Db { db, code_cache: Rc::new(RefCell::new(HashMap::new())) },
        rule,
    );
    if inspected {
        evm.set_inspector(Frames::default());
    }
    let mut ex = BlockExecutor::new(evm, b.header.parent_hash, b.header.parent_beacon_block_root);
    ex.apply_pre_execution_changes().expect("pre-block");
    let mut rows = Vec::new();
    for (i, t) in b.txs.iter().enumerate() {
        let l = txlines[i];
        assert_eq!(l["index"], json!(i));
        assert_eq!(l["hash"], t["hash"]);
        assert!(l.get("envelope_error").is_none(), "invalid envelope");
        let raw = blockdb::bytes(&l["raw"]);
        let tx = replay::decode(&raw, t).expect("decode");
        let mut row = json!({"kind":"tx","block":n,"index":i,"hash":t["hash"],"type":t["type"]});
        match ex.run_transaction(&tx, &raw) {
            Err(e) => {
                row["status"] = json!("refused");
                row["reason"] = json!(e);
            }
            Ok(out) => {
                let r = &out.result;
                let (status, halt) = driver::status_halt(r);
                row["status"] = status;
                row["halt"] = halt;
                row["output"] = json!(blockdb::hex(&r.output));
                row["gas_used"] = json!(r.tx_gas_used());
                row["receipt_gas"] = json!(out.gas_used);
                row["usage"] = json!(out.usage);
                row["gas_split"] = json!(out.gas);
                row["system_tx"] = json!(out.is_system_transaction);
                row["da_size"] = json!(out.da_size);
                row["tx_size"] = json!(out.tx_size);
                row["diff"] = pending_diff(&out.pending);
                match ex.commit_transaction_outcome(out) {
                    Err(e) => {
                        row["status"] = json!("refused_at_commit");
                        row["reason"] = json!(e);
                    }
                    Ok(_) => {
                        let r = ex.receipts.last().unwrap();
                        row["receipt"] = json!(blockdb::hex(&encode_receipt(r)));
                        row["receipt_status"] = json!(r.status);
                        row["cumulative_gas_used"] = json!(r.cumulative_gas_used);
                        row["logs"]=json!(r.logs.iter().map(|l|json!({"address":format!("{}",l.address),"topics":l.topics().iter().map(|t|format!("{t}")).collect::<Vec<_>>(),"data":blockdb::hex(&l.data.data)})).collect::<Vec<_>>());
                    }
                }
            }
        }
        rows.push(row);
    }
    let l = &ex.limiter;
    rows.push(json!({"kind":"block_counters","block":n,"gas_used":l.block_gas_used,"da_footprint_used":l.block_da_footprint_used,"da_size_used":l.block_da_size_used,"tx_size_used":l.block_tx_size_used,"usage":l.usage,"gas":l.gas}));
    let accounts: Map<String, Value> = ex
        .changes
        .accounts()
        .filter_map(|(a, t)| {
            let (o, c) = (render_info(t.original.as_ref()), render_info(t.current.as_ref()));
            (o != c).then(|| (format!("{a}"), c))
        })
        .collect();
    let storage: Map<String, Value> = ex
        .changes
        .storage()
        .filter(|(_, t)| t.original != t.current)
        .map(|(k, t)| {
            (format!("{}:{:#x}", k.address(), k.key()), json!(format!("{:#x}", t.current)))
        })
        .collect();
    rows.push(json!({"kind":"post_state","block":n,"accounts":accounts,"storage":storage}));
    rows
}
/// The differences of one row of a block: the reference's row `s` against the engine's plain row
/// `p`, the engine's plain against its inspected row `q`, and the reference's own plain and
/// inspected runs disagreeing.
fn row_differences(s: &Value, p: &Value, q: &Value) -> Vec<String> {
    let mut d = gate::differences(s, p);
    d.extend(
        gate::same_engine_differences(p, q).into_iter().map(|f| format!("evm2_inspected:{f}")),
    );
    if s["inspected_equal"] != true {
        d.push("satin_plain_vs_inspected".into());
    }
    d
}

#[test]
fn test_blocks_match_reference_plain_and_inspected() {
    let reference = std::env::var("MEGA_EVM2_BLOCK_REF").expect("MEGA_EVM2_BLOCK_REF");
    let txref = std::env::var("MEGA_EVM2_REPLAY_REF").expect("MEGA_EVM2_REPLAY_REF");
    let outdir = std::env::var("MEGA_EVM2_OUT").ok();
    let classes = gate::known_classes();
    let mut reruns: Vec<(String, &str)> = Vec::new();
    let mut totals = BTreeMap::<String, u64>::new();
    let mut fields = BTreeMap::<String, u64>::new();
    let mut processed = Vec::new();
    for (set, path) in blockdb::block_list() {
        let b = blockdb::load_block(&path, &set);
        let n = b.number;
        processed.push(format!("{set}:{n}"));
        let txcount = b.txs.len();
        drop(b);
        let sb = read_lines(&format!("{reference}/{set}/{n}.jsonl"));
        let st = read_lines(&format!("{txref}/{set}/{n}.jsonl"));
        assert_eq!(sb.len(), txcount + 2, "oracle count {set}:{n}");
        let mut p = run_block(&set, &path, &st, false, None);
        let mut q = run_block(&set, &path, &st, true, None);
        assert_eq!(p.len(), sb.len());
        assert_eq!(q.len(), sb.len());
        // A block that differs is of a class the gate runs blocks again for, when the file enables
        // it, if the whole block run again under its rule, plain and inspected, has no difference
        // from any row; its rows are then that run's, and count as they would.
        let mut class = None;
        if sb.iter().zip(&p).zip(&q).any(|((s, p), q)| !row_differences(s, p, q).is_empty()) {
            for (name, rule) in gate::REPLAY_RERUN_CLASSES {
                if !classes.iter().any(|c| c == name) {
                    continue;
                }
                let p2 = run_block(&set, &path, &st, false, Some(rule));
                let q2 = run_block(&set, &path, &st, true, Some(rule));
                if sb
                    .iter()
                    .zip(&p2)
                    .zip(&q2)
                    .all(|((s, p), q)| row_differences(s, p, q).is_empty())
                {
                    (p, q, class) = (p2, q2, Some(name));
                    reruns.push((format!("{set}:{n}"), name));
                    break;
                }
            }
        }
        let mut out: Box<dyn Write> = match &outdir {
            Some(dir) => {
                std::fs::create_dir_all(format!("{dir}/{set}")).unwrap();
                Box::new(std::io::BufWriter::new(
                    std::fs::File::create(format!("{dir}/{set}/{n}.jsonl")).unwrap(),
                ))
            }
            None => Box::new(std::io::sink()),
        };
        let mut bc = BTreeMap::<String, u64>::new();
        for (i, ((s, p), q)) in sb.iter().zip(&p).zip(&q).enumerate() {
            let d = row_differences(s, p, q);
            for f in &d {
                *fields.entry(f.clone()).or_default() += 1;
            }
            let verdict = if !d.is_empty() {
                format!(
                    "{}differ",
                    if i == txcount {
                        "block_counters:"
                    } else if i == txcount + 1 {
                        "post_state:"
                    } else {
                        ""
                    }
                )
            } else if i == txcount {
                "block_counters:match".into()
            } else if i == txcount + 1 {
                "post_state:match".into()
            } else if s["status"].as_str().unwrap().starts_with("refused") {
                format!("refused_both:{}", gate::error_kind(s["reason"].as_str().unwrap()))
            } else {
                "match".into()
            };
            *totals.entry(verdict.clone()).or_default() += 1;
            *bc.entry(verdict.clone()).or_default() += 1;
            writeln!(out,"{}",json!({"kind":s["kind"],"block":n,"index":s["index"],"verdict":verdict,"class":class,"fields":d,"satin":if d.is_empty(){Value::Null}else{s.clone()},"evm2":if d.is_empty(){Value::Null}else{p.clone()},"inspected":if d.is_empty(){Value::Null}else{q.clone()}})).unwrap();
        }
        match class {
            Some(class) => println!("{set}:{n} {bc:?} class={class}"),
            None => println!("{set}:{n} {bc:?}"),
        }
        *totals.entry("blocks".into()).or_default() += 1;
    }
    println!("TOTAL {totals:?}");
    println!("FIELDS {fields:?}");
    // The blocks a class took by its rerun, which the summary counts as their rows' verdicts: their
    // number is pinned on its own (`common.sh`).
    println!("RERUN block blocks={} {reruns:?}", reruns.len());
    assert!(!totals.is_empty());
    // Only a run of exactly the manifest's blocks is the gate: any other selection prints a
    // line `gate.sh` does not take for a summary.
    let complete = blockdb::check_processed_identities(&processed);
    println!("{}{}", if complete { "" } else { "PARTIAL " }, gate::block_line(&totals));
    gate::expected("MEGA_EVM2_EXPECT_RERUN", reruns.len() as u64);
    gate::finish(&totals);
}
