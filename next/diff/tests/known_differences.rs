//! The accepted-difference logic of the synthetic gate: only exactly the listed difference
//! passes; an unlisted difference, a mismatching value and a stale entry all fail.

use mega_evm2_diff::gate::{Known, Verdict, classify, known_differences};
use serde_json::{Value, json};

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
    assert_eq!(classify(&known(), "s", vec!["halt".into()], &r, &c), Verdict::Known);
}

#[test]
fn test_listed_scenario_with_another_field_differs() {
    let (r, c) = (outcome("StackOverflow"), outcome("StackUnderflow"));
    let verdict = classify(&known(), "s", vec!["halt".into(), "gas_used".into()], &r, &c);
    assert_eq!(verdict, Verdict::Differ(vec!["gas_used".into()]));
}

#[test]
fn test_listed_field_with_other_values_differs() {
    let (r, c) = (outcome("StackOverflow"), outcome("InvalidJump"));
    assert!(matches!(classify(&known(), "s", vec!["halt".into()], &r, &c), Verdict::Differ(_)));
    let (r, c) = (outcome("OutOfGas"), outcome("StackUnderflow"));
    assert!(matches!(classify(&known(), "s", vec!["halt".into()], &r, &c), Verdict::Differ(_)));
}

#[test]
fn test_listed_entry_that_no_longer_differs_is_stale() {
    let (r, c) = (outcome("StackUnderflow"), outcome("StackUnderflow"));
    assert!(
        matches!(classify(&known(), "s", vec![], &r, &c), Verdict::Differ(d) if d[0].contains("stale"))
    );
}

#[test]
fn test_unlisted_scenario_differs_or_matches() {
    let (r, c) = (outcome("StackOverflow"), outcome("StackUnderflow"));
    assert_eq!(
        classify(&known(), "other", vec!["halt".into()], &r, &c),
        Verdict::Differ(vec!["halt".into()])
    );
    assert_eq!(classify(&known(), "other", vec![], &r, &r), Verdict::Match);
}

#[test]
fn test_known_differences_file_lists_exactly_the_dupn_entry() {
    let k = known_differences();
    assert_eq!(k.len(), 1);
    assert_eq!((k[0].scenario.as_str(), k[0].field.as_str()), ("p6_dupn", "halt"));
    assert_eq!(
        (k[0].reference.as_str(), k[0].candidate.as_str()),
        ("StackOverflow", "StackUnderflow")
    );
}
