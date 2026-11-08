//! The accepted-difference logic of the gates: only exactly the listed difference passes; an
//! unlisted difference, a mismatching value or frame and a stale entry all fail.

use alloy_primitives::Address;
use mega_evm2_diff::gate::{
    AUTH_PREFLIGHT_READS, DUP_STACK_LABEL, HALTED_FIRST_FRAME, INVALID_IMMEDIATE_LABEL, Known,
    MODEXP, MODEXP_BOUND_LABEL, MODEXP_FORMULA_PRICE, SLOTNUM_ZERO, Verdict, classify,
    classify_case, compared_frames, known_classes, known_differences, known_replay_differences,
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
    let listed: Vec<_> = k
        .iter()
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
    assert_eq!(
        known_classes(),
        [
            DUP_STACK_LABEL,
            INVALID_IMMEDIATE_LABEL,
            HALTED_FIRST_FRAME,
            AUTH_PREFLIGHT_READS,
            MODEXP_BOUND_LABEL,
            MODEXP_FORMULA_PRICE,
            SLOTNUM_ZERO,
        ]
    );
    let text = include_str!("../known-differences.toml");
    assert_eq!(text.matches(HALTED_FIRST_FRAME_ENTRY).count(), 2);
    // Five entries, three classes and two replay entries.
    assert_eq!(text.matches("owner sign-off 2026-11-05").count(), 10);
    // The four classes of the decided changes the reference has not made yet, each by its rule.
    for rule in ["SATIN-AUTH-005", "SATIN-PCMP-007", "SATIN-DET-023", "SATIN-BASE-005"] {
        assert_eq!(text.matches(&format!("owner decision 2026-11-08 ({rule})")).count(), 1);
    }
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

// --- the class `modexp_bound_label`
// -----------------------------------------------------------

fn bound_label() -> Vec<String> {
    vec![MODEXP_BOUND_LABEL.into()]
}

/// A transaction whose own call is ModExp's.
const MODEXP_CALL: Option<Address> = Some(MODEXP);

#[test]
fn test_the_modexp_label_class_accepts_exactly_its_label() {
    let halt = || vec!["halt".to_string()];
    let (r, c) = (outcome("Base(PrecompileError)"), outcome("PrecompileOOG"));
    let f = root_halt("Call");
    assert_eq!(
        classify_case(&[], &bound_label(), "s", halt(), &r, &c, (&f, &f), MODEXP_CALL),
        Verdict::Class(MODEXP_BOUND_LABEL)
    );
    assert_eq!(
        classify_case(&[], NO_CLASS, "s", halt(), &r, &c, (&f, &f), MODEXP_CALL),
        Verdict::Differ(halt())
    );
    let mut more = halt();
    more.push("gas_used".into());
    assert_eq!(
        classify_case(&[], &bound_label(), "s", more.clone(), &r, &c, (&f, &f), MODEXP_CALL),
        Verdict::Differ(more)
    );
    // The labels the other way round, or other labels.
    for (a, b) in [
        ("PrecompileOOG", "Base(PrecompileError)"),
        ("Base(OutOfGas(Basic))", "PrecompileOOG"),
        ("Base(PrecompileError)", "OutOfGas"),
    ] {
        assert_eq!(
            classify_case(
                &[],
                &bound_label(),
                "s",
                halt(),
                &outcome(a),
                &outcome(b),
                (&f, &f),
                MODEXP_CALL
            ),
            Verdict::Differ(halt()),
            "{a} vs {b}"
        );
    }
    // Only the transaction's own call: one halted level-0 call on each side.
    let none = json!([]);
    let mut child = root_halt("Call");
    child[0]["level"] = json!(1);
    let two = json!([root_halt("Call")[0].clone(), child[0].clone()]);
    for frames in [(&none, &f), (&f, &none), (&child, &child), (&two, &two)] {
        assert_eq!(
            classify_case(&[], &bound_label(), "s", halt(), &r, &c, frames, MODEXP_CALL),
            Verdict::Differ(halt())
        );
    }
    // The transaction's own call is not ModExp's, or no recipient is known (`classify`).
    for other in [Some(Address::with_last_byte(6)), None] {
        assert_eq!(
            classify_case(&[], &bound_label(), "s", halt(), &r, &c, (&f, &f), other),
            Verdict::Differ(halt())
        );
    }
    assert_eq!(
        classify(&[], &bound_label(), "s", halt(), &r, &c, (&f, &f)),
        Verdict::Differ(halt())
    );
}

// --- the class `modexp_formula_price`
// -----------------------------------------------------------

fn formula_price() -> Vec<String> {
    vec![MODEXP_FORMULA_PRICE.into()]
}

/// `MegaLimitExceeded(2, limit)` as a frame's output.
fn compute_stop(limit: u64) -> String {
    mega_evm2_scenarios::record::hex(&mega_evm2::limit_exceeded_output(2, limit))
}

/// The case of `edge_modexp_over_bound_detained_allowance_above_500`: the reference runs the
/// ModExp call (1,000,000 forwarded) and burns its forward; the engine answers the compute stop.
fn price_case() -> (Value, Value, Value, Value, Vec<String>) {
    let call = |status: &str, output: String, left: u64| {
        json!({"kind": "Call", "level": 1, "gas_limit": 1_000_000, "reservoir": 0, "value": "0",
               "status": status, "output": output, "gas_left": left, "reservoir_left": 0})
    };
    let top = |left: u64| {
        json!({"kind": "Call", "level": 0, "gas_limit": 29_957_720, "reservoir": 0, "value": "0",
               "status": "success", "output": "0x", "gas_left": left, "reservoir_left": 0})
    };
    let r =
        json!({"status": "success", "halt": null, "gas_used": 21_023_443, "regular": 20_996_163});
    let c = json!({"status": "success", "halt": null, "gas_used": 20_023_443, "regular": 19_996_163,
                   "priced": [[33_282, 18_841, true, MODEXP.to_string(), true]]});
    let fr = json!([top(8_976_557), call("halt", "0x".into(), 0)]);
    let fc = json!([top(9_976_557), call("revert", compute_stop(20_000_002), 1_000_000)]);
    let d =
        vec!["gas_used".into(), "regular".into(), "frame 0 gas_left: 8976557 vs 9976557".into()];
    (r, c, fr, fc, d)
}

#[test]
fn test_the_modexp_price_class_accepts_exactly_its_shape() {
    let (r, c, fr, fc, d) = price_case();
    assert_eq!(
        classify(&[], &formula_price(), "s", d.clone(), &r, &c, (&fr, &fc)),
        Verdict::Class(MODEXP_FORMULA_PRICE)
    );
    // The other way round: the reference answered the stop (allowance 251 < 500 <= 20,000 forward
    // < the formula price), the engine ran the call.
    let call = |status: &str, output: String, left: u64| {
        json!({"kind": "Call", "level": 1, "gas_limit": 20_000, "reservoir": 0, "value": "0",
               "status": status, "output": output, "gas_left": left, "reservoir_left": 0})
    };
    let top = |left: u64| {
        json!({"kind": "Call", "level": 0, "gas_limit": 29_957_720, "reservoir": 0, "value": "0",
               "status": "success", "output": "0x", "gas_left": left, "reservoir_left": 0})
    };
    let r2 =
        json!({"status": "success", "halt": null, "gas_used": 20_042_033, "regular": 20_014_753});
    let c2 = json!({"status": "success", "halt": null, "gas_used": 20_062_033, "regular": 20_034_753,
                    "priced": [[33_282, 251, false, MODEXP.to_string(), true]]});
    let fr2 = json!([top(9_957_967), call("revert", compute_stop(20_000_002), 20_000)]);
    let fc2 = json!([top(9_937_967), call("halt", "0x".into(), 0)]);
    let d2 = vec![
        "gas_used".to_string(),
        "regular".into(),
        "frame 0 gas_left: 9957967 vs 9937967".into(),
    ];
    assert_eq!(
        classify(&[], &formula_price(), "s", d2.clone(), &r2, &c2, (&fr2, &fc2)),
        Verdict::Class(MODEXP_FORMULA_PRICE)
    );
    // Off unless the file enables it.
    assert_eq!(
        classify(&[], NO_CLASS, "s", d.clone(), &r, &c, (&fr, &fc)),
        Verdict::Differ(d.clone())
    );
}

#[test]
fn test_the_modexp_price_class_takes_no_other_case() {
    let (r, c, fr, fc, d) = price_case();
    let reject = |r: &Value, c: &Value, fr: &Value, fc: &Value, d: &[String]| {
        assert!(
            matches!(
                classify(&[], &formula_price(), "s", d.to_vec(), r, c, (fr, fc)),
                Verdict::Differ(_)
            ),
            "{r} / {c} / {fr} / {fc}"
        );
    };
    // Another difference with it.
    let mut more = d.clone();
    more.insert(1, "kv".into());
    reject(&r, &c, &fr, &fc, &more);
    // The gas differs by other than the forward, or the two gas figures by different amounts.
    let mut r1 = r.clone();
    r1["gas_used"] = json!(21_023_444);
    reject(&r1, &c, &fr, &fc, &d);
    r1["regular"] = json!(20_996_164);
    reject(&r1, &c, &fr, &fc, &d);
    // The engine's side is the one that used less: it must be the one that answered the stop.
    reject(&r, &c, &fc, &fr, &d);
    // The stop's output, kind, or gas left; the halted call's gas left.
    for (k, v) in [
        (
            "output",
            json!(mega_evm2_scenarios::record::hex(&mega_evm2::limit_exceeded_output(
                1, 20_000_002
            ))),
        ),
        ("output", json!("0x")),
        ("gas_left", json!(999_999)),
        ("status", json!("halt")),
    ] {
        let mut fc1 = fc.clone();
        fc1[1][k] = v;
        reject(&r, &c, &fr, &fc1, &d);
    }
    let mut fr1 = fr.clone();
    fr1[1]["gas_left"] = json!(1);
    reject(&r, &c, &fr1, &fc, &d);
    // Another frame differs too; the caller's gas left by other than the forward.
    let mut fc1 = fc.clone();
    fc1[0]["gas_left"] = json!(9_976_556);
    reject(&r, &c, &fr, &fc1, &d);
    let mut fc1 = fc.clone();
    fc1[0]["status"] = json!("revert");
    reject(&r, &c, &fr, &fc1, &d);
    // The differing frame is the transaction's own, or a creation.
    let (mut fr1, mut fc1) = (fr.clone(), fc.clone());
    fr1[1]["level"] = json!(0);
    fc1[1]["level"] = json!(0);
    reject(&r, &c, &fr1, &fc1, &d);
    let (mut fr1, mut fc1) = (fr.clone(), fc.clone());
    fr1[1]["kind"] = json!("Create");
    fc1[1]["kind"] = json!("Create");
    reject(&r, &c, &fr1, &fc1, &d);
    // The engine's price record: none, not held, not straddling, an allowance below the minimum
    // the reference's price would have crossed, two entries that straddle.
    let m = MODEXP.to_string();
    for priced in [
        json!([]),
        json!([[33_282, 18_841, false, m, true]]),
        json!([[1_000_001, 18_841, true, m, true]]),
        json!([[33_282, 499, true, m, true]]),
        json!([[33_282, 18_841, true, m, true], [33_282, 18_841, true, m, true]]),
        json!([[33_282, 18_841, true, Address::with_last_byte(8).to_string(), true]]),
        // An input within EIP-7823's bound: a price the two implementations disagree on is no
        // decided difference.
        json!([[33_282, 18_841, true, m, false]]),
        json!([[33_282, 18_841, true, m]]),
    ] {
        let mut c1 = c.clone();
        c1["priced"] = priced;
        reject(&r, &c1, &fr, &fc, &d);
    }
}

// --- the classes checked by a run under the reference's rule: `slotnum_zero`,
// `auth_preflight_reads` -----------------------------------------------------------

/// The engine's records of `s` (plain, inspected), as its harness renders them.
fn engine_runs(s: &mega_evm2_scenarios::Scenario) -> (Value, Value) {
    (mega_evm2_diff::driver::run(s, false), mega_evm2_diff::driver::run(s, true))
}

/// The engine's records `(plain, inspected)` shaped as a reference record: no price record, the
/// inspected run's frames, the two runs equal.
fn as_reference((plain, inspected): (Value, Value)) -> Value {
    let mut r = plain;
    let o = r.as_object_mut().unwrap();
    o.remove("priced");
    o.insert("frames".into(), inspected["frames"].clone());
    o.insert("inspected_equal".into(), json!(true));
    r
}

fn scenario(name: &str) -> mega_evm2_scenarios::Scenario {
    let mut s = mega_evm2_scenarios::by_name(name).expect("a scenario");
    mega_evm2_scenarios::add_chain_accounts(&mut s);
    s
}

/// The verdict of `s` against `reference`, with the accepted `classes` and the engine's reruns.
fn rerun_verdict(
    classes: &[String],
    s: &mega_evm2_scenarios::Scenario,
    reference: &Value,
) -> Verdict {
    use mega_evm2_diff::{driver::runs_under, gate::scenario_verdict};
    let (plain, inspected) = engine_runs(s);
    let recipient = mega_evm2_diff::driver::recipient(s);
    scenario_verdict(&[], classes, s.name, reference, &plain, &inspected, recipient, |rule| {
        runs_under(s, rule)
    })
}

#[test]
fn test_a_rerun_class_takes_a_case_whose_only_difference_is_its_rule() {
    use mega_evm2::ReferenceRule;
    use mega_evm2_diff::driver::runs_under;
    let both = vec![SLOTNUM_ZERO.to_string(), AUTH_PREFLIGHT_READS.to_string()];
    for (name, rule, class) in [
        ("edge_slotnum_returned", ReferenceRule::SlotNumber, SLOTNUM_ZERO),
        ("p6_slotnum", ReferenceRule::SlotNumber, SLOTNUM_ZERO),
        (
            "e17_authority_charges_out_of_gas",
            ReferenceRule::ShortListUnsimulated,
            AUTH_PREFLIGHT_READS,
        ),
    ] {
        let s = scenario(name);
        // The reference runs under its own rule: the engine's run under that rule.
        let reference = as_reference(runs_under(&s, rule));
        assert_eq!(rerun_verdict(&both, &s, &reference), Verdict::Class(class), "{name}");
        // Off unless the file enables it.
        let others: Vec<String> = both.iter().filter(|c| *c != class).cloned().collect();
        assert!(matches!(rerun_verdict(&others, &s, &reference), Verdict::Differ(_)), "{name}");
        // Any other difference: the run under the reference's rule no longer matches.
        let mut other = reference.clone();
        other["gas_used"] = json!(other["gas_used"].as_u64().unwrap() + 1);
        assert!(matches!(rerun_verdict(&both, &s, &other), Verdict::Differ(_)), "{name}");
    }
}

#[test]
fn test_a_rerun_class_takes_no_extra_read_its_rule_does_not_make() {
    // A transaction without a list that ran out of gas before its first frame, whose reference
    // read one account less: the read set has the preflight class's look, and is not the class.
    let s = scenario("edge_start_record_unpaid_absent_recipient");
    let both = vec![SLOTNUM_ZERO.to_string(), AUTH_PREFLIGHT_READS.to_string()];
    let mut reference = as_reference(engine_runs(&s));
    let accounts = reference["reads"]["accounts"].as_array_mut().unwrap();
    assert!(accounts.len() > 1);
    accounts.pop();
    assert!(matches!(rerun_verdict(&both, &s, &reference), Verdict::Differ(_)));
    // A case that differs in nothing matches; one that runs neither rule's code and differs is no
    // class.
    let s = scenario("p6_add");
    let reference = as_reference(engine_runs(&s));
    assert_eq!(rerun_verdict(&both, &s, &reference), Verdict::Match);
    let mut other = reference.clone();
    other["gas_used"] = json!(other["gas_used"].as_u64().unwrap() + 1);
    assert!(matches!(rerun_verdict(&both, &s, &other), Verdict::Differ(_)));
}

/// The other direction (the reference answered the stop, the engine ran the call), held to its own
/// price conditions.
#[test]
fn test_the_modexp_price_class_holds_the_other_direction_to_its_prices() {
    let call = |status: &str, output: String, left: u64| {
        json!({"kind": "Call", "level": 1, "gas_limit": 20_000, "reservoir": 0, "value": "0",
               "status": status, "output": output, "gas_left": left, "reservoir_left": 0})
    };
    let top = |left: u64| {
        json!({"kind": "Call", "level": 0, "gas_limit": 29_957_720, "reservoir": 0, "value": "0",
               "status": "success", "output": "0x", "gas_left": left, "reservoir_left": 0})
    };
    let r =
        json!({"status": "success", "halt": null, "gas_used": 20_042_033, "regular": 20_014_753});
    let fr = json!([top(9_957_967), call("revert", compute_stop(20_000_002), 20_000)]);
    let fc = json!([top(9_937_967), call("halt", "0x".into(), 0)]);
    let d = vec![
        "gas_used".to_string(),
        "regular".into(),
        "frame 0 gas_left: 9957967 vs 9937967".into(),
    ];
    let m = MODEXP.to_string();
    let verdict = |priced: Value| {
        let c = json!({"status": "success", "halt": null, "gas_used": 20_062_033,
                       "regular": 20_034_753, "priced": priced});
        classify(&[], &formula_price(), "s", d.clone(), &r, &c, (&fr, &fc))
    };
    assert_eq!(
        verdict(json!([[33_282, 251, false, m, true]])),
        Verdict::Class(MODEXP_FORMULA_PRICE)
    );
    for priced in [
        // Held, or an allowance the reference's 500 fits, or a formula price within the forward,
        // or an input within the bound.
        json!([[33_282, 251, true, m, true]]),
        json!([[33_282, 500, false, m, true]]),
        json!([[19_999, 251, false, m, true]]),
        json!([[33_282, 251, false, m, false]]),
    ] {
        assert!(matches!(verdict(priced.clone()), Verdict::Differ(_)), "{priced}");
    }
}

/// A ModExp call two frames deep, after a sibling call of its caller's caller: the frames running
/// when it was called (the transaction's own and its caller) have the forward more left on the
/// engine's side, the sibling the same.
#[test]
fn test_the_modexp_price_class_shifts_exactly_the_running_frames() {
    let frame = |level: u64, limit: u64, status: &str, output: String, left: u64| {
        json!({"kind": "Call", "level": level, "gas_limit": limit, "reservoir": 0, "value": "0",
               "status": status, "output": output, "gas_left": left, "reservoir_left": 0})
    };
    let f = 1_000_000;
    let r =
        json!({"status": "success", "halt": null, "gas_used": 21_000_000, "regular": 20_900_000});
    let c = json!({"status": "success", "halt": null, "gas_used": 20_000_000, "regular": 19_900_000,
                   "priced": [[33_282, 18_841, true, MODEXP.to_string(), true]]});
    let frames = |top: u64, sibling: u64, caller: u64, stopped: bool| {
        let call = if stopped {
            frame(2, f, "revert", compute_stop(20_000_002), f)
        } else {
            frame(2, f, "halt", "0x".into(), 0)
        };
        json!([
            frame(0, 29_000_000, "success", "0x".into(), top),
            frame(1, 50_000, "success", "0x".into(), sibling),
            frame(1, 5_000_000, "success", "0x".into(), caller),
            call
        ])
    };
    let d = vec!["gas_used".to_string(), "regular".into(), "frame 0 gas_left: 1 vs 2".into()];
    let fr = frames(7_000_000, 10_000, 3_000_000, false);
    let classify_frames =
        |fc: &Value| classify(&[], &formula_price(), "s", d.clone(), &r, &c, (&fr, fc));
    assert_eq!(
        classify_frames(&frames(8_000_000, 10_000, 4_000_000, true)),
        Verdict::Class(MODEXP_FORMULA_PRICE)
    );
    // The sibling shifted too, or the caller not.
    assert!(matches!(
        classify_frames(&frames(8_000_000, 1_010_000, 4_000_000, true)),
        Verdict::Differ(_)
    ));
    assert!(matches!(
        classify_frames(&frames(8_000_000, 10_000, 3_000_000, true)),
        Verdict::Differ(_)
    ));
    assert!(matches!(
        classify_frames(&frames(7_000_000, 10_000, 4_000_000, true)),
        Verdict::Differ(_)
    ));
}
