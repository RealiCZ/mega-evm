//! What each tracer must show of every scenario, held against the receipt, the state and the
//! other tracers, so that a missing, extra or misordered event fails by name rather than only as
//! a golden-file difference:
//!
//! - the call tracer's root against the receipt, and its logs, in execution order, against the
//!   receipt's;
//! - the prestate tracer against the state the transaction changed;
//! - the struct logger, the JavaScript tracer and the opcode counter against one another;
//! - the flat call, four-byte, ERC-7562 and Parity traces against the call tracer;
//! - the mux tracer against its tracers run alone;
//! - the transfer and Otterscan inspectors against each other, the access list against the storage
//!   inspector, and the state-gas tracer against the receipt.
//!
//! It also runs every inspector at once, owned by the EVM as a block session holds one, and a
//! block session with an inspector against the same block without one.

use crate::{
    common, golden, hard_frames,
    harness::{
        Asm, Case, JS_TRACER, Tracer, all_traces, assert_same_outcome, outcome,
        precompile_addresses,
    },
    reference_scenarios::{self, CALLER, CONTRACT, GAS, PAYEE, state, tx},
};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, keccak256};
use evm2::{inspector::Inspector, interpreter::op};
use evm2_inspectors::{
    access_list::AccessListInspector,
    otterscan::InternalOperationsInspector,
    tracing::{DebugInspector, TracingInspector, TracingInspectorConfig},
    transfer::TransferInspector,
};
use mega_evm2::MegaTypes;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Every scenario: the reference ones and the hard frames.
fn every_case() -> Vec<Case> {
    let mut v = reference_scenarios::cases();
    v.extend(hard_frames::cases());
    v
}

fn hex_u64(v: &Value) -> u64 {
    match v {
        Value::Number(n) => n.as_u64().unwrap(),
        Value::String(s) => u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap(),
        other => panic!("not a quantity: {other}"),
    }
}

fn lower(a: &Value) -> String {
    a.as_str().unwrap_or_default().to_lowercase()
}

/// The call tracer's frames depth first, each with its path, type, from, to, input, value and
/// error.
fn call_frames(call: &Value) -> Vec<&Value> {
    fn walk<'a>(f: &'a Value, out: &mut Vec<&'a Value>) {
        out.push(f);
        for c in f["calls"].as_array().into_iter().flatten() {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(call, &mut out);
    out
}

/// The call tracer's logs in execution order: a frame's logs and calls interleaved by each log's
/// `position` (the calls the frame made before it).
fn logs_in_order(f: &Value, out: &mut Vec<Value>) {
    let logs = f["logs"].as_array().cloned().unwrap_or_default();
    let calls = f["calls"].as_array().cloned().unwrap_or_default();
    for at in 0..=calls.len() {
        for l in logs.iter().filter(|l| hex_u64(&l["position"]) == at as u64) {
            out.push(l.clone());
        }
        if let Some(c) = calls.get(at) {
            logs_in_order(c, out);
        }
    }
}

fn is_precompile(to: &str) -> bool {
    precompile_addresses().iter().any(|a| format!("{a:#x}") == to)
}

#[test]
fn inspector_call_tracer_root_and_logs_match_the_receipt() {
    for case in every_case() {
        let (call, ran) = Tracer::Call.trace(&case);
        let receipt = &ran.outcome["receipt"];
        let name = case.name;
        assert_eq!(hex_u64(&call["gas"]), case.tx.gas_limit_of(), "{name}: root gas");
        assert_eq!(
            hex_u64(&call["gasUsed"]),
            receipt["gas_used"].as_u64().unwrap(),
            "{name}: root gasUsed"
        );
        assert_eq!(call["error"].is_null(), ran.success(), "{name}: root error");
        let mut logs = Vec::new();
        logs_in_order(&call, &mut logs);
        let got: Vec<_> = logs
            .iter()
            .map(|l| (lower(&l["address"]), l["topics"].clone(), lower(&l["data"])))
            .collect();
        let want: Vec<_> = receipt["logs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| (lower(&l["address"]), l["topics"].clone(), lower(&l["data"])))
            .collect();
        assert_eq!(
            got, want,
            "{name}: the call tracer's logs, in execution order, are the receipt's"
        );
        // A log's index is its index in the receipt: the logs of a failed frame are not counted.
        let indexes: Vec<u64> = logs.iter().map(|l| hex_u64(&l["index"])).collect();
        assert_eq!(indexes, (0..logs.len() as u64).collect::<Vec<_>>(), "{name}: log indexes");
    }
}

#[test]
fn inspector_prestate_names_every_changed_account() {
    for case in every_case() {
        let (diff, ran) = Tracer::PrestateDiff.trace(&case);
        let (pre, _) = Tracer::Prestate.trace(&case);
        let name = case.name;
        // Every account whose balance, nonce, code or storage the transaction changed is in the
        // diff, in its post state unless the transaction destroyed it.
        let mut changed = BTreeSet::new();
        for (address, c) in ran.outcome["state"].as_object().unwrap() {
            let slots_changed = c["slots"].as_array().unwrap().iter().any(|s| s[1] != s[2]);
            let info_changed = c["original"] != c["current"] && !c["current"].is_null() ||
                c["original"].is_null() != c["current"].is_null();
            // An account the transaction created and destroyed was empty before and is gone
            // after: the diff names it nowhere, as geth's does not.
            let created_and_destroyed = c["selfdestructed"] == true && c["created"] == true;
            if (info_changed || slots_changed || c["selfdestructed"] == true) &&
                !created_and_destroyed
            {
                changed.insert(address.to_lowercase());
            }
        }
        let in_diff: BTreeSet<String> = diff["pre"]
            .as_object()
            .unwrap()
            .keys()
            .chain(diff["post"].as_object().unwrap().keys())
            .map(|a| a.to_lowercase())
            .collect();
        assert_eq!(in_diff, changed, "{name}: the diff names exactly the changed accounts");
        // The default mode names every account the transaction loaded that existed before it.
        for (address, c) in ran.outcome["state"].as_object().unwrap() {
            let existed = c["original"].as_array().is_some_and(|o| {
                o[0] != json!("0") ||
                    o[1] != json!(0) ||
                    o[2] != json!(alloy_primitives::KECCAK256_EMPTY.to_string())
            });
            if existed {
                assert!(
                    pre.get(address.to_lowercase()).is_some() || pre.get(address).is_some(),
                    "{name}: the prestate misses {address}"
                );
            }
        }
    }
}

#[test]
fn inspector_struct_log_js_and_opcode_counter_see_the_same_steps() {
    for case in every_case() {
        let name = case.name;
        let (structs, _) = Tracer::StructLog.trace(&case);
        let (full, _) = Tracer::StructLogFull.trace(&case);
        let (js, _) = Tracer::Js.trace(&case);
        let (counts, _) = Tracer::OpcodeGas.trace(&case);
        let steps = structs["structLogs"].as_array().unwrap();
        let full_steps = full["structLogs"].as_array().unwrap();
        assert_eq!(steps.len(), full_steps.len(), "{name}: the full struct logger");
        let js_steps = js["steps"].as_array().unwrap();
        // Every step, a keyless deployment's creation below its dispatcher included
        // (`inspector_keyless_step_traces_show_the_creation`).
        assert_eq!(steps.len(), js_steps.len(), "{name}: the JavaScript tracer's steps");
        // The gas of each frame a creation enters, in order: the JavaScript tracer's `getCost()`
        // of a step that starts a creation counts it (a call step's cost counts the forwarded
        // gas in both).
        let mut created_frame_gas = js["frames"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| {
                f[0] == json!("enter") && matches!(f[1].as_str(), Some("CREATE" | "CREATE2"))
            })
            .map(|f| f[5].as_u64().unwrap());
        for (s, j) in steps.iter().zip(js_steps) {
            assert_eq!(s["depth"], j[1], "{name}: depth");
            assert_eq!(s["pc"], j[2], "{name}: pc");
            assert_eq!(s["gas"], j[3], "{name}: gas");
            // The struct logger's `gasCost` is the JavaScript tracer's `getCost()`: the gas the
            // step took from the frame's gas, without the state and history gas that spilled onto
            // it, as go-ethereum leaves state gas out. The step's state and history gas is
            // `stateGasCost` beside it, whichever pool paid it.
            let (cost, js_cost) = (s["gasCost"].as_u64().unwrap(), j[4].as_u64().unwrap());
            if js_cost == cost {
                continue;
            }
            // Known shape: a step that starts a creation (CREATE, CREATE2, the keyless
            // dispatcher) costs the created frame's gas more under the JavaScript tracer.
            let op_name = s["op"].as_str().unwrap();
            assert!(
                matches!(op_name, "CREATE" | "CREATE2") || op_name.starts_with("opcode "),
                "{name}: step {s}: JavaScript cost {js_cost}"
            );
            assert_eq!(
                Some(js_cost - cost),
                created_frame_gas.next(),
                "{name}: step {s}: the created frame's gas"
            );
        }
        // The opcode counter counts every step of a defined opcode, and charges each opcode but
        // a call or a creation (whose charge it nets of the child) what the struct logs cost it.
        let mut by_op = BTreeMap::<String, (u64, u64)>::new();
        for s in steps {
            let op = s["op"].as_str().unwrap();
            if !op.starts_with("opcode ") {
                let entry = by_op.entry(op.to_string()).or_default();
                entry.0 += 1;
                entry.1 += s["gasCost"].as_u64().unwrap();
            }
        }
        let counted: BTreeMap<String, (u64, u64)> = counts
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c[0].as_str().unwrap().to_string(),
                    (c[1].as_u64().unwrap(), c[2].as_u64().unwrap()),
                )
            })
            .collect();
        assert_eq!(
            by_op.iter().map(|(op, (n, _))| (op, n)).collect::<Vec<_>>(),
            counted.iter().map(|(op, (n, _))| (op, n)).collect::<Vec<_>>(),
            "{name}: the opcode counter counts every step"
        );
        for (op, (_, gas)) in &by_op {
            let calls = ["CALL", "CALLCODE", "DELEGATECALL", "STATICCALL", "CREATE", "CREATE2"];
            if !calls.contains(&op.as_str()) {
                assert_eq!(counted[op].1, *gas, "{name}: the opcode counter's {op} gas");
            }
        }
    }
}

#[test]
fn inspector_flat_call_four_byte_and_erc7562_follow_the_call_tracer() {
    for case in every_case() {
        let name = case.name;
        let (call, _) = Tracer::Call.trace(&case);
        let (flat, _) = Tracer::FlatCall.trace(&case);
        let (four, _) = Tracer::FourByte.trace(&case);
        let (erc, _) = Tracer::Erc7562.trace(&case);
        let frames = call_frames(&call);
        // The flat call tracer: every frame but the calls of precompiles, in the same order.
        let kept: Vec<(String, String)> = frames
            .iter()
            .enumerate()
            .filter(|(i, f)| {
                *i == 0 || f["type"] != json!("SELFDESTRUCT") && !is_precompile(&lower(&f["to"]))
            })
            .map(|(_, f)| f)
            .map(|f| (lower(&f["from"]), lower(&f["to"])))
            .collect();
        let flat_frames: Vec<(String, String)> = flat
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["type"] != json!("suicide"))
            .map(|t| {
                let to = if t["type"] == json!("create") {
                    lower(&t["result"]["address"])
                } else {
                    lower(&t["action"]["to"])
                };
                (lower(&t["action"]["from"]), to)
            })
            .collect();
        assert_eq!(
            kept.len(),
            flat_frames.len(),
            "{name}: flat call frames {flat_frames:?} vs {kept:?}"
        );
        for ((cf, ct), (ff, ft)) in kept.iter().zip(&flat_frames) {
            assert_eq!(cf, ff, "{name}: flat call from");
            if !ft.is_empty() && !ct.is_empty() {
                assert_eq!(ct, ft, "{name}: flat call to");
            }
        }
        // The four-byte tracer: the selector and argument length of every call (not a creation)
        // of a non-precompile with at least four bytes of input.
        let mut want = BTreeMap::<String, u64>::new();
        for f in &frames {
            let input = lower(&f["input"]);
            let is_call = matches!(
                f["type"].as_str(),
                Some("CALL" | "STATICCALL" | "DELEGATECALL" | "CALLCODE")
            );
            if is_call && input.len() >= 10 && !is_precompile(&lower(&f["to"])) {
                let key = format!("{}-{}", &input[..10], (input.len() - 2) / 2 - 4);
                *want.entry(key).or_default() += 1;
            }
        }
        let got: BTreeMap<String, u64> = four
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_u64().unwrap()))
            .collect();
        assert_eq!(got, want, "{name}: four-byte");
        // The ERC-7562 tracer: the call tracer's frames (precompiles and self-destructs aside).
        let erc_frames: Vec<(String, String)> =
            call_frames(&erc).iter().map(|f| (lower(&f["from"]), lower(&f["to"]))).collect();
        let call_frames_: Vec<(String, String)> = frames
            .iter()
            .filter(|f| f["type"] != json!("SELFDESTRUCT"))
            .map(|f| (lower(&f["from"]), lower(&f["to"])))
            .filter(|(_, to)| erc_frames.iter().any(|(_, t)| t == to) || !is_precompile(to))
            .collect();
        assert_eq!(
            erc_frames.len(),
            call_frames_.len(),
            "{name}: ERC-7562 frames {erc_frames:?} vs {call_frames_:?}"
        );
    }
}

#[test]
fn inspector_mux_equals_its_tracers_run_alone() {
    for case in every_case() {
        let name = case.name;
        let (mux, _) = Tracer::Mux.trace(&case);
        for (key, alone) in [
            ("callTracer", Tracer::Call),
            ("prestateTracer", Tracer::PrestateDiff),
            ("flatCallTracer", Tracer::FlatCall),
            ("4byteTracer", Tracer::FourByte),
            (JS_TRACER, Tracer::Js),
        ] {
            let (single, _) = alone.trace(&case);
            assert_eq!(
                mux[key],
                single,
                "{name}: the mux's {} differs from the tracer alone",
                alone.name()
            );
        }
    }
}

#[test]
fn inspector_noop_and_state_gas_tracers() {
    for case in every_case() {
        let name = case.name;
        let (noop, _) = Tracer::Noop.trace(&case);
        assert_eq!(noop, json!({}), "{name}: the no-op tracer");
        let (gas, ran) = Tracer::StateGas.trace(&case);
        assert_eq!(
            hex_u64(&gas["gasUsed"]),
            ran.outcome["receipt"]["gas_used"].as_u64().unwrap(),
            "{name}"
        );
        assert_eq!(
            hex_u64(&gas["stateGasUsed"]),
            ran.outcome["gas"]["state_gas_spent"].as_u64().unwrap(),
            "{name}: Satin's state and history gas are the state-gas tracer's"
        );
    }
}

#[test]
fn inspector_js_sees_every_frame_below_the_root() {
    for case in every_case() {
        let name = case.name;
        let (js, ran) = Tracer::Js.trace(&case);
        let (call, _) = Tracer::Call.trace(&case);
        let entered =
            js["frames"].as_array().unwrap().iter().filter(|f| f[0] == json!("enter")).count();
        let exited =
            js["frames"].as_array().unwrap().iter().filter(|f| f[0] == json!("exit")).count();
        assert_eq!(entered, exited, "{name}: every frame entered is exited");
        let below_root = call_frames(&call).len() - 1;
        assert_eq!(
            entered, below_root,
            "{name}: the JavaScript tracer enters every frame below the root"
        );
        assert_eq!(
            js["gasUsed"].as_u64().unwrap(),
            ran.outcome["receipt"]["gas_used"].as_u64().unwrap(),
            "{name}: ctx.gasUsed"
        );
        assert_eq!(
            js["error"].is_null() || js["error"] == json!(""),
            ran.success(),
            "{name}: ctx.error"
        );
    }
}

#[test]
fn inspector_parity_trace_state_diff_and_vm_trace() {
    for case in every_case() {
        let name = case.name;
        let (parity, _) = Tracer::Parity.trace(&case);
        let (call, _) = Tracer::Call.trace(&case);
        let (structs, _) = Tracer::StructLog.trace(&case);
        // The trace: every frame of the call tracer but the calls of precompiles without value,
        // in the same order.
        let frames = call_frames(&call);
        let kept = frames
            .iter()
            .enumerate()
            .filter(|(i, f)| {
                *i == 0 ||
                    f["type"] == json!("SELFDESTRUCT") ||
                    !is_precompile(&lower(&f["to"])) ||
                    f["value"].as_str().is_some_and(|v| v != "0x0")
            })
            .count();
        assert_eq!(parity["trace"].as_array().unwrap().len(), kept, "{name}: parity traces");
        // The state diff: every account the transaction changed.
        let diffed: BTreeSet<String> =
            parity["stateDiff"].as_object().unwrap().keys().map(|a| a.to_lowercase()).collect();
        let (prestate, _) = Tracer::PrestateDiff.trace(&case);
        let in_prestate: BTreeSet<String> = prestate["pre"]
            .as_object()
            .unwrap()
            .keys()
            .chain(prestate["post"].as_object().unwrap().keys())
            .map(|a| a.to_lowercase())
            .collect();
        assert!(
            in_prestate.is_subset(&diffed),
            "{name}: the state diff {diffed:?} misses one of {in_prestate:?}"
        );
        // The VM trace: one op per struct-log step, in execution order with each sub after the op
        // that started it, costing what the struct logs cost it; a keyless deployment's creation
        // is its dispatcher's sub.
        fn costs(vm: &Value, out: &mut Vec<u64>) {
            for o in vm["ops"].as_array().into_iter().flatten() {
                out.push(o["cost"].as_u64().unwrap());
                if !o["sub"].is_null() {
                    costs(&o["sub"], out);
                }
            }
        }
        let mut vm_costs = Vec::new();
        costs(&parity["vmTrace"], &mut vm_costs);
        // A step that runs an invalid opcode is not in a Parity VM trace (OpenEthereum records an
        // instruction as it prepares to execute it, which an invalid one never is).
        let struct_costs: Vec<u64> = structs["structLogs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| s["op"] != json!("INVALID"))
            .map(|s| s["gasCost"].as_u64().unwrap())
            .collect();
        assert_eq!(vm_costs, struct_costs, "{name}: VM trace ops and their costs");
    }
}

#[test]
fn inspector_transfer_and_otterscan_list_the_same_value_moves() {
    for case in every_case() {
        let name = case.name;
        let (transfers, _) = Tracer::Transfer.trace(&case);
        let (ots, _) = Tracer::Otterscan.trace(&case);
        let kind = |k: &str| match k {
            "Call" => 0,
            "SelfDestruct" => 1,
            "Create" => 2,
            "Create2" => 3,
            other => panic!("{other}"),
        };
        // The transfer inspector lists the transaction's own transfer first, when it moves value;
        // the Otterscan inspector lists internal operations only.
        let moved: Vec<(u64, String, String, U256)> = transfers["transfers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                (
                    kind(t["kind"].as_str().unwrap()),
                    lower(&t["from"]),
                    lower(&t["to"]),
                    t["value"].as_str().unwrap().parse().unwrap(),
                )
            })
            .collect();
        let internal: Vec<(u64, String, String, U256)> = ots
            .as_array()
            .unwrap()
            .iter()
            .map(|o| {
                (
                    o["type"].as_u64().unwrap(),
                    lower(&o["from"]),
                    lower(&o["to"]),
                    U256::from_str_radix(o["value"].as_str().unwrap().trim_start_matches("0x"), 16)
                        .unwrap(),
                )
            })
            .collect();
        let top_level_moves = case.tx.value_of() != U256::ZERO;
        let below: Vec<_> = moved.iter().skip(usize::from(top_level_moves)).cloned().collect();
        // The transfer inspector drops the transfers of a frame that failed, as reth's
        // `ots_getInternalOperations` reports them; the Otterscan inspector keeps every internal
        // operation, as Erigon's does. What the first lists, the second lists in the same order.
        let mut rest = internal.iter();
        for m in &below {
            assert!(
                rest.any(|o| o == m),
                "{name}: {m:?} is not among the internal operations {internal:?}"
            );
        }
    }
}

#[test]
fn inspector_access_list_names_every_storage_access() {
    for case in every_case() {
        let name = case.name;
        let (list, _) = Tracer::AccessList.trace(&case);
        let (storage, _) = Tracer::Storage.trace(&case);
        let excluded: BTreeSet<String> = precompile_addresses()
            .iter()
            .map(|a| format!("{a:#x}"))
            .chain([format!("{:#x}", case.sender())])
            .chain(case.tx.to().map(|a| format!("{a:#x}")))
            .collect();
        let listed: BTreeSet<(String, String)> = list
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|e| {
                let a = lower(&e["address"]);
                e["storageKeys"].as_array().unwrap().iter().map(move |k| (a.clone(), lower(k)))
            })
            .collect();
        for s in storage["slots"].as_array().unwrap() {
            let address = lower(&s[0]);
            if excluded.contains(&address) {
                continue;
            }
            let slot = format!("{:#066x}", s[1].as_str().unwrap().parse::<U256>().unwrap());
            assert!(
                listed.contains(&(address.clone(), slot.clone())),
                "{name}: the access list misses {address} {slot}"
            );
        }
    }
}

/// A transaction that hashes memory twice (two preimages) and writes and reads storage.
fn keccak_case() -> Case {
    let code_ = Asm::default()
        .mstore(0, b"satin under every tracer")
        .push_u8(24)
        .op(op::PUSH0)
        .op(op::KECCAK256)
        .op(op::POP)
        .push_u8(32)
        .op(op::PUSH0)
        .op(op::KECCAK256)
        .op(op::POP)
        .sstore(U256::from(1), U256::from(7))
        .push_u8(1)
        .op(op::SLOAD)
        .op(op::POP)
        .stop()
        .build();
    Case::new(
        "keccak_and_storage",
        state([(CONTRACT, crate::harness::code(&code_))]),
        tx(CONTRACT, 0, Bytes::new(), GAS),
    )
}

#[test]
fn inspector_keccak_preimage_and_storage_tracers() {
    let case = keccak_case();
    let traces = crate::harness::pin(&case);
    let preimages = traces["keccak_preimage"].as_object().unwrap();
    assert_eq!(preimages.len(), 2, "two preimages");
    for (hash, preimage) in preimages {
        let bytes = alloy_primitives::hex::decode(preimage.as_str().unwrap()).unwrap();
        assert_eq!(hash.to_lowercase(), format!("{:#x}", keccak256(bytes)));
    }
    let slots = traces["storage"]["slots"].as_array().unwrap();
    assert_eq!(slots.len(), 1, "one slot, written then read: {slots:?}");
}

/// Every inspector the crate provides at once, as one inspector.
type Everything = (
    (DebugInspector, DebugInspector),
    ((TracingInspector, TransferInspector), (AccessListInspector, InternalOperationsInspector)),
);

fn everything() -> Everything {
    let options = |t: Tracer| t.geth_options().unwrap();
    (
        (
            DebugInspector::new(options(Tracer::StructLogFull)).unwrap(),
            DebugInspector::new(options(Tracer::Mux)).unwrap(),
        ),
        (
            (
                TracingInspector::new(TracingInspectorConfig::all()),
                TransferInspector::new(false).with_logs(true),
            ),
            (AccessListInspector::default(), InternalOperationsInspector::default()),
        ),
    )
}

#[test]
fn inspector_plain_every_inspector_at_once_owned_by_the_evm() {
    for case in every_case() {
        let plain = case.plain();
        // Owned by the EVM, as a block session holds its inspector, rather than borrowed for one
        // transaction.
        let mut evm = case.evm();
        evm.set_inspector(everything());
        let executed = evm.transact(&case.tx).map(evm2::evm::ExecutedTx::detach);
        let result = executed.unwrap_or_else(|e| panic!("{}: refused {e:?}", case.name));
        let inspected = outcome(&result, evm.ext());
        assert_same_outcome(
            &plain.outcome,
            &inspected,
            &format!("{} under every inspector", case.name),
        );
        let mut inspector = evm.clear_inspector_as::<Everything>().expect("the inspector");
        // The struct logger and the mux built from the shared run are the ones built alone.
        let (struct_log, _) = Tracer::StructLogFull.trace(&case);
        let built = inspector
            .0
            .0
            .get_result(Some(case.context()), &case.tx, &case.block, &result, &mut case.pre_db())
            .unwrap();
        assert_eq!(
            serde_json::to_value(built).unwrap(),
            struct_log,
            "{}: struct logs under every inspector",
            case.name
        );
    }
}

#[test]
fn inspector_plain_every_tracer_on_every_hard_frame() {
    // `Tracer::trace` holds each traced run to the plain one; this names the property for the
    // hard frames alone, every tracer and inspector of the crate.
    for case in hard_frames::cases() {
        for tracer in Tracer::ALL {
            let (_, ran) = tracer.trace(&case);
            assert_same_outcome(
                &case.plain().outcome,
                &ran.outcome,
                &format!("{} under {}", case.name, tracer.name()),
            );
        }
    }
}

#[test]
fn inspector_plain_block_session_with_an_inspector_observing_its_pre_block_calls() {
    use mega_evm2::factory::BlockServices;
    use std::{cell::RefCell, rc::Rc};
    /// An inspector the test keeps a handle on while the session owns it.
    struct Shared(Rc<RefCell<DebugInspector>>);
    impl Inspector<MegaTypes> for Shared {
        fn inspector_config(&self) -> evm2::InspectorConfig {
            Inspector::<MegaTypes>::inspector_config(&*self.0.borrow())
        }
        fn initialize_interp(&mut self, i: &mut evm2::interpreter::Interpreter<'_, '_, MegaTypes>) {
            Inspector::<MegaTypes>::initialize_interp(&mut *self.0.borrow_mut(), i);
        }
        fn step(&mut self, i: &mut evm2::interpreter::Interpreter<'_, '_, MegaTypes>) {
            Inspector::<MegaTypes>::step(&mut *self.0.borrow_mut(), i);
        }
        fn step_end(&mut self, i: &mut evm2::interpreter::Interpreter<'_, '_, MegaTypes>) {
            Inspector::<MegaTypes>::step_end(&mut *self.0.borrow_mut(), i);
        }
        fn log(&mut self, log: &alloy_primitives::Log, host: &mut evm2::Evm<'_, MegaTypes>) {
            Inspector::<MegaTypes>::log(&mut *self.0.borrow_mut(), log, host);
        }
        fn call(
            &mut self,
            i: &mut evm2::interpreter::Interpreter<'_, '_, MegaTypes>,
            m: &mut evm2::interpreter::Message<MegaTypes>,
        ) -> Option<evm2::interpreter::MessageResult<MegaTypes>> {
            Inspector::<MegaTypes>::call(&mut *self.0.borrow_mut(), i, m)
        }
        fn call_end(
            &mut self,
            i: &mut evm2::interpreter::Interpreter<'_, '_, MegaTypes>,
            m: &evm2::interpreter::Message<MegaTypes>,
            r: &mut evm2::interpreter::MessageResult<MegaTypes>,
        ) {
            Inspector::<MegaTypes>::call_end(&mut *self.0.borrow_mut(), i, m, r);
        }
        fn create(
            &mut self,
            i: &mut evm2::interpreter::Interpreter<'_, '_, MegaTypes>,
            m: &mut evm2::interpreter::Message<MegaTypes>,
        ) -> Option<evm2::interpreter::MessageResult<MegaTypes>> {
            Inspector::<MegaTypes>::create(&mut *self.0.borrow_mut(), i, m)
        }
        fn create_end(
            &mut self,
            i: &mut evm2::interpreter::Interpreter<'_, '_, MegaTypes>,
            m: &evm2::interpreter::Message<MegaTypes>,
            r: &mut evm2::interpreter::MessageResult<MegaTypes>,
        ) {
            Inspector::<MegaTypes>::create_end(&mut *self.0.borrow_mut(), i, m, r);
        }
        fn selfdestruct(
            &mut self,
            c: &Address,
            t: &Address,
            v: &U256,
            host: &mut evm2::Evm<'_, MegaTypes>,
        ) {
            Inspector::<MegaTypes>::selfdestruct(&mut *self.0.borrow_mut(), c, t, v, host);
        }
    }

    // A block of three transactions from one funded sender: a value transfer to a new account,
    // the call whose callee logs and reverts (`nested_inner_revert`) and a creation. The block's
    // pre-block sequence deploys the system contracts and seeds the registry.
    let db = reference_scenarios::cases()
        .into_iter()
        .find(|c| c.name == "nested_inner_revert")
        .expect("a reference scenario")
        .db;
    let txs = [
        crate::harness::legacy(CALLER, TxKind::Call(PAYEE), U256::from(1), Bytes::new(), GAS, 0),
        crate::harness::legacy(CALLER, TxKind::Call(CONTRACT), U256::ZERO, Bytes::new(), GAS, 1),
        crate::harness::legacy(
            CALLER,
            TxKind::Create,
            U256::ZERO,
            reference_scenarios::deploying_runtime(),
            GAS,
            2,
        ),
    ];
    let input = common::block_input(1, 1_000, 100_000_000, Some(B256::ZERO));
    // Runs the block, with `inspector` observing it when given; returns what the block did and,
    // under the inspector, the frames it saw before the first transaction (the pre-block calls)
    // and each transaction's struct logs, built and the inspector fused after each, as a node
    // traces a block.
    let run = |inspector: Option<Rc<RefCell<DebugInspector>>>| {
        let services = common::services(B256::ZERO);
        let services = BlockServices {
            inspector: inspector
                .clone()
                .map(|i| Box::new(Shared(i)) as Box<dyn Inspector<MegaTypes>>),
            ..services
        };
        let mut session = common::factory()
            .start_block(&input, evm2::evm::Db::new(db.clone()), services)
            .expect("the block starts");
        let mut pre_block_frames = 0;
        if let Some(i) = &inspector {
            if let DebugInspector::Default(tracing, _) = &*i.borrow() {
                pre_block_frames = tracing.traces().nodes().len();
            }
            i.borrow_mut().fuse().unwrap();
        }
        let mut rows = Vec::new();
        let mut traces = Vec::new();
        for (i, tx) in txs.iter().enumerate() {
            let raw = tx.enveloped_tx().cloned().unwrap_or_default();
            let out = session.execute_candidate(tx, &raw).expect("a valid candidate");
            rows.push(json!({
                "status": out.result().status,
                "gas": out.gas(),
                "usage": out.usage(),
                "gas_used": out.gas_used(),
                "output": format!("{}", out.result().output),
            }));
            if let Some(inspector) = &inspector {
                let result = evm2::TxResultWithState::<MegaTypes> {
                    result: out.result().clone(),
                    pending_state: out.pending().clone(),
                    ..Default::default()
                };
                let block = *session.evm().block();
                let mut pre = evm2::evm::Db::new(db.clone());
                let trace = inspector
                    .borrow_mut()
                    .get_result(None, tx, &block, &result, &mut pre)
                    .expect("the trace builds");
                traces.push(serde_json::to_value(trace).unwrap());
                inspector.borrow_mut().fuse().unwrap();
            }
            session.commit_candidate(out).expect("commits");
            rows.push(json!(format!("{:?}", session.receipts()[i])));
        }
        let mut accounts: Vec<String> =
            session.changes().accounts().map(|(a, t)| format!("{a} {t:?}")).collect();
        accounts.sort();
        let mut storage: Vec<String> = session
            .changes()
            .storage()
            .map(|(k, t)| format!("{} {:#x} {t:?}", k.address(), k.key()))
            .collect();
        storage.sort();
        (json!({"rows": rows, "accounts": accounts, "storage": storage}), pre_block_frames, traces)
    };
    let (plain, _, _) = run(None);
    let shared = Rc::new(RefCell::new(
        DebugInspector::new(Tracer::StructLog.geth_options().unwrap()).unwrap(),
    ));
    let (inspected, pre_block_frames, traces) = run(Some(shared));
    assert_eq!(plain, inspected, "the block under an inspector");
    // The session's inspector observed the pre-block system calls (the EIP-2935 and EIP-4788
    // calls, the registry's) before the first transaction.
    assert!(pre_block_frames > 0, "the pre-block calls are observed");
    // Each transaction's struct logs in the block are those of the transaction traced alone.
    let alone = reference_scenarios::cases()
        .into_iter()
        .find(|c| c.name == "nested_inner_revert")
        .map(|c| Tracer::StructLog.trace(&c).0)
        .unwrap();
    assert_eq!(traces[1]["structLogs"], alone["structLogs"], "the block's second transaction");
    assert_eq!(traces.len(), txs.len());
}

/// The keyless dispatcher's step and the creation below it: the frame of a native keyless
/// deployment runs one instruction, the engine's dispatcher (an opcode no other code can run),
/// which starts the creation. The step tracers show that instruction as an undefined opcode, and
/// the builders enter the child right after it, as after a call or create opcode: the struct logs
/// show the creation's steps below the dispatcher, as the JavaScript tracer does, and the Parity VM
/// trace gives the dispatcher the creation as its sub. A refused deployment starts no creation, and
/// its dispatcher has none.
#[test]
fn inspector_keyless_step_traces_show_the_creation() {
    for (scenario, creates) in
        [("keyless_success", true), ("keyless_creation_reverts", true), ("keyless_refused", false)]
    {
        let case = every_case().into_iter().find(|c| c.name == scenario).unwrap();
        let traces = all_traces(&case);
        let structs = traces["struct_log"]["structLogs"].as_array().unwrap().clone();
        assert_eq!(structs[0]["op"], json!("opcode 0xc not defined"), "{scenario}");
        assert_eq!(structs[0]["depth"], json!(1), "{scenario}");
        // The inspector recorded the dispatcher, then the creation's steps in a child frame.
        let mut inspector = TracingInspector::new(TracingInspectorConfig::all());
        let _ = case.traced(&mut inspector, "a tracing inspector");
        let nodes = inspector.traces().nodes();
        assert_eq!(nodes[0].trace.steps.len(), 1, "{scenario}: the dispatcher");
        assert_eq!(nodes.len(), 1 + usize::from(creates), "{scenario}");
        let created = nodes.get(1).map_or(0, |node| node.trace.steps.len());
        assert_eq!(created > 0, creates, "{scenario}: the creation's steps");
        // The struct logs: the dispatcher, then every step of the creation at depth 2.
        assert_eq!(structs.len(), 1 + created, "{scenario}: {structs:?}");
        assert!(structs[1..].iter().all(|s| s["depth"] == json!(2)), "{scenario}");
        let js_steps = traces["js"]["steps"].as_array().unwrap();
        let depths =
            |steps: &[Value], at: usize| steps.iter().map(|s| s[at].clone()).collect::<Vec<_>>();
        let struct_depths: Vec<Value> = structs.iter().map(|s| s["depth"].clone()).collect();
        assert_eq!(struct_depths, depths(js_steps, 1), "{scenario}: the JavaScript tracer's steps");
        // The VM trace: the dispatcher's one op, with the creation as its sub.
        let vm = &traces["parity"]["vmTrace"]["ops"];
        assert_eq!(vm.as_array().unwrap().len(), 1, "{scenario}");
        assert_eq!(vm[0]["op"], json!("UNKNOWN(0x0C)"), "{scenario}");
        assert_eq!(vm[0]["cost"], structs[0]["gasCost"], "{scenario}");
        let sub_ops = vm[0]["sub"]["ops"].as_array().map(Vec::len);
        assert_eq!(sub_ops, creates.then_some(created), "{scenario}: the creation's VM trace");
        if creates {
            assert_eq!(traces["js"]["frames"][0][1], json!("CREATE"), "{scenario}");
        }
    }
}

/// A storage write whose state gas spills onto the frame's gas costs the same in every step
/// tracer, the struct logs, the Parity VM trace, the JavaScript tracer's `getCost()` and the
/// opcode gas inspector: its 12,100, state gas left out as go-ethereum leaves it, with the state
/// and history gas beside it as `stateGasCost`. The spill shows as the gas the next step starts
/// with. Below the cap the reservoir is empty and a fresh slot's state and history gas (97,920 for
/// the slot, 3,520 for its write record) spill in full; above it the reservoir pays all of it but
/// the 10,000 legacy share, which the frame's gas always pays.
#[test]
fn inspector_spilled_sstore_costs_the_same_in_every_step_tracer() {
    let cost = 12_100;
    for (scenario, spilled, reservoir) in
        [("sstore_new_slot", 97_920 + 3_520, 0), ("reservoir_sstore", 10_000, 1_972_720)]
    {
        let case = every_case().into_iter().find(|c| c.name == scenario).unwrap();
        let traces = all_traces(&case);
        let structs = traces["struct_log"]["structLogs"].as_array().unwrap();
        let at = structs.iter().position(|s| s["op"] == json!("SSTORE")).unwrap();
        assert_eq!(structs.iter().filter(|s| s["op"] == json!("SSTORE")).count(), 1, "{scenario}");
        let (sstore, next) = (&structs[at], &structs[at + 1]);
        assert_eq!(sstore["gasCost"], json!(cost), "{scenario}: struct log");
        assert_eq!(sstore["stateGasCost"], json!(101_440), "{scenario}: state gas beside it");
        assert_eq!(sstore["stateGasReservoir"], json!(reservoir), "{scenario}: the reservoir");
        let gas = sstore["gas"].as_u64().unwrap();
        assert_eq!(next["gas"], json!(gas - cost - spilled), "{scenario}: the spill");
        let vm: Vec<&Value> = traces["parity"]["vmTrace"]["ops"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["op"] == json!("SSTORE"))
            .collect();
        assert_eq!(vm.len(), 1, "{scenario}");
        assert_eq!(vm[0]["cost"], json!(cost), "{scenario}: VM trace");
        assert_eq!(vm[0]["ex"]["used"], json!(gas - cost - spilled), "{scenario}: VM trace used");
        let js = traces["js"]["steps"].as_array().unwrap();
        assert_eq!(
            (&js[at][0], &js[at][4]),
            (&json!("SSTORE"), &json!(cost)),
            "{scenario}: getCost"
        );
        assert_eq!(js[at + 1][3], json!(gas - cost - spilled), "{scenario}: JavaScript getGas");
        let counted: Vec<&Value> = traces["opcode_gas"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c[0] == json!("SSTORE"))
            .collect();
        assert_eq!(counted.len(), 1, "{scenario}");
        assert_eq!(
            (&counted[0][1], &counted[0][2]),
            (&json!(1), &json!(cost)),
            "{scenario}: opcode gas"
        );
    }
}

/// Gas detention under the step tracers: the withheld gas leaves the frame's gas at the volatile
/// read, so the step that reads shows it as its cost, every later step shows only what detention
/// leaves, and the step that crosses reads as an out-of-gas that cost what was left. The receipt
/// bills none of the withheld gas.
#[test]
fn inspector_detention_step_costs() {
    let case =
        reference_scenarios::cases().into_iter().find(|c| c.name == "timestamp_detention").unwrap();
    let (structs, ran) = Tracer::StructLog.trace(&case);
    let steps = structs["structLogs"].as_array().unwrap();
    let read = &steps[0];
    assert_eq!(read["op"], json!("TIMESTAMP"));
    let cap = 80;
    assert_eq!(steps[1]["gas"], json!(cap), "after the read, the frame shows the cap");
    assert_eq!(
        read["gasCost"].as_u64().unwrap(),
        read["gas"].as_u64().unwrap() - cap,
        "the read's cost is the withheld gas plus its own"
    );
    let last = steps.last().unwrap();
    assert_eq!(last["error"], json!("Some(OutOfGas)"));
    assert_eq!(last["gasCost"], last["gas"], "the crossing costs what detention left");
    let billed = ran.outcome["receipt"]["gas_used"].as_u64().unwrap();
    assert!(
        read["gasCost"].as_u64().unwrap() > billed,
        "the receipt bills none of the withheld gas"
    );
}

#[test]
fn inspector_golden_files_are_all_referenced() {
    let mut names: BTreeSet<String> = every_case().iter().map(|c| c.name.to_string()).collect();
    names.insert(keccak_case().name.to_string());
    let dir = golden::path("x").parent().unwrap().to_path_buf();
    let files: BTreeSet<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().trim_end_matches(".json").to_string())
        .collect();
    assert_eq!(files, names, "every golden file belongs to a scenario, and every scenario has one");
}

trait TxFields {
    fn gas_limit_of(&self) -> u64;
    fn value_of(&self) -> U256;
    fn to(&self) -> Option<Address>;
}

impl TxFields for alloy_consensus::transaction::Recovered<op_evm2::OpTxEnvelope> {
    fn gas_limit_of(&self) -> u64 {
        alloy_consensus::Transaction::gas_limit(&**self)
    }
    fn value_of(&self) -> U256 {
        alloy_consensus::Transaction::value(&**self)
    }
    fn to(&self) -> Option<Address> {
        alloy_consensus::Transaction::to(&**self)
    }
}
