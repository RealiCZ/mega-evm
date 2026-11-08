//! Synthetic differential gate: the shared scenarios run on the Satin engine, plain and
//! inspected, against the reference outputs. Normalization and exclusions live in
//! `src/gate.rs`; the accepted differences in `known-differences.toml`.
//!
//! Environment: `MEGA_EVM2_ORACLE` (the reference output file), `MEGA_EVM2_GATE_NAME` (the name in
//! the summary line, default `main`), `MEGA_EVM2_EXPECT` (the expected scenario count), and the
//! scenario selectors of `mega_evm2_scenarios::everything` (`MEGA_EVM2_FUZZ*`, `MEGA_EVM2_ONLY`).
//! Output: the `GATE` summary line (`known`: listed entries; `class`: cases accepted by an
//! accepted class's shape); `MEGA_EVM2_OUT` additionally writes every comparison.
//! Every reference entry must belong to a selected scenario: an extra one is a difference. A run
//! that selects scenarios by name (`MEGA_EVM2_ONLY`) leaves the others out on purpose; it is a
//! debugging run, does not check for extra entries, and prints `PARTIAL GATE ...`.

use mega_evm2_diff::{
    driver::{recipient, run, runs_under, scenarios},
    gate::{self, Verdict},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};

#[test]
fn test_synthetic_matches_reference_plain_and_inspected() {
    let path = std::env::var("MEGA_EVM2_ORACLE").expect("MEGA_EVM2_ORACLE");
    let name = std::env::var("MEGA_EVM2_GATE_NAME").unwrap_or("main".into());
    let oracle: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("oracle file required"))
            .unwrap();
    let oracle_count = oracle.len();
    let by_name: HashMap<String, Value> =
        oracle.into_iter().map(|o| (o["name"].as_str().unwrap().to_owned(), o)).collect();
    assert_eq!(by_name.len(), oracle_count, "duplicate oracle names");
    let known = gate::known_differences();
    let classes = gate::known_classes();
    let partial = std::env::var_os("MEGA_EVM2_ONLY").is_some();
    let mut report = Vec::new();
    let (mut matched, mut accepted, mut class, mut differ, mut missing) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut groups = BTreeMap::<&str, (u64, u64)>::new();
    let selected = scenarios::everything();
    let mut extra: Vec<&String> = if partial {
        Vec::new()
    } else {
        let names: HashSet<&str> = selected.iter().map(|s| s.name).collect();
        by_name.keys().filter(|n| !names.contains(n.as_str())).collect()
    };
    extra.sort();
    for name in &extra {
        println!("EXTRA {name}");
    }
    for s in selected {
        let Some(o) = by_name.get(s.name) else {
            missing += 1;
            println!("MISSING {}", s.name);
            continue;
        };
        let plain = run(&s, false);
        let insp = run(&s, true);
        let group = groups.entry(s.group).or_default();
        let verdict = gate::scenario_verdict(
            &known,
            &classes,
            s.name,
            o,
            &plain,
            &insp,
            recipient(&s),
            |rule| runs_under(&s, rule),
        );
        match &verdict {
            Verdict::Match => {
                matched += 1;
                group.0 += 1;
                println!("MATCH {}", s.name);
            }
            Verdict::Known => {
                accepted += 1;
                group.0 += 1;
                println!("KNOWN {}", s.name);
            }
            Verdict::Class(c) => {
                class += 1;
                group.0 += 1;
                println!("CLASS {} {c}", s.name);
            }
            Verdict::Differ(d) => {
                differ += 1;
                group.1 += 1;
                println!("DIFFER {} {d:?}", s.name);
            }
        }
        report.push(json!({"name": s.name, "group": s.group, "verdict": format!("{verdict:?}"), "candidate": plain, "inspected": insp}));
    }
    if let Ok(out) = std::env::var("MEGA_EVM2_OUT") {
        std::fs::write(out, serde_json::to_vec(&report).unwrap()).unwrap();
    }
    for (g, (m, d)) in &groups {
        println!("group {g}: {m} match, {d} differ");
    }
    // A listed entry whose scenario was not selected is not evaluated (a fuzz batch does not run
    // the hand-written scenarios); one that was selected and does not differ is `Differ` above.
    let total = report.len() as u64;
    println!(
        "{}GATE {name} total={total} match={matched} known={accepted} class={class} differ={}",
        if partial { "PARTIAL " } else { "" },
        differ + missing + extra.len() as u64
    );
    assert_eq!(missing, 0, "missing oracle entries");
    assert!(extra.is_empty(), "oracle entries of no selected scenario: {extra:?}");
    assert_eq!(differ, 0, "differential mismatches");
    assert!(!report.is_empty(), "empty selection");
    gate::expected("MEGA_EVM2_EXPECT", total);
}
