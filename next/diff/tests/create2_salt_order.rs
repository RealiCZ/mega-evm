//! A CREATE2 short of its salt on the engine (`SATIN-SIZE-005`, `SATIN-SIZE-006`): it pays the
//! initcode cost and the memory before it fails on the missing salt, so under gas detention the
//! memory charge is the crossing, and the frame stops with the limit revert and keeps its gas
//! instead of halting on the stack and burning it. Each scenario of `edge::create2_salt_order` is
//! held to the reference's own record of it.

use mega_evm2::limit_exceeded_output;
use mega_evm2_diff::{
    driver::{run, scenarios},
    gate,
};
use serde_json::Value;

/// The body's 310 bytes of history.
const BODY: u64 = 27_280;

/// The scenario's inspected record, after checking that its plain run renders the same.
fn record(name: &str) -> Value {
    let s = scenarios::by_name(name).unwrap_or_else(|| panic!("no scenario {name}"));
    let plain = run(&s, false);
    let inspected = run(&s, true);
    assert!(plain.get("error").is_none(), "{name} refused: {plain}");
    assert_eq!(gate::same_engine_differences(&plain, &inspected), Vec::<String>::new(), "{name}");
    inspected
}

fn u(r: &Value, key: &str) -> u64 {
    r[key].as_u64().unwrap_or_else(|| panic!("{key}: {}", r[key]))
}

/// The record's outcome: `(status, halt, gas used, regular gas, limit exceeded)`.
fn outcome(r: &Value) -> (String, Value, u64, u64, Value) {
    (
        r["status"].as_str().unwrap().to_owned(),
        gate::halt(&r["halt"]),
        u(r, "gas_used"),
        u(r, "regular"),
        r["limit_exceeded"].clone(),
    )
}

/// Frame `i` (in the order the frames opened): `(level, status, gas limit, gas left)`.
fn frame(r: &Value, i: usize) -> (u64, String, u64, u64) {
    let f = &r["frames"][i];
    (u(f, "level"), f["status"].as_str().unwrap().to_owned(), u(f, "gas_limit"), u(f, "gas_left"))
}

/// The limit revert's output: detention's compute limit (kind 2) and the limit it crossed.
fn limit_output(limit: u64) -> String {
    format!("0x{}", alloy_primitives::hex::encode(limit_exceeded_output(2, limit)))
}

fn revert(regular: u64, limit: u64) -> (String, Value, u64, u64, Value) {
    ("revert".into(), Value::Null, regular + BODY, regular, format!("2:{limit}").into())
}

/// `TIMESTAMP` caps the frame's compute at 100; the CREATE2's memory expansion (0x8000 bytes) is
/// the crossing, so the frame stops at the limit (102 spent) and keeps the rest of its gas.
#[test]
fn test_a_detained_create2_short_of_its_salt_stops_at_the_crossing() {
    let r = record("edge_create2_missing_salt_detained");
    assert_eq!(outcome(&r), revert(15_011, 102));
    assert_eq!(r["output"], limit_output(102));
    assert_eq!(r["frames"].as_array().unwrap().len(), 1);
    assert_eq!(frame(&r, 0), (0, "revert".into(), 957_720, 957_709));
}

/// The same CREATE2 in a child called with 100,000 gas: the child stops at the crossing and keeps
/// its gas, and the transaction stops with it at the limit, 3,118 spent.
#[test]
fn test_a_detained_child_create2_short_of_its_salt_stops_at_the_crossing() {
    let r = record("edge_create2_missing_salt_detained_child");
    assert_eq!(outcome(&r), revert(18_030, 3_118));
    assert_eq!(r["output"], limit_output(3_118));
    assert_eq!(r["frames"].as_array().unwrap().len(), 2);
    assert_eq!(frame(&r, 0), (0, "revert".into(), 957_720, 954_690));
    assert_eq!(frame(&r, 1), (1, "revert".into(), 100_000, 99_989));
}

/// Satin's default cap: after 19.99M of compute past the `TIMESTAMP` read, the 1 MiB CREATE2's
/// initcode cost and memory cross the 20M limit, so the frame stops there and keeps the rest of its
/// 150M.
#[test]
fn test_a_create2_short_of_its_salt_crosses_the_default_cap() {
    let r = record("edge_create2_missing_salt_default_cap");
    assert_eq!(outcome(&r), revert(20_005_038, 20_000_002));
    assert_eq!(r["output"], limit_output(20_000_002));
    assert_eq!(r["frames"].as_array().unwrap().len(), 1);
    assert_eq!(frame(&r, 0), (0, "revert".into(), 149_957_720, 129_967_682));
}

/// Undetained, the 1 MiB expansion cannot be paid: the frame halts on the memory charge, not on
/// the stack, and the halt consumes its gas.
#[test]
fn test_an_unpayable_create2_short_of_its_salt_halts_on_the_memory_charge() {
    let r = record("edge_create2_missing_salt_unpayable");
    assert_eq!(
        outcome(&r),
        ("halt".into(), Value::from("MemoryOOG"), 1_000_000, 972_720, Value::Null)
    );
    assert_eq!(frame(&r, 0).1, "halt");
}
