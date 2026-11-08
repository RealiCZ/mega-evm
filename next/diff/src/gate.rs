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
//! Accepted differences are listed by case (`[[difference]]`, `[[replay_difference]]`) or, for a
//! class the owner signed off whose shape the gate checks in full, enabled by name (`[[class]]`,
//! [`HALTED_FIRST_FRAME`], [`INVALID_IMMEDIATE_LABEL`], [`DUP_STACK_LABEL`]).
use mega_evm2_scenarios::record::{self, Harness};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// One accepted difference between the reference and the Satin engine, from
/// `known-differences.toml`.
///
/// `field` is a compared field, whose normalized values must be `reference` and `candidate`, or
/// `frames`: then the frames the inspector recorded must differ, and `reference` and `candidate`
/// are the two frame lists as JSON, each frame cut to the keys the frame comparison reads
/// ([`compared_frames`]). A `frames` entry also states its `class`, `why` and `signoff`.
#[derive(Debug, Clone)]
pub struct Known {
    pub scenario: String,
    pub field: String,
    pub reference: String,
    pub candidate: String,
}

/// An accepted difference of the engine replay gate: one transaction, keyed by its set, block
/// and index, and checked against its hash.
#[derive(Debug, Clone)]
pub struct KnownReplay {
    pub set: String,
    pub block: u64,
    pub index: u64,
    pub hash: String,
    /// The difference, with `scenario` set to [`replay_key`].
    pub known: Known,
}

/// The key a replay transaction is classified under: `<set>:<block>:<index>`.
pub fn replay_key(set: &str, block: u64, index: u64) -> String {
    format!("{set}:{block}:{index}")
}

/// `known-differences.toml` as built, or the file `MEGA_EVM2_KNOWN_DIFFERENCES` names (fault
/// injection; the gate scripts refuse it from outside).
fn known_table() -> toml::Table {
    let text = match std::env::var("MEGA_EVM2_KNOWN_DIFFERENCES") {
        Ok(path) => std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")),
        Err(_) => include_str!("../known-differences.toml").to_owned(),
    };
    text.parse().expect("known-differences.toml")
}

fn entries(doc: &toml::Table, kind: &str) -> Vec<toml::Value> {
    doc.get(kind).and_then(toml::Value::as_array).cloned().unwrap_or_default()
}

fn text(e: &toml::Value, k: &str) -> String {
    e.get(k)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("known difference: missing `{k}`"))
        .to_owned()
}

fn number(e: &toml::Value, k: &str) -> u64 {
    let n = e
        .get(k)
        .and_then(toml::Value::as_integer)
        .unwrap_or_else(|| panic!("known difference: missing `{k}`"));
    u64::try_from(n).unwrap_or_else(|_| panic!("known difference: negative `{k}`"))
}

fn known_entry(e: &toml::Value, scenario: String) -> Known {
    let k = Known {
        scenario,
        field: text(e, "field"),
        reference: text(e, "reference"),
        candidate: text(e, "candidate"),
    };
    if k.field == RECORD {
        // A whole-record difference is accepted only as a stated one with both records pinned.
        for key in ["why", "signoff"] {
            assert!(
                !text(e, key).trim().is_empty(),
                "known difference {}: empty `{key}`",
                k.scenario
            );
        }
        for rec in [&k.reference, &k.candidate] {
            let v: Value = serde_json::from_str(rec)
                .unwrap_or_else(|e| panic!("known difference {}: record: {e}", k.scenario));
            assert!(v.is_object(), "known difference {}: a record is an object", k.scenario);
        }
    }
    if k.field == "frames" {
        // A frame difference is accepted only as a stated, signed-off class with pinned frames.
        for key in ["class", "why", "signoff"] {
            assert!(
                !text(e, key).trim().is_empty(),
                "known difference {}: empty `{key}`",
                k.scenario
            );
        }
        for list in [&k.reference, &k.candidate] {
            let v: Value = serde_json::from_str(list)
                .unwrap_or_else(|e| panic!("known difference {}: frames: {e}", k.scenario));
            assert!(v.is_array(), "known difference {}: frames must be a list", k.scenario);
        }
    }
    k
}

/// The `field` of an entry that pins the two whole normalized records ([`canonical`], as compact
/// JSON, keys sorted): for a case in which one side executes and the other refuses, where no one
/// field is the difference. The entry accepts the case only when both records are exactly the
/// pinned ones, and takes every difference of a field of the record.
pub const RECORD: &str = "record";

/// The accepted differences of the synthetic gate (`[[difference]]`).
pub fn known_differences() -> Vec<Known> {
    entries(&known_table(), "difference")
        .iter()
        .map(|e| known_entry(e, text(e, "scenario")))
        .collect()
}

/// The accepted differences of the engine replay gate (`[[replay_difference]]`).
pub fn known_replay_differences() -> Vec<KnownReplay> {
    entries(&known_table(), "replay_difference")
        .iter()
        .map(|e| {
            let (set, block, index) = (text(e, "set"), number(e, "block"), number(e, "index"));
            let key = replay_key(&set, block, index);
            KnownReplay { known: known_entry(e, key), set, block, index, hash: text(e, "hash") }
        })
        .collect()
}

/// The class of evm2 #871: a transaction whose first frame's runtime gas phase runs out of gas.
/// The reference opens no frame; evm2 reports one halted level-0 frame to the inspector
/// (go-ethereum's `haltTopFrame`). Its shape, all of which [`halted_first_frame`] checks:
/// - the reference's frame list is empty;
/// - the engine's inspected frame list is one frame: level 0, status `halt`, kind `Call` or
///   `Create`;
/// - the only difference of the case is that frame count (`frame count 0 vs 1`): every other
///   compared field is equal, and so are each side's plain and inspected runs;
/// - the transaction ran (no refusal) and failed: status `halt` with `OutOfGas`, or a failed
///   deposit.
pub const HALTED_FIRST_FRAME: &str = "halted_first_frame";

/// The halt label of DUPN, SWAPN and EXCHANGE with an immediate EIP-8024 rejects: revm reports
/// `OpcodeNotFound`, evm2 `InvalidImmediateEncoding` (the label only: gas, output and state are
/// equal). Its shape, all of which [`invalid_immediate_label`] checks: the only difference of the
/// case is `halt`, normalized `OpcodeNotFound` on the reference's side and
/// `InvalidImmediateEncoding` on the engine's, both runs halted.
pub const INVALID_IMMEDIATE_LABEL: &str = "invalid_immediate_label";

/// The halt label of a failed DUP (`DUP1` to `DUP16` deeper than the stack, and `DUPN`): revm
/// reports `StackOverflow` (its `dup` returns false for both underflow and overflow, and its DUP
/// instructions map every false to overflow), evm2 `StackUnderflow` (the label only: gas, output
/// and state are equal). Its shape, all of which [`dup_stack_label`] checks: the only difference of
/// the case is `halt`, normalized `StackOverflow` on the reference's side and `StackUnderflow` on
/// the engine's, both runs halted.
pub const DUP_STACK_LABEL: &str = "dup_stack_label";

/// The classes the gate knows the shape of; a `[[class]]` table names one of them.
const CLASSES: [&str; 3] = [HALTED_FIRST_FRAME, INVALID_IMMEDIATE_LABEL, DUP_STACK_LABEL];

/// The accepted classes of `known-differences.toml` (`[[class]]`): each names a class of
/// [`CLASSES`] and states its `why` and `signoff`. A class not enabled there is not accepted.
pub fn known_classes() -> Vec<String> {
    let mut out = Vec::new();
    for e in entries(&known_table(), "class") {
        let name = text(&e, "name");
        assert!(CLASSES.contains(&name.as_str()), "known class {name}: no such class");
        for key in ["why", "signoff"] {
            assert!(!text(&e, key).trim().is_empty(), "known class {name}: empty `{key}`");
        }
        assert!(!out.contains(&name), "known class {name}: listed twice");
        out.push(name);
    }
    out
}

/// Whether the differences `diffs` of one case, whose outcomes are `reference` and `candidate`
/// (not normalized) and whose recorded frames are `frames` (reference, inspected engine), are
/// exactly the shape of [`HALTED_FIRST_FRAME`].
pub fn halted_first_frame(
    diffs: &[String],
    reference: &Value,
    candidate: &Value,
    frames: (&Value, &Value),
) -> bool {
    let [only] = diffs else { return false };
    if only != "frame count 0 vs 1" {
        return false;
    }
    let (Some(r), Some([frame])) = (frames.0.as_array(), frames.1.as_array().map(Vec::as_slice))
    else {
        return false;
    };
    let root_halt = frame["level"] == 0 &&
        frame["status"] == "halt" &&
        matches!(frame["kind"].as_str(), Some("Call" | "Create"));
    let failed = |o: &Value| {
        let o = canonical(o);
        o.get("error").is_none() &&
            o["status"] == "halt" &&
            matches!(o["halt"].as_str(), Some("OutOfGas" | "FailedDeposit"))
    };
    r.is_empty() && root_halt && failed(reference) && failed(candidate)
}

/// Whether the differences `diffs` of one case, whose outcomes are `reference` and `candidate`
/// (not normalized), are exactly the shape of [`INVALID_IMMEDIATE_LABEL`].
pub fn invalid_immediate_label(diffs: &[String], reference: &Value, candidate: &Value) -> bool {
    halt_label_only(diffs, reference, candidate, ("OpcodeNotFound", "InvalidImmediateEncoding"))
}

/// Whether the differences `diffs` of one case, whose outcomes are `reference` and `candidate`
/// (not normalized), are exactly the shape of [`DUP_STACK_LABEL`].
pub fn dup_stack_label(diffs: &[String], reference: &Value, candidate: &Value) -> bool {
    halt_label_only(diffs, reference, candidate, ("StackOverflow", "StackUnderflow"))
}

/// Whether the only difference `diffs` holds is the halt label, normalized `labels.0` on the
/// reference's side and `labels.1` on the engine's, both runs halted.
fn halt_label_only(
    diffs: &[String],
    reference: &Value,
    candidate: &Value,
    labels: (&str, &str),
) -> bool {
    let [only] = diffs else { return false };
    let (a, b) = (canonical(reference), canonical(candidate));
    only == "halt" &&
        a["status"] == "halt" &&
        b["status"] == "halt" &&
        a["halt"] == labels.0 &&
        b["halt"] == labels.1
}

/// How one scenario's differences relate to the accepted list.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// No difference and no accepted entry.
    Match,
    /// The differences are exactly the scenario's accepted entry.
    Known,
    /// The scenario has no entry, and its differences are exactly the shape of an accepted class
    /// (`[[class]]`), named here.
    Class(&'static str),
    /// Anything else: an unlisted difference, a listed entry that differs differently, or a
    /// listed entry that no longer differs (stale).
    Differ(Vec<String>),
}

/// Classifies the differences `diffs` of scenario `name`, where `reference` and `candidate` are
/// the two outcomes (not normalized) and `frames` the frames the reference and the inspected
/// candidate recorded. Only a difference in exactly the listed field, with exactly the listed
/// normalized values (for `frames`, exactly the listed frames), is accepted; a scenario without an
/// entry is accepted as a class of `classes` only when its differences are exactly that class's
/// shape. A listed scenario is held to its entries alone.
pub fn classify(
    known: &[Known],
    classes: &[String],
    name: &str,
    diffs: Vec<String>,
    reference: &Value,
    candidate: &Value,
    frames: (&Value, &Value),
) -> Verdict {
    let entries: Vec<&Known> = known.iter().filter(|k| k.scenario == name).collect();
    if entries.is_empty() {
        if diffs.is_empty() {
            return Verdict::Match;
        }
        let enabled = |class: &str| classes.iter().any(|c| c == class);
        if enabled(HALTED_FIRST_FRAME) && halted_first_frame(&diffs, reference, candidate, frames) {
            return Verdict::Class(HALTED_FIRST_FRAME);
        }
        if enabled(INVALID_IMMEDIATE_LABEL) && invalid_immediate_label(&diffs, reference, candidate)
        {
            return Verdict::Class(INVALID_IMMEDIATE_LABEL);
        }
        if enabled(DUP_STACK_LABEL) && dup_stack_label(&diffs, reference, candidate) {
            return Verdict::Class(DUP_STACK_LABEL);
        }
        return Verdict::Differ(diffs);
    }
    if diffs.is_empty() {
        return Verdict::Differ(vec![format!("stale known difference: {name} no longer differs")]);
    }
    let (a, b) = (canonical(reference), canonical(candidate));
    let mut remaining = diffs;
    for k in entries {
        if k.field == RECORD {
            let pinned = |s: &str| serde_json::from_str::<Value>(s).ok();
            if pinned(&k.reference).as_ref() == Some(&a) &&
                pinned(&k.candidate).as_ref() == Some(&b)
            {
                let fields: BTreeSet<&String> =
                    a.as_object().into_iter().chain(b.as_object()).flat_map(|o| o.keys()).collect();
                remaining.retain(|d| !fields.contains(d));
                continue;
            }
            return Verdict::Differ(vec![format!(
                "known difference {name}.{} did not occur as listed",
                k.field
            )]);
        }
        let (hit, same) = if k.field == "frames" {
            let listed = |list: &str| serde_json::from_str::<Value>(list).ok();
            (
                remaining.iter().position(|d| is_frames_difference(d)),
                compared_frames(frames.0).is_some_and(|f| Some(f) == listed(&k.reference)) &&
                    compared_frames(frames.1).is_some_and(|f| Some(f) == listed(&k.candidate)),
            )
        } else {
            (
                remaining.iter().position(|d| *d == k.field),
                a.get(&k.field).and_then(Value::as_str) == Some(k.reference.as_str()) &&
                    b.get(&k.field).and_then(Value::as_str) == Some(k.candidate.as_str()),
            )
        };
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

/// The differences of one scenario of the synthetic gate: a record without exactly the fields
/// its harness renders (`record_fields:<harness>:<what>`, [`record::check_fields`]), between the
/// reference's record `reference` and the engine's plain record `plain` ([`differences`]), the
/// reference's own plain and inspected runs disagreeing (`reference_plain_vs_inspected`), the
/// engine's plain and inspected records disagreeing (`candidate_inspected:<field>`,
/// [`same_engine_differences`]) and the frames the reference and the inspected engine recorded
/// ([`frames_diff`], when neither refused the transaction).
pub fn scenario_differences(reference: &Value, plain: &Value, inspected: &Value) -> Vec<String> {
    let mut diffs = Vec::new();
    for (harness, r) in
        [(Harness::Reference, reference), (Harness::Engine, plain), (Harness::Engine, inspected)]
    {
        let keys = r.as_object().expect("outcome object").keys().map(String::as_str);
        if let Err(e) = record::check_fields(harness, keys) {
            diffs.push(format!("record_fields:{harness:?}:{e}"));
        }
    }
    diffs.extend(differences(reference, plain));
    if reference["inspected_equal"] != true {
        diffs.push("reference_plain_vs_inspected".into());
    }
    diffs.extend(
        same_engine_differences(plain, inspected)
            .into_iter()
            .map(|k| format!("candidate_inspected:{k}")),
    );
    if reference.get("error").is_none() &&
        inspected.get("error").is_none() &&
        let Some(d) = frames_diff(&reference["frames"], &inspected["frames"])
    {
        diffs.push(d);
    }
    diffs
}

/// The verdict of the synthetic gate on scenario `name`: its [`scenario_differences`],
/// [`classify`]d against the accepted differences `known`.
pub fn scenario_verdict(
    known: &[Known],
    classes: &[String],
    name: &str,
    reference: &Value,
    plain: &Value,
    inspected: &Value,
) -> Verdict {
    let diffs = scenario_differences(reference, plain, inspected);
    classify(
        known,
        classes,
        name,
        diffs,
        reference,
        plain,
        (&reference["frames"], &inspected["frames"]),
    )
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
        "GATE replay blocks={} match={} pre={} refused={} known={} class={} differ={}",
        sum(totals, |k| k == "blocks"),
        sum(totals, |k| k == "match"),
        sum(totals, |k| k == "pre_block:match"),
        sum(totals, |k| k.starts_with("refused_both")),
        sum(totals, |k| k == "known"),
        sum(totals, |k| k == "class"),
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
        (
            &[
                "a transaction from the system address must be a legacy call to a whitelisted contract",
            ][..],
            "system_shape",
        ),
        (&["GasFloorMoreThanGasLimit", "GasLimitBelowFloor"][..], "floor_gas"),
        (&["CallerGasLimitMoreThanBlock", "GasLimitMoreThanBlock"][..], "tx_gas_above_block"),
        (&["SALT bucket lookup failed"][..], "salt_lookup"),
        (&["MissingChainId", "missing chain id"][..], "missing_chain_id"),
        (&["InvalidChainId", "invalid chain id"][..], "chain_id"),
        (&["NonceOverflow", "nonce overflow"][..], "nonce_overflow"),
        (&["CreateInitCodeSizeLimit"][..], "initcode_size"),
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

/// The frame keys [`frames_diff`] compares: every key of a recorded frame.
const FRAME_KEYS: [&str; 9] = mega_evm2_scenarios::record::FRAME;

/// Whether `d` is a difference [`frames_diff`] reported.
fn is_frames_difference(d: &str) -> bool {
    d.starts_with("frame ")
}

/// `frames` cut to what [`frames_diff`] reads of a frame on its own: every key but the output of a
/// halted frame and the gas left of a frame that did not succeed. `None` when `frames` is not a
/// list of frames.
pub fn compared_frames(frames: &Value) -> Option<Value> {
    let frames = frames.as_array()?;
    let mut out = Vec::new();
    for f in frames {
        let f = f.as_object()?;
        let status = f.get("status")?;
        let mut cut = serde_json::Map::new();
        for k in FRAME_KEYS {
            if (k == "output" && status == "halt") ||
                ((k == "gas_left" || k == "reservoir_left") && status != "success")
            {
                continue;
            }
            cut.insert(k.into(), f.get(k)?.clone());
        }
        out.push(Value::Object(cut));
    }
    Some(Value::Array(out))
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
        for k in FRAME_KEYS {
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

#[cfg(test)]
mod tests {
    use super::error_kind;

    /// A refusal both engines make is one kind, whichever text each writes for it: the texts
    /// below are the two sides' own for the same transactions.
    #[test]
    fn test_one_refusal_is_one_kind_on_both_sides() {
        for (reference, engine, kind) in [
            (
                "Transaction(OpTxError(Base(GasFloorMoreThanGasLimit { gas_floor: 28632, gas_limit: 23000 })))",
                "GasLimitBelowFloor { required: 28632, got: 23000 }",
                "floor_gas",
            ),
            (
                "Transaction(OpTxError(Base(CallerGasLimitMoreThanBlock)))",
                "GasLimitMoreThanBlock { gas_limit: 10000000001, block_gas_limit: 10000000000 }",
                "tx_gas_above_block",
            ),
            (
                "Transaction(OpTxError(Base(CallGasCostMoreThanGasLimit { initial_gas: 15852, gas_limit: 15000 })))",
                "IntrinsicGasTooLow { required: 61876, got: 15000 }",
                "intrinsic_gas",
            ),
            (
                "Transaction(OpTxError(Base(CreateInitCodeSizeLimit)))",
                "CreateInitCodeSizeLimit { limit: 1048576, got: 1048577 }",
                "initcode_size",
            ),
        ] {
            assert_eq!(error_kind(reference), kind, "{reference}");
            assert_eq!(error_kind(engine), kind, "{engine}");
        }
        // The floor and the intrinsic gas stay apart.
        assert_ne!(error_kind("GasLimitBelowFloor { required: 2, got: 1 }"), "intrinsic_gas");
    }
}
