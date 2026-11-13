//! The frames that are hard to trace under Satin, each pinned under every tracer and each traced
//! run held to the plain one:
//!
//! - a native keyless deployment, whose creation is a child of the outer transaction's call;
//! - system-contract calls the engine answers before they run (`MegaLimitControl`,
//!   `MegaAccessControl`, the Oracle's hints) and their value refusals;
//! - a first frame that runs out of gas in its runtime gas phase, before it runs an instruction;
//! - failed deposits;
//! - limit stops a parent catches, of each kind: data size (in `reference_scenarios.rs`), KV
//!   updates, state growth, and compute under gas detention;
//! - logs of frames that revert;
//! - EIP-7702 delegation;
//! - precompiles, one of them priced against gas detention;
//! - `SELFDESTRUCT`;
//! - a detention stop, and the reservoir's give-back of a failed frame's state gas.

use crate::{
    common,
    harness::{Asm, Case, NO_LIMITS, Traces, code, create, deposit, eip7702, pin},
    reference_scenarios::{
        CALLER, CHILD, CONTRACT, EXECUTION_CAP, GAS, PAYEE, deploying_runtime, frames,
        keyless_call, signed_deployment, state, steps, system_state, tx,
    },
};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, address, keccak256};
use evm2::{evm::AccountInfo, interpreter::op};
use mega_evm2::{TxLimits, ledger::limit_exceeded_output};
use serde_json::{Value, json};

pub const AUTHORITY: Address = address!("0x0000000000000000000000000000000000400004");
pub const HEIR: Address = address!("0x0000000000000000000000000000000000400005");
pub const ORACLE: Address = address!("0x6342000000000000000000000000000000000001");
pub const ACCESS_CONTROL: Address = address!("0x6342000000000000000000000000000000000004");

fn selector(signature: &str) -> Vec<u8> {
    keccak256(signature)[..4].to_vec()
}

/// No limits but `f`'s.
fn limits(f: impl FnOnce(&mut TxLimits)) -> TxLimits {
    let mut l = NO_LIMITS;
    f(&mut l);
    l
}

/// A child that writes `n` fresh slots (1..=n) and stops.
fn writing(n: u64) -> Bytes {
    let mut a = Asm::default();
    for slot in 1..=n {
        a = a.sstore(U256::from(slot), U256::from(1));
    }
    a.stop().build()
}

/// A parent that calls [`CHILD`] with all its gas, drops the status and stops.
fn calling_child_then_stop() -> Bytes {
    Asm::default().call_plain(op::CALL, CHILD, 0).op(op::POP).stop().build()
}

/// `sendHint(bytes32 topic, bytes data)`'s input.
fn send_hint(topic: B256, data: &[u8]) -> Vec<u8> {
    let mut input = selector("sendHint(bytes32,bytes)");
    input.extend_from_slice(topic.as_slice());
    input.extend_from_slice(&U256::from(0x40).to_be_bytes::<32>());
    input.extend_from_slice(&U256::from(data.len()).to_be_bytes::<32>());
    let mut padded = data.to_vec();
    padded.resize(data.len().div_ceil(32) * 32, 0);
    input.extend_from_slice(&padded);
    input
}

/// A contract account holding `code` and `wei`.
fn funded_code(code_: &[u8], wei: u64) -> AccountInfo {
    let mut info = code(code_);
    info.balance = U256::from(wei);
    info
}

/// Every hard frame, by name.
pub fn cases() -> Vec<Case> {
    let mut v = Vec::new();

    // A keyless deployment whose creation reverts: the creation is the call's child, and fails.
    let init_reverts = Bytes::from_static(&[
        0x63, 0xde, 0xad, 0xbe, 0xef, 0x60, 0x00, 0x52, 0x60, 0x04, 0x60, 0x1c, 0xfd,
    ]);
    let (raw, _) = signed_deployment(0, init_reverts);
    v.push(Case::new(
        "keyless_creation_reverts",
        system_state(),
        tx(common::KEYLESS, 0, keyless_call(&raw), GAS),
    ));
    // A keyless call carrying value: answered before evm2 builds its frame (`NoEtherTransfer()`).
    let (raw, _) = signed_deployment(0, deploying_runtime());
    v.push(Case::new(
        "keyless_with_value",
        system_state(),
        tx(common::KEYLESS, 1, keyless_call(&raw), GAS),
    ));
    // A keyless call from a contract: only a depth-0 call is dispatched, so this one runs the
    // contract's bytecode.
    let calldata = keyless_call(&raw);
    let caller = Asm::default()
        .call_input(op::CALL, common::KEYLESS, 0, &calldata)
        .op(op::POP)
        .stop()
        .build();
    let mut db = system_state();
    db.insert_account_info(&CONTRACT, code(&caller));
    v.push(Case::new("keyless_from_a_contract", db, tx(CONTRACT, 0, Bytes::new(), GAS)));

    // `MegaLimitControl.remainingComputeGas()`, by STATICCALL and by CALL, under detention's cap:
    // both answered before they run. Then a CALL of it carrying value, refused before the value
    // moves (`NonZeroTransfer()`).
    let remaining = selector("remainingComputeGas()");
    let code_ = Asm::default()
        .ops([op::NUMBER, op::POP])
        .call_input(op::STATICCALL, common::LIMIT_CONTROL, 0, &remaining)
        .op(op::POP)
        .call_input(op::CALL, common::LIMIT_CONTROL, 0, &remaining)
        .op(op::POP)
        .call_input(op::CALL, common::LIMIT_CONTROL, 1, &remaining)
        .op(op::POP)
        .stop()
        .build();
    let mut db = system_state();
    db.insert_account_info(&CONTRACT, funded_code(&code_, 10));
    v.push(
        Case::new("intercept_limit_control", db, tx(CONTRACT, 0, Bytes::new(), GAS))
            .limits(limits(|l| l.cap = 1_000_000)),
    );
    // `MegaAccessControl`: a child switches volatile access off, then reads `TIMESTAMP`, which is
    // refused (the child reverts); the parent asks whether access is off and stops.
    let disable = selector("disableVolatileDataAccess()");
    let child = Asm::default()
        .call_input(op::CALL, ACCESS_CONTROL, 0, &disable)
        .op(op::POP)
        .ops([op::TIMESTAMP, op::POP, op::STOP])
        .build();
    let parent = Asm::default()
        .call_plain(op::CALL, CHILD, 0)
        .op(op::POP)
        .call_input(op::STATICCALL, ACCESS_CONTROL, 0, &selector("isVolatileDataAccessDisabled()"))
        .op(op::POP)
        .stop()
        .build();
    let mut db = system_state();
    db.insert_account_info(&CONTRACT, code(&parent));
    db.insert_account_info(&CHILD, code(&child));
    v.push(Case::new("intercept_access_control", db, tx(CONTRACT, 0, Bytes::new(), GAS)));
    // The Oracle's `sendHint`: admitted and handed to the service before the Oracle's bytecode
    // runs.
    let hint = send_hint(B256::repeat_byte(0x11), b"satin");
    let code_ = Asm::default().call_input(op::CALL, ORACLE, 0, &hint).op(op::POP).stop().build();
    let mut db = system_state();
    db.insert_account_info(&CONTRACT, code(&code_));
    v.push(
        Case::new("intercept_oracle_hint", db, tx(CONTRACT, 0, Bytes::new(), GAS))
            .oracle(Vec::new()),
    );

    // A first frame whose runtime gas phase cannot pay: a value call to a new account and a
    // creation, each with gas for its intrinsic gas and body only.
    v.push(Case::new(
        "pre_frame_value_to_a_new_account",
        state([]),
        tx(PAYEE, 1, Bytes::new(), 60_000),
    ));
    v.push(Case::new("pre_frame_creation", state([]), create(CALLER, deploying_runtime(), 80_000)));

    // Failed deposits: one whose execution halts, one whose first frame cannot pay its
    // recipient's new account.
    v.push(Case::new(
        "failed_deposit_halts",
        state([(CONTRACT, code(&[op::INVALID]))]),
        deposit(CALLER, TxKind::Call(CONTRACT), 1_000, U256::from(7), Bytes::new(), GAS),
    ));
    v.push(Case::new(
        "failed_deposit_pre_frame",
        state([]),
        deposit(CALLER, TxKind::Call(PAYEE), 10, U256::from(5), Bytes::new(), 150_000),
    ));

    // Limit stops a parent catches: a child over the KV-update limit, and one over the
    // state-growth limit; the parent drops the status and stops.
    v.push(
        Case::new(
            "caught_stop_kv_updates",
            state([(CONTRACT, code(&calling_child_then_stop())), (CHILD, code(&writing(4)))]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        )
        .limits(limits(|l| l.tx[1] = 4)),
    );
    v.push(
        Case::new(
            "caught_stop_state_growth",
            state([(CONTRACT, code(&calling_child_then_stop())), (CHILD, code(&writing(3)))]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        )
        .limits(limits(|l| l.tx[2] = 2)),
    );
    // A child that loops past the compute detention allows after its parent read `TIMESTAMP`:
    // the child returns the stop, and the parent, on what the limit leaves it, drops the status
    // and stops.
    {
        let params = &mega_evm2::satin_version().gas_params;
        let cold = params.get(evm2::version::GasId::WarmStorageReadCost) +
            params.cold_account_additional_cost();
        let cap = 2 + (4 * 2 + 2 * 3 + 2 + cold) + 32 * (1 + 3 + 8) - 2;
        let parent = Asm::default()
            .ops([op::PUSH0, op::POP, op::TIMESTAMP, op::POP])
            .call(CHILD, U256::ZERO)
            .op(op::POP)
            .stop()
            .build();
        let child = Asm::default().op(op::JUMPDEST).push_u8(0).op(op::JUMP).build();
        v.push(
            Case::new(
                "caught_stop_compute_detention",
                state([(CONTRACT, code(&parent)), (CHILD, code(&child))]),
                tx(CONTRACT, 0, Bytes::new(), GAS),
            )
            .limits(limits(|l| l.cap = cap)),
        );
    }

    // Logs of frames that revert: the parent logs, a child logs and succeeds, another logs and
    // reverts, then the parent reverts as well.
    {
        let logs_and_stops = Asm::default().ops([op::PUSH0, op::PUSH0, op::LOG0, op::STOP]).build();
        let logs_and_reverts =
            Asm::default().push_u8(7).ops([op::PUSH0, op::PUSH0, op::LOG1]).revert().build();
        let parent = Asm::default()
            .ops([op::PUSH0, op::PUSH0, op::LOG0])
            .call_plain(op::CALL, CHILD, 1)
            .op(op::POP)
            .call_plain(op::CALL, PAYEE, 0)
            .op(op::POP)
            .revert()
            .build();
        v.push(Case::new(
            "reverted_log_root",
            state([
                (CONTRACT, funded_code(&parent, 1)),
                (CHILD, code(&logs_and_stops)),
                (PAYEE, code(&logs_and_reverts)),
            ]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        ));
    }

    // EIP-7702: a transaction that delegates AUTHORITY to CONTRACT's code and calls it; and a
    // contract calling an account delegated before the transaction.
    {
        let delegate = Asm::default()
            .sstore(U256::from(1), U256::from(2))
            .ops([op::PUSH0, op::PUSH0, op::LOG0, op::STOP])
            .build();
        v.push(Case::new(
            "delegation_set_and_called",
            state([(CONTRACT, code(&delegate))]),
            eip7702(CALLER, AUTHORITY, Bytes::new(), GAS, &[(CONTRACT, AUTHORITY, 0)]),
        ));
        let delegated = AccountInfo::default()
            .with_nonce(1)
            .with_code(evm2::bytecode::Bytecode::new_eip7702(CONTRACT));
        let caller = Asm::default().call_plain(op::CALL, AUTHORITY, 0).op(op::POP).stop().build();
        v.push(Case::new(
            "delegated_account_called",
            state([(CONTRACT, code(&delegate)), (AUTHORITY, delegated), (CHILD, code(&caller))]),
            tx(CHILD, 0, Bytes::new(), GAS),
        ));
    }

    // Precompiles: ecrecover, SHA-256 and identity called from a contract; and, after a read of
    // `NUMBER` under a detention cap of 25,000, a MODEXP whose price the remaining allowance
    // cannot pay, priced against detention before it runs.
    {
        let code_ = Asm::default()
            .call_input(op::STATICCALL, Address::with_last_byte(1), 0, &[0u8; 128])
            .op(op::POP)
            .call_input(op::STATICCALL, Address::with_last_byte(2), 0, b"satin")
            .op(op::POP)
            .call_input(op::CALL, Address::with_last_byte(4), 1, b"satin")
            .op(op::POP)
            .stop()
            .build();
        v.push(Case::new(
            "precompile_calls",
            state([(CONTRACT, funded_code(&code_, 1))]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        ));
        // MODEXP of a 64-byte base, exponent and modulus: an exponent of 2^512 - 1 prices it far
        // above 25,000.
        let mut modexp = Vec::new();
        for len in [64u64, 64, 64] {
            modexp.extend_from_slice(&U256::from(len).to_be_bytes::<32>());
        }
        modexp.extend_from_slice(&[0x03; 64]);
        modexp.extend_from_slice(&[0xff; 64]);
        modexp.extend_from_slice(&[0x07; 64]);
        let code_ = Asm::default()
            .ops([op::NUMBER, op::POP])
            .call_input(op::STATICCALL, Address::with_last_byte(5), 0, &modexp)
            .op(op::POP)
            .stop()
            .build();
        v.push(
            Case::new(
                "precompile_priced_by_detention",
                state([(CONTRACT, code(&code_))]),
                tx(CONTRACT, 0, Bytes::new(), GAS),
            )
            .limits(limits(|l| l.cap = 25_000)),
        );
    }

    // SELFDESTRUCT: of an account that existed before the transaction (it only moves its
    // balance), and of one the transaction created (destroyed).
    v.push(Case::new(
        "selfdestruct_existing",
        state([(
            CONTRACT,
            funded_code(&Asm::default().push_address(HEIR).op(op::SELFDESTRUCT).build(), 5),
        )]),
        tx(CONTRACT, 0, Bytes::new(), GAS),
    ));
    {
        let init = Asm::default().push_address(HEIR).op(op::SELFDESTRUCT).build();
        let code_ = Asm::default()
            .mstore(0, &init)
            .push_min(init.len() as u64)
            .op(op::PUSH0)
            .push_u8(3)
            .op(op::CREATE)
            .op(op::POP)
            .stop()
            .build();
        v.push(Case::new(
            "selfdestruct_created",
            state([(CONTRACT, funded_code(&code_, 5))]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        ));
    }

    // A detention stop in a child that the parent does not survive: the parent reads `TIMESTAMP`
    // and calls a child that reads `NUMBER` and loops.
    {
        let parent = Asm::default()
            .ops([op::TIMESTAMP, op::POP])
            .call(CHILD, U256::ZERO)
            .op(op::POP)
            .sstore(U256::ZERO, U256::from(1))
            .stop()
            .build();
        let child =
            Asm::default().ops([op::NUMBER, op::POP, op::JUMPDEST]).push_u8(2).op(op::JUMP).build();
        v.push(
            Case::new(
                "detention_stop_in_a_child",
                state([(CONTRACT, code(&parent)), (CHILD, code(&child))]),
                tx(CONTRACT, 0, Bytes::new(), GAS),
            )
            .limits(limits(|l| l.cap = 3_400)),
        );
    }

    // The reservoir's give-back: above the execution cap, a child writes a fresh slot (its state
    // gas paid from the reservoir) and reverts, which gives that state gas back to the reservoir;
    // the parent then writes a slot of its own and stops.
    {
        let child = Asm::default().sstore(U256::from(1), U256::from(1)).revert().build();
        let parent = Asm::default()
            .call_plain(op::CALL, CHILD, 0)
            .op(op::POP)
            .sstore(U256::from(2), U256::from(1))
            .stop()
            .build();
        v.push(Case::new(
            "reservoir_give_back",
            state([(CONTRACT, code(&parent)), (CHILD, code(&child))]),
            tx(CONTRACT, 0, Bytes::new(), EXECUTION_CAP + 2_000_000),
        ));
    }
    // The same give-back below the cap, where the spilled state gas is given back to the
    // reservoir the frame did not start with.
    {
        let child = Asm::default().sstore(U256::from(1), U256::from(1)).revert().build();
        let parent = Asm::default()
            .call_plain(op::CALL, CHILD, 0)
            .op(op::POP)
            .sstore(U256::from(2), U256::from(1))
            .stop()
            .build();
        v.push(Case::new(
            "reservoir_give_back_below_the_cap",
            state([(CONTRACT, code(&parent)), (CHILD, code(&child))]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        ));
    }
    v
}

/// The hard frame `name`, pinned.
pub fn pinned(name: &str) -> (Case, Traces) {
    let case = cases().into_iter().find(|c| c.name == name).expect("a listed hard frame");
    let traces = pin(&case);
    (case, traces)
}

/// The call tracer's frame at `path` (child indexes from the root).
fn frame<'a>(call: &'a Value, path: &[usize]) -> &'a Value {
    path.iter().fold(call, |f, i| &f["calls"][*i])
}

fn used(f: &Value) -> u64 {
    u64::from_str_radix(f["gasUsed"].as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
}

/// The `MegaLimitExceeded(kind, limit)` a frame's output holds, if it holds one.
fn stop_of(f: &Value) -> Option<(u8, u64)> {
    let out = alloy_primitives::hex::decode(f["output"].as_str()?).ok()?;
    let selector = limit_exceeded_output(0, 0);
    (out.len() == 68 && out[..4] == selector[..4])
        .then(|| (out[35], U256::from_be_slice(&out[36..68]).to::<u64>()))
}

#[test]
fn inspector_keyless_creation_reverts() {
    let (_, t) = pinned("keyless_creation_reverts");
    // The call answers (a deployment that failed is reported in its answer), and its creation,
    // the call's child, reverted with the init code's data.
    assert!(t.plain.success());
    let f = frames(&t["call"]);
    assert_eq!(f.len(), 2, "{f:?}");
    assert_eq!((f[1].0.as_str(), f[1].3.as_deref()), ("CREATE", Some("execution reverted")));
    assert_eq!(frame(&t["call"], &[0])["output"], json!("0xdeadbeef"));
    // The JavaScript tracer enters and leaves the creation.
    assert_eq!(t["js"]["frames"][0][1], json!("CREATE"));
}

#[test]
fn inspector_keyless_with_value_answered_before_its_frame() {
    let (_, t) = pinned("keyless_with_value");
    // Answered before evm2 builds the frame: one frame, no instruction, `NoEtherTransfer()`.
    assert!(!t.plain.success());
    assert_eq!(frames(&t["call"]).len(), 1);
    assert_eq!(steps(&t["struct_log"]).len(), 0, "no instruction ran");
    assert_eq!(
        t["call"]["output"],
        json!(format!("0x{}", alloy_primitives::hex::encode(selector("NoEtherTransfer()"))))
    );
}

#[test]
fn inspector_keyless_from_a_contract_runs_the_bytecode() {
    let (_, t) = pinned("keyless_from_a_contract");
    // Only a depth-0 call is dispatched: this one runs KeylessDeploy's bytecode, which reverts,
    // and no creation starts.
    let f = frames(&t["call"]);
    assert_eq!(f.len(), 2, "no creation below a depth-1 call: {f:?}");
    assert_eq!(f[1].3.as_deref(), Some("execution reverted"));
    assert!(steps(&t["struct_log"]).iter().any(|(d, _)| *d == 2), "the bytecode ran");
}

#[test]
fn inspector_intercept_limit_control() {
    let (_, t) = pinned("intercept_limit_control");
    // Each call of `MegaLimitControl` is answered before it runs: a frame that spent nothing and
    // ran no instruction; the call carrying value is refused (`NonZeroTransfer()`).
    assert!(t.plain.success());
    let calls = t["call"]["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 3);
    for c in calls {
        assert_eq!(used(c), 0, "an answered call spends nothing: {c}");
    }
    assert!(calls[0]["error"].is_null() && calls[1]["error"].is_null());
    assert_eq!(calls[2]["error"], json!("execution reverted"));
    assert_eq!(
        calls[2]["output"],
        json!(format!("0x{}", alloy_primitives::hex::encode(selector("NonZeroTransfer()"))))
    );
    assert!(steps(&t["struct_log"]).iter().all(|(d, _)| *d == 1), "no step below the root");
}

#[test]
fn inspector_intercept_access_control() {
    let (_, t) = pinned("intercept_access_control");
    // The child switched its access off (an answered call), then its `TIMESTAMP` was refused,
    // which reverts it; the parent's query is answered too.
    assert!(t.plain.success());
    let child = frame(&t["call"], &[0]);
    assert_eq!(child["error"], json!("execution reverted"), "the refused read reverts the child");
    assert_eq!(used(frame(&t["call"], &[0, 0])), 0, "the switch is answered");
    assert_eq!(used(frame(&t["call"], &[1])), 0, "the query is answered");
    let s = steps(&t["struct_log"]);
    assert!(s.iter().any(|(d, o)| *d == 2 && o == "TIMESTAMP"), "the refused read is a step");
}

#[test]
fn inspector_intercept_oracle_hint() {
    let (_, t) = pinned("intercept_oracle_hint");
    // The hint is admitted and handed to the service, and the Oracle's bytecode then runs: a
    // frame with steps.
    assert!(t.plain.success());
    assert_eq!(frames(&t["call"]).len(), 2);
    assert!(used(frame(&t["call"], &[0])) > 0);
    assert!(steps(&t["struct_log"]).iter().any(|(d, _)| *d == 2));
}

#[test]
fn inspector_pre_frame_value_to_a_new_account() {
    let (_, t) = pinned("pre_frame_value_to_a_new_account");
    // The first frame's runtime gas phase cannot pay the new account: the tracers see one halted
    // level-0 frame that ran nothing and spent the whole gas limit.
    assert!(!t.plain.success());
    assert_eq!(t["call"]["error"], json!("out of gas"));
    assert_eq!(used(&t["call"]), 60_000);
    assert_eq!(steps(&t["struct_log"]).len(), 0);
    assert_eq!(t["js"]["frames"], json!([]));
}

#[test]
fn inspector_pre_frame_creation() {
    let (_, t) = pinned("pre_frame_creation");
    assert!(!t.plain.success());
    assert_eq!(
        (t["call"]["type"].clone(), t["call"]["error"].clone()),
        (json!("CREATE"), json!("out of gas"))
    );
    assert_eq!(steps(&t["struct_log"]).len(), 0);
}

#[test]
fn inspector_failed_deposit_halts() {
    let (_, t) = pinned("failed_deposit_halts");
    // A failed deposit: it uses its whole gas limit, keeps only the mint and the nonce, and its
    // frame halted on the instruction.
    assert!(!t.plain.success());
    assert_eq!(used(&t["call"]), 10_000_000);
    assert_eq!(t["call"]["error"], json!("invalid opcode: INVALID"));
    let post = &t["prestate_diff"]["post"][format!("{CALLER:#x}")];
    assert_eq!(post["nonce"], json!(1));
    assert!(
        t["prestate_diff"]["post"].get(format!("{CONTRACT:#x}")).is_none(),
        "the value did not move"
    );
}

#[test]
fn inspector_failed_deposit_pre_frame() {
    let (_, t) = pinned("failed_deposit_pre_frame");
    assert!(!t.plain.success());
    assert_eq!(t["call"]["error"], json!("out of gas"));
    assert_eq!(used(&t["call"]), 150_000);
}

#[test]
fn inspector_caught_stop_kv_updates() {
    let (_, t) = pinned("caught_stop_kv_updates");
    assert!(t.plain.success());
    let child = frame(&t["call"], &[0]);
    assert_eq!(child["error"], json!("execution reverted"));
    assert_eq!(stop_of(child).map(|s| s.0), Some(1), "a KV-update stop");
}

#[test]
fn inspector_caught_stop_state_growth() {
    let (_, t) = pinned("caught_stop_state_growth");
    assert!(t.plain.success());
    let child = frame(&t["call"], &[0]);
    assert_eq!(stop_of(child).map(|s| s.0), Some(3), "a state-growth stop");
}

#[test]
fn inspector_caught_stop_compute_detention() {
    let (_, t) = pinned("caught_stop_compute_detention");
    assert!(t.plain.success(), "{}", t.plain.outcome);
    let child = frame(&t["call"], &[0]);
    assert_eq!(stop_of(child).map(|s| s.0), Some(2), "a compute stop under detention");
    let s = steps(&t["struct_log"]);
    assert_eq!(s.last(), Some(&(1, "STOP".to_string())), "the parent resumed and stopped");
}

#[test]
fn inspector_reverted_log_root() {
    let (_, t) = pinned("reverted_log_root");
    // Every frame logged and the root reverted: no tracer shows a log, though the steps ran.
    assert!(!t.plain.success());
    for f in super::tracers_frames(&t["call"]) {
        assert!(f.get("logs").is_none(), "{f}");
    }
    assert!(steps(&t["struct_log"]).iter().filter(|(_, o)| o.starts_with("LOG")).count() >= 3);
    assert_eq!(t["transfer"]["logs"], json!([]));
}

#[test]
fn inspector_delegation_set_and_called() {
    let (_, t) = pinned("delegation_set_and_called");
    // The authority's delegation is set and its call runs the delegate's code, in the authority's
    // own storage.
    assert!(t.plain.success(), "{}", t.plain.outcome);
    assert_eq!(t["call"]["to"], json!(format!("{AUTHORITY:#x}")));
    assert_eq!(t["call"]["logs"][0]["address"], json!(format!("{AUTHORITY:#x}")));
    let post = &t["prestate_diff"]["post"][format!("{AUTHORITY:#x}")];
    assert_eq!(post["code"], json!(format!("0xef0100{}", alloy_primitives::hex::encode(CONTRACT))));
    assert_eq!(post["storage"].as_object().map(|s| s.len()), Some(1));
}

#[test]
fn inspector_delegated_account_called() {
    let (_, t) = pinned("delegated_account_called");
    assert!(t.plain.success());
    let callee = frame(&t["call"], &[0]);
    assert_eq!(callee["to"], json!(format!("{AUTHORITY:#x}")));
    assert_eq!(callee["logs"][0]["address"], json!(format!("{AUTHORITY:#x}")));
}

#[test]
fn inspector_precompile_calls() {
    let (_, t) = pinned("precompile_calls");
    // Three precompile frames, priced at their Satin prices; the identity call carries value, so
    // its transfer log is the frame's.
    assert!(t.plain.success());
    let calls = t["call"]["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(used(&calls[0]), 3_000, "ecrecover");
    assert_eq!(calls[2]["logs"].as_array().map(Vec::len), Some(1), "the value's transfer log");
    assert_eq!(calls[2]["output"], json!("0x736174696e"));
}

#[test]
fn inspector_precompile_priced_by_detention() {
    let (_, t) = pinned("precompile_priced_by_detention");
    // After the read of `NUMBER`, MODEXP's price is held against detention's allowance, which
    // it is over: the call returns the compute stop without running, spending nothing, and the
    // contract survives it.
    assert!(t.plain.success());
    let call = frame(&t["call"], &[0]);
    assert_eq!(call["to"], json!(format!("{:#x}", Address::with_last_byte(5))));
    assert_eq!(stop_of(call).map(|s| s.0), Some(2));
    assert_eq!(used(call), 0);
}

#[test]
fn inspector_selfdestruct_existing() {
    let (_, t) = pinned("selfdestruct_existing");
    // An account that existed before the transaction moves its balance and stays (EIP-6780).
    assert!(t.plain.success());
    let sd = frame(&t["call"], &[0]);
    assert_eq!(
        (sd["type"].clone(), sd["to"].clone(), sd["value"].clone()),
        (json!("SELFDESTRUCT"), json!(format!("{HEIR:#x}")), json!("0x5"))
    );
    assert!(t["prestate_diff"]["post"].get(format!("{CONTRACT:#x}")).is_some(), "it stays");
    assert_eq!(t["transfer"]["transfers"][0]["kind"], json!("SelfDestruct"));
    assert_eq!(t["otterscan"][0]["type"], json!(1));
}

#[test]
fn inspector_selfdestruct_created() {
    let (_, t) = pinned("selfdestruct_created");
    // An account the transaction created is destroyed: the diff names it nowhere.
    assert!(t.plain.success());
    let created = frame(&t["call"], &[0]);
    let address = created["to"].as_str().unwrap().to_string();
    assert_eq!(frame(&t["call"], &[0, 0])["type"], json!("SELFDESTRUCT"));
    assert!(t["prestate_diff"]["post"].get(&address).is_none());
    assert!(t["prestate_diff"]["pre"].get(&address).is_none());
}

#[test]
fn inspector_detention_stop_in_a_child() {
    let (_, t) = pinned("detention_stop_in_a_child");
    // The child crosses the limit the parent's read set, and the parent, resumed on what the limit
    // leaves it, crosses it again: both return the same stop.
    assert!(!t.plain.success());
    let (root, child) = (&t["call"], frame(&t["call"], &[0]));
    assert_eq!(stop_of(root).map(|s| s.0), Some(2));
    assert_eq!(stop_of(root), stop_of(child));
}

#[test]
fn inspector_reservoir_give_back() {
    let (_, t) = pinned("reservoir_give_back");
    let (_, below) = pinned("reservoir_give_back_below_the_cap");
    // The same bill whichever pool paid. Above the cap the reservoir pays the child's state gas,
    // so the child's frame spends regular gas only; below it the state gas spills onto the
    // child's regular gas, which its `gasUsed` counts though the revert gave the state gas back
    // (to the reservoir, never to regular gas).
    assert!(t.plain.success() && below.plain.success());
    assert_eq!(t.plain.outcome["receipt"]["gas_used"], below.plain.outcome["receipt"]["gas_used"]);
    let (above_child, below_child) = (frame(&t["call"], &[0]), frame(&below["call"], &[0]));
    let logs = t["struct_log"]["structLogs"].as_array().unwrap();
    let sstore =
        logs.iter().position(|s| s["depth"] == json!(2) && s["op"] == json!("SSTORE")).unwrap();
    // What the reservoir paid of the slot's state gas: all of it but the legacy share, which
    // regular gas pays first in either case.
    let reservoir = |i: usize| logs[i]["stateGasReservoir"].as_u64().unwrap();
    let drawn = reservoir(sstore) - reservoir(sstore + 1);
    let state = logs[sstore]["stateGasCost"].as_u64().unwrap();
    assert!(drawn < state, "the legacy share is paid by regular gas");
    assert_eq!(used(below_child), used(above_child) + drawn, "the spill, counted on the child");
    // The parent's next step sees the reservoir the child's state gas came back to: all of it,
    // the legacy share included.
    let back = logs.iter().skip(sstore + 1).find(|s| s["depth"] == json!(1)).unwrap();
    let call =
        logs.iter().rposition(|s| s["depth"] == json!(1) && s["op"] == json!("CALL")).unwrap();
    assert_eq!(
        back["stateGasReservoir"].as_u64().unwrap(),
        reservoir(call) + (state - drawn),
        "{back}"
    );
}

#[test]
fn inspector_reservoir_give_back_below_the_cap() {
    let (_, t) = pinned("reservoir_give_back_below_the_cap");
    // Below the cap there is no reservoir to start with; the child's revert gives its spilled
    // state gas back to the reservoir, which the parent's next step shows.
    assert!(t.plain.success());
    let logs = t["struct_log"]["structLogs"].as_array().unwrap();
    let sstore =
        logs.iter().position(|s| s["depth"] == json!(2) && s["op"] == json!("SSTORE")).unwrap();
    let state = logs[sstore]["stateGasCost"].as_u64().unwrap();
    let back = logs.iter().skip(sstore + 1).find(|s| s["depth"] == json!(1)).unwrap();
    assert!(back["stateGasReservoir"].as_u64().unwrap() >= state, "{back}");
}
