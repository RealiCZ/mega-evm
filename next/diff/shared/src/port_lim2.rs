//! Scenarios of group "port" (second part of the limit tests): gas against limits at the
//! boundary, creations and deposits that cannot pay, the order in which limits are named, and the
//! synthetic frames' gas above the cap.
#![allow(dead_code)]
use super::{
    A, Asm, Auth, B, C, D, EMPTY, KEYLESS, LIMIT_CONTROL, ORACLE, Scenario, acct, funded, op, pp::*,
};
use crate::q8::{ACCESS_CONTROL, mem_call, sel, send_hint};
use alloy_primitives::{Address, U256};

const R_DATA: [&str; 4] = ["SATIN-LIM-001", "SATIN-LIM-013", "SATIN-STOP-013", "SATIN-OUT-006"];
const X1: Address = Address::new([0x71; 20]);
type Edit = Box<dyn Fn(&mut Scenario)>;
const E: Address = Address::with_last_byte(0xee);

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(binds_first());
    v.extend(deposit_charges());
    v.extend(growth_more());
    v.extend(stops_matrix());
    v.extend(synthetic_frames());
    v
}

fn fresh(n: u64) -> Vec<u8> {
    writes(Asm::new(), 0, n).stop().build()
}

/// The three writes whose gas and data size are compared: a fresh slot, a one-byte log, a
/// `SELFDESTRUCT` of a balance.
fn kinds() -> Vec<(&'static str, Vec<u8>, u64)> {
    vec![
        ("sstore", Asm::new().sstore(0, 1).stop().build(), 40),
        ("log", log0(Asm::new(), 1).stop().build(), 33),
        ("selfdestruct", destruct(Asm::new(), B).build(), 240),
    ]
}

fn binds_first() -> Vec<Scenario> {
    let mut v = Vec::new();
    // whichever of gas and data size binds first is reported, at the top frame and at a child's
    for (kind, body, bytes) in kinds() {
        for (lname, limit) in [("fits", 310 + bytes), ("short", 310 + bytes - 1)] {
            for gas in [50_000u64, 100_000, 152_305, 152_306, 155_825, 155_826, 400_000] {
                let mut s = scn(
                    format!("port_bind_top_{kind}_{lname}_{gas}"),
                    &R_DATA,
                    vec![funded(A, body.clone(), 1)],
                    gas,
                );
                s.limits.data = Some(limit);
                v.push(s);
            }
            for fwd in [60_000u64, 100_000, 153_000, 160_000, 400_000] {
                let a = call_rec(Asm::new(), B, 0, 1).stop().build();
                let mut s = scn(
                    format!("port_bind_child_{kind}_{lname}_{fwd}"),
                    &R_DATA,
                    vec![acct(A, a), funded(B, body.clone(), 1)],
                    fwd + 400_000,
                );
                // the forward is the call's explicit gas
                s.accounts[1].code = call_rec_gas(Asm::new(), B, fwd, 1).stop().build();
                s.limits.data = Some(limit);
                v.push(s);
            }
        }
    }
    // above the cap: a record a limit refuses is not charged where the reservoir has one gas too
    // few
    for (kind, body, bytes) in kinds().into_iter().take(2) {
        for (lname, limit) in [("fits", 310 + 40 + bytes), ("short", 310 + 40 + bytes - 1)] {
            for extra in [0u64, 1, 3_519, 3_520, 101_439, 101_440] {
                let a = call_rec_gas(Asm::new(), B, 1_000_000, 1).stop().build();
                let mut s = scn(
                    format!("port_bind_reservoir_{kind}_{lname}_{extra}"),
                    &R_DATA,
                    vec![acct(A, a), funded(B, body.clone(), 1)],
                    200_000_000 + 40_000 + extra,
                );
                s.limits.data = Some(limit);
                v.push(s);
            }
        }
    }
    v
}

fn call_rec_gas(a: Asm, to: Address, gas: u64, slot: u64) -> Asm {
    a.call_with(op::CALL, to, 0, gas, 0).record_call(slot)
}

fn deposit_charges() -> Vec<Scenario> {
    let mut v = Vec::new();
    // a creation one gas short of the hash, of its state gas, of its history
    let init = Asm::new()
        .push_bytes(&[0x63, 0xff, 0xe0, 0x00])
        .op(op::MLOAD)
        .op(op::POP)
        .push(3_200)
        .push(0)
        .op(op::RETURN)
        .build();
    let b = Asm::new().create(&init, 0).op(op::POP).stop().build();
    let state = 3_200 * 1_530u64;
    let hist = 3_200 * 88u64;
    let rules = ["SATIN-HIST-011", "SATIN-STATE-011", "SATIN-OUT-007"];
    for (tag, gas, base) in [
        ("hash_above_cap", 300_000_000u64, 600u64),
        ("state_below_cap", 10_000_000, 600 + state),
        ("history_below_cap", 10_000_000, 600 + state + hist),
    ] {
        for d in [-2i64, -1, 0, 1] {
            let fwd = (base as i64 + 40_000 + d) as u64;
            let a = call_rec_gas(Asm::new(), B, fwd, 1).stop().build();
            v.push(scn(
                format!("port_depcharge_{tag}_{}", d + 2),
                &rules,
                vec![acct(A, a), acct(B, b.clone())],
                gas,
            ));
        }
    }
    // failed deposits
    let tx_over = |name: &'static str, gas: u64, sender: Address, existing: bool| {
        let mut s = sc(
            name,
            &["SATIN-DEP-004", "SATIN-DEP-005", "SATIN-DEP-006", "SATIN-OUT-005"],
            vec![acct(EMPTY, vec![])],
            gas,
        );
        s.deposit = Some(7);
        s.sender = sender;
        s.to = EMPTY;
        if existing {
            s.accounts.push(funded(sender, vec![], 1_000));
        }
        s
    };
    for gas in [14_999u64, 1_000_000] {
        let mut s = tx_over(
            nm(format!("port_faildep_over_limit_{gas}")),
            gas,
            Address::with_last_byte(0x77),
            false,
        );
        s.limits.data = Some(309);
        v.push(s);
    }
    for (tag, gas) in
        [("short1", 14_999u64), ("exact", 15_000), ("plus1", 15_001), ("ample", 100_000)]
    {
        for (who, existing) in [("absent", false), ("existing", true)] {
            for (lname, growth) in [("zero", Some(0u64)), ("none", None)] {
                let mut s = tx_over(
                    nm(format!("port_faildep_caller_growth_{tag}_{who}_{lname}")),
                    gas,
                    Address::with_last_byte(0x78),
                    existing,
                );
                s.limits.growth = growth;
                v.push(s);
            }
        }
    }
    // a failed deposit keeps the hints it forwarded
    {
        let hint = send_hint(3, &[0x11; 64]);
        let a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 500_000, &hint)
            .op(op::POP)
            .op(op::INVALID)
            .build();
        let mut s = sc(
            "port_faildep_keeps_hints_deposit",
            &["SATIN-DEP-006", "SATIN-ORCL-009"],
            vec![acct(A, a.clone()), acct(ORACLE, vec![op::STOP])],
            1_000_000,
        );
        s.deposit = Some(0);
        v.push(s);
        let mut s = sc(
            "port_faildep_keeps_hints_call",
            &["SATIN-DEP-006", "SATIN-ORCL-009"],
            vec![acct(A, a), acct(ORACLE, vec![op::STOP])],
            1_000_000,
        );
        v.push(s.clone());
        s.name = "port_faildep_keeps_hints_call_data";
        s.limits.data = Some(400);
        v.push(s);
    }
    v
}

fn growth_more() -> Vec<Scenario> {
    use crate::block;
    let mut v = Vec::new();
    let rules = ["SATIN-ACCT-014", "SATIN-AUTH-008", "SATIN-ACCT-015"];
    // an authority an earlier transaction funded is no new account; one it touched is counted as
    // it is charged
    for (name, first_value) in
        [("port_growth_authority_funded", 1u64), ("port_growth_authority_touched", 0)]
    {
        let mut s = sc(name, &rules, vec![], 1_000_000);
        s.to = X1;
        s.value = U256::from(first_value);
        s.limits.growth = Some(1);
        let blk = block(
            s,
            vec![Box::new(|t: &mut Scenario| {
                t.to = A;
                t.value = U256::ZERO;
                t.auths = vec![Auth { chain_id: 1, address: D, nonce: 0, authority: Some(X1) }];
            })],
        );
        v.extend(blk);
    }
    // a state charge the frame cannot pay runs out of gas whatever the limit
    {
        let body: Vec<(&'static str, Vec<u8>, u64, u64)> = vec![
            ("sstore", fresh(1), 0, 0),
            ("selfdestruct", destruct(Asm::new(), Address::with_last_byte(0x99)).build(), 5, 0),
            (
                "value_call",
                call_all(Asm::new(), Address::with_last_byte(0x98), 1).stop().build(),
                5,
                0,
            ),
            ("create", Asm::new().create(&[op::STOP], 1).op(op::POP).stop().build(), 5, 0),
        ];
        for (kind, code, bal, _) in body {
            for gas in [60_000u64, 100_000, 150_000, 152_305, 152_306, 200_000] {
                let mut s = scn(
                    format!("port_growth_cannot_pay_{kind}_{gas}"),
                    &["SATIN-OUT-007", "SATIN-LIM-010", "SATIN-STATE-004"],
                    vec![funded(A, code.clone(), bal)],
                    gas,
                );
                s.limits.data = Some(310);
                v.push(s);
            }
        }
        let mut s = sc(
            "port_growth_cannot_pay_value_tx",
            &["SATIN-OUT-007", "SATIN-STATE-004"],
            vec![],
            60_000,
        );
        s.to = Address::with_last_byte(0x97);
        s.value = U256::from(1u64);
        s.limits.data = Some(310);
        v.push(s);
    }
    // a slot written back to zero falls and makes room
    {
        let a = Asm::new().sstore(1, 1).sstore(1, 0).sstore(2, 1).sstore(3, 1).stop().build();
        for g in [1u64, 2] {
            let mut s = scn(
                format!("port_growth_back_to_zero_limit{g}"),
                &["SATIN-ACCT-016", "SATIN-ACCT-018"],
                vec![acct(A, a.clone())],
                5_000_000,
            );
            s.limits.growth = Some(g);
            v.push(s);
        }
        let b = Asm::new().sstore(1, 0).sstore(4, 1).sstore(5, 1).stop().build();
        let a = Asm::new()
            .sstore(1, 1)
            .call_with(op::DELEGATECALL, B, 0, 3_000_000, 0)
            .op(op::POP)
            .stop()
            .build();
        let mut s = sc(
            "port_growth_child_clears_callers_slot",
            &["SATIN-ACCT-018", "SATIN-LIM-004"],
            vec![acct(A, a), acct(B, b)],
            5_000_000,
        );
        s.limits.growth = Some(2);
        v.push(s);
    }
    // a creation is budgeted after its creator's counts
    for n in [979u64, 980] {
        let init = init_with(n, 0);
        let a = Asm::new().create(&init, 0).op(op::POP).stop().build();
        let mut s = scn(
            format!("port_growth_creation_budget_{n}"),
            &["SATIN-LIM-003", "SATIN-LIM-008"],
            vec![acct(A, a)],
            400_000_000,
        );
        v.push(s.clone());
        s.name = nm(format!("port_growth_creation_budget_{n}_after_writes"));
        s.accounts[1].code =
            write_loop(Asm::new(), 5_000, 20).create(&init, 0).op(op::POP).stop().build();
        v.push(s);
    }
    {
        let mut s = sc(
            "port_growth_creation_tx_999",
            &["SATIN-LIM-003", "SATIN-LIM-008"],
            vec![],
            400_000_000,
        );
        s.create = true;
        s.data = init_with(999, 0);
        v.push(s.clone());
        s.name = "port_growth_creation_tx_1000";
        s.data = init_with(1_000, 0);
        v.push(s);
    }
    // a slot crossing several limits stops on the first in order
    {
        let one = fresh(1);
        let cases: [(&str, Edit); 6] = [
            (
                "data_and_growth",
                Box::new(|s| {
                    s.limits.data = Some(349);
                    s.limits.growth = Some(0);
                }),
            ),
            (
                "kv_and_growth",
                Box::new(|s| {
                    s.limits.kv = Some(0);
                    s.limits.growth = Some(0);
                }),
            ),
            (
                "detention_and_growth",
                Box::new(|s| {
                    s.limits.block_env_cap = Some(1_000);
                    s.limits.growth = Some(0);
                    s.accounts[1].code =
                        Asm::new().op(op::TIMESTAMP).op(op::POP).sstore(0, 1).stop().build();
                }),
            ),
            (
                "data_and_kv",
                Box::new(|s| {
                    s.limits.data = Some(349);
                    s.limits.kv = Some(0);
                }),
            ),
            (
                "all_three",
                Box::new(|s| {
                    s.limits.data = Some(349);
                    s.limits.kv = Some(0);
                    s.limits.growth = Some(0);
                }),
            ),
            ("none", Box::new(|_| {})),
        ];
        for (tag, edit) in cases {
            let mut s = scn(
                format!("port_growth_first_in_order_{tag}"),
                &["SATIN-LIM-011", "SATIN-STOP-018"],
                vec![acct(A, one.clone())],
                300_000_000,
            );
            edit(&mut s);
            v.push(s);
        }
    }
    // a deposit's new caller is counted before its first frame
    for (tag, n, g) in [("one_slot", 1u64, 2u64), ("two_slots", 2, 2), ("three_slots", 3, 2)] {
        let mut s = scn(
            format!("port_growth_deposit_new_caller_{tag}"),
            &["SATIN-ACCT-020", "SATIN-DEP-003"],
            vec![acct(A, fresh(n))],
            5_000_000,
        );
        s.deposit = Some(0);
        s.sender = Address::with_last_byte(0x79);
        s.limits.growth = Some(g);
        v.push(s);
    }
    // a deposit's failed first answer is held to no limit
    {
        let mut bad = vec![0u8; 128];
        bad[31] = 1;
        bad[63] = 1;
        let mut s = sc(
            "port_growth_deposit_failed_answer",
            &["SATIN-DEP-004", "SATIN-LIM-014"],
            vec![],
            1_000_000,
        );
        s.deposit = Some(0);
        s.sender = Address::with_last_byte(0x7a);
        s.to = Address::with_last_byte(6);
        s.data = bad;
        s.limits.growth = Some(0);
        v.push(s);
    }
    v
}

/// The matrix of the stop: every limit, at the transaction's frame and at depth 3, below and
/// above the execution cap; and the nested stops that leave the callers running.
fn stops_matrix() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-STOP-001", "SATIN-STOP-002", "SATIN-STOP-004", "SATIN-STOP-005"];
    // A -> B -> C -> D, each logs, A writes 2 slots; the frame at depth 0 or 3 crosses a limit
    for (tier, gas) in [("below", 50_000_000u64), ("above", 300_000_000)] {
        for depth in [0usize, 3] {
            for dim in ["data", "kv", "growth", "compute"] {
                let mk = |me: usize, next: Option<Address>| {
                    let mut a = Asm::new();
                    if me == 0 {
                        a = a.sstore(10, 1).sstore(11, 1);
                    }
                    a = log0(a, 1);
                    if me == depth && dim == "compute" {
                        a = endless(a.op(op::TIMESTAMP).op(op::POP));
                        return a.build();
                    }
                    if me == depth && dim != "compute" {
                        a = a.sstore(12, 1);
                    }
                    if let Some(n) = next {
                        a = call_all(a, n, 0);
                    }
                    a.stop().build()
                };
                let mut s = scn(
                    format!("port_stopmx_{dim}_depth{depth}_{tier}"),
                    &rules,
                    vec![
                        acct(A, mk(0, Some(B))),
                        acct(B, mk(1, Some(C))),
                        acct(C, mk(2, Some(D))),
                        acct(D, mk(3, None)),
                    ],
                    gas,
                );
                // one short of the totals the chain makes before the crossing slot
                match dim {
                    "data" => s.limits.data = Some(310 + 2 * 40 + 4 * 33 + 40 - 1),
                    "kv" => s.limits.kv = Some(2 + 1 - 1),
                    "growth" => s.limits.growth = Some(2),
                    _ => s.limits.block_env_cap = Some(20_000),
                }
                v.push(s);
            }
        }
    }
    // a frame budget crossed three calls down reverts that frame alone
    for (tier, gas) in [("below", 50_000_000u64), ("above", 300_000_000)] {
        let d = write_loop(Asm::new(), 0, 30).stop().build();
        let mid = |next: Address| call_all(Asm::new(), next, 0).sstore(99, 1).stop().build();
        let mut s = scn(
            format!("port_stopmx_budget_depth3_{tier}"),
            &rules,
            vec![acct(A, mid(B)), acct(B, mid(C)), acct(C, mid(D)), acct(D, d)],
            gas,
        );
        s.limits.data = Some(310 + 6 * 40);
        v.push(s);
    }
    // a halt three calls down burns its frame's gas and its caller resumes
    {
        let halts: Vec<(&'static str, Vec<u8>)> = vec![
            ("loop", endless(Asm::new()).build()),
            (
                "ecrecover_short",
                Asm::new()
                    .call_with(op::CALL, Address::with_last_byte(1), 0, 2_000, 0)
                    .op(op::POP)
                    .op(op::INVALID)
                    .build(),
            ),
            (
                "modexp_short",
                Asm::new()
                    .call_with(op::CALL, Address::with_last_byte(5), 0, 100, 0)
                    .op(op::POP)
                    .op(op::INVALID)
                    .build(),
            ),
        ];
        for (kind, d) in halts {
            for fwd in [200_000u64, 200_050] {
                let mid =
                    |next: Address, f: u64| call_rec_gas(Asm::new(), next, f, 1).stop().build();
                let s = scn(
                    format!("port_stopmx_halt_depth3_{kind}_{fwd}"),
                    &["SATIN-OUT-007", "SATIN-OUT-011", "SATIN-STOP-008"],
                    vec![
                        acct(A, mid(B, 5_000_000)),
                        acct(B, mid(C, 1_000_000)),
                        acct(C, mid(D, fwd)),
                        acct(D, d.clone()),
                    ],
                    20_000_000,
                );
                let mut s = s;
                s.limits = lim(Some(100_000), Some(100), Some(100));
                v.push(s);
            }
        }
    }
    // a data-limit exceed in a nested call; a state revert when exceeding
    {
        let a = call_all(Asm::new(), B, 0).stop().build();
        let b = Asm::new().push(1).push(0).op(op::SLOAD).op(op::POP).sstore(0, 1).stop().build();
        let mut s = sc("port_stopmx_nested_data", &rules, vec![acct(A, a), acct(B, b)], 1_000_000);
        s.limits.data = Some(311);
        v.push(s);
        let mut s = sc("port_stopmx_value_start_cross", &rules, vec![acct(B, fresh(1))], 1_000_000);
        s.to = B;
        s.value = U256::from(1u64);
        s.limits.data = Some(311);
        v.push(s);
    }
    // detention plus intrinsic data size overflow: refused at validation all the same
    {
        let mut s = sc(
            "port_stopmx_detained_body_over",
            &["SATIN-STOP-003", "SATIN-DET-008"],
            vec![],
            1_000_000,
        );
        s.to = EMPTY;
        s.value = U256::from(1u64);
        s.beneficiary = crate::SENDER;
        s.limits.data = Some(309);
        v.push(s);
    }
    v
}

fn synthetic_frames() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-STOP-014", "SATIN-STOP-013", "SATIN-OUT-009"];
    // a stopped first frame keeps the reservoir at any gas limit
    for gas in [10_000_000u64, 300_000_000] {
        let mut s = scn(
            format!("port_synth_stopped_first_frame_{gas}"),
            &rules,
            vec![acct(B, fresh(1))],
            gas,
        );
        s.to = B;
        s.value = U256::from(1u64);
        s.limits.data = Some(310 + 39);
        v.push(s);
    }
    // an intercepted answer keeps the reservoir; an intercepted refusal keeps it too
    let methods: [(&str, Address, &str); 4] = [
        ("remaining", LIMIT_CONTROL, "remainingComputeGas()"),
        ("disable", ACCESS_CONTROL, "disableVolatileDataAccess()"),
        ("enable", ACCESS_CONTROL, "enableVolatileDataAccess()"),
        ("query", ACCESS_CONTROL, "isVolatileDataAccessDisabled()"),
    ];
    for (tag, to, sig) in methods {
        for gas in [1_000_000u64, 300_000_000] {
            let mut s = scn(format!("port_synth_intercepted_{tag}_{gas}"), &rules, vec![], gas);
            s.to = to;
            s.data = sel(sig).to_vec();
            v.push(s);
            let mut s = scn(format!("port_synth_refused_{tag}_{gas}"), &rules, vec![], gas);
            s.to = to;
            s.data = sel(sig).to_vec();
            s.value = U256::from(1u64);
            v.push(s);
        }
    }
    // a forwarded hint keeps the reservoir (a top-level sendHint above the cap)
    {
        let mut s = sc(
            "port_synth_direct_hint_300m",
            &rules,
            vec![acct(ORACLE, vec![op::STOP])],
            300_000_000,
        );
        s.to = ORACLE;
        s.data = send_hint(1, &[0x22; 32]);
        v.push(s);
    }
    // the keyless value refusal keeps the reservoir
    {
        let mut s = sc("port_synth_keyless_value_refused_300m", &rules, vec![], 300_000_000);
        s.to = KEYLESS;
        s.value = U256::from(1u64);
        s.data = crate::keyless_calldata(0);
        v.push(s);
    }
    v
}
