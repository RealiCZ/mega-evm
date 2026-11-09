//! Scenarios of group "port": history gas, the history allowance, the reservoir and the forwarding
//! of gas between frames (the other implementation's `history_*`, `reservoir_*`, `limit_matrix`
//! forwarding and a few validation tests that no scenario did yet).
#![allow(dead_code)]
use super::{
    A, Asm, Auth, B, C, D, EMPTY, KEYLESS, LIMIT_CONTROL, Scenario, acct, funded, op, pp::*,
};
use crate::q8::{ACCESS_CONTROL, sel};
use alloy_primitives::{Address, U256, keccak256};

const R_ALLOW: [&str; 4] = ["SATIN-HIST-018", "SATIN-HIST-019", "SATIN-HIST-020", "SATIN-HIST-021"];
const R_HGAS: [&str; 4] = ["SATIN-HIST-009", "SATIN-HIST-010", "SATIN-HIST-013", "SATIN-HIST-014"];
const R_RES: [&str; 4] = ["SATIN-POOL-005", "SATIN-POOL-009", "SATIN-POOL-012", "SATIN-HIST-012"];
const R_FWD: [&str; 3] = ["SATIN-POOL-010", "SATIN-POOL-011", "SATIN-POOL-012"];
const FEE_VAULT: Address =
    Address::new([0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x19]);

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(allowance());
    v.extend(history_gas());
    v.extend(reservoir());
    v.extend(forwarding());
    v.extend(drain());
    v.extend(validation());
    v.extend(misc());
    v
}

/// `LOG3` of `n` bytes of memory from 0 with three zero topics.
fn log3(a: Asm, n: u64) -> Asm {
    a.push(0).push(0).push(0).push(n).push(0).op(op::LOG3)
}

/// `CALL(to, value 1, forward gas)`, its answer dropped.
fn vcall(a: Asm, to: Address, gas: u64) -> Asm {
    call(a, to, 1, gas)
}

fn allowance() -> Vec<Scenario> {
    let mut v = Vec::new();
    // the allowance cannot be spent on computation
    let countdown = spin_loop(Asm::new(), 100).stop().build();
    for g in 296u64..=312 {
        let a = vcall(Asm::new(), B, g).stop().build();
        v.push(scn(
            format!("port_allow_computation_{g}"),
            &R_ALLOW,
            vec![funded(A, a, 1), acct(B, countdown.clone())],
            1_000_000,
        ));
    }
    // an event larger than the allowance pays the rest or runs out of gas
    for len in [100u64, 128, 129, 200, 400] {
        for g in [600u64, 800, 1_000, 1_200, 1_400, 2_000] {
            let b = log3(Asm::new(), len).stop().build();
            let a = vcall(Asm::new(), B, g).stop().build();
            v.push(scn(
                format!("port_allow_big_event_{len}_{g}"),
                &R_ALLOW,
                vec![funded(A, a, 1), acct(B, b)],
                1_000_000,
            ));
        }
    }
    // the allowance cannot be spent on a write record or on state
    for (tag, b, store) in [
        ("write", Asm::new().push(2).push(0).op(op::SSTORE).stop().build(), vec![(0u64, 1u64)]),
        ("state", Asm::new().sstore(1, 1).stop().build(), vec![]),
    ] {
        for g in
            [380_000u64, 420_000, 430_000, 440_000, 450_000, 460_000, 480_000, 500_000, 600_000]
                .into_iter()
        {
            let a = vcall(Asm::new(), B, 1_000_000).stop().build();
            let mut b_acct = acct(B, b.clone());
            b_acct.storage = store.clone();
            v.push(scn(
                format!("port_allow_not_on_{tag}_{g}"),
                &R_ALLOW,
                vec![funded(A, a, 1), b_acct],
                g,
            ));
        }
    }
    // a top-level transfer gets no allowance
    {
        let mut s = sc(
            "port_allow_top_level_transfer",
            &R_ALLOW,
            vec![acct(B, log3(Asm::new(), 32).stop().build())],
            1_000_000,
        );
        s.to = B;
        s.value = U256::from(1u64);
        v.push(s);
    }
    // an intercepted call neither takes nor adds an allowance
    for second in [false, true] {
        let mut b = Asm::new()
            .mstore_selector(sel("isVolatileDataAccessDisabled()"))
            .call_with(op::STATICCALL, ACCESS_CONTROL, 0, 50_000, 4)
            .op(op::POP);
        b = log3(b, 32);
        if second {
            b = log3(b, 32);
        }
        let a = vcall(Asm::new(), B, 5_000).stop().build();
        v.push(scn(
            format!("port_allow_intercepted_{}", if second { "two_logs" } else { "one_log" }),
            &R_ALLOW,
            vec![funded(A, a, 1), acct(B, b.stop().build())],
            1_000_000,
        ));
    }
    // a transaction-level stop does not hand the allowance back
    for (tag, spends) in [("spent", true), ("unspent", false)] {
        let b = if spends { log3(Asm::new(), 32).stop().build() } else { vec![op::STOP] };
        let a = log0(vcall(Asm::new(), B, 5_000), 1_000).stop().build();
        let mut s = scn(
            format!("port_allow_tx_stop_{tag}"),
            &R_ALLOW,
            vec![funded(A, a, 1), acct(B, b)],
            250_000_000,
        );
        s.limits.data = Some(310 + 400);
        v.push(s);
    }
    // an unspent allowance does not return to the caller
    for (tag, b) in [("code_stop", Some(vec![op::STOP])), ("no_code", None)] {
        let a = vcall(Asm::new(), B, 0).stop().build();
        let mut accts = vec![funded(A, a, 1)];
        if let Some(c) = b {
            accts.push(acct(B, c));
        } else {
            accts.push(funded(B, vec![], 1));
        }
        v.push(scn(format!("port_allow_unspent_{tag}"), &R_ALLOW, accts, 1_000_000));
    }
    // a chain of transfers grants each frame its own allowance
    {
        let c = log3(Asm::new(), 32).stop().build();
        let b = vcall(log3(Asm::new(), 32), C, 0).stop().build();
        let a = vcall(Asm::new(), B, 0).stop().build();
        v.push(scn(
            "port_allow_chain".to_string(),
            &R_ALLOW,
            vec![funded(A, a, 1), funded(B, b, 1), acct(C, c)],
            1_000_000,
        ));
    }
    // the gap the allowance opens between bytes and gas
    {
        let main = {
            let mut x = Asm::new();
            for _ in 0..32 {
                x = x.call_with(op::CALL, A, 1, 100_000, 1).op(op::POP);
            }
            x.stop()
        };
        let code = branch_on_data(main, log3(Asm::new(), 32).stop());
        v.push(scn(
            "port_allow_gap_bytes_gas".to_string(),
            &R_ALLOW,
            vec![funded(A, code, 1_000_000_000)],
            5_000_000,
        ));
    }
    // a drained allowance is not refunded by a revert
    {
        let b = log3(Asm::new(), 32).revert0().build();
        let a = vcall(Asm::new(), B, 0).stop().build();
        v.push(scn(
            "port_allow_drained_revert".to_string(),
            &R_ALLOW,
            vec![funded(A, a, 1), acct(B, b)],
            1_000_000,
        ));
    }
    v
}

fn history_gas() -> Vec<Scenario> {
    let mut v = Vec::new();
    // a reservoir that runs out changes only who paid
    {
        let creator = {
            let init = crate::init_returning(32);
            Asm::new()
                .sstore(1, 1)
                .push(32)
                .push(0)
                .op(op::LOG0)
                .call_with(op::CALL, EMPTY, 1, 0, 0)
                .op(op::POP)
                .create(&init, 0)
                .op(op::POP)
                .stop()
                .build()
        };
        for r in [0u64, 100_000, 300_000, 1_000_000, 5_000_000, 20_000_000] {
            let mut s = scn(
                format!("port_hbytes_reservoir_{r}"),
                &R_RES,
                vec![funded(A, creator.clone(), 5)],
                200_000_000 + r,
            );
            s.data = vec![0xcc; 10];
            v.push(s);
        }
    }
    // a call whose caller cannot pay its records starts no frame
    for g in [350_000u64, 400_000, 420_000, 440_000, 450_000, 460_000, 470_000, 480_000, 500_000] {
        let a = call_all(Asm::new(), B, 1).stop().build();
        v.push(scn(
            format!("port_hgas_call_cannot_pay_{g}"),
            &R_HGAS,
            vec![funded(A, a, 1), acct(B, vec![op::STOP])],
            g,
        ));
        let a = Asm::new().create(&[], 1).op(op::POP).stop().build();
        v.push(scn(format!("port_hgas_create_cannot_pay_{g}"), &R_HGAS, vec![funded(A, a, 1)], g));
    }
    // a creation onto an account the transaction changed charges its creator one record
    {
        let callee_code =
            Asm::new().push(0).push(0).push(0).push(0).op(op::CREATE2).op(op::POP).stop().build();
        let x = B.create2(alloy_primitives::B256::ZERO, keccak256([]));
        for g in [60_000u64, 120_000, 200_000, 352_000, 360_000, 450_000] {
            let c = {
                let a = call_all(Asm::new(), x, 1);
                call(a, B, 0, g).stop().build()
            };
            v.push(scn(
                format!("port_hgas_create_onto_changed_{g}"),
                &R_HGAS,
                vec![funded(A, c, 1), acct(B, callee_code.clone())],
                5_000_000,
            ));
        }
    }
    // every call kind charges its records after its forward
    for (tag, kind) in [
        ("call", op::CALL),
        ("callcode", op::CALLCODE),
        ("delegatecall", op::DELEGATECALL),
        ("staticcall", op::STATICCALL),
    ] {
        for g in [10_000u64, 50_000, 100_000, 300_000, 352_000, 400_000] {
            for (cap, gas) in [("below", 5_000_000u64), ("above", 300_000_000)] {
                let contract = {
                    let x = Asm::new()
                        .call_with(
                            kind,
                            D,
                            if kind == op::DELEGATECALL || kind == op::STATICCALL { 0 } else { 1 },
                            20_000,
                            0,
                        )
                        .op(op::POP);
                    x.stop().build()
                };
                let callee = call_rec_gas(Asm::new(), C, g, 1).stop().build();
                v.push(scn(
                    format!("port_hgas_call_kind_{tag}_{g}_{cap}"),
                    &R_HGAS,
                    vec![acct(A, callee), funded(C, contract, 5), funded(D, vec![op::STOP], 5)],
                    gas,
                ));
            }
        }
    }
    // no gas limit buys a write record for nothing
    for (cap, base) in [("below", 0u64), ("above", 200_000_000)] {
        for d in [-6i64, -4, -2, -1, 0, 1, 3, 6] {
            for r in ["short", "ample"] {
                let b = Asm::new().sstore(1, 1).stop().build();
                let a = call_all(Asm::new(), B, 1).stop().build();
                let g = base as i64 +
                    352_000 +
                    d * 1_000 +
                    if r == "ample" && base > 0 { 5_000_000 } else { 0 };
                v.push(scn(
                    format!("port_hgas_no_free_record_{cap}_{}_{r}", d + 6),
                    &R_HGAS,
                    vec![funded(A, a, 1), acct(B, b)],
                    g as u64,
                ));
            }
        }
    }
    // a beneficiary that is a fee vault changes nothing
    for (tag, ben) in [("vault", FEE_VAULT), ("other", C)] {
        let a = call_all(Asm::new(), B, 1).stop().build();
        let mut s = scn(
            format!("port_hgas_fee_vault_beneficiary_{tag}"),
            &R_HGAS,
            vec![funded(A, a, 1), funded(B, vec![], 0)],
            1_000_000,
        );
        s.gas_price = 1;
        s.beneficiary = ben;
        v.push(s);
    }
    v
}

fn call_rec_gas(a: Asm, to: Address, gas: u64, slot: u64) -> Asm {
    a.call_with(op::CALL, to, 0, gas, 0).record_call(slot)
}

fn reservoir() -> Vec<Scenario> {
    let mut v = Vec::new();
    // a halting child gives its records back as a reverting one does
    for (kind, b) in [("halts", vec![op::INVALID]), ("reverts", Asm::new().revert0().build())] {
        for gas in [300_000_000u64, 50_000_000] {
            let a = vcall(Asm::new(), B, 100_000).stop().build();
            v.push(scn(
                format!("port_res_child_{kind}_{gas}"),
                &R_RES,
                vec![funded(A, a, 1), acct(B, b.clone())],
                gas,
            ));
        }
    }
    // an intercepted value call gives its records back once
    for to in [LIMIT_CONTROL, ACCESS_CONTROL] {
        let a = Asm::new()
            .mstore_selector(sel("remainingComputeGas()"))
            .call_with(op::CALL, to, 1, 100_000, 4)
            .record_call(1)
            .stop()
            .build();
        v.push(scn(
            if to == LIMIT_CONTROL {
                "port_res_intercepted_value_call_limit"
            } else {
                "port_res_intercepted_value_call_access"
            }
            .to_string(),
            &R_RES,
            vec![funded(A, a, 1)],
            300_000_000,
        ));
    }
    // an out of gas before the first frame keeps the body the reservoir paid
    for m in [300_000_000u64 / 183_600 + 2, 3, 100] {
        let mut s =
            scn(format!("port_res_oog_before_first_frame_{m}"), &R_RES, vec![], 300_000_000);
        s.to = EMPTY;
        s.value = U256::from(1u64);
        s.salt = vec![(EMPTY, None, m)];
        v.push(s);
    }
    v
}

fn forwarding() -> Vec<Scenario> {
    let mut v = Vec::new();
    let body_spill = 200_000_000u64 + 4_896_000;
    // a reverted child's share unwinds the caller's spill first
    {
        let a = call_all(call_all(Asm::new(), B, 0), C, 0).stop().build();
        v.push(scn(
            "port_fwd_reverted_child_unwinds_spill".to_string(),
            &R_FWD,
            vec![acct(A, a), acct(B, fresh(50)), acct(C, Asm::new().revert0().build())],
            body_spill + 100_000,
        ));
    }
    // a one-gas call still forwards the reservoir
    {
        let b = Asm::new().op(op::GAS).push(0).op(op::SSTORE).stop().build();
        for value in [0u64, 1] {
            let a = Asm::new().call_with(op::CALL, B, value, 1, 0).op(op::POP).stop().build();
            v.push(scn(
                format!("port_fwd_one_gas_call_value{value}"),
                &R_FWD,
                vec![funded(A, a, 1), acct(B, b.clone())],
                300_000_000,
            ));
        }
    }
    // a call with 1000 / 99 / 50 gas left forwards 980 / 98 / 49
    for (tag, base) in [("1000", 1_130u64), ("99", 230), ("50", 180)] {
        for d in 0u64..10 {
            let child = call_rec_gas(Asm::new(), D, u64::MAX >> 1, 1).stop().build();
            let child = {
                let _ = child;
                Asm::new().call_all(op::CALL, D, 0).stop().build()
            };
            let a = call_rec_gas(Asm::new(), B, base + d, 1).stop().build();
            v.push(scn(
                format!("port_fwd_left_{tag}_{d}"),
                &R_FWD,
                vec![acct(A, a), acct(B, child), funded(D, vec![op::STOP], 1)],
                300_000_000,
            ));
        }
    }
    // a value call with 10,000 left forwards 12,100
    for d in 0u64..12 {
        let child = Asm::new().call_all(op::CALL, D, 1).stop().build();
        let a = call_rec_gas(Asm::new(), B, 21_450 + d * 10, 1).stop().build();
        v.push(scn(
            format!("port_fwd_value_left_{d}"),
            &R_FWD,
            vec![acct(A, a), funded(B, child, 5), funded(D, vec![op::STOP], 1)],
            300_000_000,
        ));
    }
    // a reservoir of 1 and a frame budget of 1
    {
        let b = call_all(writes(Asm::new(), 0, 49), C, 0).stop().build();
        let mut s = sc(
            "port_fwd_reservoir_one_budget_one",
            &R_FWD,
            vec![acct(A, call_all(Asm::new(), B, 0).stop().build()), acct(B, b), acct(C, fresh(1))],
            200_000_000 + 40_000 + 1,
        );
        s.limits.data = Some(310 + 2_002);
        v.push(s);
    }
    // a returned reservoir unwinds the caller's spill
    {
        let b = Asm::new().sstore(0, 1).sstore(0, 0).stop().build();
        v.push(scn(
            "port_fwd_returned_reservoir_unwinds_spill".to_string(),
            &R_FWD,
            vec![acct(A, call_all(Asm::new(), B, 0).stop().build()), acct(B, b)],
            5_000_000,
        ));
    }
    v
}

fn fresh(n: u64) -> Vec<u8> {
    writes(Asm::new(), 0, n).stop().build()
}

fn drain() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-POOL-010", "SATIN-POOL-011", "SATIN-LIM-003"];
    // a log-heavy callee: LOG0 with data in a loop, a greedy LOG4 loop, an INVALID
    let heavy = |data: u64| endless(log0(Asm::new(), data)).build();
    let greedy =
        endless(Asm::new().push(0).push(0).push(0).push(0).push(0).push(0).op(0xa4)).build();
    let callees: Vec<(&'static str, Vec<u8>)> = vec![
        ("log_data", heavy(1_000)),
        ("log4_greedy", greedy),
        ("invalid", vec![op::INVALID]),
        ("writer", endless(write_loop(Asm::new(), 0, 3)).build()),
    ];
    for (tag, callee) in &callees {
        for fwd in [10_000u64, 100_000, 1_000_000, 0] {
            for (gtag, gas) in [("400m", 400_000_000u64), ("200m", 200_000_000)] {
                let a = {
                    let mut x = Asm::new().op(op::GAS).push(0).op(op::SSTORE);
                    x = if fwd == 0 {
                        x.call_all(op::CALL, B, 0)
                    } else {
                        x.call_with(op::CALL, B, 0, fwd, 0)
                    };
                    x.record_call(2).op(op::GAS).push(1).op(op::SSTORE);
                    let x = write_loop(
                        Asm::new()
                            .op(op::GAS)
                            .push(0)
                            .op(op::SSTORE)
                            .call_with(op::CALL, B, 0, if fwd == 0 { 300_000_000 } else { fwd }, 0)
                            .record_call(2)
                            .op(op::GAS)
                            .push(1)
                            .op(op::SSTORE),
                        100,
                        30,
                    );
                    x.stop().build()
                };
                let mut s = scn(
                    format!("port_drain_{tag}_{fwd}_{gtag}"),
                    &rules,
                    vec![acct(A, a), acct(B, callee.clone())],
                    gas,
                );
                s.limits.growth = Some(u64::MAX);
                v.push(s);
            }
        }
    }
    // the frame budget binds a callee once the reservoir outgrows it
    for gas in
        [400_000_000u64, 800_000_000, 1_353_400_536, 1_353_400_537, 1_353_400_538, 2_000_000_000]
    {
        let a = write_loop(call_rec_gas(Asm::new(), B, 300_000_000_000, 1), 100, 30).stop().build();
        let a = {
            let _ = a;
            write_loop(Asm::new().call_all(op::CALL, B, 0).record_call(2), 100, 30).stop().build()
        };
        let mut s = scn(
            format!("port_drain_budget_binds_{gas}"),
            &rules,
            vec![acct(A, a), acct(B, heavy(1_000))],
            gas,
        );
        s.limits.growth = Some(u64::MAX);
        v.push(s);
    }
    // a writing callee's growth budget stops it before its share runs out
    for fwd in [10_000u64, 100_000, 1_000_000, 0] {
        let a = if fwd == 0 {
            call_rec(Asm::new(), B, 0, 1)
        } else {
            call_rec_gas(Asm::new(), B, fwd, 1)
        }
        .stop()
        .build();
        let writer = endless(write_loop(Asm::new(), 0, 1_500)).build();
        let writer = {
            let _ = writer;
            write_loop(Asm::new(), 0, 981).stop().build()
        };
        v.push(scn(
            format!("port_drain_growth_budget_{fwd}"),
            &rules,
            vec![acct(A, a), acct(B, writer)],
            400_000_000,
        ));
    }
    // a deploying callee charges the reservoir hundreds of times its forward
    for (kib, fwd) in [(96u64, 100_000u64), (96, 1_000_000), (512, 1_000_000)] {
        let init = Asm::new().push(kib * 1024).push(0).op(op::RETURN).build();
        let c = Asm::new().create(&init, 0).op(op::POP).stop().build();
        let a = write_loop(call_rec_gas(Asm::new(), B, fwd, 1), 100, 30).stop().build();
        v.push(scn(
            format!("port_drain_deploy_{kib}k_{fwd}"),
            &rules,
            vec![acct(A, a), acct(B, c)],
            2_000_000_000,
        ));
    }
    // a caller's reservoir after a child is what it kept with what the child hands back
    {
        let kids: Vec<(&'static str, Vec<u8>)> = vec![
            ("stops", Asm::new().sstore(0, 1).stop().build()),
            ("reverts", Asm::new().sstore(0, 1).revert0().build()),
            ("halts", Asm::new().sstore(0, 1).op(op::INVALID).build()),
            ("past_share", write_loop(Asm::new(), 0, 2_000).stop().build()),
        ];
        let mut a = Asm::new();
        for (i, _) in kids.iter().enumerate() {
            a = call_all(a, [B, C, D, EMPTY][i], 0);
        }
        let mut accts = vec![acct(A, a.stop().build())];
        for (i, (_, code)) in kids.iter().enumerate() {
            accts.push(acct([B, C, D, EMPTY][i], code.clone()));
        }
        let mut s = scn("port_drain_settlement_children".to_string(), &R_RES, accts, 400_000_000);
        s.limits.growth = Some(u64::MAX);
        v.push(s);
    }
    // the regular gas a call asks for does not bound the reservoir it forwards
    for value in [0u64, 1] {
        let b = Asm::new().op(op::GAS).push(0).op(op::SSTORE).stop().build();
        let a = Asm::new().call_with(op::CALL, B, value, 1, 0).op(op::POP).stop().build();
        v.push(scn(
            format!("port_drain_regular_ask_value{value}"),
            &R_RES,
            vec![funded(A, a, 1), acct(B, b)],
            300_000_000,
        ));
    }
    v
}

fn validation() -> Vec<Scenario> {
    let mut v = Vec::new();
    // the execution cap bounds the calldata a transaction may carry (floor at the cap)
    for n in [3_124_765usize, 3_124_766] {
        let mut s = scn(
            format!("port_valid_calldata_cap_{n}"),
            &["SATIN-SCHED-001", "SATIN-HIST-017"],
            vec![acct(B, vec![op::STOP])],
            700_000_000,
        );
        s.to = B;
        s.data = vec![0xff; n];
        s.limits.data = Some(u64::MAX);
        v.push(s);
    }
    // validation bounds the body below the data-size limit
    for n in [3_124_812usize, 3_124_813] {
        let mut s = scn(
            format!("port_valid_body_data_limit_{n}"),
            &["SATIN-SCHED-001", "SATIN-LIM-017"],
            vec![acct(B, vec![op::STOP])],
            700_000_000,
        );
        s.to = B;
        s.data = vec![0xff; n];
        v.push(s);
    }
    for n in [25_587usize, 25_588] {
        let mut s = scn(
            format!("port_valid_auth_list_{n}"),
            &["SATIN-SCHED-001", "SATIN-AUTH-001"],
            vec![acct(B, vec![op::STOP])],
            900_000_000,
        );
        s.to = B;
        s.auths = (0..n)
            .map(|i| {
                let mut a = [0x61u8; 20];
                a[16..].copy_from_slice(&(i as u32).to_be_bytes());
                Auth { chain_id: 1, address: D, nonce: 0, authority: Some(Address::new(a)) }
            })
            .collect();
        v.push(s);
    }
    v
}

fn misc() -> Vec<Scenario> {
    let mut v = Vec::new();
    // every verification failure of the point evaluation burns the forwarded gas
    {
        let mut good = vec![0x01u8; 192];
        good[0] = 0x01;
        let cases: [(&'static str, Vec<u8>); 3] = {
            let mut bad_proof = good.clone();
            bad_proof[191] ^= 1;
            let mut bad_hash = good.clone();
            bad_hash[0] = 0x02;
            [("proof", bad_proof), ("hash", bad_hash), ("short191", good[..191].to_vec())]
        };
        for (tag, input) in cases {
            let a = crate::q8::mem_call(
                Asm::new(),
                op::STATICCALL,
                Address::with_last_byte(0x0a),
                0,
                200_000,
                &input,
            )
            .record_call(1)
            .stop()
            .build();
            v.push(scn(
                format!("port_precompile_kzg_fails_{tag}"),
                &["SATIN-OUT-011", "SATIN-PCMP-001"],
                vec![acct(A, a)],
                1_000_000,
            ));
        }
    }
    // init code costs its calldata tokens and an EIP-3860 word
    for n in [32usize, 33, 63, 64, 65, 96] {
        let mut s = scn(
            format!("port_sched_init_code_{n}"),
            &["SATIN-SCHED-001", "SATIN-SIZE-004"],
            vec![],
            5_000_000,
        );
        s.create = true;
        s.data = vec![0u8; n];
        v.push(s);
    }
    // a create frame rejected for its first byte draws no state gas
    {
        let a = Asm::new().create(&init_first(0xef, 1), 0).op(op::POP).stop().build();
        v.push(scn(
            "port_sched_create_rejected_first_byte".to_string(),
            &["SATIN-STATE-011", "SATIN-SCHED-001"],
            vec![acct(A, a)],
            5_000_000,
        ));
    }
    // CLZ runs on the base spec
    {
        let a = Asm::new().push(1).op(0x1e).push(0).op(op::SSTORE).stop().build();
        v.push(scn(
            "port_sched_clz".to_string(),
            &["SATIN-BASE-004", "SATIN-BASE-001"],
            vec![acct(A, a)],
            1_000_000,
        ));
    }
    // a write through a delegator leaves the delegate's storage alone
    {
        let mut x_code = vec![0xef, 0x01, 0x00];
        x_code.extend_from_slice(D.as_slice());
        let d_code = branch_on_data(
            Asm::new().sstore(1, 1).stop(),
            Asm::new().push(1).op(op::SLOAD).push(0).op(op::MSTORE).push(32).push(0).op(op::RETURN),
        );
        let a = Asm::new()
            .call_all(op::CALL, B, 0)
            .op(op::POP)
            .call_with(op::CALL, D, 0, 100_000, 1)
            .op(op::POP)
            .return_data_to_slot(7)
            .stop()
            .build();
        let mut s = scn(
            "port_state_delegator_write".to_string(),
            &["SATIN-BASE-001", "SATIN-AUTH-003"],
            vec![acct(A, a), acct(B, x_code), acct(D, d_code)],
            1_000_000,
        );
        s.accounts[3].storage = vec![(1, 5)];
        v.push(s);
    }
    let _ = KEYLESS;
    v
}

trait Ext {
    fn return_data_to_slot(self, slot: u64) -> Self;
}

impl Ext for Asm {
    fn return_data_to_slot(self, slot: u64) -> Self {
        self.store_returndata_word(slot)
    }
}
