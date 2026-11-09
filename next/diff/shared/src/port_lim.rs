//! Scenarios of group "port": the limit tests of the other implementation (data size, KV updates,
//! state growth, the frame budgets, the stop and what it leaves) that no scenario did yet.
#![allow(dead_code)]
use super::{A, Asm, B, C, D, EMPTY, LIMIT_CONTROL, ORACLE, Scenario, acct, funded, op, pp::*};
use crate::q8::{mem_call, send_hint};
use alloy_primitives::{Address, U256};

const R_DATA: [&str; 4] = ["SATIN-LIM-001", "SATIN-LIM-003", "SATIN-STOP-001", "SATIN-STOP-004"];
const R_KV: [&str; 4] = ["SATIN-ACCT-012", "SATIN-LIM-003", "SATIN-STOP-001", "SATIN-STOP-018"];
const R_GROWTH: [&str; 4] = ["SATIN-ACCT-013", "SATIN-LIM-003", "SATIN-STOP-001", "SATIN-ACCT-019"];
type Edit = Box<dyn Fn(&mut Scenario)>;
const E: Address = Address::with_last_byte(0xee);
const X: Address = Address::with_last_byte(0x1e);

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(data_size());
    v.extend(data_size_more());
    v.extend(kv_and_growth());
    v.extend(budgets());
    v.extend(stops());
    v.extend(deposits());
    v
}

fn fresh(n: u64) -> Vec<u8> {
    writes(Asm::new(), 0, n).stop().build()
}

/// A calls B and carries on: `CALL(GAS, to) POP`.
fn call_then(a: Asm, to: Address) -> Asm {
    call_all(a, to, 0)
}

fn data_size() -> Vec<Scenario> {
    let mut v = Vec::new();
    // --- a chain of four calls: the deepest log crosses the transaction limit; every caller
    // resumes
    let d = log0(Asm::new(), 8_000).stop().build();
    let chain = |next: Address| call_then(Asm::new(), next).sstore(2, 1).stop().build();
    let mut s = sc(
        "port_dsx_depth3_tx_limit",
        &R_DATA,
        vec![acct(A, chain(B)), acct(B, chain(C)), acct(C, chain(D)), acct(D, d)],
        20_000_000,
    );
    s.limits.data = Some(8_000);
    v.push(s);
    // --- code one byte over the transaction limit is not left deployed
    let mut init = Asm::new().push(32).push(0).op(op::RETURN).build();
    init.resize(32, 0);
    for (name, limit) in [
        ("port_dsx_code_fits", 310 + 32 + 40 + 32),
        ("port_dsx_code_one_over", 310 + 32 + 40 + 32 - 1),
    ] {
        let mut s = sc(name, &R_DATA, vec![], 20_000_000);
        s.create = true;
        s.data = init.clone();
        s.limits.data = Some(limit);
        v.push(s);
    }
    // --- a creation is held to its share from its start
    let cases: [(u64, u64, u64); 8] = [
        (40, 0, 0),
        (41, 0, 0),
        (80, 0, 0),
        (2000, 0, 1920),
        (2000, 0, 1921),
        (204, 1, 0),
        (205, 1, 0),
        (240, 1, 0),
    ];
    let more: [(u64, u64, u64); 2] = [(2160, 1, 1916), (2160, 1, 1917)];
    for (left, e, n) in cases.into_iter().chain(more) {
        let init = crate::init_returning(n);
        let a = Asm::new().create(&init, e).op(op::POP).stop().build();
        let mut s = scn(
            format!("port_dsx_creation_share_{left}_{e}_{n}"),
            &R_DATA,
            vec![funded(A, a, e)],
            20_000_000,
        );
        s.limits.data = Some(310 + left);
        v.push(s);
    }
    // --- the code a creation refuses is not counted
    for (name, init, gas, limit) in [
        ("port_dsx_refused_code_ef_61", init_first(0xef, 61), 20_000_000u64, 410u64),
        ("port_dsx_refused_code_oversize", init_first(0x60, 524_289), 2_000_000_000, 410),
        ("port_dsx_control_code_61", init_first(0x60, 61), 2_000_000_000, 410),
        ("port_dsx_control_code_max", init_first(0x60, 524_288), 2_000_000_000, 410),
    ] {
        let a = Asm::new().create(&init, 0).op(op::ISZERO).push(1).op(op::SSTORE).stop().build();
        let mut s = sc(name, &R_DATA, vec![acct(A, a)], gas);
        s.limits.data = Some(limit);
        v.push(s);
    }
    // --- an account write one byte over stops on the recipient
    for (name, limit) in [
        ("port_dsx_account_write_fits", 310 + 40 + TRANSFER_LOG + 40),
        ("port_dsx_account_write_start_short", 310 + 39),
        ("port_dsx_account_write_log_short", 310 + 40 + TRANSFER_LOG - 1),
    ] {
        let mut s =
            sc(name, &R_DATA, vec![acct(B, Asm::new().sstore(0, 1).stop().build())], 20_000_000);
        s.to = B;
        s.value = U256::from(1u64);
        s.limits.data = Some(limit);
        v.push(s);
    }
    // --- a start is charged before its records are counted
    let a_call = call_rec(Asm::new(), EMPTY, 1, 1).stop().build();
    for (name, limit) in [
        ("port_dsx_start_charged_first_cross", 310 + 240 - 1),
        ("port_dsx_start_charged_first_fits", 310 + 240),
    ] {
        for gas in [592_194u64, 592_195, 592_196, 592_197] {
            let mut s =
                scn(format!("{name}_{gas}"), &R_DATA, vec![funded(A, a_call.clone(), 1)], gas);
            s.limits.data = Some(limit);
            v.push(s);
        }
    }
    // --- an out-of-funds caller one gas short of its records runs out of gas
    // (shares the world above, with a larger sweep around the boundary)
    for gas in [592_190u64, 592_194, 592_195, 592_196] {
        let mut s = scn(
            format!("port_dsx_caller_short_{gas}"),
            &R_DATA,
            vec![funded(A, a_call.clone(), 1)],
            gas,
        );
        s.limits.data = Some(549);
        v.push(s);
    }
    // --- a transaction over the limit in its body is refused at every gas limit
    let body_cases: [(&'static str, Edit); 2] = [
        (
            "value_to_empty",
            Box::new(|s| {
                s.to = EMPTY;
                s.value = U256::from(1u64);
            }),
        ),
        (
            "creation",
            Box::new(|s| {
                s.create = true;
                s.data = vec![0x00; 12];
            }),
        ),
    ];
    for (tag, edit) in &body_cases {
        for (gname, gas) in [("0", 0u64), ("21000", 21_000), ("60000", 60_000), ("70000", 70_000)] {
            let mut s = scn(format!("port_dsx_body_over_{tag}_{gname}"), &R_DATA, vec![], gas);
            edit(&mut s);
            s.limits.data = Some(BODY + s.data.len() as u64 - 1);
            v.push(s);
        }
    }
    let mut s = sc("port_dsx_body_over_7702", &R_DATA, vec![], 200_000);
    s.auths = vec![crate::Auth { chain_id: 1, address: D, nonce: 0, authority: Some(X) }];
    s.limits.data = Some(310);
    v.push(s);
    // --- a child gets 98 percent of what its parent has left, in slots
    let kids: [(&'static str, u64, u64, u64); 7] = [
        ("b98", 98, 0, 0),
        ("b99", 99, 0, 0),
        ("a20_b78", 78, 20, 0),
        ("a20_b79", 79, 20, 0),
        ("c96", 0, 0, 96),
        ("c97", 0, 0, 97),
        ("b99_c98", 99, 0, 98),
    ];
    for (tag, b_slots, a_first, c_slots) in kids {
        let b = if c_slots > 0 && b_slots == 0 {
            call_then(Asm::new(), C).stop().build()
        } else if c_slots > 0 {
            call_then(writes(Asm::new(), 0, b_slots), C).stop().build()
        } else {
            fresh(b_slots)
        };
        let a = call_then(writes(Asm::new(), 100, a_first), B).stop().build();
        let mut s = scn(
            format!("port_dsx_child_98pct_{tag}"),
            &R_DATA,
            vec![acct(A, a), acct(B, b), acct(C, fresh(c_slots))],
            50_000_000,
        );
        s.limits.data = Some(310 + 100 * RECORD);
        v.push(s);
    }
    // --- the body and the execution add up to the stop
    for calldata in [0usize, 200] {
        let name = nm(format!("port_dsx_body_and_execution_{calldata}"));
        let mut s = sc(name, &R_DATA, vec![acct(A, fresh(4))], 50_000_000);
        s.data = vec![0xab; calldata];
        s.limits.data = Some(310 + calldata as u64 + RECORD);
        v.push(s);
    }
    // --- a body over the limit is rejected before an interceptor
    for (name, limit) in
        [("port_dsx_interceptor_answered", 314u64), ("port_dsx_interceptor_refused_for_body", 313)]
    {
        let mut s = sc(name, &R_DATA, vec![], 1_000_000);
        s.to = LIMIT_CONTROL;
        s.data = crate::remaining_compute_gas_selector().to_vec();
        s.limits.data = Some(limit);
        v.push(s);
    }
    v
}

fn data_size_more() -> Vec<Scenario> {
    let mut v = Vec::new();
    // --- a failed creation's nonce record holds its creator to its budget
    for (left, label) in [(40u64, "fits"), (39, "short")] {
        for (kind, c2) in [("create", false), ("create2", true)] {
            let n = 3_920 - left - 32;
            let mut b = log0(Asm::new(), n);
            b = if c2 {
                b.push(0).push(0).push(0).push(0).op(op::CREATE2)
            } else {
                b.push(0).push(0).push(0).op(op::CREATE)
            };
            let b = b.op(op::POP).stop().build();
            let a = Asm::new().call_all(op::CALL, B, 0).op(op::POP).stop().build();
            let mut s = scn(
                format!("port_dsx_failed_creation_nonce_{kind}_{label}"),
                &R_DATA,
                vec![acct(A, a), acct(B, b)],
                50_000_000,
            );
            s.limits.data = Some(310 + 4_000);
            v.push(s);
        }
    }
    // --- a creation that counts nothing runs, and its creator is stopped at the return
    {
        let b =
            Asm::new().push(0).push(0).push(0).push(0).op(op::CREATE2).op(op::POP).stop().build();
        let a = Asm::new()
            .call_all(op::CALL, E, 1)
            .op(op::POP)
            .call_all(op::CALL, B, 0)
            .op(op::POP)
            .stop()
            .build();
        let mut s = sc(
            "port_dsx_creation_counting_nothing",
            &R_DATA,
            vec![funded(A, a, 1), acct(B, b), acct(E, vec![op::STOP])],
            50_000_000,
        );
        s.limits.data = Some(310 + 2 * RECORD + TRANSFER_LOG + 39);
        v.push(s);
    }
    // --- a caller a failed callee left over the transaction limit is stopped
    {
        let hint = send_hint(7, &[0xab; 64]);
        for (tag, halts) in [("halts", true), ("writes", false)] {
            // A: a call with no data writes slot 0 and calls B; B re-enters A (a call with data,
            // which writes the slot back), sends a hint, then halts or writes a fresh slot.
            let a = branch_on_data(
                call_then(Asm::new().sstore(0, 1), B).stop(),
                Asm::new().sstore(0, 0).stop(),
            );
            let mut b = Asm::new().call_with(op::CALL, A, 0, 1_000_000, 1).op(op::POP);
            b = mem_call(b, op::CALL, ORACLE, 0, 200_000, &hint).op(op::POP);
            let b = if halts { b.op(op::INVALID).build() } else { b.sstore(5, 1).stop().build() };
            let mut s = scn(
                format!("port_dsx_caller_left_over_{tag}"),
                &R_DATA,
                vec![acct(A, a), acct(B, b), acct(ORACLE, vec![op::STOP])],
                50_000_000,
            );
            s.limits.data = Some(310 + 64 + 100);
            v.push(s);
        }
    }
    // --- a reverted child's write and write-back leave its caller's write
    {
        let a = call_then(Asm::new().sstore(9, 1), B).stop().build();
        let b = Asm::new().sstore(0, 1).sstore(0, 0).revert0().build();
        let mut s = sc(
            "port_dsx_reverted_write_and_back",
            &R_DATA,
            vec![acct(A, a), acct(B, b)],
            50_000_000,
        );
        s.limits.data = Some(310 + 40);
        v.push(s);
    }
    v
}

fn kv_and_growth() -> Vec<Scenario> {
    let mut v = Vec::new();
    // --- the KV records: the 11th record stops the transaction's own frame
    for n in [10u64, 11] {
        let mut s = scn(
            format!("port_kv_eleventh_record_{n}"),
            &R_KV,
            vec![acct(A, fresh(n))],
            2_000_000_000,
        );
        s.limits.kv = Some(10);
        s.limits.growth = Some(u64::MAX);
        v.push(s);
    }
    // a child gets 98 percent of what its caller has left (kv limit 10)
    for (tag, a_extra) in [("fit", 8u64), ("stopped", 9)] {
        let a = call_then(writes(Asm::new(), 100, 2), B);
        let a = writes(a, 200, a_extra).stop().build();
        let mut s = scn(
            format!("port_kv_child_98pct_{tag}"),
            &R_KV,
            vec![acct(A, a), acct(B, fresh(8))],
            2_000_000_000,
        );
        s.limits.kv = Some(10);
        v.push(s);
    }
    // the budgets at depth: D writes 95 fresh slots
    {
        let chain = |next: Address| call_then(Asm::new(), next).sstore(1, 1).stop().build();
        let mut s = sc(
            "port_kv_depth3_95",
            &R_KV,
            vec![acct(A, chain(B)), acct(B, chain(C)), acct(C, chain(D)), acct(D, fresh(95))],
            2_000_000_000,
        );
        s.limits.kv = Some(100);
        v.push(s);
    }
    // a record crossing data size and KV stops on data size
    for (tag, kv_cut) in [("both", 1u64), ("kv_only", 0)] {
        let n = 5u64;
        let mut s = scn(
            format!("port_kv_cross_data_and_kv_{tag}"),
            &R_KV,
            vec![acct(A, fresh(n))],
            2_000_000_000,
        );
        s.limits.data = Some(if kv_cut == 1 { 310 + n * 40 - 1 } else { 1_000_000 });
        s.limits.kv = Some(n - 1);
        v.push(s);
    }
    // a negative net does not enlarge the next child's budget
    {
        let a = Asm::new()
            .sstore(1, 1)
            .call_with(op::DELEGATECALL, B, 0, 20_000_000, 0)
            .op(op::POP)
            .stop()
            .build();
        let b = call_then(Asm::new().sstore(1, 0), C).stop().build();
        let mut s = sc(
            "port_kv_negative_net_budget",
            &R_KV,
            vec![acct(A, a), acct(B, b), acct(C, fresh(96))],
            2_000_000_000,
        );
        s.limits.kv = Some(100);
        v.push(s);
    }
    // --- the exemption
    let sys = |name: &'static str, program: Vec<u8>, l: (Option<u64>, Option<u64>, Option<u64>)| {
        let mut s = crate::edge::from_system(name, crate::edge::system_account(vec![], 0), ORACLE);
        s.group = "port";
        s.rules = vec!["SATIN-MST-008", "SATIN-LIM-017", "SATIN-MST-007"];
        s.chain_id = Some(1);
        s.accounts.retain(|x| x.address != ORACLE);
        s.accounts.push(acct(ORACLE, program));
        s.limits.data = l.0;
        s.limits.kv = l.1;
        s.limits.growth = l.2;
        s
    };
    let oracle_body = |helper: Address| {
        let mut a = Asm::new().op(op::CALLDATASIZE).push(0).op(op::SWAP1).op(op::POP).op(op::POP);
        a = a.sstore(1, 1).sstore(2, 1);
        a = a.push(64).push(0).op(op::LOG0);
        a = a.call_with(op::CALL, helper, 0, 1_000_000, 0).op(op::POP).stop();
        a.build()
    };
    let helper = acct(B, Asm::new().sstore(3, 1).stop().build());
    for (tag, l) in [
        ("data", (Some(310u64), None, None)),
        ("kv", (None, Some(0), None)),
        ("growth", (None, None, Some(0))),
        ("all", (Some(310), Some(0), Some(0))),
    ] {
        let mut s = sys(nm(format!("port_exempt_system_{tag}")), oracle_body(B), l);
        s.accounts.push(helper.clone());
        v.push(s);
    }
    // a deposit and a user call to a system contract are held to the limits
    for (tag, l) in [
        ("data", (Some(310u64), None, None)),
        ("kv", (None, Some(0), None)),
        ("growth", (None, None, Some(0))),
    ] {
        let mut s = scn(
            format!("port_exempt_deposit_{tag}"),
            &["SATIN-MST-008", "SATIN-DEP-007"],
            vec![acct(A, oracle_body(B)), helper.clone()],
            5_000_000,
        );
        s.deposit = Some(0);
        s.limits.data = l.0;
        s.limits.kv = l.1;
        s.limits.growth = l.2;
        v.push(s);
        let mut s = scn(
            format!("port_exempt_user_to_oracle_{tag}"),
            &["SATIN-MST-008", "SATIN-LIM-017"],
            vec![acct(ORACLE, oracle_body(B)), helper.clone()],
            5_000_000,
        );
        s.to = ORACLE;
        s.limits.data = l.0;
        s.limits.kv = l.1;
        s.limits.growth = l.2;
        v.push(s);
    }
    // a system transaction keeps more state growth than the limit
    {
        let program = write_loop(Asm::new(), 0, 1_001).stop().build();
        let mut s = sys("port_exempt_system_growth_1001", program, (None, None, None));
        s.gas_limit = 200_000_000 + 1_001 * (SLOT_STATE + RECORD_HISTORY) + 100_000_000;
        v.push(s);
    }
    // --- state growth
    // the 1001st fresh slot stops the transaction's own frame
    for n in [1_000u64, 1_001] {
        let a = write_loop(Asm::new(), 0, n).stop().build();
        let mut s =
            scn(format!("port_growth_fresh_slots_{n}"), &R_GROWTH, vec![acct(A, a)], 400_000_000);
        v.push(s.clone());
        s.name = nm(format!("port_growth_fresh_slots_{n}_big_gas"));
        s.gas_limit = 2_000_000_000;
        v.push(s);
    }
    v
}

fn budgets() -> Vec<Scenario> {
    let mut v = Vec::new();
    let g = 2_000_000_000u64;
    // a child budget of 960, then a later child of 19; a child's 961st slot stops at 960
    for (tag, b_slots, c_slots) in [("960_19", 960u64, 19u64), ("961_961", 961, 961)] {
        let a = call_then(call_then(write_loop(Asm::new(), 10_000, 20), B), C).stop().build();
        let mut s = scn(
            format!("port_budget_child_{tag}"),
            &R_GROWTH,
            vec![
                acct(A, a),
                acct(B, write_loop(Asm::new(), 0, b_slots).stop().build()),
                acct(C, write_loop(Asm::new(), 0, c_slots).stop().build()),
            ],
            g,
        );
        s.limits.growth = None;
        v.push(s);
    }
    // KV weighs more than the data-size cap: scaled to the block's gas limit (the full test writes
    // 327,673 slots, 33 billion gas), the data limit stops a transaction whose slots the KV and
    // growth limits would still admit
    for (tag, n) in [("below", 999u64), ("at", 1_000), ("over", 1_001)] {
        let a = write_loop(Asm::new(), 0, n).stop().build();
        let mut s =
            scn(format!("port_budget_kv_vs_data_{tag}"), &R_DATA, vec![acct(A, a)], 600_000_000);
        s.limits.data = Some(310 + 40 * 1_000);
        s.limits.growth = Some(u64::MAX);
        v.push(s);
    }
    // a record that crosses data size and state growth names data size
    {
        let a = call_then(Asm::new(), B).stop().build();
        let mut s = sc(
            "port_budget_data_and_growth_child",
            &R_DATA,
            vec![acct(A, a), acct(B, write_loop(Asm::new(), 0, 981).stop().build())],
            g,
        );
        s.limits.data = Some(310 + 40_000);
        v.push(s);
        let mut s = sc(
            "port_budget_data_and_growth_top",
            &R_DATA,
            vec![acct(A, write_loop(Asm::new(), 0, 1_001).stop().build())],
            g,
        );
        s.limits.data = Some(310 + 40_000);
        v.push(s);
    }
    // growth at depth 2 and 3 stops at 940
    for n in [940u64, 941] {
        let chain = |next: Address| call_then(Asm::new(), next).sstore(1, 1).stop().build();
        let mut s = scn(
            format!("port_budget_growth_depth3_{n}"),
            &R_GROWTH,
            vec![
                acct(A, chain(B)),
                acct(B, chain(C)),
                acct(C, chain(D)),
                acct(D, write_loop(Asm::new(), 10, n).stop().build()),
            ],
            g,
        );
        v.push(s.clone());
        s.name = nm(format!("port_budget_growth_depth2_{n}"));
        s.accounts = vec![
            acct(A, chain(B)),
            acct(B, chain(C)),
            acct(C, write_loop(Asm::new(), 10, n).stop().build()),
        ];
        s.accounts.insert(0, crate::sender());
        v.push(s);
    }
    // a cleared net does not raise the child's remaining
    for (tag, b_reverts) in [("kept", false), ("reverted", true)] {
        let a = {
            let x = write_loop(Asm::new(), 0, 100);
            let x = x.call_with(op::DELEGATECALL, B, 0, 100_000_000, 0).op(op::POP);
            call_then(x, D).stop().build()
        };
        let b_clear = {
            // clears slots 1..=100 of the caller's storage, then calls C (865 fresh slots)
            let mut x = Asm::new().push(100);
            let dest = x.0.len() as u64;
            x = x
                .op(op::JUMPDEST)
                .push(0)
                .op(op::DUP2)
                .op(op::SSTORE)
                .push(1)
                .op(op::SWAP1)
                .op(op::SUB)
                .op(op::DUP1)
                .push(dest)
                .op(op::JUMPI)
                .op(op::POP);
            x = call_then(x, C);
            if b_reverts { x.revert0().build() } else { x.stop().build() }
        };
        let mut s = scn(
            format!("port_budget_cleared_net_{tag}"),
            &R_GROWTH,
            vec![
                acct(A, a),
                acct(B, b_clear),
                acct(C, write_loop(Asm::new(), 1_000, 865).stop().build()),
                acct(D, write_loop(Asm::new(), 5_000, 981).stop().build()),
            ],
            g,
        );
        s.limits.growth = None;
        v.push(s);
    }
    // a pre-funded creation keeps a zero net until the 1001st slot
    for n in [1_000u64, 1_001] {
        let ctor = Asm::new().push_addr(E).op(op::SELFDESTRUCT).build();
        let a = {
            let x = Asm::new().push_bytes(&ctor_pad(&ctor)).push(0).op(op::MSTORE);
            let x = x.push(0).push(ctor.len() as u64).push(0).push(0).op(op::CREATE2).op(op::POP);
            write_loop(x, 20_000, n).stop().build()
        };
        let mut s =
            scn(format!("port_budget_prefunded_creation_{n}"), &R_GROWTH, vec![acct(A, a)], g);
        let created = A.create2(alloy_primitives::B256::ZERO, alloy_primitives::keccak256(&ctor));
        s.accounts.push(funded(created, vec![], 1));
        s.accounts.push(funded(E, vec![], 1));
        v.push(s);
    }
    // --- the hints and the logs: twelve hints of a mebibyte, then a log that crosses the limit
    {
        let hint = send_hint(1, &vec![0xcd; 1_048_576]);
        let mut a = Asm::new();
        for _ in 0..12 {
            a = mem_call(a, op::CALL, ORACLE, 0, 10_000_000, &hint).op(op::POP);
        }
        let top = log0(a.clone(), 524_288).stop().build();
        let child = call_then(a, B).stop().build();
        for (name, code) in
            [("port_budget_hints_then_top_log", top), ("port_budget_hints_then_child_log", child)]
        {
            let mut s = sc(
                name,
                &R_DATA,
                vec![
                    acct(A, code),
                    acct(B, log0(Asm::new(), 524_288).stop().build()),
                    acct(ORACLE, vec![op::STOP]),
                ],
                2_000_000_000,
            );
            s.limits.data = None;
            v.push(s);
        }
    }
    // a successful child's negative net leaves the transaction at 410; reverting: 450
    for (tag, rev) in [("kept", false), ("reverted", true)] {
        let hint = send_hint(1, &[0xee; 100]);
        let a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 1_000_000, &hint)
            .op(op::POP)
            .sstore(1, 1)
            .call_with(op::DELEGATECALL, B, 0, 1_000_000, 0)
            .op(op::POP)
            .stop()
            .build();
        let b = if rev {
            Asm::new().sstore(1, 0).revert0().build()
        } else {
            Asm::new().sstore(1, 0).stop().build()
        };
        let mut s = scn(
            format!("port_budget_negative_net_data_{tag}"),
            &R_DATA,
            vec![acct(A, a), acct(B, b), acct(ORACLE, vec![op::STOP])],
            10_000_000,
        );
        s.limits.data = Some(1_000_000);
        v.push(s);
    }
    // a negative data-size net does not raise what a frame forwards
    {
        let a = Asm::new()
            .sstore(1, 1)
            .call_with(op::DELEGATECALL, B, 0, 10_000_000, 0)
            .op(op::POP)
            .stop()
            .build();
        let b = call_then(Asm::new().sstore(1, 0), C).stop().build();
        let mut s = sc(
            "port_budget_negative_net_forward",
            &R_DATA,
            vec![acct(A, a), acct(B, b), acct(C, fresh(1))],
            10_000_000,
        );
        s.limits.data = Some(310 + 81);
        v.push(s);
    }
    v
}

fn ctor_pad(c: &[u8]) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[..c.len()].copy_from_slice(c);
    w
}

fn stops() -> Vec<Scenario> {
    let mut v = Vec::new();
    // --- records
    // CREATE2 onto a pre-funded address is not a rise
    {
        let ctor = vec![op::STOP];
        let created = A.create2(alloy_primitives::B256::ZERO, alloy_primitives::keccak256(&ctor));
        let a = Asm::new()
            .push_bytes(&ctor_pad(&ctor))
            .push(0)
            .op(op::MSTORE)
            .push(0)
            .push(1)
            .push(0)
            .push(0)
            .op(op::CREATE2)
            .op(op::POP)
            .stop()
            .build();
        for (tag, kv) in [("plain", None), ("kv1", Some(1u64))] {
            let mut s = scn(
                format!("port_rec_create2_prefunded_{tag}"),
                &R_KV,
                vec![acct(A, a.clone()), funded(created, vec![], 1)],
                5_000_000,
            );
            s.limits.kv = kv;
            v.push(s);
        }
    }
    // a creation whose creator's nonce is stuck counts nothing
    {
        let a = Asm::new().create(&[op::STOP], 0).op(op::POP).stop().build();
        let mut s = sc(
            "port_rec_stuck_nonce_create",
            &R_KV,
            vec![crate::Account { nonce: u64::MAX, ..acct(A, a) }],
            5_000_000,
        );
        s.limits.kv = Some(1);
        v.push(s);
    }
    // a second non-zero write neither rises nor falls
    {
        let a = Asm::new().sstore(1, 1).sstore(1, 2).stop().build();
        let b = Asm::new().sstore(2, 0).sstore(2, 7).stop().build();
        let mut s =
            sc("port_rec_nonzero_after_nonzero_fresh", &R_GROWTH, vec![acct(A, a)], 5_000_000);
        s.limits.growth = Some(1);
        v.push(s);
        let mut s = sc(
            "port_rec_nonzero_original",
            &R_GROWTH,
            vec![crate::Account { storage: vec![(2, 5)], ..acct(A, b) }],
            5_000_000,
        );
        s.limits.growth = Some(0);
        v.push(s);
    }
    // slot sequences in one frame rise once
    {
        let a = Asm::new()
            .sstore(1, 0)
            .sstore(2, 1)
            .sstore(2, 1)
            .sstore(3, 1)
            .sstore(3, 2)
            .sstore(4, 1)
            .sstore(4, 0)
            .sstore(4, 2)
            .sstore(5, 7)
            .stop()
            .build();
        for g in [3u64, 2] {
            let mut s = scn(
                format!("port_rec_slot_sequences_growth{g}"),
                &R_GROWTH,
                vec![crate::Account { storage: vec![(5, 3)], ..acct(A, a.clone()) }],
                5_000_000,
            );
            s.limits.growth = Some(g);
            v.push(s);
        }
    }
    // --- the stop: a record that crosses KV names the frame budget (one authorization applied)
    {
        let mut s =
            sc("port_stop_kv_with_authorization", &R_KV, vec![acct(A, fresh(10))], 5_000_000);
        s.auths = vec![crate::Auth { chain_id: 1, address: D, nonce: 0, authority: Some(X) }];
        s.limits.kv = Some(10);
        v.push(s);
    }
    // a charge that crosses detention stops before the write is counted
    {
        let b = Asm::new().op(op::TIMESTAMP).op(op::POP).sstore(1, 1).stop().build();
        let a = call_then(writes(Asm::new(), 10, 3), B).stop().build();
        let mut s = sc(
            "port_stop_detention_before_count",
            &R_GROWTH,
            vec![acct(A, a), acct(B, b)],
            300_000_000,
        );
        s.limits.block_env_cap = Some(1_000);
        s.limits.growth = Some(4);
        v.push(s);
    }
    // KV at depth 2 and 3 stops at 94
    {
        let chain = |next: Address| call_then(Asm::new(), next).sstore(1, 1).stop().build();
        let mut s = sc(
            "port_stop_kv_depth3",
            &R_KV,
            vec![acct(A, chain(B)), acct(B, chain(C)), acct(C, chain(D)), acct(D, fresh(95))],
            2_000_000_000,
        );
        s.limits.kv = Some(100);
        v.push(s);
    }
    // a creator's nonce record stops the creator at its KV budget
    for (tag, b_writes) in [("nine", 9u64), ("eight", 8)] {
        let b = writes(Asm::new(), 0, b_writes).create(&[op::STOP], 0).op(op::POP).stop().build();
        let mut s = scn(
            format!("port_stop_creator_nonce_kv_{tag}"),
            &R_KV,
            vec![acct(A, call_then(Asm::new(), B).stop().build()), acct(B, b)],
            5_000_000,
        );
        s.limits.kv = Some(10);
        v.push(s);
    }
    // a precompile answered from its price is held to the price alone (KV limit 2)
    {
        let a = Asm::new()
            .op(op::TIMESTAMP)
            .op(op::POP)
            .call_with(op::CALL, Address::with_last_byte(8), 1, 1_000_000, 0)
            .op(op::POP)
            .stop()
            .build();
        for (tag, cap, kv) in [("cap", Some(20_000u64), None), ("kv2", Some(20_000), Some(2u64))] {
            let mut s = scn(
                format!("port_stop_precompile_price_{tag}"),
                &R_KV,
                vec![funded(A, a.clone(), 1)],
                5_000_000,
            );
            s.limits.block_env_cap = cap;
            s.limits.kv = kv;
            v.push(s);
        }
    }
    // --- a contract reverting with a stop's bytes is no stop
    for kind in 0u64..4 {
        let mut data = crate::q8::sel("MegaLimitExceeded(uint8,uint64)").to_vec();
        data.extend_from_slice(&U256::from(kind).to_be_bytes::<32>());
        data.extend_from_slice(&U256::from(10u64).to_be_bytes::<32>());
        let mut a = Asm::new();
        for (i, w) in data.chunks(32).enumerate() {
            let mut x = [0u8; 32];
            x[..w.len()].copy_from_slice(w);
            a = a.push_bytes(&x).push(32 * i as u64).op(op::MSTORE);
        }
        let code = a.push(68).push(0).op(op::REVERT).build();
        for (stem, rules) in
            [("port_stop_spoofed_revert", &R_KV), ("port_outcome_spoofed_revert", &R_DATA)]
        {
            v.push(scn(
                format!("{stem}_kind{kind}"),
                rules,
                vec![acct(A, code.clone())],
                5_000_000,
            ));
        }
    }
    // --- a start three calls down crossing the limit is stopped at its start
    {
        let c = Asm::new().call_all(op::CALL, EMPTY, 1).op(op::POP).sstore(3, 1).stop().build();
        let b = call_then(Asm::new(), C).stop().build();
        let a = call_then(Asm::new().op(op::TIMESTAMP).op(op::POP), B).stop().build();
        for (tag, gas) in [("below_cap", 5_000_000u64), ("above_cap", 300_000_000)] {
            let mut s = scn(
                format!("port_stop_start_depth3_{tag}"),
                &R_DATA,
                vec![acct(A, a.clone()), acct(B, b.clone()), funded(C, c.clone(), 1)],
                gas,
            );
            s.limits.data = Some(310 + 40 + 100);
            v.push(s);
        }
    }
    // a stopped answer gives its caller back its share
    {
        let b = {
            let x = writes(Asm::new(), 0, 3);
            let x = call_all(x, EMPTY, 1);
            writes(x, 10, 7).stop().build()
        };
        for (tag, gas) in [("below_cap", 5_000_000u64), ("above_cap", 300_000_000)] {
            for (dim, lim) in [
                ("data", lim(Some(310 + 360), None, None)),
                ("kv", lim(None, Some(9), None)),
                ("growth", lim(None, None, Some(9))),
            ] {
                let mut s = scn(
                    format!("port_stop_answer_gives_back_{dim}_{tag}"),
                    &R_DATA,
                    vec![acct(A, call_then(Asm::new(), B).stop().build()), funded(B, b.clone(), 1)],
                    gas,
                );
                s.limits = lim;
                v.push(s);
            }
        }
    }
    // --- the strategic stop
    // the cap crossed at depth three stops each frame that crosses it
    {
        let mid = |next: Address| {
            call_then(Asm::new().sstore(1, 1), next)
                .sstore(2, 1)
                .push(1)
                .push(0)
                .op(op::LOG0)
                .stop()
                .build()
        };
        let mut s = sc(
            "port_strat_depth3",
            &R_DATA,
            vec![acct(A, mid(B)), acct(B, mid(C)), acct(C, mid(D)), acct(D, fresh(3))],
            50_000_000,
        );
        s.limits.data = Some(310 + 180);
        v.push(s);
    }
    // a true out of gas still halts and burns
    {
        let mut s = sc("port_strat_true_oog", &R_DATA, vec![acct(A, fresh(1))], 152_305);
        s.limits.data = Some(310);
        v.push(s);
    }
    // a child out of gas under a cap is no stop
    {
        let a = call_with_gas(Asm::new(), B, 40_000).sstore(1, 1).stop().build();
        let mut s =
            sc("port_strat_child_oog", &R_DATA, vec![acct(A, a), acct(B, fresh(1))], 5_000_000);
        s.limits.data = Some(310 + 40);
        v.push(s);
    }
    // the cap crossed before the first frame reverts without running
    {
        let mut s =
            sc("port_strat_before_first_frame", &R_DATA, vec![acct(B, fresh(1))], 5_000_000);
        s.to = B;
        s.value = U256::from(1u64);
        s.limits.data = Some(311);
        v.push(s);
    }
    // a creation transaction stopped before its first frame bumps the nonce
    {
        let init = crate::init_returning(1);
        let mut s = sc("port_strat_create_tx_stopped", &R_DATA, vec![], 5_000_000);
        s.create = true;
        s.data = init.clone();
        s.limits.data = Some(310 + init.len() as u64 + 39);
        v.push(s);
    }
    v
}

fn call_with_gas(a: Asm, to: Address, gas: u64) -> Asm {
    call(a, to, 0, gas)
}

fn deposits() -> Vec<Scenario> {
    let mut v = Vec::new();
    // a new depositor's 1000th slot stops at 999; an existing depositor's 1001st at 1000
    for (tag, n, existing) in [
        ("new_1000", 1_000u64, false),
        ("existing_1001", 1_001, true),
        ("new_999", 999, false),
        ("existing_1000", 1_000, true),
    ] {
        let a = write_loop(Asm::new(), 0, n).stop().build();
        let mut s = scn(format!("port_depo_slots_{tag}"), &R_GROWTH, vec![acct(A, a)], 400_000_000);
        s.deposit = Some(0);
        if existing {
            s.sender = D;
            s.accounts.push(funded(D, vec![], 1_000_000_000));
        } else {
            s.sender = Address::with_last_byte(0x77);
        }
        v.push(s);
    }
    // keyed state growth: an authority list
    // a fitting list keeps the authorities when the recipient stops
    {
        let auths = vec![
            crate::Auth {
                chain_id: 1,
                address: D,
                nonce: 0,
                authority: Some(Address::new([0x71; 20])),
            },
            crate::Auth {
                chain_id: 1,
                address: D,
                nonce: 0,
                authority: Some(Address::new([0x72; 20])),
            },
        ];
        let mut s = sc("port_depo_list_fits_recipient_stops", &R_GROWTH, vec![], 5_000_000);
        s.to = EMPTY;
        s.value = U256::from(1u64);
        s.auths = auths.clone();
        s.limits.growth = Some(2);
        v.push(s);
        // a list that does not fit is not the out-of-gas path
        let mut s = sc("port_depo_list_not_fitting", &R_GROWTH, vec![], 60_000);
        s.auths = auths[..1].to_vec();
        s.limits.growth = Some(0);
        v.push(s);
        // two entries for one new authority accept the first
        let dup = crate::Auth {
            chain_id: 1,
            address: D,
            nonce: 0,
            authority: Some(Address::new([0x71; 20])),
        };
        let mut s = sc("port_depo_list_same_authority_twice", &R_GROWTH, vec![], 5_000_000);
        s.auths = vec![dup, dup];
        v.push(s);
    }
    v
}
