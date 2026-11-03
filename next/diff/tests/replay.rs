//! Engine replay gate: replays the recorded blocks on the Satin engine and compares every
//! transaction, plain and inspected, with the reference's JSON lines
//! (`$MEGA_EVM2_REPLAY_REF/<set>/<n>.jsonl`). Both sides start each transaction from the same
//! state: the reference run's committed diff is applied to the shared block state after every
//! transaction and pre-block call.
//!
//! Environment: `MEGA_EVM2_BLOCK_DIR` (recorded blocks), `MEGA_EVM2_REPLAY_REF`, the count
//! expectations of `shared/gate.rs`; `MEGA_EVM2_OUT` additionally writes every comparison.
//! `RUST_MIN_STACK=8388608` is required. Output: the `GATE` summary line.

use alloy_primitives::{Address, B256, Bytes, U256, address, keccak256};
use mega_evm2_diff::{
    gate,
    replay::{self, blockdb},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    io::{BufRead, Write},
    rc::Rc,
};

const KEYLESS: Address = address!("0x6342000000000000000000000000000000000003");
const ACCESS_CONTROL: Address = address!("0x6342000000000000000000000000000000000004");
const ORACLE: Address = address!("0x6342000000000000000000000000000000000001");
const HISTORY_STORAGE: Address = address!("0x0000F90827F1C53a10cb7A02335B175320002935");
const BEACON_ROOTS: Address = address!("0x000F3df6D732807Ef1319fB7B8bB8522d0Beac02");

fn differ(a: &Value, b: &Value) -> Vec<String> {
    gate::differences(a, b)
}

/// A kind of transaction run on the Satin engine only, by what the Satin run did.
fn satin_only(line: &Value) -> Option<&'static str> {
    // The engine executes every kind itself; `MEGA_EVM2_SATIN_APPLY=eip7702,keyless,...` hands
    // them back to the reference, for controls. The gate fails on any such transaction.
    let applied = std::env::var("MEGA_EVM2_SATIN_APPLY").unwrap_or_default();
    let applied: Vec<&str> = applied.split(',').collect();
    if line["type"].as_u64() == Some(4) && applied.contains(&"eip7702") {
        return Some("eip7702");
    }
    let hint =
        format!("0x{}", alloy_primitives::hex::encode(&keccak256("sendHint(bytes32,bytes)")[..4]));
    for f in line["frames"].as_array().into_iter().flatten() {
        let target: Address = f["target"].as_str().unwrap_or_default().parse().unwrap_or_default();
        if target == KEYLESS && applied.contains(&"keyless") {
            return Some("keyless");
        }
        if target == ACCESS_CONTROL && applied.contains(&"access_control") {
            return Some("access_control");
        }
        if applied.contains(&"oracle_hint") &&
            target == ORACLE &&
            f["selector"].as_str() == Some(hint.as_str())
        {
            return Some("oracle_hint");
        }
    }
    None
}

#[test]
fn test_replay_matches_reference_plain_and_inspected() {
    let satin_dir = std::env::var("MEGA_EVM2_REPLAY_REF").expect("MEGA_EVM2_REPLAY_REF");
    let cmp_dir = std::env::var("MEGA_EVM2_OUT").ok();
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    let mut field_counts: BTreeMap<String, u64> = BTreeMap::new();
    for (set, path) in blockdb::block_list() {
        let b = blockdb::load_block(&path, &set);
        let n = b.number;
        let satin_path = format!("{satin_dir}/{set}/{n}.jsonl");
        let f = std::fs::File::open(&satin_path)
            .unwrap_or_else(|e| panic!("missing oracle {satin_path}: {e}"));
        let lines: Vec<Value> = std::io::BufReader::new(f)
            .lines()
            .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
            .collect();
        let tx_lines: Vec<_> = lines.iter().filter(|l| l["kind"] == "tx").collect();
        assert_eq!(tx_lines.len(), b.txs.len(), "oracle transaction count {set}:{n}");
        for (i, l) in tx_lines.iter().enumerate() {
            assert_eq!(l["index"].as_u64(), Some(i as u64), "oracle order {set}:{n}");
            assert_eq!(l["hash"], b.txs[i]["hash"], "oracle identity {set}:{n}:{i}");
        }
        assert_eq!(
            lines.iter().filter(|l| l["kind"] == "pre_block").count(),
            1,
            "pre-block entry required"
        );
        assert_eq!(
            lines.iter().filter(|l| l["kind"] == "block_end").count(),
            1,
            "block-end entry required"
        );
        let txs_by_hash: HashMap<String, &Value> =
            b.txs.iter().map(|t| (t["hash"].as_str().unwrap().to_string(), t)).collect();
        let header = b.header.clone();
        let chain_id = b.chain_id;
        let db = Rc::new(RefCell::new(b.db));
        db.borrow_mut().record = true;
        let code_cache = Rc::new(RefCell::new(HashMap::new()));
        let mut out: Box<dyn Write> = match &cmp_dir {
            Some(dir) => {
                std::fs::create_dir_all(format!("{dir}/{set}")).unwrap();
                Box::new(std::io::BufWriter::new(
                    std::fs::File::create(format!("{dir}/{set}/{n}.jsonl")).unwrap(),
                ))
            }
            None => Box::new(std::io::sink()),
        };
        let mut block_counts: BTreeMap<String, u64> = BTreeMap::new();
        for line in &lines {
            match line["kind"].as_str() {
                Some("pre_block") => {
                    // The registry check reads the same slots on both sides; its answer must agree.
                    let due = {
                        let mut d = db.borrow_mut();
                        d.begin_run();
                        replay::registry_due(
                            &mut d,
                            n,
                            [(U256::from(7), U256::from(8)), (U256::from(9), U256::from(10))],
                        )
                    };
                    let mut calls = Vec::new();
                    if due != line["registry_due"].as_bool().unwrap() {
                        calls.push(json!({"name": "registry_due", "verdict": "differ"}));
                        *totals.entry("registry_due:differ".into()).or_default() += 1;
                    }
                    let names: Vec<_> = line["calls"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|c| c["name"].as_str().unwrap())
                        .collect();
                    assert_eq!(
                        names,
                        if due {
                            vec!["eip2935", "eip4788", "apply_pending_changes"]
                        } else {
                            vec!["eip2935", "eip4788"]
                        },
                        "pre-block calls {n}"
                    );
                    for c in line["calls"].as_array().unwrap() {
                        let name = c["name"].as_str().unwrap();
                        let (target, data) = match name {
                            "eip2935" => {
                                (HISTORY_STORAGE, Bytes::from(header.parent_hash.0.to_vec()))
                            }
                            "eip4788" => (
                                BEACON_ROOTS,
                                Bytes::from(
                                    header
                                        .parent_beacon_block_root
                                        .unwrap_or(B256::ZERO)
                                        .0
                                        .to_vec(),
                                ),
                            ),
                            _ => (
                                mega_evm2::SEQUENCER_REGISTRY,
                                Bytes::from(keccak256("applyPendingChanges()")[..4].to_vec()),
                            ),
                        };
                        let e = replay::run_system_call(
                            &header,
                            chain_id,
                            &db,
                            &code_cache,
                            target,
                            data.clone(),
                            false,
                        );
                        let ei = replay::run_system_call(
                            &header,
                            chain_id,
                            &db,
                            &code_cache,
                            target,
                            data,
                            true,
                        );
                        let d: Vec<&str> = ["status", "accounts", "storage", "misses"]
                            .into_iter()
                            .filter(|f| c[*f] != e[*f])
                            .collect();
                        let reads_differ = c["reads"] != e["reads"];
                        let verdict = if d.is_empty() &&
                            !reads_differ &&
                            e == ei &&
                            c["inspected_equal"] == true
                        {
                            "match"
                        } else {
                            "differ"
                        };
                        *totals.entry(format!("pre_block:{verdict}")).or_default() += 1;
                        calls.push(json!({"name": name, "verdict": verdict, "fields": d, "reads_differ": reads_differ, "gas_satin": c["gas_used"], "gas_evm2": e["gas_used"],
                            "evm2": if verdict == "differ" { e.clone() } else { Value::Null }}));
                        db.borrow_mut().apply_diff(&c["diff"]);
                    }
                    writeln!(out, "{}", json!({"kind": "pre_block", "block": n, "calls": calls}))
                        .unwrap();
                }
                Some("tx") => {
                    let index = line["index"].as_u64().unwrap();
                    let hash = line["hash"].as_str().unwrap();
                    let mut rec = json!({"kind": "tx", "set": set, "block": n, "index": index, "hash": hash, "type": line["type"]});
                    let verdict: String;
                    if line.get("envelope_error").is_some() {
                        verdict = "envelope_error".into();
                    } else if let Some(kind) = satin_only(line) {
                        verdict = format!("satin_applied:{kind}");
                    } else {
                        let raw = blockdb::bytes(&line["raw"]);
                        let tx = replay::decode(&raw, txs_by_hash[hash]).expect("decode");
                        let (plain, _) =
                            replay::run_tx(&header, chain_id, &db, &code_cache, &tx, false);
                        let (inspected, frames) =
                            replay::run_tx(&header, chain_id, &db, &code_cache, &tx, true);
                        let satin = &line["satin"];
                        let mut d = differ(satin, &plain);
                        if satin.get("error").is_none() &&
                            inspected.get("error").is_none() &&
                            let Some(f) = gate::frames_diff(&line["frames"], &json!(frames))
                        {
                            d.push(f);
                        }
                        let insp = gate::same_engine_differences(&plain, &inspected);
                        let satin_insp = line["inspected_equal"].as_bool() != Some(true);
                        for f in &d {
                            *field_counts.entry(f.clone()).or_default() += 1;
                        }
                        let o = rec.as_object_mut().unwrap();
                        if !insp.is_empty() {
                            o.insert("evm2_inspected_differs".into(), json!(insp));
                            d.push("evm2_plain_vs_inspected".into());
                        }
                        if satin_insp {
                            o.insert("satin_inspected_differs".into(), json!(true));
                            d.push("satin_plain_vs_inspected".into());
                        }
                        verdict = if satin.get("error").is_some() && d.is_empty() {
                            "refused_both".into()
                        } else if d.is_empty() {
                            "match".into()
                        } else {
                            "differ".into()
                        };
                        o.insert(
                            "status".into(),
                            satin.get("status").cloned().unwrap_or(json!("refused")),
                        );
                        o.insert("misses".into(), satin["misses"].clone());
                        if !d.is_empty() {
                            o.insert("fields".into(), json!(d));
                            o.insert("satin".into(), satin.clone());
                            o.insert("evm2".into(), plain);
                            if !insp.is_empty() {
                                o.insert("evm2_inspected".into(), inspected);
                            }
                        }
                    }
                    rec.as_object_mut().unwrap().insert("verdict".into(), json!(verdict));
                    *block_counts.entry(verdict.clone()).or_default() += 1;
                    *totals.entry(verdict).or_default() += 1;
                    if let Some(diff) = line.get("diff") {
                        db.borrow_mut().apply_diff(diff);
                    }
                    writeln!(out, "{rec}").unwrap();
                }
                _ => {}
            }
        }
        println!("{set}:{n} {block_counts:?}");
        *totals.entry("blocks".into()).or_default() += 1;
    }
    println!("TOTAL {totals:?}");
    println!("FIELDS {field_counts:?}");
    assert!(!totals.is_empty(), "empty block selection");
    println!("{}", gate::replay_line(&totals));
    gate::finish(&totals);
}
