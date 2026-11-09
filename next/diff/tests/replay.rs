//! Engine replay gate: replays the recorded blocks on the Satin engine and compares every
//! transaction, plain and inspected, with the reference's JSON lines
//! (`$MEGA_EVM2_REPLAY_REF/<set>/<n>.jsonl`). Both sides start each transaction from the same
//! state: the reference run's committed diff is applied to the shared block state after every
//! transaction and pre-block call.
//!
//! Environment: `MEGA_EVM2_BLOCK_DIR` (recorded blocks), `MEGA_EVM2_REPLAY_REF`, the optional
//! `MEGA_EVM2_EXPECT` count check of `src/gate.rs`; `MEGA_EVM2_OUT` additionally writes every
//! comparison.
//! `RUST_MIN_STACK=8388608` is required. Output: the `GATE` summary line and the regular-gas
//! invariant's `INVARIANT` line (`mega_evm2_diff::invariant`).

use alloy_primitives::{Address, B256, Bytes, U256, address, keccak256};
use mega_evm2_diff::{
    gate::{self, Verdict},
    invariant,
    replay::{self, blockdb},
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    io::{BufRead, Write},
    rc::Rc,
};

const HISTORY_STORAGE: Address = address!("0x0000F90827F1C53a10cb7A02335B175320002935");
const BEACON_ROOTS: Address = address!("0x000F3df6D732807Ef1319fB7B8bB8522d0Beac02");

fn differ(a: &Value, b: &Value) -> Vec<String> {
    gate::differences(a, b)
}

/// The differences of one replayed transaction: its reference row `line` (the outcome `satin`, the
/// recorded `frames`, `inspected_equal`) against the engine's plain and inspected records and the
/// inspected run's `frames`; also whether the frames differ, the engine's own plain-versus-
/// inspected differences, and whether the engine's records have a fault of their own (a record
/// without exactly the fields the engine renders, `record_fields:<run>:<what>`, or the two
/// disagreeing), which no rerun waives.
fn tx_differences(
    line: &Value,
    plain: &Value,
    inspected: &Value,
    frames: &Value,
) -> (Vec<String>, bool, Vec<String>, bool) {
    let satin = &line["satin"];
    let mut d = differ(satin, plain);
    let mut candidate_fault = false;
    for (run, r) in [("plain", plain), ("inspected", inspected)] {
        if let Err(e) = replay::check_record_fields(r) {
            d.push(format!("record_fields:{run}:{e}"));
            candidate_fault = true;
        }
    }
    let frames_differ = if satin.get("error").is_none() &&
        inspected.get("error").is_none() &&
        let Some(f) = gate::frames_diff(&line["frames"], frames)
    {
        d.push(f);
        true
    } else {
        false
    };
    let insp = gate::same_engine_differences(plain, inspected);
    if !insp.is_empty() {
        d.push("evm2_plain_vs_inspected".into());
        candidate_fault = true;
    }
    if line["inspected_equal"].as_bool() != Some(true) {
        d.push("satin_plain_vs_inspected".into());
    }
    (d, frames_differ, insp, candidate_fault)
}

#[test]
fn test_replay_matches_reference_plain_and_inspected() {
    let satin_dir = std::env::var("MEGA_EVM2_REPLAY_REF").expect("MEGA_EVM2_REPLAY_REF");
    let cmp_dir = std::env::var("MEGA_EVM2_OUT").ok();
    let known = gate::known_replay_differences();
    let classes = gate::known_classes();
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    let mut field_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut invariant = invariant::Tally::default();
    let mut processed = Vec::new();
    for (set, path) in blockdb::block_list() {
        let b = blockdb::load_block(&path, &set);
        let n = b.number;
        processed.push(format!("{set}:{n}"));
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
        // The accepted differences of this block, each of which must be evaluated.
        let block_known: Vec<&gate::KnownReplay> =
            known.iter().filter(|k| k.set == set && k.block == n).collect();
        let mut evaluated = Vec::new();
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
                    } else {
                        let raw = blockdb::bytes(&line["raw"]);
                        let tx = replay::decode(&raw, txs_by_hash[hash]).expect("decode");
                        let (mut plain, _) =
                            replay::run_tx(&header, chain_id, &db, &code_cache, &tx, false);
                        let (mut inspected, frames) =
                            replay::run_tx(&header, chain_id, &db, &code_cache, &tx, true);
                        invariant.case(hash, invariant::take_runs() >= 2, [&plain, &inspected]);
                        gate::candidate_fault(hash, &mut plain, &mut inspected);
                        let satin = &line["satin"];
                        let frames = json!(frames);
                        let (mut d, frames_differ, insp, candidate_fault) =
                            tx_differences(line, &plain, &inspected, &frames);
                        for f in &d {
                            *field_counts.entry(f.clone()).or_default() += 1;
                        }
                        let o = rec.as_object_mut().unwrap();
                        if !insp.is_empty() {
                            o.insert("evm2_inspected_differs".into(), json!(insp));
                        }
                        if line["inspected_equal"].as_bool() != Some(true) {
                            o.insert("satin_inspected_differs".into(), json!(true));
                        }
                        let key = gate::replay_key(&set, n, index);
                        let listed: Vec<gate::Known> = block_known
                            .iter()
                            .filter(|k| k.index == index)
                            .map(|k| {
                                assert_eq!(k.hash, hash, "known replay difference {key}: hash");
                                k.known.clone()
                            })
                            .collect();
                        if !listed.is_empty() {
                            evaluated.push(index);
                        }
                        let mut accepted = gate::classify(
                            &listed,
                            &classes,
                            &key,
                            d,
                            satin,
                            &plain,
                            (&line["frames"], &frames),
                        );
                        // A transaction without an entry that no class above takes is of a class
                        // the replay gate runs again for, when the file enables it, if the engine
                        // run again under its rule, plain and inspected, has no difference at all.
                        // A rerun stands in for the comparison with the reference only: none is
                        // made when the engine's own records have a fault.
                        if matches!(accepted, Verdict::Differ(_)) &&
                            listed.is_empty() &&
                            !candidate_fault
                        {
                            for (class, rule) in gate::REPLAY_RERUN_CLASSES {
                                if !classes.iter().any(|c| c == class) {
                                    continue;
                                }
                                let run = |inspected| {
                                    replay::run_tx_under(
                                        &header,
                                        chain_id,
                                        &db,
                                        &code_cache,
                                        &tx,
                                        inspected,
                                        Some(rule),
                                    )
                                };
                                let (plain, _) = run(false);
                                let (inspected, frames) = run(true);
                                let (d, _, _, _) =
                                    tx_differences(line, &plain, &inspected, &json!(frames));
                                if d.is_empty() {
                                    accepted = Verdict::Class(class);
                                    break;
                                }
                            }
                        }
                        d = match &accepted {
                            Verdict::Differ(d) => d.clone(),
                            Verdict::Match | Verdict::Known | Verdict::Class(_) => Vec::new(),
                        };
                        verdict = if accepted == Verdict::Known {
                            "known".into()
                        } else if let Verdict::Class(class) = accepted {
                            o.insert("class".into(), json!(class));
                            "class".into()
                        } else if satin.get("error").is_some() && d.is_empty() {
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
                        if frames_differ {
                            o.insert(
                                "frames".into(),
                                json!({"satin": line["frames"], "evm2": frames}),
                            );
                        }
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
                Some("block_end") => {}
                other => panic!("unknown row kind {other:?} in {set}:{n}"),
            }
        }
        // A listed transaction of a processed block that was never compared fails the gate.
        for k in block_known.iter().filter(|k| !evaluated.contains(&k.index)) {
            println!("KNOWN_MISSING {}", k.known.scenario);
            *block_counts.entry("known_missing:differ".into()).or_default() += 1;
            *totals.entry("known_missing:differ".into()).or_default() += 1;
        }
        println!("{set}:{n} {block_counts:?}");
        *totals.entry("blocks".into()).or_default() += 1;
    }
    println!("TOTAL {totals:?}");
    println!("FIELDS {field_counts:?}");
    assert!(!totals.is_empty(), "empty block selection");
    // Only a run of exactly the manifest's blocks is the gate: any other selection prints a
    // line `gate.sh` does not take for a summary.
    let complete = blockdb::check_processed_identities(&processed);
    println!("{}", invariant.line("replay"));
    println!("{}{}", if complete { "" } else { "PARTIAL " }, gate::replay_line(&totals));
    gate::finish(&totals);
}
