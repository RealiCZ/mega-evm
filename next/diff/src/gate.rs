//! Differential normalization and the complete exclusion policy of the gates.
//!
//! Exclusions: metadata (name/group), debug-only precompile `priced` records; inspector opcode
//! steps (keyless uses a dispatcher opcode); failed-frame remaining gas (revm observes it before
//! settlement, evm2 after); halted-frame output (revm retains creation bytes, evm2 clears them).
//! Real replay frame target/selector are reference-only metadata. Pre-block system-call gas is
//! excluded across engines because their system budgets differ; state, reads and status gate it.
//! Error strings are compared by explicit refusal kind, not presentation or numeric payload.
//! Account lifecycle flags cover changed accounts, not journal-internal touched-only entries.
//! Block net state compares changed account info and slots, not internal bundle/wipe encodings.
//! Every other emitted semantic field is compared; missing fields and unknown kinds fail closed.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// One accepted difference between the reference and the Satin engine, from
/// `known-differences.toml`.
#[derive(Debug, Clone)]
pub struct Known {
    pub scenario: String,
    pub field: String,
    pub reference: String,
    pub candidate: String,
}

/// The accepted differences of the synthetic gate: `known-differences.toml` as built, or the file
/// `MEGA_EVM2_KNOWN_DIFFERENCES` names (fault injection; the gate scripts refuse it from outside).
pub fn known_differences() -> Vec<Known> {
    let text = match std::env::var("MEGA_EVM2_KNOWN_DIFFERENCES") {
        Ok(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")),
        Err(_) => include_str!("../known-differences.toml").to_owned(),
    };
    let doc: toml::Table = text.parse().expect("known-differences.toml");
    let entries =
        doc.get("difference").and_then(toml::Value::as_array).cloned().unwrap_or_default();
    entries
        .iter()
        .map(|e| {
            let text = |k: &str| {
                e.get(k)
                    .and_then(toml::Value::as_str)
                    .unwrap_or_else(|| panic!("known difference: missing `{k}`"))
                    .to_owned()
            };
            Known {
                scenario: text("scenario"),
                field: text("field"),
                reference: text("reference"),
                candidate: text("candidate"),
            }
        })
        .collect()
}

/// How one scenario's differences relate to the accepted list.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// No difference and no accepted entry.
    Match,
    /// The differences are exactly the scenario's accepted entry.
    Known,
    /// Anything else: an unlisted difference, a listed entry that differs differently, or a
    /// listed entry that no longer differs (stale).
    Differ(Vec<String>),
}

/// Classifies the differences `diffs` of scenario `name`, where `reference` and `candidate` are
/// the two outcomes (not normalized). Only a difference in exactly the listed field, with exactly
/// the listed normalized values, is accepted.
pub fn classify(
    known: &[Known],
    name: &str,
    diffs: Vec<String>,
    reference: &Value,
    candidate: &Value,
) -> Verdict {
    let entries: Vec<&Known> = known.iter().filter(|k| k.scenario == name).collect();
    if entries.is_empty() {
        return if diffs.is_empty() { Verdict::Match } else { Verdict::Differ(diffs) };
    }
    if diffs.is_empty() {
        return Verdict::Differ(vec![format!("stale known difference: {name} no longer differs")]);
    }
    let (a, b) = (canonical(reference), canonical(candidate));
    let mut remaining = diffs;
    for k in entries {
        let hit = remaining.iter().position(|d| *d == k.field);
        let same = a.get(&k.field).and_then(Value::as_str) == Some(k.reference.as_str()) &&
            b.get(&k.field).and_then(Value::as_str) == Some(k.candidate.as_str());
        match hit {
            Some(i) if same => {
                remaining.remove(i);
            }
            _ => {
                return Verdict::Differ(vec![format!(
                    "known difference {name}.{} did not occur as listed",
                    k.field
                )])
            }
        }
    }
    if remaining.is_empty() { Verdict::Known } else { Verdict::Differ(remaining) }
}

/// The counts of a gate's summary line, when `MEGA_EVM2_EXPECT` names them (the summary line
/// comparison in `gate-lib.sh` covers the rest).
pub fn expected(key: &str, actual: u64) {
    if let Ok(s) = std::env::var(key) {
        assert_eq!(actual, s.parse::<u64>().expect("numeric expected count"), "{key}");
    }
}

pub fn finish(totals: &BTreeMap<String, u64>) {
    let bad: Vec<_> = totals
        .iter()
        .filter(|(k, n)| {
            **n > 0 &&
                (k.contains("differ") || k.contains("missing") || k.contains("envelope_error"))
        })
        .collect();
    assert!(bad.is_empty(), "differential gate: {bad:?}");
    expected("MEGA_EVM2_EXPECT", *totals.get("match").unwrap_or(&0));
}

fn sum(totals: &BTreeMap<String, u64>, pick: impl Fn(&str) -> bool) -> u64 {
    totals.iter().filter(|(k, _)| pick(k)).map(|(_, n)| n).sum()
}

fn differing(totals: &BTreeMap<String, u64>) -> u64 {
    sum(totals, |k| k.contains("differ") || k.contains("missing") || k.contains("envelope_error"))
}

/// The summary line of the engine replay gate.
pub fn replay_line(totals: &BTreeMap<String, u64>) -> String {
    format!(
        "GATE replay blocks={} match={} pre={} refused={} differ={}",
        sum(totals, |k| k == "blocks"),
        sum(totals, |k| k == "match"),
        sum(totals, |k| k == "pre_block:match"),
        sum(totals, |k| k.starts_with("refused_both")),
        differing(totals),
    )
}

/// The summary line of the block executor gate.
pub fn block_line(totals: &BTreeMap<String, u64>) -> String {
    format!(
        "GATE block blocks={} included={} refused={} counters={} post={} differ={}",
        sum(totals, |k| k == "blocks"),
        sum(totals, |k| k == "match"),
        sum(totals, |k| k.starts_with("refused_both")),
        sum(totals, |k| k == "block_counters:match"),
        sum(totals, |k| k == "post_state:match"),
        differing(totals),
    )
}

pub fn halt(v: &Value) -> Value {
    let Some(mut s) = v.as_str() else {
        assert!(v.is_null(), "bad halt: {v}");
        return Value::Null;
    };
    while (s.starts_with("Op(") || s.starts_with("Base(")) && s.ends_with(')') {
        s = &s[s.find('(').unwrap() + 1..s.len() - 1];
    }
    let s = match s {
        "OutOfGas(Basic)" => "OutOfGas",
        "OutOfGas(Memory)" => "MemoryOOG",
        "OutOfGas(MemoryLimit)" => "MemoryLimitOOG",
        "OutOfGas(Precompile)" => "PrecompileOOG",
        "OutOfGas(InvalidOperand)" => "InvalidOperandOOG",
        "OutOfGas(ReentrancySentry)" => "ReentrancySentryOOG",
        "CreateContractSizeLimit" | "CreateContractSizeLimitExceeded" => "CreateContractSizeLimit",
        other => other,
    };
    json!(s)
}

pub fn error_kind(s: &str) -> String {
    if s.contains("transaction body and authorization records exceed") {
        return "tx_body_records".into();
    }
    if s.contains("more than blocks available gas") {
        return "block_gas_limit".into();
    }
    if s.contains("Block transactions data limit") {
        return "block_data_limit".into();
    }
    if s.contains("Block KV update limit") {
        return "block_kv_limit".into();
    }
    if s.contains("Block state growth limit") {
        return "block_growth_limit".into();
    }
    if s.contains("InvalidNonce") {
        let num = |label: &str| -> u64 {
            let p = s.find(label).expect("nonce field") + label.len();
            s[p..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .expect("nonce number")
        };
        return if num("got: ") < num("expected: ") { "nonce_too_low" } else { "nonce_too_high" }
            .into();
    }
    if s.contains("nonce") && s.contains("too low") || s.contains("NonceTooLow") {
        return "nonce_too_low".into();
    }
    if s.contains("nonce") && s.contains("too high") || s.contains("NonceTooHigh") {
        return "nonce_too_high".into();
    }
    for (needles, kind) in [
        (&["InsufficientFunds", "LackOfFundForMaxFee", "lack of funds"][..], "insufficient_funds"),
        (
            &[
                "FeeCapLessThanBaseFee",
                "GasPriceLessThanBasefee",
                "fee cap less than base fee",
                "gas price is less than basefee",
            ][..],
            "fee_below_base",
        ),
        (
            &["IntrinsicGasTooLow", "CallGasCostMoreThanGasLimit", "intrinsic gas too low"][..],
            "intrinsic_gas",
        ),
        (&["RejectCallerWithCode", "caller has code"][..], "caller_code"),
        (&["InvalidChainId", "invalid chain id"][..], "chain_id"),
        (&["NonceOverflow", "nonce overflow"][..], "nonce_overflow"),
    ] {
        if needles.iter().any(|n| s.contains(n)) {
            return kind.into();
        }
    }
    // Unknown errors remain exact: two different unclassified refusals cannot pass.
    format!("unmapped:{s}")
}

pub fn canonical(v: &Value) -> Value {
    let mut v = v.clone();
    let o = v.as_object_mut().expect("outcome object");
    for k in ["name", "group", "frames", "priced", "inspected_equal", "inspected"] {
        o.remove(k);
    }
    if let Some(h) = o.get_mut("halt") {
        *h = halt(h);
    }
    if let Some(e) = o.get_mut("error") {
        *e = json!(error_kind(e.as_str().expect("error string")));
    }
    if let Some(e) = o.get_mut("reason") {
        *e = json!(error_kind(e.as_str().expect("refusal string")));
    }
    if let Some(diff) = o.get_mut("diff") {
        for a in diff["accounts"].as_object_mut().expect("diff accounts").values_mut() {
            if a["destroyed"] == true {
                // Destroyed account contents are not post-state; both engines erase them.
                for k in ["balance", "nonce", "code_hash", "code", "storage"] {
                    a.as_object_mut().unwrap().remove(k);
                }
            }
        }
    }
    v
}

pub fn differences(a: &Value, b: &Value) -> Vec<String> {
    let (a, b) = (canonical(a), canonical(b));
    let keys: BTreeSet<_> =
        a.as_object().unwrap().keys().chain(b.as_object().unwrap().keys()).collect();
    keys.into_iter().filter(|k| a.get(*k) != b.get(*k)).cloned().collect()
}

/// Inspector equality within one engine keeps raw halt/refusal details and debug outputs.
pub fn same_engine_differences(a: &Value, b: &Value) -> Vec<String> {
    let (mut a, mut b) = (a.clone(), b.clone());
    a.as_object_mut().unwrap().remove("frames");
    b.as_object_mut().unwrap().remove("frames");
    let keys: BTreeSet<_> =
        a.as_object().unwrap().keys().chain(b.as_object().unwrap().keys()).collect();
    keys.into_iter().filter(|k| a.get(*k) != b.get(*k)).cloned().collect()
}

pub fn frames_diff(a: &Value, b: &Value) -> Option<String> {
    let (a, b) = (
        a.as_array().expect("reference frames required"),
        b.as_array().expect("evm2 frames required"),
    );
    if a.len() != b.len() {
        return Some(format!("frame count {} vs {}", a.len(), b.len()));
    }
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        for k in [
            "kind",
            "level",
            "gas_limit",
            "reservoir",
            "value",
            "status",
            "output",
            "gas_left",
            "reservoir_left",
        ] {
            if k == "output" && x["status"] == "halt" && y["status"] == "halt" {
                continue;
            }
            if (k == "gas_left" || k == "reservoir_left") && x["status"] != "success" {
                continue;
            }
            assert!(x.get(k).is_some() && y.get(k).is_some(), "missing frame {k}");
            if x[k] != y[k] {
                return Some(format!("frame {i} {k}: {} vs {}", x[k], y[k]));
            }
        }
    }
    None
}
