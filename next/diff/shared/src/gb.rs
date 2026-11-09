//! Scenarios of group "gb": the give-back rule and the legacy share. Every give-back of state or
//! history gas credits a reservoir and never regular gas (`SATIN-HIST-012`, `SATIN-POOL-014`), and
//! a fresh slot, a new account and a creation's account take a flat legacy share from regular gas
//! first (`SATIN-STATE-012`, `SATIN-POOL-008`). They are the trajectories of the analysis behind
//! the rule — a failure handler after a starved call (`gb_t1_*`), a lock slot cleared after a
//! starved call (`gb_t2_*`), a reverting intermediary (`gb_t3_*`), a descendant's take-back
//! (`gb_t4_*`) and a failed frame's rollback (`gb_t5_*`), without and with a reservoir and at
//! `m = 1` and `m = 4` — and the worked examples of the pages the rule lives on (`gb_ex_*`).
//!
//! A frame measures what a segment costs it with [`measure`]: `GAS`, the segment, `GAS`, their
//! difference into a memory word, so the figure is the segment's charges to the frame's regular
//! gas plus the second `GAS`'s 2.
#![allow(dead_code)]
use super::{
    A, Account, Asm, Auth, B, C, D, EMPTY, KEYLESS, LIMIT_CONTROL, Limits, SENDER, Scenario, acct,
    funded, op, scenario,
};
use crate::q8::keyless_call_raw;
use alloy_primitives::{Address, hex};

/// A gas limit 300,000 above the 200,000,000 execution cap: a reservoir of 300,000 less the body's
/// history (`SATIN-POOL-005`).
pub const RESERVOIR_GAS: u64 = 200_300_000;
/// A gas limit below the execution cap: no reservoir.
pub const NO_RESERVOIR_GAS: u64 = 10_000_000;

const LOG2: u8 = 0xa2;
const DUP6: u8 = 0x85;

const fn local(b: u8) -> Address {
    let mut a = [0u8; 20];
    a[0] = 0x10;
    a[18] = 0x0b;
    a[19] = b;
    Address::new(a)
}

/// The callee that is starved: it halts at once, consuming its whole forward.
pub const STARVED: Address = local(0x01);
/// An account the state holds no leaf for.
pub const NEW: Address = local(0x02);
/// An account with a balance and no code.
pub const E: Address = local(0x03);
/// The authority of [`set_then_clear`]: neither the sender nor the recipient.
pub const AUTHORITY: Address = local(0x04);

fn gb(name: &'static str, rules: &[&'static str], accounts: Vec<Account>, gas: u64) -> Scenario {
    let mut s = scenario(name, accounts, gas, Limits::default());
    s.group = "gb";
    s.rules = rules.to_vec();
    s
}

/// `GAS`, `segment`, `GAS`, and their difference into memory word `i`.
pub fn measure(a: Asm, segment: Asm, i: u64) -> Asm {
    a.op(op::GAS).ops(&segment.0).op(op::GAS).op(op::SWAP1).op(op::SUB).push(32 * i).op(op::MSTORE)
}

/// Returns memory words `0..n`.
pub fn ret(a: Asm, n: u64) -> Vec<u8> {
    a.push(32 * n).push(0).op(op::RETURN).build()
}

/// `kind(gas, to, value, 0, 0, 0, 0)` with an explicit `gas`, the success word popped.
fn call_gas(a: Asm, kind: u8, to: Address, value: u64, gas: u64) -> Asm {
    a.call_with(kind, to, value, gas, 0).op(op::POP)
}

/// `CALL(GAS, to, value, 0, 0, 0, 0)`, the success word popped.
fn call_all(a: Asm, to: Address, value: u64) -> Asm {
    a.call_all(op::CALL, to, value).op(op::POP)
}

fn starved() -> Account {
    acct(STARVED, vec![op::INVALID])
}

/// `m` on slot `key` (or the account leaf) of `address`; `m = 1` leaves the minimum bucket.
fn at_m(s: &mut Scenario, address: Address, key: Option<u64>, m: u64) {
    if m != 1 {
        s.salt.push((address, key, m));
    }
}

fn gas_for(reservoir: bool) -> u64 {
    if reservoir { RESERVOIR_GAS } else { NO_RESERVOIR_GAS }
}

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(t1());
    v.extend(t2());
    v.extend(t3());
    v.extend(t4());
    v.extend(t5());
    v.extend(examples());
    v.extend(combined());
    v
}

/// A give-back the other implementation credits to regular gas, in a frame that also reads
/// `SLOTNUM`, which pushes zero where the other implementation pushes the slot number
/// (`SATIN-BASE-005`): A returns its `SLOTNUM` (word 0) and what a call of B costs it (word 1), B
/// writing a fresh slot from regular gas and reverting, which gives the slot's charges back into
/// B's reservoir. Neither difference alone accounts for the case; the gate's run under both
/// reference rules does (`gate::scenario_verdict`).
fn combined() -> Vec<Scenario> {
    let a = Asm::new().op(op::SLOTNUM).push(0).op(op::MSTORE);
    let a = measure(a, call_all(Asm::new(), B, 0), 1);
    vec![gb(
        "gb_combined_slotnum_reverted_child",
        &["SATIN-POOL-014", "SATIN-BASE-005"],
        vec![acct(A, ret(a, 2)), acct(B, Asm::new().sstore(1, 1).revert0().build())],
        NO_RESERVOIR_GAS,
    )]
}

/// T1, a failure handler after a starved call: A calls [`STARVED`] with 10,000 of gas, then writes
/// a fresh cold slot (word 0) and moves 1 wei, forwarding no gas, to an account the state does not
/// hold (word 1), each measured. Their regular price is the slot's 12,100 and 10,000 legacy share,
/// and the account's 3,000 access, 11,300 value transfer (its 2,300 stipend coming back) and
/// 25,000 legacy share; the rest of each state charge, and the records' history, are paid by the
/// reservoir as far as it reaches and by regular gas beyond it.
fn t1() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, reservoir, m) in [
        ("gb_t1_handler_m1", false, 1),
        ("gb_t1_handler_m4", false, 4),
        ("gb_t1_handler_res_m1", true, 1),
        ("gb_t1_handler_res_m4", true, 4),
    ] {
        let a = call_gas(Asm::new(), op::CALL, STARVED, 0, 10_000);
        let a = measure(a, Asm::new().sstore(1, 1), 0);
        let a = measure(a, call_gas(Asm::new(), op::CALL, NEW, 1, 0), 1);
        let mut s = gb(
            name,
            &["SATIN-STATE-012", "SATIN-POOL-008", "SATIN-HIST-010"],
            vec![funded(A, ret(a, 2), 1), starved()],
            gas_for(reservoir),
        );
        at_m(&mut s, A, Some(1), m);
        at_m(&mut s, NEW, None, m);
        v.push(s);
    }
    v
}

/// T2, a lock slot cleared after a starved call: A sets slot 0, calls [`STARVED`] with all its gas,
/// clears slot 0 (word 0) and records the failure in fresh slot 1 (word 1). The clear gives the
/// slot's state gas and its record's history into A's reservoir and costs A's regular gas its 100;
/// the record takes 22,100 of regular gas, the rest from that reservoir. The threshold scenarios
/// run A's code without its measurements from B, with exactly the least gas A completes from and
/// with one gas less.
fn t2() -> Vec<Scenario> {
    let rules = ["SATIN-HIST-012", "SATIN-STATE-008", "SATIN-TAKE-005", "SATIN-STATE-012"];
    let mut v = Vec::new();
    for (name, reservoir, m) in [
        ("gb_t2_lock_m1", false, 1),
        ("gb_t2_lock_m4", false, 4),
        ("gb_t2_lock_res_m1", true, 1),
        ("gb_t2_lock_res_m4", true, 4),
    ] {
        let a = call_all(Asm::new().sstore(0, 1), STARVED, 0);
        let a = measure(a, Asm::new().sstore(0, 0), 0);
        let a = measure(a, Asm::new().sstore(1, 1), 1);
        let mut s = gb(name, &rules, vec![acct(A, ret(a, 2)), starved()], gas_for(reservoir));
        at_m(&mut s, A, Some(0), m);
        at_m(&mut s, A, Some(1), m);
        v.push(s);
    }
    let plain =
        call_all(Asm::new().sstore(0, 1), STARVED, 0).sstore(0, 0).sstore(1, 1).stop().build();
    for (name, m, gas) in [
        ("gb_t2_threshold_m1", 1, T2_THRESHOLD_M1),
        ("gb_t2_threshold_below_m1", 1, T2_THRESHOLD_M1 - 1),
        ("gb_t2_threshold_m4", 4, T2_THRESHOLD_M4),
        ("gb_t2_threshold_below_m4", 4, T2_THRESHOLD_M4 - 1),
    ] {
        let b = call_gas(Asm::new(), op::CALL, A, 0, gas).stop().build();
        let mut s =
            gb(name, &rules, vec![acct(B, b), acct(A, plain.clone()), starved()], NO_RESERVOIR_GAS);
        s.to = B;
        at_m(&mut s, A, Some(0), m);
        at_m(&mut s, A, Some(1), m);
        v.push(s);
    }
    v
}

/// The least gas A runs T2's lock from at `m = 1` without a reservoir: the set (`PUSH1`, `PUSH0`,
/// the cold write's 12,100, the slot's 97,920 and its record's 3,520), the call's seven pushes and
/// its 3,000 cold access, then 50 times what A needs once the starved call returned (`POP`, the
/// clear's pushes and 100, the record's pushes, 12,100 and 10,000 legacy share): the forward
/// leaves A `⌊available / 50⌋`.
pub const T2_THRESHOLD_M1: u64 = (3 + 2 + 12_100 + 97_920 + 3_520) +
    (2 * 5 + 3 + 2) +
    3_000 +
    50 * (2 + (2 + 2 + 100) + (3 + 3 + 12_100 + 10_000));
/// [`T2_THRESHOLD_M1`] at `m = 4`: the set's slot costs 391,680.
pub const T2_THRESHOLD_M4: u64 = T2_THRESHOLD_M1 + 3 * 97_920;

/// T3, a reverting intermediary: A calls D with `F` of gas (word 0: the call's cost to A); D writes
/// a fresh cold slot (or emits a 10,240-byte `LOG0`), calls [`STARVED`] with all its gas and
/// bubbles the failure with `REVERT`. A's regular gas gets D's unspent regular gas back only, and
/// D's state and history charges arrive in A's reservoir.
fn t3() -> Vec<Scenario> {
    let rules = ["SATIN-POOL-014", "SATIN-POOL-012", "SATIN-HIST-012", "SATIN-TAKE-011"];
    let mut v = Vec::new();
    let slot_d = call_all(Asm::new().sstore(0, 1), STARVED, 0).revert0().build();
    let log_d =
        call_all(Asm::new().push(10_240).push(0).op(op::LOG0), STARVED, 0).revert0().build();
    for (name, d, forward, reservoir, m) in [
        ("gb_t3_slot_m1", &slot_d, 500_000, false, 1),
        ("gb_t3_slot_m4", &slot_d, 2_000_000, false, 4),
        ("gb_t3_slot_res_m1", &slot_d, 500_000, true, 1),
        ("gb_t3_slot_res_m4", &slot_d, 2_000_000, true, 4),
        ("gb_t3_log", &log_d, 2_000_000, false, 1),
        ("gb_t3_log_res", &log_d, 2_000_000, true, 1),
    ] {
        let a = measure(Asm::new(), call_gas(Asm::new(), op::CALL, D, 0, forward), 0);
        let mut s = gb(
            name,
            &rules,
            vec![acct(A, ret(a, 1)), acct(D, d.clone()), starved()],
            gas_for(reservoir),
        );
        at_m(&mut s, D, Some(0), m);
        v.push(s);
    }
    v
}

/// T4, a descendant's take-back (the descendant example of `SATIN-TAKE-005`): B calls P (at A) with
/// all its gas, so that P's frame is recorded as it ends; P, whose slot 0 holds 7, writes 8 and
/// makes a `DELEGATECALL` to C, which emits an empty `LOG0` and writes 7 back (measured, its word
/// returned or reverted with), then returns or reverts; P then stops, reverts or halts. The 3,520
/// of P's record go into C's reservoir, and C's regular gas does not rise.
fn t4() -> Vec<Scenario> {
    let rules = ["SATIN-HIST-007", "SATIN-TAKE-005", "SATIN-TAKE-002", "SATIN-POOL-014"];
    let c = |revert: bool| {
        let a = measure(Asm::new().push(0).push(0).op(op::LOG0), Asm::new().sstore(0, 7), 0);
        let a = a.push(32).push(0);
        a.op(if revert { op::REVERT } else { op::RETURN }).build()
    };
    let p = |end: u8| {
        let a = Asm::new().sstore(0, 8).push(0).push(0).push(0).push(0).push_addr(C);
        let a = a.op(op::GAS).op(op::DELEGATECALL).op(op::POP);
        let a = if end == op::REVERT { a.revert0() } else { a.op(end) };
        a.build()
    };
    let mut v = Vec::new();
    for (name, c_reverts, p_end, reservoir) in [
        ("gb_t4_descendant_kept", false, op::STOP, false),
        ("gb_t4_descendant_reverted", true, op::REVERT, false),
        ("gb_t4_descendant_halted", true, op::INVALID, false),
        ("gb_t4_descendant_kept_res", false, op::STOP, true),
        ("gb_t4_descendant_reverted_res", true, op::REVERT, true),
    ] {
        let mut p_acct = acct(A, p(p_end));
        p_acct.storage = vec![(0, 7)];
        let b = call_all(Asm::new(), A, 0).stop().build();
        let mut s =
            gb(name, &rules, vec![acct(B, b), p_acct, acct(C, c(c_reverts))], gas_for(reservoir));
        s.to = B;
        // `m` does not reach a slot whose original is not zero.
        at_m(&mut s, A, Some(0), 4);
        v.push(s);
    }
    v
}

/// T5, a failed frame's rollback with history (the example of `SATIN-TAKE-011`): A calls C with an
/// explicit forward; C writes a fresh cold slot and reverts or halts. With a reservoir, A's is
/// 102,040 at the call, so that C's share is exactly 100,000.
fn t5() -> Vec<Scenario> {
    let rules = ["SATIN-POOL-014", "SATIN-TAKE-011", "SATIN-HIST-012", "SATIN-STATE-012"];
    let reverts = Asm::new().sstore(0, 1).revert0().build();
    let halts = Asm::new().sstore(0, 1).op(op::INVALID).build();
    let mut v = Vec::new();
    for (name, c, forward, reservoir, m) in [
        ("gb_t5_reverted_res_m1", &reverts, 50_000, true, 1),
        ("gb_t5_halted_res_m1", &halts, 50_000, true, 1),
        ("gb_t5_reverted_res_m4", &reverts, 500_000, true, 4),
        ("gb_t5_halted_res_m4", &halts, 500_000, true, 4),
        ("gb_t5_reverted_m1", &reverts, 150_000, false, 1),
        ("gb_t5_halted_m1", &halts, 150_000, false, 1),
        ("gb_t5_reverted_m4", &reverts, 500_000, false, 4),
        ("gb_t5_halted_m4", &halts, 500_000, false, 4),
    ] {
        let a = call_gas(Asm::new(), op::CALL, C, 0, forward).stop().build();
        let gas = if reservoir { T5_RESERVOIR_GAS } else { NO_RESERVOIR_GAS };
        let mut s = gb(name, &rules, vec![acct(A, a), acct(C, c.clone())], gas);
        at_m(&mut s, C, Some(0), m);
        v.push(s);
    }
    v
}

/// The body's history: 310 bytes at 88.
pub const BODY_HISTORY: u64 = 310 * 88;
/// The gas limit that leaves A a reservoir of 102,040, of which C's share is
/// `102,040 − ⌊102,040 × 2 / 100⌋ = 100,000`.
pub const T5_RESERVOIR_GAS: u64 = 200_000_000 + BODY_HISTORY + 102_040;

/// The worked examples of the pages of the rule, each one scenario (two for a comparison).
fn examples() -> Vec<Scenario> {
    let mut v = Vec::new();
    // `SATIN-POOL-012`: a caller with 100 of reservoir keeps 2 and gives the child 98, which the
    // child returns: the caller has 100 again.
    let a = call_all(Asm::new(), C, 0).stop().build();
    v.push(gb(
        "gb_ex_pool_share_returns",
        &["SATIN-POOL-012", "SATIN-POOL-011"],
        vec![acct(A, a), acct(C, Asm::new().stop().build())],
        200_000_000 + BODY_HISTORY + 100,
    ));
    // `SATIN-POOL-019`: a call that clears one cold slot whose original value is not zero and does
    // nothing else (`PUSH1 0; PUSH1 0; SSTORE`).
    let clear = Asm::new().push_bytes(&[0]).push_bytes(&[0]).op(op::SSTORE).stop().build();
    let mut a = acct(A, clear);
    a.storage = vec![(0, 5)];
    v.push(gb("gb_ex_pool_clear_ledgers", &["SATIN-POOL-019"], vec![a], NO_RESERVOIR_GAS));
    // `SATIN-STATE-012`: a `SELFDESTRUCT` that moves a non-zero balance to a warm beneficiary empty
    // under EIP-161. B calls A, which warms [`NEW`] and destroys itself to it.
    let a_code = Asm::new()
        .push_addr(NEW)
        .op(op::BALANCE)
        .op(op::POP)
        .push_addr(NEW)
        .op(op::SELFDESTRUCT)
        .build();
    for (name, reservoir) in
        [("gb_ex_selfdestruct_share_res", true), ("gb_ex_selfdestruct_share", false)]
    {
        let b = call_all(Asm::new(), A, 0).stop().build();
        let mut s = gb(
            name,
            &["SATIN-STATE-012", "SATIN-SD-018", "SATIN-SD-019"],
            vec![acct(B, b), funded(A, a_code.clone(), 1)],
            gas_for(reservoir),
        );
        s.to = B;
        v.push(s);
    }
    // `SATIN-HIST-019`, `SATIN-HIST-022`: a value call whose child has only the stipend and no
    // reservoir emits an empty `LOG0`, which the history allowance pays.
    let a = call_gas(Asm::new(), op::CALL, C, 1, 0).stop().build();
    v.push(gb(
        "gb_ex_allowance_log",
        &["SATIN-HIST-019", "SATIN-HIST-022"],
        vec![funded(A, a, 1), acct(C, Asm::new().push(0).push(0).op(op::LOG0).stop().build())],
        NO_RESERVOIR_GAS,
    ));
    // `SATIN-HIST-022`: a deposit with empty calldata that makes no write.
    let mut s = gb("gb_ex_deposit_body", &["SATIN-HIST-022", "SATIN-HIST-016"], vec![], 1_000_000);
    s.to = EMPTY;
    s.deposit = Some(0);
    v.push(s);
    // `SATIN-DET-013`: value calls of 1 wei that forward no gas to an account with no code.
    for (name, calls) in
        [("gb_ex_det_stipend_calls", 18_000), ("gb_ex_det_stipend_calls_fewer", 17_000)]
    {
        let mut s = gb(
            name,
            &["SATIN-DET-013", "SATIN-DET-010", "SATIN-MLC-002"],
            vec![funded(A, stipend_calls(calls), calls), funded(E, vec![], 1)],
            200_000_000,
        );
        s.limits.block_env_cap = Some(DET_EXAMPLE_CAP);
        v.push(s);
    }
    v.extend(sent_back());
    v.extend(destroyed_creation());
    v.extend(set_then_clear());
    v.extend(log_reverted());
    v.extend(answer_stopped());
    // `SATIN-TAKE-012`: a deposit of 1,000,000 gas from a sender empty under EIP-161, whose first
    // frame halts.
    let mut s = gb(
        "gb_ex_failed_deposit",
        &["SATIN-TAKE-012", "SATIN-DEP-004"],
        vec![acct(A, vec![op::INVALID])],
        1_000_000,
    );
    s.accounts.retain(|a| a.address != SENDER);
    s.deposit = Some(0);
    v.push(s);
    v.extend(keyless());
    v
}

/// The detention cap of the `SATIN-DET-013` example: above the compute its calls count, so the
/// limit never binds.
pub const DET_EXAMPLE_CAP: u64 = 210_000_000;
/// The regular charges of one turn of [`stipend_calls`]' loop besides the call's own: `JUMPDEST`,
/// `DUP1`, `ISZERO`, `PUSH1`, `JUMPI`, the call's seven pushes, `POP`, `PUSH1`, `SWAP1`, `SUB`,
/// `PUSH1` and `JUMP`.
pub const STIPEND_LOOP_TURN: u64 = 1 + 3 + 3 + 3 + 10 + (2 * 4 + 3 + 3 + 2) + 2 + 3 + 3 + 3 + 3 + 8;

/// `TIMESTAMP`, then `calls` value calls of 1 wei with no gas to [`E`] in a loop, then the answer
/// of `remainingComputeGas()` (word 0) and `GAS` (word 1).
fn stipend_calls(calls: u64) -> Vec<u8> {
    let mut selector = [0u8; 32];
    selector[..4].copy_from_slice(&alloy_primitives::keccak256("remainingComputeGas()")[..4]);
    let head = Asm::new().op(op::TIMESTAMP).op(op::POP).push(calls).0;
    let body = |end: u8, start: u8| {
        Asm::new()
            .op(op::JUMPDEST)
            .op(op::DUP1)
            .op(op::ISZERO)
            .push_bytes(&[end])
            .op(op::JUMPI)
            .call_with(op::CALL, E, 1, 0, 0)
            .op(op::POP)
            .push_bytes(&[1])
            .op(op::SWAP1)
            .op(op::SUB)
            .push_bytes(&[start])
            .op(op::JUMP)
            .0
    };
    let start = head.len() as u8;
    let end = start + body(0, 0).len() as u8;
    let mut code = head;
    code.extend_from_slice(&body(end, start));
    let tail = Asm::new()
        .op(op::JUMPDEST)
        .op(op::POP)
        .push_bytes(&selector)
        .push(0)
        .op(op::MSTORE)
        .push(32)
        .push(0)
        .push(4)
        .push(0)
        .push_addr(LIMIT_CONTROL)
        .op(op::GAS)
        .op(op::STATICCALL)
        .op(op::POP)
        .op(op::GAS)
        .push(32)
        .op(op::MSTORE);
    code.extend_from_slice(&tail.0);
    ret(Asm(code), 2)
}

/// P of [`sent_back`]: called with value, it stops; otherwise it runs `first`, then calls Q with 1
/// wei and `gas` (`None`: all its gas).
fn sent_back_p(first: Asm, gas: Option<u64>) -> Vec<u8> {
    let tail = |a: Asm| {
        let a = first.0.iter().fold(a, |a, o| a.op(*o));
        let a = match gas {
            Some(g) => call_gas(a, op::CALL, B, 1, g),
            None => call_all(a, B, 1),
        };
        a.op(op::STOP)
    };
    let end = (Asm::new().op(op::CALLVALUE).push_bytes(&[0]).op(op::JUMPI).0.len() +
        tail(Asm::new()).0.len()) as u8;
    let a = Asm::new().op(op::CALLVALUE).push_bytes(&[end]).op(op::JUMPI);
    let a = tail(a);
    assert_eq!(a.0.len(), end as usize, "P's jump target");
    a.op(op::JUMPDEST).op(op::STOP).build()
}

/// `SATIN-TAKE-006`'s sent-back example and its two variants: P (at A) calls Q (at B) with 1 wei
/// and Q calls P back with 1 wei. In `gb_ex_sent_back_nonce` P first bumps its nonce by a `CREATE`;
/// in `gb_ex_sent_back_earlier` a top contract first has Q send P 1 wei, and Q, called back with
/// 200,000 of gas, forwards to P all but what leaves it 5,000 after the call's charges.
fn sent_back() -> Vec<Scenario> {
    let rules = ["SATIN-TAKE-006", "SATIN-HIST-009", "SATIN-ACCT-004"];
    let q_back = call_all(Asm::new(), A, 1).stop().build();
    let mut v = Vec::new();
    for (name, first) in [
        ("gb_ex_sent_back", Asm::new()),
        ("gb_ex_sent_back_nonce", Asm::new().push(0).push(0).push(0).op(op::CREATE).op(op::POP)),
    ] {
        let mut s = gb(
            name,
            &rules,
            vec![funded(A, sent_back_p(first, None), 10), funded(B, q_back.clone(), 10)],
            NO_RESERVOIR_GAS,
        );
        s.to = A;
        v.push(s);
    }
    // Q: with calldata, sends P 1 wei with no gas; without, calls P back with 1 wei, forwarding
    // `GAS − 16,409`: 9 for `PUSH2`, `SWAP1` and `SUB`, 11,400 for the call, and 5,000 left.
    let back = Asm::new()
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push(1)
        .push_addr(A)
        .op(op::GAS)
        .push_bytes(&[0x40, 0x19])
        .op(op::SWAP1)
        .op(op::SUB)
        .op(op::CALL)
        .op(op::POP)
        .op(op::STOP);
    let send_at = (Asm::new().op(op::CALLDATASIZE).push_bytes(&[0]).op(op::JUMPI).0.len() +
        back.0.len()) as u8;
    let q = Asm::new().op(op::CALLDATASIZE).push_bytes(&[send_at]).op(op::JUMPI).ops(&back.0);
    assert_eq!(q.0.len(), send_at as usize, "Q's jump target");
    let q = call_gas(q.op(op::JUMPDEST), op::CALL, A, 1, 0).stop().build();
    // The top contract: Q with one byte of calldata, then P.
    let top = Asm::new()
        .push(0)
        .push(0)
        .push(1)
        .push(0)
        .push(0)
        .push_addr(B)
        .op(op::GAS)
        .op(op::CALL)
        .op(op::POP);
    let top = call_all(top, A, 0).stop().build();
    let mut s = gb(
        "gb_ex_sent_back_earlier",
        &rules,
        vec![acct(C, top), funded(A, sent_back_p(Asm::new(), Some(200_000)), 10), funded(B, q, 10)],
        NO_RESERVOIR_GAS,
    );
    s.to = C;
    v.push(s);
    v
}

/// The init code of [`destroyed_creation`]'s C: writes slots 1 and 2, then returns `runtime`.
fn init_returning(runtime: &[u8]) -> Vec<u8> {
    let head = Asm::new().sstore(1, 1).sstore(2, 1);
    let tail = |at: u8| {
        Asm::new()
            .push(runtime.len() as u64)
            .push_bytes(&[at])
            .push(0)
            .op(op::CODECOPY)
            .push(runtime.len() as u64)
            .push(0)
            .op(op::RETURN)
    };
    let at = (head.0.len() + tail(0).0.len()) as u8;
    let mut init = head.ops(&tail(at).0).0;
    assert_eq!(init.len(), at as usize);
    init.extend_from_slice(runtime);
    init
}

/// `SATIN-TAKE-007`'s examples: P (at A) creates C at an address the state holds no account for;
/// C's constructor writes two fresh slots and C deploys 100 bytes of code; then P calls C, which
/// destroys itself to [`NEW`], holding no balance (`gb_ex_destroyed_creation`) or the 1 wei P
/// endowed it with (`gb_ex_destroyed_creation_beneficiary`).
fn destroyed_creation() -> Vec<Scenario> {
    let rules =
        ["SATIN-TAKE-007", "SATIN-SD-005", "SATIN-SD-007", "SATIN-SD-019", "SATIN-STATE-012"];
    let mut runtime = Asm::new()
        .push_addr(NEW)
        .op(op::BALANCE)
        .op(op::POP)
        .push_addr(NEW)
        .op(op::SELFDESTRUCT)
        .build();
    runtime.resize(100, op::STOP);
    let init = init_returning(&runtime);
    let mut v = Vec::new();
    for (name, value) in
        [("gb_ex_destroyed_creation", 0), ("gb_ex_destroyed_creation_beneficiary", 1)]
    {
        let p = Asm::new().create(&init, value).push(0).push(0).push(0).push(0).push(0);
        let p = p.op(DUP6).op(op::GAS).op(op::CALL).op(op::POP).op(op::POP).stop().build();
        let mut s = gb(name, &rules, vec![funded(A, p, value)], NO_RESERVOIR_GAS);
        s.to = A;
        v.push(s);
    }
    v
}

/// `SATIN-TAKE-008`'s set-then-clear example: two accepted authorizations for [`AUTHORITY`], which
/// the state holds no account for, the first delegating to D at nonce 0, the second clearing at
/// nonce 1.
fn set_then_clear() -> Vec<Scenario> {
    let mut s = gb(
        "gb_ex_set_then_clear",
        &["SATIN-TAKE-008", "SATIN-AUTH-009"],
        vec![acct(A, Asm::new().stop().build()), acct(D, Asm::new().stop().build())],
        NO_RESERVOIR_GAS,
    );
    s.auths = vec![
        Auth { chain_id: 1, address: D, nonce: 0, authority: Some(AUTHORITY) },
        Auth { chain_id: 1, address: Address::ZERO, nonce: 1, authority: Some(AUTHORITY) },
    ];
    vec![s]
}

/// `SATIN-TAKE-009`'s log-reverted example: C emits `LOG2` with 64 bytes of data (160 bytes, 14,080
/// of history gas) and reverts; as a value call's child, the history allowance pays the log.
fn log_reverted() -> Vec<Scenario> {
    let child = Asm::new().push(1).push(2).push(64).push(0).op(LOG2).revert0().build();
    let mut v = Vec::new();
    for (name, value) in [("gb_ex_log_reverted", 0), ("gb_ex_log_reverted_allowance", 1)] {
        let a = call_gas(Asm::new(), op::CALL, C, value, 100_000).stop().build();
        v.push(gb(
            name,
            &["SATIN-TAKE-009", "SATIN-HIST-019", "SATIN-POOL-014"],
            vec![funded(A, a, 1), acct(C, child.clone())],
            NO_RESERVOIR_GAS,
        ));
    }
    v
}

/// `SATIN-TAKE-010`'s answer-stopped example: A, under a transaction KV limit of 3, writes two
/// fresh slots, then calls [`NEW`], empty and without code, with 1 wei and no gas; it returns the
/// call's return data.
fn answer_stopped() -> Vec<Scenario> {
    let a = Asm::new().sstore(1, 1).sstore(2, 1);
    let a = call_gas(a, op::CALL, NEW, 1, 0);
    let a = a.op(op::RETURNDATASIZE).push(0).push(0).op(op::RETURNDATACOPY);
    let code = a.op(op::RETURNDATASIZE).push(0).op(op::RETURN).build();
    let mut s = gb(
        "gb_ex_answer_stopped",
        &["SATIN-TAKE-010", "SATIN-POOL-015", "SATIN-STATE-012", "SATIN-LIM-013"],
        vec![funded(A, code, 1)],
        NO_RESERVOIR_GAS,
    );
    s.limits.kv = Some(3);
    vec![s]
}

/// `SATIN-KEY-015`'s two examples: a keyless creation whose constructor writes a fresh slot and
/// reverts, and one whose constructor is `ORIGIN; SELFDESTRUCT`, each on 500,000 of forward and no
/// reservoir.
fn keyless() -> Vec<Scenario> {
    let rules = ["SATIN-KEY-015", "SATIN-KEY-005", "SATIN-STATE-012", "SATIN-SD-005"];
    let mut v = Vec::new();
    for (name, raw) in
        [("gb_ex_keyless_revert", KEYLESS_REVERT_TX), ("gb_ex_keyless_destroy", KEYLESS_DESTROY_TX)]
    {
        let mut s = gb(name, &rules, vec![], 2_000_000);
        s.to = KEYLESS;
        s.data = keyless_call_raw(&hex::decode(raw).unwrap(), 500_000);
        v.push(s);
    }
    v
}

/// A pre-EIP-155 creation (gas price 100 gwei, gas limit 200,000) with the fixed signature of the
/// keyless fixtures, of `PUSH1 1; PUSH0; SSTORE; PUSH0; PUSH0; REVERT`; signed by
/// [`KEYLESS_REVERT_SIGNER`].
pub const KEYLESS_REVERT_TX: &str = "f8588085174876e80083030d4080808760015f555f5ffd1ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222";
pub const KEYLESS_REVERT_SIGNER: Address =
    alloy_primitives::address!("0xb23E340B5Ec2029efe5936144D0539f7E3553D66");
/// The same of `ORIGIN; SELFDESTRUCT`; signed by [`KEYLESS_DESTROY_SIGNER`].
pub const KEYLESS_DESTROY_TX: &str = "f8538085174876e80083030d4080808232ff1ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222";
pub const KEYLESS_DESTROY_SIGNER: Address =
    alloy_primitives::address!("0x4daA96Df502ffD582784b485d6b585303869Ba15");

#[cfg(test)]
mod tests {
    use super::*;

    /// The scenario names are unique, and each scenario names its rules.
    #[test]
    fn test_give_back_scenarios_are_named_and_ruled() {
        let all = all();
        let mut names: Vec<_> = all.iter().map(|s| s.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), all.len());
        assert!(all.iter().all(|s| !s.rules.is_empty() && s.group == "gb"));
    }
}
