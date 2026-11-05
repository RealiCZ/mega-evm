//! The accepted-difference logic of the gates: only exactly the listed difference passes; an
//! unlisted difference, a mismatching value or frame and a stale entry all fail.

use mega_evm2_diff::gate::{
    Known, Verdict, classify, compared_frames, known_differences, known_replay_differences,
};
use serde_json::{Value, json};

/// No frames on either side.
const NO_FRAMES: (&Value, &Value) = (&Value::Null, &Value::Null);

fn known() -> Vec<Known> {
    vec![Known {
        scenario: "s".into(),
        field: "halt".into(),
        reference: "StackOverflow".into(),
        candidate: "StackUnderflow".into(),
    }]
}

fn outcome(halt: &str) -> Value {
    json!({"status": "halt", "halt": halt, "gas_used": 1})
}

#[test]
fn test_listed_difference_is_known() {
    let (r, c) = (outcome("StackOverflow"), outcome("StackUnderflow"));
    assert_eq!(classify(&known(), "s", vec!["halt".into()], &r, &c, NO_FRAMES), Verdict::Known);
}

#[test]
fn test_listed_scenario_with_another_field_differs() {
    let (r, c) = (outcome("StackOverflow"), outcome("StackUnderflow"));
    let verdict =
        classify(&known(), "s", vec!["halt".into(), "gas_used".into()], &r, &c, NO_FRAMES);
    assert_eq!(verdict, Verdict::Differ(vec!["gas_used".into()]));
}

#[test]
fn test_listed_field_with_other_values_differs() {
    let (r, c) = (outcome("StackOverflow"), outcome("InvalidJump"));
    assert!(matches!(
        classify(&known(), "s", vec!["halt".into()], &r, &c, NO_FRAMES),
        Verdict::Differ(_)
    ));
    let (r, c) = (outcome("OutOfGas"), outcome("StackUnderflow"));
    assert!(matches!(
        classify(&known(), "s", vec!["halt".into()], &r, &c, NO_FRAMES),
        Verdict::Differ(_)
    ));
}

#[test]
fn test_listed_entry_that_no_longer_differs_is_stale() {
    let (r, c) = (outcome("StackUnderflow"), outcome("StackUnderflow"));
    assert!(
        matches!(classify(&known(), "s", vec![], &r, &c, NO_FRAMES), Verdict::Differ(d) if d[0].contains("stale"))
    );
}

#[test]
fn test_unlisted_scenario_differs_or_matches() {
    let (r, c) = (outcome("StackOverflow"), outcome("StackUnderflow"));
    assert_eq!(
        classify(&known(), "other", vec!["halt".into()], &r, &c, NO_FRAMES),
        Verdict::Differ(vec!["halt".into()])
    );
    assert_eq!(classify(&known(), "other", vec![], &r, &r, NO_FRAMES), Verdict::Match);
}

/// A halted level-0 call frame, as the inspector records it.
fn halted_call(gas_limit: u64) -> Value {
    json!([{"kind": "Call", "level": 0, "gas_limit": gas_limit, "reservoir": 0, "value": "1",
            "status": "halt", "output": "0x", "gas_left": 0, "reservoir_left": 0}])
}

fn frames_known() -> Vec<Known> {
    vec![Known {
        scenario: "s".into(),
        field: "frames".into(),
        reference: "[]".into(),
        candidate: r#"[{"kind": "Call", "level": 0, "gas_limit": 100, "reservoir": 0,
            "value": "1", "status": "halt"}]"#
            .into(),
    }]
}

#[test]
fn test_listed_frames_are_known() {
    let (o, none, call) = (outcome("OutOfGas"), json!([]), halted_call(100));
    let d = vec!["frame count 0 vs 1".into()];
    assert_eq!(classify(&frames_known(), "s", d, &o, &o, (&none, &call)), Verdict::Known);
}

#[test]
fn test_listed_frames_with_another_frame_differ() {
    let (o, none) = (outcome("OutOfGas"), json!([]));
    let d = || vec!["frame count 0 vs 1".to_string()];
    let other = halted_call(101);
    assert!(matches!(
        classify(&frames_known(), "s", d(), &o, &o, (&none, &other)),
        Verdict::Differ(d) if d[0].contains("did not occur as listed")
    ));
    // The same frame on the reference side is not the listed case either.
    let call = halted_call(100);
    assert!(matches!(
        classify(&frames_known(), "s", d(), &o, &o, (&call, &none)),
        Verdict::Differ(_)
    ));
}

#[test]
fn test_listed_frames_with_a_field_difference_differ() {
    let (o, none, call) = (outcome("OutOfGas"), json!([]), halted_call(100));
    let d = vec!["frame count 0 vs 1".into(), "gas_used".into()];
    assert_eq!(
        classify(&frames_known(), "s", d, &o, &o, (&none, &call)),
        Verdict::Differ(vec!["gas_used".into()])
    );
}

#[test]
fn test_listed_frames_that_no_longer_differ_are_stale() {
    let (o, call) = (outcome("OutOfGas"), halted_call(100));
    assert!(matches!(
        classify(&frames_known(), "s", vec![], &o, &o, (&call, &call)),
        Verdict::Differ(d) if d[0].contains("stale")
    ));
}

#[test]
fn test_compared_frames_keep_what_the_comparison_reads() {
    let cut = compared_frames(&halted_call(100)).unwrap();
    assert_eq!(
        cut,
        json!([{"kind": "Call", "level": 0, "gas_limit": 100, "reservoir": 0, "value": "1",
                "status": "halt"}])
    );
    assert_eq!(compared_frames(&Value::Null), None);
}

/// The class every accepted frames entry of the file states.
const HALTED_FIRST_FRAME: &str =
    "frame count 0 vs 1, halted level-0 frame in the runtime gas phase";

#[test]
fn test_known_differences_file_lists_exactly_its_entries() {
    let k = known_differences();
    let listed: Vec<_> = k.iter().map(|k| (k.scenario.as_str(), k.field.as_str())).collect();
    assert_eq!(
        listed,
        [
            ("p6_dupn", "halt"),
            ("p6_salt_first_value_call_m8", "frames"),
            ("p6_salt_code_deposit_m8", "frames"),
        ]
    );
    assert_eq!(
        (k[0].reference.as_str(), k[0].candidate.as_str()),
        ("StackOverflow", "StackUnderflow")
    );
    for f in &k[1..] {
        assert_eq!(f.reference, "[]");
    }
    let text = include_str!("../known-differences.toml");
    assert_eq!(text.matches(HALTED_FIRST_FRAME).count(), 4);
    assert_eq!(text.matches("owner sign-off 2026-11-05").count(), 4);
}

#[test]
fn test_known_replay_differences_are_keyed_by_set_block_and_index() {
    let k = known_replay_differences();
    let keys: Vec<_> = k.iter().map(|k| (k.set.as_str(), k.block, k.index)).collect();
    assert_eq!(keys, [("d4", 8_064_067, 11), ("d4", 6_780_842, 14_807)]);
    assert_eq!(k[0].known.scenario, "d4:8064067:11");
    assert!(k.iter().all(|k| k.hash.len() == 66 && k.known.field == "frames"));
}
