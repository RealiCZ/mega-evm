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
//! [`HALTED_FIRST_FRAME`], [`INVALID_IMMEDIATE_LABEL`], [`DUP_STACK_LABEL`],
//! [`AUTH_PREFLIGHT_READS`], [`MODEXP_FORMULA_PRICE`], [`SLOTNUM_ZERO`],
//! [`RESERVOIR_GIVE_BACK`], [`DEPOSIT_HISTORY`], [`ORACLE_READ_BEFORE_CHARGE`],
//! [`REFUSAL_COSTS_NOTHING`], [`KEYLESS_DECODED_DISPATCH`], [`HINT_WITH_VALUE`],
//! [`MST_PACKING_BUDGETS`], [`ORACLE_NO_SERVICE`], [`SYSTEM_CONTRACT_DEPLOYMENT`]).
use mega_evm2::ReferenceRule;
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

/// The read set of an EIP-7702 transaction whose authorization list ran out of gas part-way: the
/// engine simulates every list before applying it, whatever its length, so it loads every
/// authority whose entry passes the chain-id, nonce and signature gates; the reference simulates
/// only a list that could cross a limit, so it loads only the authorities its application reached
/// (the other implementation has not made the decided change yet). A read set does not say which
/// account is an authority, so the shape is checked by running the case again on the engine
/// without the simulation of a list too short to cross a limit, as the reference runs it
/// ([`ReferenceRule::ShortListUnsimulated`]): the case is of the class when that run, plain and
/// inspected, has no difference from the reference at all ([`scenario_verdict`]).
pub const AUTH_PREFLIGHT_READS: &str = "auth_preflight_reads";

/// ModExp charges its 500 minimum, then its formula price, and only then checks EIP-7823's bound
/// (evm2 upstream's order, the specification's); the reference checks the bound first, so an
/// over-bound input costs it the 500 minimum and is priced at it (the other implementation has not
/// made the decided change yet). The difference shows as a halt label (`PrecompileOOG` against
/// `PrecompileError` when the gas lies between the two prices), and under gas detention as a call
/// decided differently from its price, the compute stop on one side where the other runs the call
/// and burns its forward, with whatever the program then does with the gas it has. So the shape is
/// checked by running the case again on the engine with the reference's order
/// ([`ReferenceRule::ModexpBoundFirst`]): the case is of the class when that run, plain and
/// inspected, has no difference from the reference at all ([`scenario_verdict`]).
pub const MODEXP_FORMULA_PRICE: &str = "modexp_formula_price";

/// `SLOTNUM` pushes zero in the engine, the reference's slot number in the reference (the other
/// implementation has not made the decided change yet). What the pushed value changes is the
/// program's to decide, so the shape is checked by running the case again on the engine with
/// `SLOTNUM` pushing the block environment's slot number, as the reference does
/// ([`ReferenceRule::SlotNumber`]): the case is of the class when that run, plain and inspected,
/// has no difference from the reference at all ([`scenario_verdict`]).
pub const SLOTNUM_ZERO: &str = "slotnum_zero";

/// Every give-back of state and history gas credits a reservoir and never regular gas, and the
/// state an opcode or a keyless deployment creates takes its legacy share from regular gas first
/// (`SATIN-HIST-012`, `SATIN-POOL-014`, `SATIN-STATE-012`); the reference still credits a give-back
/// to regular gas first, up to the state gas that gas paid, returns a failed frame's spill to its
/// regular gas and draws every state charge from the reservoir first (the other implementation has
/// not made the decided change yet). What that moves — `GAS`, the gas a call forwards, which
/// charge runs out of gas, the split of the gas left between the two pools, gas detention's
/// compute and a keyless deployment's `gasUsed` — is the program's to decide, so the shape is
/// checked by running the case again on the engine under EIP-8037's give-back rule without legacy
/// shares, as the reference runs it ([`ReferenceRule::Eip8037GiveBack`]): the case is of the class
/// when that run, plain and inspected, has no difference from the reference at all
/// ([`scenario_verdict`]). A case that meets this change and another class's is checked by running
/// it under both rules, and is that other class's.
///
/// Recorded blocks reach it as well (a reentrancy lock set and cleared gives its record back, and
/// a transaction that ran close to its gas can end otherwise), so the engine replay gate runs a
/// transaction again under this rule too ([`REPLAY_RERUN_CLASSES`]), with the same test: it is of
/// the class when that run, plain and inspected, has no difference from the reference's row at
/// all. The block executor gate runs a block that differs again, whole, under this rule: the block
/// is of the class when that run, plain and inspected, has no difference from any of the
/// reference's rows (its transactions, its counters and its post-state). Neither runs a case
/// again for the other three classes of [`RERUN_CLASSES`], and a case of one there differs.
pub const RESERVOIR_GIVE_BACK: &str = "reservoir_give_back";

/// A deposit and a system-originated transaction pay history gas as any transaction does (their
/// body, calldata, logs, records and deposited code), and a failed deposit reports its body and
/// calldata as history bytes (`SATIN-HIST-016`, `SATIN-HIST-022`, `SATIN-DEP-001`); the reference
/// prices no history for them (the other implementation has not made the decided change yet).
/// What that moves — a deposit's gas used, which deposit fails, its reservoir, what it may still
/// do — is the transaction's to decide, so the shape is checked by running the case again on the
/// engine with that exemption ([`ReferenceRule::DepositHistoryExempt`]): the case is of the class
/// when that run, plain and inspected, has no difference from the reference at all
/// ([`scenario_verdict`]). Recorded blocks reach it (every block starts with a deposit), so the
/// engine replay gate and the block executor gate run it again too ([`REPLAY_RERUN_CLASSES`]).
pub const DEPOSIT_HISTORY: &str = "deposit_history";

/// An Oracle `SLOAD` whose regular gas cannot pay the cold access, `COLD_SLOAD_COST` with its warm
/// base, still loads the slot, asks the service and marks the read before it runs out of gas
/// (`SATIN-ORCL-004`, `SATIN-DET-003`); the reference charges the warm base first, runs out of gas
/// before any of it when the rest cannot then be paid, and marks a read once both are paid.
/// Checked by running the case again with the reference's order
/// ([`ReferenceRule::OracleColdSkip`]).
pub const ORACLE_READ_BEFORE_CHARGE: &str = "oracle_read_before_charge";

/// A refused volatile read costs nothing: the frame reverts on the gas it held when the opcode
/// was reached, and an opcode volatile whatever its operands is refused before they are checked
/// (`SATIN-MAC-003`, `SATIN-MAC-004`); the reference charges the static gas, and an account
/// opcode's body charges, first. Checked by running the case again with the reference's order
/// ([`ReferenceRule::RefusalAfterStaticGas`]).
pub const REFUSAL_COSTS_NOTHING: &str = "refusal_costs_nothing";

/// A `keylessDeploy` call is recognized only once its arguments decode; one whose arguments do not
/// runs KeylessDeploy's bytecode and pays no overhead (`SATIN-ICPT-006`); the reference charges
/// the overhead and answers `MalformedEncoding()`. Checked by running the case again with the
/// reference's dispatch ([`ReferenceRule::KeylessSelectorOnly`]).
pub const KEYLESS_DECODED_DISPATCH: &str = "keyless_decoded_dispatch";

/// A `sendHint` call is admitted whether it carries value or not (`SATIN-ORCL-006`), a
/// transaction's own forwarded its first frame's whole gas; the reference does not admit one that
/// carries value. Checked by running the case again with the reference's
/// condition ([`ReferenceRule::HintValueRefused`]).
pub const HINT_WITH_VALUE: &str = "hint_with_value";

/// A Mega System Transaction is held to the block's data-size, KV-update and state-growth budgets
/// (`SATIN-BLK-007`); the reference exempts it. Only a block reaches it; checked by running the
/// block again with the exemption ([`ReferenceRule::MstPackingExempt`]).
pub const MST_PACKING_BUDGETS: &str = "mst_packing_budgets";

/// The oracle service is asked for no read of a deposit, a Mega System Transaction or a system
/// call, which push the slot's value on the chain (`SATIN-ORCL-005`); the reference asks it for a
/// deposit's and a Mega System Transaction's reads. Checked by running the case again with the
/// reference's rule ([`ReferenceRule::OracleAsksDepositsAndMsts`]).
pub const ORACLE_NO_SERVICE: &str = "oracle_no_service";

/// Every block deploys the system contracts before its transactions (`SATIN-SYSC-003`,
/// `SATIN-REG-008`): a missing one is installed, one whose code differs has its code replaced, a
/// SequencerRegistry of version 1.0.0 is migrated in place; the reference deploys none of them. A
/// recorded block of an earlier spec, whose state holds an earlier Oracle or lacks a later
/// contract, reaches it; checked by running the block again without the deployments
/// ([`ReferenceRule::NoPreBlockDeployment`]).
pub const SYSTEM_CONTRACT_DEPLOYMENT: &str = "system_contract_deployment";

/// The live system address is resolved once, by a block after its other pre-block steps and by a
/// single transaction's EVM when it is made, and no transaction reads the registry, or finds its
/// account or slot warm, for its classification (`SATIN-REG-002`); the reference reads both for
/// every transaction, warm for the rest of it. A transaction that then reads the registry, as every
/// Mega System Transaction does through the Oracle's check of its caller, pays their cold access.
/// Checked by running the case or the block again with that read
/// ([`ReferenceRule::RegistryReadPerTransaction`]).
pub const ONCE_PER_BLOCK_RESOLUTION: &str = "once_per_block_resolution";

/// The classes the gate knows the shape of; a `[[class]]` table names one of them.
const CLASSES: [&str; 16] = [
    HALTED_FIRST_FRAME,
    INVALID_IMMEDIATE_LABEL,
    DUP_STACK_LABEL,
    AUTH_PREFLIGHT_READS,
    MODEXP_FORMULA_PRICE,
    SLOTNUM_ZERO,
    RESERVOIR_GIVE_BACK,
    DEPOSIT_HISTORY,
    ORACLE_READ_BEFORE_CHARGE,
    REFUSAL_COSTS_NOTHING,
    KEYLESS_DECODED_DISPATCH,
    HINT_WITH_VALUE,
    MST_PACKING_BUDGETS,
    ORACLE_NO_SERVICE,
    SYSTEM_CONTRACT_DEPLOYMENT,
    ONCE_PER_BLOCK_RESOLUTION,
];

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

/// The candidate's own faults in a scenario's records: a plain or inspected record without exactly
/// the fields the engine renders (`record_fields:Engine:<what>`, a record carrying a regular-gas
/// invariant violation among them, [`crate::invariant`]) and the two records disagreeing
/// (`candidate_inspected:<field>`). Both are also among [`scenario_differences`]; a rerun under a
/// reference's rule never waives them ([`scenario_verdict`]).
pub fn candidate_differences(plain: &Value, inspected: &Value) -> Vec<String> {
    let mut diffs = Vec::new();
    for r in [plain, inspected] {
        let keys = r.as_object().expect("outcome object").keys().map(String::as_str);
        if let Err(e) = record::check_fields(Harness::Engine, keys) {
            diffs.push(format!("record_fields:{:?}:{e}", Harness::Engine));
        }
    }
    diffs.extend(
        same_engine_differences(plain, inspected)
            .into_iter()
            .map(|k| format!("candidate_inspected:{k}")),
    );
    diffs
}

/// Applies the candidate-side fault `MEGA_EVM2_CANDIDATE_FAULT` names to a case's first records,
/// for `faults.sh` (`faults.py`), which shows the gates reject it: `<case>|<plain or
/// inspected>|<field>|<bump or drop>` adds 1 to the numeric field, or removes it, in that record
/// of the case whose key is `<case>` (a scenario's name, a replayed transaction's hash, a block
/// row's `<set>:<block>:<row>`). `gate.sh` refuses to run with the variable set.
pub fn candidate_fault(case: &str, plain: &mut Value, inspected: &mut Value) {
    let Ok(spec) = std::env::var("MEGA_EVM2_CANDIDATE_FAULT") else { return };
    let parts: Vec<&str> = spec.split('|').collect();
    let [target, side, field, action] = parts[..] else {
        panic!("MEGA_EVM2_CANDIDATE_FAULT: <case>|<plain|inspected>|<field>|<bump|drop>: {spec}")
    };
    if target != case {
        return;
    }
    let record = match side {
        "plain" => plain,
        "inspected" => inspected,
        _ => panic!("MEGA_EVM2_CANDIDATE_FAULT: unknown record {side}"),
    };
    let o = record.as_object_mut().expect("outcome object");
    match action {
        "bump" => {
            let v = o.get(field).and_then(Value::as_u64).expect("a numeric field to bump");
            o.insert(field.into(), serde_json::json!(v + 1));
        }
        "drop" => {
            o.remove(field).expect("a field to drop");
        }
        _ => panic!("MEGA_EVM2_CANDIDATE_FAULT: unknown action {action}"),
    }
    println!("CANDIDATE-FAULT {case} {side} {field} {action}");
}

/// The classes checked by running the case again on the engine under the reference's rule, each
/// with that rule. The give-back rule comes last: a case that meets it and another class's change
/// is that other class's ([`rerun_class`]).
pub const RERUN_CLASSES: [(&str, ReferenceRule); 11] = [
    (SLOTNUM_ZERO, ReferenceRule::SlotNumber),
    (AUTH_PREFLIGHT_READS, ReferenceRule::ShortListUnsimulated),
    (MODEXP_FORMULA_PRICE, ReferenceRule::ModexpBoundFirst),
    (DEPOSIT_HISTORY, ReferenceRule::DepositHistoryExempt),
    (ORACLE_READ_BEFORE_CHARGE, ReferenceRule::OracleColdSkip),
    (REFUSAL_COSTS_NOTHING, ReferenceRule::RefusalAfterStaticGas),
    (KEYLESS_DECODED_DISPATCH, ReferenceRule::KeylessSelectorOnly),
    (HINT_WITH_VALUE, ReferenceRule::HintValueRefused),
    (ORACLE_NO_SERVICE, ReferenceRule::OracleAsksDepositsAndMsts),
    (ONCE_PER_BLOCK_RESOLUTION, ReferenceRule::RegistryReadPerTransaction),
    (RESERVOIR_GIVE_BACK, ReferenceRule::Eip8037GiveBack),
];

/// The classes the engine replay gate checks by running a transaction again under the reference's
/// rule, and the block executor gate by running a block again, each with that rule: those recorded
/// blocks reach, and, for blocks, the packing budgets of a Mega System Transaction, which only a
/// block has.
pub const REPLAY_RERUN_CLASSES: [(&str, ReferenceRule); 5] = [
    (DEPOSIT_HISTORY, ReferenceRule::DepositHistoryExempt),
    (ORACLE_NO_SERVICE, ReferenceRule::OracleAsksDepositsAndMsts),
    (MST_PACKING_BUDGETS, ReferenceRule::MstPackingExempt),
    (ONCE_PER_BLOCK_RESOLUTION, ReferenceRule::RegistryReadPerTransaction),
    (RESERVOIR_GIVE_BACK, ReferenceRule::Eip8037GiveBack),
];

/// The classes the block executor gate checks by running a block again: those of
/// [`REPLAY_RERUN_CLASSES`], and the system contracts' deployment, which only a block's pre-block
/// sequence makes.
pub const BLOCK_RERUN_CLASSES: [(&str, ReferenceRule); 6] = [
    (DEPOSIT_HISTORY, ReferenceRule::DepositHistoryExempt),
    (ORACLE_NO_SERVICE, ReferenceRule::OracleAsksDepositsAndMsts),
    (MST_PACKING_BUDGETS, ReferenceRule::MstPackingExempt),
    (ONCE_PER_BLOCK_RESOLUTION, ReferenceRule::RegistryReadPerTransaction),
    (SYSTEM_CONTRACT_DEPLOYMENT, ReferenceRule::NoPreBlockDeployment),
    (RESERVOIR_GIVE_BACK, ReferenceRule::Eip8037GiveBack),
];

/// The class of `rerun` a case is of, among those `classes` enables, `matches(rules)` telling
/// whether the engine's run of the case under the reference's `rules` has no difference from the
/// reference (or only a shape class's, [`classify`]): the first class whose rule alone accounts
/// for the case; otherwise, when the run under every enabled rule does — the case meets several
/// decided changes at once, a deposit that reads the Oracle and gives a slot's charge back — the
/// first class whose rule that run cannot do without (the give-back rule, listed last, only when
/// no other's is needed). A rule a case does not meet changes nothing in its run, so the run under
/// every rule is the run under the ones it meets.
pub fn rerun_class(
    classes: &[String],
    rerun: &[(&'static str, ReferenceRule)],
    mut matches: impl FnMut(&[ReferenceRule]) -> bool,
) -> Option<&'static str> {
    let on: Vec<_> =
        rerun.iter().filter(|(class, _)| classes.iter().any(|c| c == class)).copied().collect();
    for (class, rule) in &on {
        if matches(&[*rule]) {
            return Some(class);
        }
    }
    if on.len() < 2 {
        return None;
    }
    let all: Vec<ReferenceRule> = on.iter().map(|(_, rule)| *rule).collect();
    if !matches(&all) {
        return None;
    }
    for (class, rule) in &on {
        let without: Vec<ReferenceRule> = all.iter().filter(|r| *r != rule).copied().collect();
        if !matches(&without) {
            return Some(class);
        }
    }
    on.first().map(|(class, _)| *class)
}

/// The verdict of the synthetic gate on scenario `name`: its [`scenario_differences`],
/// [`classify`]d against the accepted differences `known`. A case without an entry that no class
/// of [`classify`] takes is of a class of [`RERUN_CLASSES`], when that class is accepted, if
/// `under(rules)` — the case run again on the engine, plain and inspected, under that class's
/// reference rule — has no difference from the reference at all, or only a shape class's. A case
/// that meets several of the decided changes is of one of their classes when the run under every
/// enabled rule has none ([`rerun_class`]; the give-back rule is a setting of the version, the
/// others of `Lite::reference_rules`, so they combine).
/// No rerun is made for a case whose own records have a fault ([`candidate_differences`]): a rerun
/// stands in for the comparison with the reference, never for the candidate's checks.
pub fn scenario_verdict(
    known: &[Known],
    classes: &[String],
    name: &str,
    reference: &Value,
    plain: &Value,
    inspected: &Value,
    under: impl Fn(&[ReferenceRule]) -> (Value, Value),
) -> Verdict {
    let diffs = scenario_differences(reference, plain, inspected);
    let verdict = classify(
        known,
        classes,
        name,
        diffs,
        reference,
        plain,
        (&reference["frames"], &inspected["frames"]),
    );
    let listed = known.iter().any(|k| k.scenario == name);
    if !matches!(verdict, Verdict::Differ(_)) || listed {
        return verdict;
    }
    // A rerun replaces only the comparison with the reference: the candidate's own records must
    // be complete and agree, plain and inspected, before any rerun can accept the case.
    if !candidate_differences(plain, inspected).is_empty() {
        return verdict;
    }
    let class = rerun_class(classes, &RERUN_CLASSES, |rules| {
        let (plain, inspected) = under(rules);
        rerun_matches(classes, name, reference, &plain, &inspected)
    });
    class.map_or(verdict, Verdict::Class)
}

/// Whether the engine's run of scenario `name` under a reference's rules, `plain` and `inspected`,
/// leaves no difference from the reference's record `reference`, or exactly the shape of a class
/// [`classify`] accepts by its shape alone (a halted first frame of evm2 #871, a halt label): the
/// decided change then accounts for everything else.
fn rerun_matches(
    classes: &[String],
    name: &str,
    reference: &Value,
    plain: &Value,
    inspected: &Value,
) -> bool {
    let diffs = scenario_differences(reference, plain, inspected);
    diffs.is_empty() ||
        matches!(
            classify(
                &[],
                classes,
                name,
                diffs,
                reference,
                plain,
                (&reference["frames"], &inspected["frames"]),
            ),
            Verdict::Class(_)
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
