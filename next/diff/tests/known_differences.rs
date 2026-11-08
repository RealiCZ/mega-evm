//! The accepted-difference logic of the gates: only exactly the listed difference passes; an
//! unlisted difference, a mismatching value or frame and a stale entry all fail.

use mega_evm2_diff::gate::{
    DUP_STACK_LABEL, HALTED_FIRST_FRAME, INVALID_IMMEDIATE_LABEL, Known, Verdict, classify,
    compared_frames, known_classes, known_differences, known_replay_differences,
};
use serde_json::{Value, json};

/// No frames on either side.
const NO_FRAMES: (&Value, &Value) = (&Value::Null, &Value::Null);

/// No accepted class.
const NO_CLASS: &[String] = &[];

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
    assert_eq!(
        classify(&known(), NO_CLASS, "s", vec!["halt".into()], &r, &c, NO_FRAMES),
        Verdict::Known
    );
}

#[test]
fn test_listed_scenario_with_another_field_differs() {
    let (r, c) = (outcome("StackOverflow"), outcome("StackUnderflow"));
    let verdict = classify(
        &known(),
        NO_CLASS,
        "s",
        vec!["halt".into(), "gas_used".into()],
        &r,
        &c,
        NO_FRAMES,
    );
    assert_eq!(verdict, Verdict::Differ(vec!["gas_used".into()]));
}

#[test]
fn test_listed_field_with_other_values_differs() {
    let (r, c) = (outcome("StackOverflow"), outcome("InvalidJump"));
    assert!(matches!(
        classify(&known(), NO_CLASS, "s", vec!["halt".into()], &r, &c, NO_FRAMES),
        Verdict::Differ(_)
    ));
    let (r, c) = (outcome("OutOfGas"), outcome("StackUnderflow"));
    assert!(matches!(
        classify(&known(), NO_CLASS, "s", vec!["halt".into()], &r, &c, NO_FRAMES),
        Verdict::Differ(_)
    ));
}

#[test]
fn test_listed_entry_that_no_longer_differs_is_stale() {
    let (r, c) = (outcome("StackUnderflow"), outcome("StackUnderflow"));
    assert!(
        matches!(classify(&known(), NO_CLASS, "s", vec![], &r, &c, NO_FRAMES), Verdict::Differ(d) if d[0].contains("stale"))
    );
}

#[test]
fn test_unlisted_scenario_differs_or_matches() {
    let (r, c) = (outcome("StackOverflow"), outcome("StackUnderflow"));
    assert_eq!(
        classify(&known(), NO_CLASS, "other", vec!["halt".into()], &r, &c, NO_FRAMES),
        Verdict::Differ(vec!["halt".into()])
    );
    assert_eq!(classify(&known(), NO_CLASS, "other", vec![], &r, &r, NO_FRAMES), Verdict::Match);
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
    assert_eq!(classify(&frames_known(), NO_CLASS, "s", d, &o, &o, (&none, &call)), Verdict::Known);
}

#[test]
fn test_listed_frames_with_another_frame_differ() {
    let (o, none) = (outcome("OutOfGas"), json!([]));
    let d = || vec!["frame count 0 vs 1".to_string()];
    let other = halted_call(101);
    assert!(matches!(
        classify(&frames_known(), NO_CLASS, "s", d(), &o, &o, (&none, &other)),
        Verdict::Differ(d) if d[0].contains("did not occur as listed")
    ));
    // The same frame on the reference side is not the listed case either.
    let call = halted_call(100);
    assert!(matches!(
        classify(&frames_known(), NO_CLASS, "s", d(), &o, &o, (&call, &none)),
        Verdict::Differ(_)
    ));
}

#[test]
fn test_listed_frames_with_a_field_difference_differ() {
    let (o, none, call) = (outcome("OutOfGas"), json!([]), halted_call(100));
    let d = vec!["frame count 0 vs 1".into(), "gas_used".into()];
    assert_eq!(
        classify(&frames_known(), NO_CLASS, "s", d, &o, &o, (&none, &call)),
        Verdict::Differ(vec!["gas_used".into()])
    );
}

#[test]
fn test_listed_frames_that_no_longer_differ_are_stale() {
    let (o, call) = (outcome("OutOfGas"), halted_call(100));
    assert!(matches!(
        classify(&frames_known(), NO_CLASS, "s", vec![], &o, &o, (&call, &call)),
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

/// The class the replay entries of the file state.
const HALTED_FIRST_FRAME_ENTRY: &str =
    "frame count 0 vs 1, halted level-0 frame in the runtime gas phase";

#[test]
fn test_known_differences_file_lists_exactly_its_entries() {
    let k = known_differences();
    // The first five are the owner's; the rest are pending fixes of decided changes.
    let pending: Vec<_> =
        k.iter().skip(5).map(|k| (k.scenario.as_str(), k.field.as_str())).collect();
    assert_eq!(
        pending,
        [
            ("port_system_nonce_stale_check_off", "record"),
            ("port_system_code_3607_off", "record"),
            ("port_system_code_and_stale_nonce_both_off", "record"),
            ("port_create_max_nonce_check_off", "record"),
            ("port_create_max_nonce_check_off", "frames"),
            ("port_protocol_caller_call", "record"),
            ("port_protocol_caller_call", "frames"),
        ]
    );
    let listed: Vec<_> = k
        .iter()
        .take(5)
        .map(|k| {
            (k.scenario.as_str(), k.field.as_str(), k.reference.as_str(), k.candidate.as_str())
        })
        .collect();
    let label = ("OpcodeNotFound", "InvalidImmediateEncoding");
    assert_eq!(
        listed,
        [
            ("p6_dupn", "halt", "StackOverflow", "StackUnderflow"),
            ("p6_dupn_invalid_immediate", "halt", label.0, label.1),
            ("p6_swapn_invalid_immediate", "halt", label.0, label.1),
            ("p6_swapn_invalid_immediate_7f", "halt", label.0, label.1),
            ("p6_exchange_invalid_immediate", "halt", label.0, label.1),
        ]
    );
    assert_eq!(known_classes(), [DUP_STACK_LABEL, INVALID_IMMEDIATE_LABEL, HALTED_FIRST_FRAME]);
    let text = include_str!("../known-differences.toml");
    assert_eq!(text.matches(HALTED_FIRST_FRAME_ENTRY).count(), 2);
    // Five entries, three classes and two replay entries.
    assert_eq!(text.matches("owner sign-off 2026-11-05").count(), 10);
    assert_eq!(text.matches("pending fix (SATIN-").count(), 7);
}

#[test]
fn test_known_replay_differences_are_keyed_by_set_block_and_index() {
    let k = known_replay_differences();
    let keys: Vec<_> = k.iter().map(|k| (k.set.as_str(), k.block, k.index)).collect();
    assert_eq!(keys, [("d4", 8_064_067, 11), ("d4", 6_780_842, 14_807)]);
    assert_eq!(k[0].known.scenario, "d4:8064067:11");
    assert!(k.iter().all(|k| k.hash.len() == 66 && k.known.field == "frames"));
}

// --- the class `halted_first_frame`
// ---------------------------------------------------------------

fn class() -> Vec<String> {
    vec![HALTED_FIRST_FRAME.into()]
}

fn ran_out_of_gas() -> Value {
    json!({"status": "halt", "halt": "Base(OutOfGas(Basic))", "gas_used": 1_000_000})
}

fn frame_count() -> Vec<String> {
    vec!["frame count 0 vs 1".into()]
}

/// One halted level-0 frame of `kind`.
fn root_halt(kind: &str) -> Value {
    json!([{"kind": kind, "level": 0, "gas_limit": 948_200, "reservoir": 0, "value": "1",
            "status": "halt", "output": "0x", "gas_left": 0, "reservoir_left": 0}])
}

#[test]
fn test_the_class_accepts_exactly_its_shape() {
    let (o, none) = (ran_out_of_gas(), json!([]));
    for kind in ["Call", "Create"] {
        let f = root_halt(kind);
        assert_eq!(
            classify(&[], &class(), "s", frame_count(), &o, &o, (&none, &f)),
            Verdict::Class(HALTED_FIRST_FRAME)
        );
    }
    // A failed deposit is a run that failed too.
    let d = json!({"status": "halt", "halt": "FailedDeposit", "gas_used": 150_000});
    let f = root_halt("Call");
    assert_eq!(
        classify(&[], &class(), "s", frame_count(), &d, &d, (&none, &f)),
        Verdict::Class(HALTED_FIRST_FRAME)
    );
}

#[test]
fn test_the_class_is_off_unless_the_file_enables_it() {
    let (o, none, f) = (ran_out_of_gas(), json!([]), root_halt("Call"));
    assert_eq!(
        classify(&[], NO_CLASS, "s", frame_count(), &o, &o, (&none, &f)),
        Verdict::Differ(frame_count())
    );
}

#[test]
fn test_the_class_takes_no_other_difference() {
    let (o, none, f) = (ran_out_of_gas(), json!([]), root_halt("Call"));
    for other in
        ["gas_used", "reads", "candidate_inspected:gas_used", "reference_plain_vs_inspected"]
    {
        let mut d = frame_count();
        d.push(other.into());
        assert_eq!(
            classify(&[], &class(), "s", d.clone(), &o, &o, (&none, &f)),
            Verdict::Differ(d)
        );
    }
    // Only the frame count: a frame difference of the same count is not the class.
    let d = vec!["frame 0 gas_limit: 1 vs 2".to_string()];
    assert_eq!(classify(&[], &class(), "s", d.clone(), &o, &o, (&none, &f)), Verdict::Differ(d));
}

#[test]
fn test_the_class_takes_only_a_frameless_reference_and_one_halted_root() {
    let (o, none) = (ran_out_of_gas(), json!([]));
    let d = frame_count;
    let reject = |frames: (&Value, &Value)| {
        assert_eq!(classify(&[], &class(), "s", d(), &o, &o, frames), Verdict::Differ(d()));
    };
    // The reference opened a frame.
    reject((&root_halt("Call"), &root_halt("Call")));
    // The engine's frame is not a halted root of a transaction.
    let mut child = root_halt("Call");
    child[0]["level"] = json!(1);
    reject((&none, &child));
    let mut reverted = root_halt("Call");
    reverted[0]["status"] = json!("revert");
    reject((&none, &reverted));
    reject((&none, &root_halt("Create2")));
    // Two frames, or none.
    let two = json!([root_halt("Call")[0].clone(), root_halt("Call")[0].clone()]);
    reject((&none, &two));
    reject((&none, &none));
    // Not a list.
    reject((&Value::Null, &root_halt("Call")));
}

#[test]
fn test_the_class_takes_only_a_run_that_failed() {
    let (none, f) = (json!([]), root_halt("Call"));
    for o in [
        json!({"status": "success", "halt": null, "gas_used": 21_000}),
        json!({"status": "revert", "halt": null, "gas_used": 21_000}),
        json!({"status": "halt", "halt": "Base(StackUnderflow)", "gas_used": 1_000_000}),
        json!({"status": "halt", "halt": "Base(OutOfGas(Basic))", "error": "x", "gas_used": 0}),
    ] {
        assert_eq!(
            classify(&[], &class(), "s", frame_count(), &o, &o, (&none, &f)),
            Verdict::Differ(frame_count()),
            "{o}"
        );
    }
    // The two sides are each checked: an outcome difference would be a difference of its own, but
    // the rule does not rest on that.
    let ok = json!({"status": "success", "halt": null, "gas_used": 1_000_000});
    assert_eq!(
        classify(&[], &class(), "s", frame_count(), &ran_out_of_gas(), &ok, (&none, &f)),
        Verdict::Differ(frame_count())
    );
}

#[test]
fn test_a_listed_case_is_held_to_its_entry_not_to_the_class() {
    let (o, none) = (ran_out_of_gas(), json!([]));
    // The entry lists another frame: the class does not take the case instead.
    let other = halted_call(101);
    assert!(matches!(
        classify(&frames_known(), &class(), "s", frame_count(), &o, &o, (&none, &other)),
        Verdict::Differ(d) if d[0].contains("did not occur as listed")
    ));
    // The entry's own frame: known, not class.
    let call = halted_call(100);
    assert_eq!(
        classify(&frames_known(), &class(), "s", frame_count(), &o, &o, (&none, &call)),
        Verdict::Known
    );
}

/// The four scenarios the file listed one by one before the class was accepted, with the frames
/// the engine records for them: the class takes each.
#[test]
fn test_the_class_takes_the_cases_it_replaced() {
    let none = json!([]);
    for (kind, gas_limit, value, halt) in [
        ("Call", 948_200, "1", "Base(OutOfGas(Basic))"),
        ("Create", 944_782, "0", "Base(OutOfGas(Basic))"),
        ("Call", 129_000, "5", "FailedDeposit"),
        ("Call", 95_400, "5", "FailedDeposit"),
    ] {
        let o = json!({"status": "halt", "halt": halt, "gas_used": 1_000_000});
        let f = json!([{"kind": kind, "level": 0, "gas_limit": gas_limit, "reservoir": 0,
                        "value": value, "status": "halt", "output": "0x", "gas_left": 0,
                        "reservoir_left": 0}]);
        assert_eq!(
            classify(&[], &class(), "s", frame_count(), &o, &o, (&none, &f)),
            Verdict::Class(HALTED_FIRST_FRAME)
        );
    }
}

// --- the class `invalid_immediate_label`
// -----------------------------------------------------------

fn label() -> Vec<String> {
    vec![INVALID_IMMEDIATE_LABEL.into()]
}

#[test]
fn test_the_label_class_accepts_exactly_its_label() {
    let (r, c) = (outcome("Base(OpcodeNotFound)"), outcome("InvalidImmediateEncoding"));
    let halt = || vec!["halt".to_string()];
    assert_eq!(
        classify(&[], &label(), "s", halt(), &r, &c, NO_FRAMES),
        Verdict::Class(INVALID_IMMEDIATE_LABEL)
    );
    // Off unless the file enables it.
    assert_eq!(classify(&[], NO_CLASS, "s", halt(), &r, &c, NO_FRAMES), Verdict::Differ(halt()));
    // Another difference with it, the labels the other way round, or another label.
    let mut more = halt();
    more.push("gas_used".into());
    assert_eq!(
        classify(&[], &label(), "s", more.clone(), &r, &c, NO_FRAMES),
        Verdict::Differ(more)
    );
    for (a, b) in [
        ("InvalidImmediateEncoding", "Base(OpcodeNotFound)"),
        ("Base(OpcodeNotFound)", "StackUnderflow"),
        ("Base(StackOverflow)", "InvalidImmediateEncoding"),
    ] {
        let (r, c) = (outcome(a), outcome(b));
        assert_eq!(
            classify(&[], &label(), "s", halt(), &r, &c, NO_FRAMES),
            Verdict::Differ(halt()),
            "{a} vs {b}"
        );
    }
    // The #871 class does not take a label difference, nor the label class a frame difference.
    assert_eq!(classify(&[], &class(), "s", halt(), &r, &c, NO_FRAMES), Verdict::Differ(halt()));
    let (o, none, f) = (ran_out_of_gas(), json!([]), root_halt("Call"));
    assert_eq!(
        classify(&[], &label(), "s", frame_count(), &o, &o, (&none, &f)),
        Verdict::Differ(frame_count())
    );
}

// --- the class `dup_stack_label`
// -----------------------------------------------------------

fn dupn() -> Vec<String> {
    vec![DUP_STACK_LABEL.into()]
}

#[test]
fn test_the_dup_class_accepts_exactly_its_label() {
    let (r, c) = (outcome("Base(StackOverflow)"), outcome("StackUnderflow"));
    let halt = || vec!["halt".to_string()];
    assert_eq!(
        classify(&[], &dupn(), "s", halt(), &r, &c, NO_FRAMES),
        Verdict::Class(DUP_STACK_LABEL)
    );
    // Off unless the file enables it.
    assert_eq!(classify(&[], NO_CLASS, "s", halt(), &r, &c, NO_FRAMES), Verdict::Differ(halt()));
    // Another difference with it, the labels the other way round, another label, or a run that
    // did not halt.
    let mut more = halt();
    more.push("gas_used".into());
    assert_eq!(classify(&[], &dupn(), "s", more.clone(), &r, &c, NO_FRAMES), Verdict::Differ(more));
    for (a, b) in [
        ("StackUnderflow", "Base(StackOverflow)"),
        ("Base(StackOverflow)", "OutOfGas"),
        ("Base(OutOfGas(Basic))", "StackUnderflow"),
        ("Base(OpcodeNotFound)", "InvalidImmediateEncoding"),
    ] {
        let (r, c) = (outcome(a), outcome(b));
        assert_eq!(
            classify(&[], &dupn(), "s", halt(), &r, &c, NO_FRAMES),
            Verdict::Differ(halt()),
            "{a} vs {b}"
        );
    }
    let reverted = json!({"status": "revert", "halt": "StackUnderflow", "gas_used": 1});
    assert_eq!(
        classify(&[], &dupn(), "s", halt(), &r, &reverted, NO_FRAMES),
        Verdict::Differ(halt())
    );
    // The other label class does not take it, nor this class the other's label.
    assert_eq!(classify(&[], &label(), "s", halt(), &r, &c, NO_FRAMES), Verdict::Differ(halt()));
    let (r2, c2) = (outcome("Base(OpcodeNotFound)"), outcome("InvalidImmediateEncoding"));
    assert_eq!(classify(&[], &dupn(), "s", halt(), &r2, &c2, NO_FRAMES), Verdict::Differ(halt()));
    // A case with an entry of its own is held to it.
    assert_eq!(classify(&known(), &dupn(), "s", halt(), &r, &c, NO_FRAMES), Verdict::Known);
}

/// A whole-record entry accepts a case only when both records are exactly the pinned ones, and
/// then takes every field difference of it.
#[test]
fn test_a_record_entry_pins_both_whole_records() {
    let (r, c) = (outcome("OutOfGas"), json!({"error": "InvalidNonce { expected: 1, got: 0 }"}));
    let pin = |a: &Value, b: &Value| Known {
        scenario: "s".into(),
        field: "record".into(),
        reference: gate_canonical(a).to_string(),
        candidate: gate_canonical(b).to_string(),
    };
    let diffs = || vec!["status".to_string(), "halt".into(), "gas_used".into(), "error".into()];
    let known = vec![pin(&r, &c)];
    assert_eq!(classify(&known, NO_CLASS, "s", diffs(), &r, &c, NO_FRAMES), Verdict::Known);
    // Another candidate record, or an extra difference no field of the record names, differs.
    let other = json!({"error": "IntrinsicGasTooLow { required: 2, got: 1 }"});
    assert!(matches!(
        classify(&known, NO_CLASS, "s", diffs(), &r, &other, NO_FRAMES),
        Verdict::Differ(_)
    ));
    let mut more = diffs();
    more.push("frame count 0 vs 1".into());
    assert!(matches!(classify(&known, NO_CLASS, "s", more, &r, &c, NO_FRAMES), Verdict::Differ(_)));
}

fn gate_canonical(v: &Value) -> Value {
    mega_evm2_diff::gate::canonical(v)
}
