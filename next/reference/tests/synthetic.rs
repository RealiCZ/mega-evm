//! Reference side of the synthetic gate: runs the shared scenarios (`mega_evm2_scenarios`) on the
//! Satin implementation under review (`mega_evm2_reference::synthetic`) and writes the outcomes to
//! `$MEGA_EVM2_ORACLE_OUT`.
//!
//! Environment: `MEGA_EVM2_ORACLE_OUT`, `MEGA_EVM2_EXPECT` (expected scenario count), and the
//! scenario selectors of `mega_evm2_scenarios::everything` (`MEGA_EVM2_FUZZ*`, `MEGA_EVM2_ONLY`).

#![allow(unreachable_pub, deprecated, missing_docs, clippy::all)]

use mega_evm2_reference::synthetic::run;
use mega_evm2_scenarios as scenarios;
use serde_json::Value;

#[test]
fn test_reference_synthetic() {
    let results: Vec<Value> = scenarios::everything().iter().map(run).collect();
    let path = std::env::var("MEGA_EVM2_ORACLE_OUT").expect("MEGA_EVM2_ORACLE_OUT");
    std::fs::write(&path, serde_json::to_string_pretty(&results).unwrap()).unwrap();
    for r in &results {
        if r.get("error").is_some() {
            println!("{:48} ERROR {}", r["name"].as_str().unwrap(), r["error"]);
            continue;
        }
        println!(
            "{:48} {:8} gas={:>9} reg={:>9} state={:>8} hist={:>6} res={:>9} ds={:>5} kv={} sg={} stop={} insp_eq={}",
            r["name"].as_str().unwrap(),
            r["status"].as_str().unwrap(),
            r["gas_used"],
            r["regular"],
            r["state"],
            r["history"],
            r["reservoir_remaining"],
            r["data_size"],
            r["kv"],
            r["growth"],
            r["limit_exceeded"],
            r["inspected_equal"],
        );
    }
    println!("{} scenarios", results.len());
    assert!(!results.is_empty(), "empty oracle");
    if let Ok(n) = std::env::var("MEGA_EVM2_EXPECT") {
        assert_eq!(results.len(), n.parse::<usize>().unwrap());
    }
    assert!(
        results.iter().all(|r| r["inspected_equal"] == true),
        "reference plain/inspected differ"
    );
}
