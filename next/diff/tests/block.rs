//! Block executor gate: every recorded block through the Satin engine's `BlockExecutor`, plain and
//! inspected, against the reference's block-level output. Each side commits its own state.
//! All deliberate normalizations/exclusions are in `shared/gate.rs`.
//!
//! Environment: `MEGA_EVM2_BLOCK_DIR` (recorded blocks), `MEGA_EVM2_REPLAY_REF` (per-transaction
//! reference lines), `MEGA_EVM2_BLOCK_REF` (block-level reference lines), the count expectations of
//! `shared/gate.rs`; `MEGA_EVM2_OUT` additionally writes every comparison.
//! `RUST_MIN_STACK=8388608` is required. Output: the `GATE` summary line.
use alloy_primitives::B256;
use evm2::evm::AccountInfo;
use mega_evm2::block::{BlockExecutor, encode_receipt};
use mega_evm2_diff::{
    driver::Frames,
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
fn run_block(set: &str, path: &str, st: &[Value], sb: &[Value], inspected: bool) -> Vec<Value> {
    let b = blockdb::load_block(path, set);
    let n = b.number;
    let txlines: Vec<_> = st.iter().filter(|l| l["kind"] == "tx").collect();
    assert_eq!(txlines.len(), b.txs.len(), "engine oracle count {set}:{n}");
    let db = Rc::new(RefCell::new(b.db));
    let mut evm = replay::new_evm(
        &b.header,
        b.chain_id,
        Evm2Db { db, code_cache: Rc::new(RefCell::new(HashMap::new())), pause: None },
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
        let apply = std::env::var("MEGA_EVM2_SATIN_APPLY").unwrap_or_default();
        let kind = if l["type"] == 4 {
            "eip7702"
        } else if l["frames"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["target"] == "0x6342000000000000000000000000000000000003")
        {
            "keyless"
        } else {
            ""
        };
        if !kind.is_empty() && apply.split(',').any(|x| x == kind) {
            let s = &sb[i];
            mega_evm2_diff::satin_applied::apply(&mut ex, s, blockdb::u64v(&t["type"]) as u8);
            row = s.clone();
            row["satin_applied"] = json!(kind);
            rows.push(row);
            continue;
        }
        match ex.run_transaction(&tx, &raw) {
            Err(e) => {
                row["status"] = json!("refused");
                row["reason"] = json!(e);
            }
            Ok(out) => {
                let r = &out.result;
                row["status"] = json!(if r.ext.failed_deposit {
                    "halt".to_string()
                } else {
                    mega_evm2_diff::driver::status_of(r.stop)
                });
                row["halt"] = if r.ext.failed_deposit {
                    json!("FailedDeposit")
                } else if r.stop.is_halt() {
                    json!(format!("{:?}", r.stop))
                } else {
                    Value::Null
                };
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
#[test]
fn test_blocks_match_reference_plain_and_inspected() {
    let reference = std::env::var("MEGA_EVM2_BLOCK_REF").expect("MEGA_EVM2_BLOCK_REF");
    let txref = std::env::var("MEGA_EVM2_REPLAY_REF").expect("MEGA_EVM2_REPLAY_REF");
    let outdir = std::env::var("MEGA_EVM2_OUT").ok();
    let mut totals = BTreeMap::<String, u64>::new();
    let mut fields = BTreeMap::<String, u64>::new();
    for (set, path) in blockdb::block_list() {
        let b = blockdb::load_block(&path, &set);
        let n = b.number;
        let txcount = b.txs.len();
        drop(b);
        let sb = read_lines(&format!("{reference}/{set}/{n}.jsonl"));
        let st = read_lines(&format!("{txref}/{set}/{n}.jsonl"));
        assert_eq!(sb.len(), txcount + 2, "oracle count {set}:{n}");
        let p = run_block(&set, &path, &st, &sb, false);
        let q = run_block(&set, &path, &st, &sb, true);
        assert_eq!(p.len(), sb.len());
        assert_eq!(q.len(), sb.len());
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
            let mut d = gate::differences(s, p);
            d.extend(
                gate::same_engine_differences(p, q)
                    .into_iter()
                    .map(|f| format!("evm2_inspected:{f}")),
            );
            if s["inspected_equal"] != true {
                d.push("satin_plain_vs_inspected".into());
            }
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
            } else if p.get("satin_applied").is_some() {
                format!("satin_applied:{}", p["satin_applied"])
            } else if s["status"].as_str().unwrap().starts_with("refused") {
                format!("refused_both:{}", gate::error_kind(s["reason"].as_str().unwrap()))
            } else {
                "match".into()
            };
            *totals.entry(verdict.clone()).or_default() += 1;
            *bc.entry(verdict.clone()).or_default() += 1;
            writeln!(out,"{}",json!({"kind":s["kind"],"block":n,"index":s["index"],"verdict":verdict,"fields":d,"satin":if d.is_empty(){Value::Null}else{s.clone()},"evm2":if d.is_empty(){Value::Null}else{p.clone()},"inspected":if d.is_empty(){Value::Null}else{q.clone()}})).unwrap();
        }
        println!("{set}:{n} {bc:?}");
        *totals.entry("blocks".into()).or_default() += 1;
    }
    println!("TOTAL {totals:?}");
    println!("FIELDS {fields:?}");
    assert!(!totals.is_empty());
    println!("{}", gate::block_line(&totals));
    gate::finish(&totals);
}
