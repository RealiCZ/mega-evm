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

/// The L1 fee vault's balance after `q45_l1_fee_nonzero` with an L1 base fee of `base_fee`: the
/// fixture writes `l1_fee`'s base fee into the L1Block's slot 1.
fn l1_fee_paid(base_fee: u64) -> u128 {
    let mut s = scenarios::by_name("q45_l1_fee_nonzero").expect("q45");
    s.l1_fee.as_mut().expect("an L1 fee").0 = base_fee;
    scenarios::add_chain_accounts(&mut s);
    let o = run(&s);
    o["accounts"]
        .get("0x420000000000000000000000000000000000001A")
        .map_or(0, |a| a.as_str().unwrap().split('/').next().unwrap().parse().expect("a balance"))
}

/// The L1 fee follows the L1 base fee `l1_fee` states: 1,600 times it for this transaction's 100
/// enveloped bytes at a scalar of 1,000,000, nothing at 0 (the engine's fixture has the same test).
#[test]
fn test_the_l1_base_fee_sets_the_l1_fee() {
    assert_eq!(l1_fee_paid(0), 0);
    assert_eq!(l1_fee_paid(1_000_000_000), 1_600_000_000_000);
    assert_eq!(l1_fee_paid(1_000_000_000_000), 1_600_000_000_000_000);
}

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
