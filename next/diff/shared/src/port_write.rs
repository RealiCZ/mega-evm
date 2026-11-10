//! Scenarios of group "port" and "blk": write records, transfer logs, the destruction table and
//! SALT pricing (the other implementation's `write_records`, `transfer_logs`, `limit_matrix`
//! destruction, `salt*` tests that no scenario did yet).
#![allow(dead_code)]
use super::{A, Asm, B, C, D, EMPTY, IDENTITY, Scenario, acct, block, funded, op, pp::*, sender};
use alloy_primitives::{Address, U256};

const R_REC: [&str; 4] = ["SATIN-ACCT-004", "SATIN-ACCT-005", "SATIN-ACCT-006", "SATIN-LOG-001"];
const R_SD: [&str; 4] = ["SATIN-SD-002", "SATIN-SD-004", "SATIN-SD-009", "SATIN-HIST-007"];
const R_SALT: [&str; 3] = ["SATIN-STATE-001", "SATIN-STATE-002", "SATIN-STATE-004"];
const HEIR: Address = Address::with_last_byte(0xe5);
const NEWB: Address = Address::with_last_byte(0xe6);
const SENDER_ADDR: Address = crate::SENDER;
const VAULTS: [Address; 4] = [
    Address::new([0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x11]),
    Address::new([0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x19]),
    Address::new([0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1a]),
    Address::new([0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x1b]),
];

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(records());
    v.extend(transfer_logs());
    v.extend(destruction());
    v.extend(destruction_blocks());
    v.extend(salt());
    v
}

fn vc(a: Asm, to: Address, value: u64) -> Asm {
    call_all(a, to, value)
}

/// `CALL(GAS, ADDRESS, value)`: a value call to the calling account itself.
fn self_call(a: Asm, value: u64) -> Asm {
    a.push(0)
        .push(0)
        .push(0)
        .push(0)
        .push(value)
        .op(op::ADDRESS)
        .op(op::GAS)
        .op(op::CALL)
        .op(op::POP)
}

fn records() -> Vec<Scenario> {
    let mut v = Vec::new();
    // value calls to one recipient record it once
    {
        let a = vc(vc(vc(Asm::new(), B, 1), B, 1), B, 1).stop().build();
        v.push(scn(
            "port_rec_three_value_calls".to_string(),
            &R_REC,
            vec![funded(A, a, 10), acct(B, vec![])],
            5_000_000,
        ));
    }
    // a transfer and its exact return take both records back; a partial return keeps both
    for (tag, sent, returned) in [("exact", 1u64, 1u64), ("partial", 2, 1)] {
        let b = call_caller_value(Asm::new(), returned).op(op::POP).stop().build();
        let a = vc(Asm::new(), B, sent).stop().build();
        v.push(scn(
            format!("port_rec_return_{tag}"),
            &R_REC,
            vec![funded(A, a, 10), funded(B, b, 0)],
            5_000_000,
        ));
    }
    // a child that reverts after writing keeps nothing
    for (tag, b_reverts_after_return) in [("child_reverts", false), ("return_reverts", true)] {
        let b = if b_reverts_after_return {
            // the value is passed on inside a frame that reverts (D), B itself succeeds
            call_all(Asm::new(), D, 0).stop().build()
        } else {
            vc(Asm::new().sstore(0, 1), C, 1).revert0().build()
        };
        let a = vc(Asm::new(), B, 1).stop().build();
        v.push(scn(
            format!("port_rec_{tag}"),
            &R_REC,
            vec![
                funded(A, a, 10),
                funded(B, b, 5),
                acct(C, vec![]),
                funded(D, vc(Asm::new(), C, 1).revert0().build(), 5),
            ],
            5_000_000,
        ));
    }
    // nested creations record each account once
    {
        let y_init = vec![op::STOP];
        let x_init = Asm::new().create(&y_init, 0).op(op::POP).stop().build();
        let a = Asm::new().create(&x_init, 0).op(op::POP).stop().build();
        v.push(scn("port_rec_nested_creations".to_string(), &R_REC, vec![acct(A, a)], 5_000_000));
    }
    // a value call to self records nothing
    v.push(scn(
        "port_rec_value_call_to_self".to_string(),
        &R_REC,
        vec![funded(A, self_call(Asm::new(), 1).stop().build(), 10)],
        5_000_000,
    ));
    // a transfer to the sender and to a fee account
    for (tag, to) in
        [("sender", SENDER_ADDR), ("base_fee_vault", VAULTS[1]), ("l1_vault", VAULTS[2])]
    {
        v.push(scn(
            format!("port_rec_transfer_to_{tag}"),
            &R_REC,
            vec![funded(A, vc(Asm::new(), to, 1).stop().build(), 10)],
            5_000_000,
        ));
    }
    // a failed first child lets the next transfer record the sender
    {
        let a = vc(vc(Asm::new(), B, 1), C, 1).stop().build();
        v.push(scn(
            "port_rec_failed_first_child".to_string(),
            &R_REC,
            vec![funded(A, a, 10), acct(B, Asm::new().revert0().build()), acct(C, vec![])],
            5_000_000,
        ));
    }
    // a delegate's record of its caller's account follows the delegate
    for (tag, revert) in [("succeeds", false), ("reverts", true)] {
        let lib = if revert {
            vc(Asm::new(), D, 1).revert0().build()
        } else {
            vc(Asm::new(), D, 1).stop().build()
        };
        let a = Asm::new().call_with(op::DELEGATECALL, B, 0, 2_000_000, 0).op(op::POP);
        let a = vc(a, D, 1).stop().build();
        v.push(scn(
            format!("port_rec_delegate_follows_{tag}"),
            &R_REC,
            vec![funded(A, a, 10), acct(B, lib), acct(D, vec![])],
            5_000_000,
        ));
    }
    // a reverted then retried create records the creator once
    {
        let a = Asm::new()
            .create(&[op::PUSH0, op::PUSH0, op::REVERT], 0)
            .op(op::POP)
            .create(&[op::PUSH0, op::PUSH0, op::REVERT], 0)
            .op(op::POP)
            .stop()
            .build();
        v.push(scn("port_rec_retried_create".to_string(), &R_REC, vec![acct(A, a)], 5_000_000));
    }
    // a nested self-transfer after a recorded sender counts nothing
    {
        let a = self_call(vc(Asm::new(), B, 1), 1).stop().build();
        v.push(scn(
            "port_rec_self_after_recorded".to_string(),
            &R_REC,
            vec![funded(A, a, 10), acct(B, vec![])],
            5_000_000,
        ));
    }
    // execution writes to the fee accounts are records
    {
        let mut a = Asm::new();
        for vault in VAULTS.iter().copied().chain([crate::SENDER]) {
            a = vc(a, vault, 1);
        }
        let a = destruct(a, VAULTS[0]).build();
        let mut s = scn(
            "port_rec_fee_accounts_gas_price".to_string(),
            &R_REC,
            vec![funded(A, a.clone(), 10_000_000)],
            5_000_000,
        );
        s.gas_price = 10;
        s.beneficiary = C;
        v.push(s);
        let mut s = scn(
            "port_rec_fee_accounts_beneficiary".to_string(),
            &R_REC,
            vec![funded(A, destruct(vc(Asm::new(), C, 1), C).build(), 10_000_000)],
            5_000_000,
        );
        s.gas_price = 10;
        s.beneficiary = C;
        v.push(s);
    }
    // an SSTORE out of gas after the write counts nothing
    for g in [100_000u64, 120_000, 140_000, 152_000, 152_305] {
        v.push(scn(
            format!("port_rec_sstore_oog_{g}"),
            &R_REC,
            vec![acct(A, Asm::new().sstore(1, 1).stop().build())],
            g,
        ));
    }
    // frames running as the sender do not record it
    {
        let mut deleg = vec![0xef, 0x01, 0x00];
        deleg.extend_from_slice(D.as_slice());
        // the delegate: with calldata, write value/state; without, call B (which calls back)
        let d_code =
            branch_on_data(call_all(Asm::new(), B, 0).stop(), Asm::new().sstore(1, 1).stop());
        let b =
            Asm::new().call_with(op::CALL, SENDER_ADDR, 1, 2_000_000, 1).op(op::POP).stop().build();
        let mut s = scn(
            "port_rec_frames_as_sender".to_string(),
            &R_REC,
            vec![acct(B, b), acct(D, d_code)],
            5_000_000,
        );
        s.accounts[0] = funded(SENDER_ADDR, deleg, 1_000_000_000_000_000_000);
        s.to = SENDER_ADDR;
        v.push(s);
        // a top-level self call counts the sender as recorded
        let d_code = vc(Asm::new(), B, 1).stop().build();
        let mut deleg = vec![0xef, 0x01, 0x00];
        deleg.extend_from_slice(D.as_slice());
        let mut s = scn(
            "port_rec_top_level_self_call".to_string(),
            &R_REC,
            vec![acct(B, vec![]), funded(D, d_code, 0)],
            5_000_000,
        );
        s.accounts[0] = funded(SENDER_ADDR, deleg, 1_000_000_000_000_000_000);
        s.to = SENDER_ADDR;
        v.push(s);
    }
    // authority records are held to the limit at validation
    for (tag, limit, twice) in [
        ("fits", 451u64, false),
        ("one_short", 450, false),
        ("twice_fits", 491, true),
        ("twice_short", 490, true),
    ] {
        let x = Address::new([0x73; 20]);
        let auth = crate::Auth { chain_id: 1, address: D, nonce: 0, authority: Some(x) };
        let mut s = scn(
            format!("port_rec_authority_limit_{tag}"),
            &["SATIN-AUTH-008", "SATIN-AUTH-010", "SATIN-ACCT-009"],
            vec![acct(A, vec![op::STOP])],
            2_000_000,
        );
        s.auths = if twice { vec![auth, crate::Auth { nonce: 1, ..auth }] } else { vec![auth] };
        s.limits.data = Some(limit);
        v.push(s);
    }
    // a value-receiving child records itself once; a zero-value child starts unrecorded
    {
        let b = vc(Asm::new(), C, 1).stop().build();
        let a = vc(Asm::new(), B, 1).stop().build();
        v.push(scn(
            "port_rec_value_passed_on".to_string(),
            &R_REC,
            vec![funded(A, a, 10), funded(B, b, 0), acct(C, vec![])],
            5_000_000,
        ));
        let a = vc(vc(Asm::new(), EMPTY, 1), B, 0).stop().build();
        let b = vc(Asm::new(), C, 1).stop().build();
        v.push(scn(
            "port_rec_zero_value_child".to_string(),
            &R_REC,
            vec![funded(A, a, 10), funded(B, b, 5), acct(C, vec![])],
            5_000_000,
        ));
    }
    // a delegatecall child inherits the frame account's record
    {
        let d_code = call_caller_value(Asm::new(), 0).op(op::POP).stop().build();
        let d_code = {
            let _ = d_code;
            vc(Asm::new(), C, 1).stop().build()
        };
        let mut s = scn(
            "port_rec_delegatecall_inherits".to_string(),
            &R_REC,
            vec![
                funded(
                    A,
                    Asm::new()
                        .call_with(op::DELEGATECALL, D, 0, 1_000_000, 0)
                        .op(op::POP)
                        .stop()
                        .build(),
                    0,
                ),
                acct(D, d_code),
                acct(C, vec![]),
            ],
            5_000_000,
        );
        s.value = U256::from(5u64);
        v.push(s);
        // a created frame's delegatecall inherits the created account's record
        let init =
            Asm::new().call_with(op::DELEGATECALL, D, 0, 1_000_000, 0).op(op::POP).stop().build();
        let a = Asm::new().create(&init, 3).op(op::POP).stop().build();
        v.push(scn(
            "port_rec_created_frame_delegatecall".to_string(),
            &R_REC,
            vec![funded(A, a, 10), acct(D, vc(Asm::new(), C, 1).stop().build()), acct(C, vec![])],
            5_000_000,
        ));
    }
    v
}

fn transfer_logs() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-LOG-001", "SATIN-LOG-006", "SATIN-LIM-013", "SATIN-LIM-015"];
    // each site stops one byte short of the transaction limit and exactly at it
    {
        let sites: Vec<(&'static str, Scenario)> = {
            let mut tx = scn("x".to_string(), &rules, vec![acct(B, vec![])], 1_000_000);
            tx.to = B;
            tx.value = U256::from(1u64);
            let relay = vc(Asm::new(), B, 1).stop().build();
            let mut call =
                scn("x".to_string(), &rules, vec![funded(A, relay, 5), acct(B, vec![])], 1_000_000);
            call.to = A;
            let create = scn(
                "x".to_string(),
                &rules,
                vec![funded(A, Asm::new().create(&[op::STOP], 1).op(op::POP).stop().build(), 5)],
                1_000_000,
            );
            let sd = scn(
                "x".to_string(),
                &rules,
                vec![funded(A, destruct(Asm::new(), B).build(), 5), acct(B, vec![])],
                1_000_000,
            );
            vec![("tx_value", tx), ("value_call", call), ("create", create), ("selfdestruct", sd)]
        };
        for (tag, base) in sites {
            for delta in [-1i64, 0, 1] {
                let mut s = base.clone();
                s.name = nm(format!("port_tlog_site_{tag}_{}", delta + 1));
                s.limits.data = Some(((310 + 160 + 80 + 160) as i64 + delta) as u64);
                v.push(s);
            }
        }
        // the LOG3-of-one-word twin
        for delta in [-1i64, 0] {
            let a = Asm::new().push(0).push(0).push(0).push(32).push(0).op(op::LOG3).stop().build();
            let mut s = scn(
                format!("port_tlog_log3_twin_{}", delta + 1),
                &rules,
                vec![acct(A, a)],
                1_000_000,
            );
            s.limits.data = Some((310 + 32 + 32 * 3 + 20 + delta + 8).max(0) as u64);
            v.push(s);
        }
    }
    // an answered value call whose start crosses is taken back before its caller resumes
    for (tag, payee) in [("no_code", B), ("identity", IDENTITY)] {
        let a = Asm::new()
            .sstore(1, 1)
            .op(op::SELFBALANCE)
            .push(2)
            .op(op::SSTORE)
            .call_with(op::CALL, payee, 1_000, 0, 0)
            .op(op::POP)
            .op(op::SELFBALANCE)
            .push(3)
            .op(op::SSTORE)
            .stop()
            .build();
        let a = {
            let _ = a;
            Asm::new()
                .sstore(1, 1)
                .op(op::SELFBALANCE)
                .push(2)
                .op(op::SSTORE)
                .call_all(op::CALL, payee, 1_000)
                .op(op::POP)
                .op(op::SELFBALANCE)
                .push(3)
                .op(op::SSTORE)
                .stop()
                .build()
        };
        for delta in [-1i64, 0, 40] {
            let mut s = scn(
                format!("port_tlog_start_crosses_{tag}_{}", delta + 1),
                &rules,
                vec![funded(A, a.clone(), 1_000), acct(B, vec![])],
                5_000_000,
            );
            s.limits.data = Some((310 + 40 + 240 + delta) as u64);
            v.push(s);
        }
    }
    // a value call its caller cannot fund runs as without a limit; funded to the last wei, stopped
    for (tag, bal) in [("unfunded", 999u64), ("funded", 1_000)] {
        let a = call_rec(Asm::new(), B, 1_000, 1).stop().build();
        let mut s = scn(
            format!("port_tlog_caller_cannot_fund_{tag}"),
            &rules,
            vec![funded(A, a, bal), acct(B, vec![])],
            5_000_000,
        );
        s.limits.data = Some(310 + 239);
        v.push(s);
    }
    for (tag, bal) in [("unfunded", 0u64), ("funded", 5)] {
        for gas in [400_000u64, 450_000, 600_000] {
            let a = call_all(Asm::new(), B, 1).stop().build();
            v.push(scn(
                format!("port_tlog_charge_nothing_{tag}_{gas}"),
                &rules,
                vec![funded(A, a, bal), acct(B, vec![])],
                gas,
            ));
        }
    }
    // a creation onto an occupied address fails on the collision
    {
        let init = vec![op::STOP];
        let x = create2_addr(A, &init);
        let a = create2_of(Asm::new(), &init, 1).op(op::POP).stop().build();
        for (tag, limit) in
            [("start_short", 310 + 239u64), ("body_short", 310 + 39), ("ample", 1_000_000)]
        {
            let mut s = scn(
                format!("port_tlog_create_collision_{tag}"),
                &rules,
                vec![funded(A, a.clone(), 5), acct(x, vec![op::STOP])],
                5_000_000,
            );
            s.limits.data = Some(limit);
            v.push(s);
        }
    }
    // a failed precompile's start keeps nothing and is held to nothing
    for (tag, limit) in [("plain", None), ("short", Some(310 + 239u64))] {
        let a = crate::q8::mem_call(
            Asm::new().push(1).push(0).op(op::MSTORE8),
            op::CALL,
            Address::with_last_byte(8),
            1_000,
            100_000,
            &[0x01],
        )
        .op(op::POP)
        .stop()
        .build();
        let mut s = scn(
            format!("port_tlog_failed_precompile_{tag}"),
            &rules,
            vec![funded(A, a, 1_000)],
            5_000_000,
        );
        s.limits.data = limit;
        v.push(s);
    }
    // a nested creation whose nonce cannot be bumped keeps nothing
    for (tag, limit) in [("plain", None), ("short", Some(310 + 239u64))] {
        let a = Asm::new().create(&[op::STOP], 1).op(op::POP).stop().build();
        let mut s = scn(
            format!("port_tlog_stuck_nonce_create_{tag}"),
            &rules,
            vec![crate::Account { nonce: u64::MAX, ..funded(A, a, 5) }],
            5_000_000,
        );
        s.limits.data = limit;
        v.push(s);
    }
    // a system transaction's value is logged and no limit stops it
    {
        let mut s = crate::edge::from_system(
            "port_tlog_system_tx_value".to_string().leak(),
            crate::edge::system_account(vec![], 0),
            super::ORACLE,
        );
        s.group = "port";
        s.rules = rules.to_vec();
        s.chain_id = Some(1);
        s.value = U256::from(1u64);
        s.limits.data = Some(310);
        v.push(s);
    }
    // transfer logs keep execution order
    {
        let a = Asm::new().push(0).push(0).op(op::LOG1).ops(&[]);
        let _ = a;
        let a = Asm::new().push(7).push(0).push(0).op(op::LOG1);
        let a = destruct(a, B).build();
        v.push(scn(
            "port_tlog_order_log_then_selfdestruct".to_string(),
            &rules,
            vec![funded(A, a, 3), acct(B, vec![])],
            5_000_000,
        ));
        let init = Asm::new().push(7).push(0).push(0).op(op::LOG1).stop().build();
        let a = Asm::new().create(&init, 1).op(op::POP).stop().build();
        v.push(scn(
            "port_tlog_order_create_then_log".to_string(),
            &rules,
            vec![funded(A, a, 3)],
            5_000_000,
        ));
    }
    // a block's transfers in execution order
    {
        let relay = vc(Asm::new().push(0).push(0).op(op::LOG0), B, 1)
            .push(0)
            .push(0)
            .op(op::LOG0)
            .stop()
            .build();
        let mut first = scn(
            "port_tlog_block".to_string(),
            &rules,
            vec![
                funded(A, relay, 5),
                funded(C, destruct(Asm::new(), B).build(), 3),
                acct(B, vec![]),
            ],
            5_000_000,
        );
        first.to = B;
        first.value = U256::from(1u64);
        let blk = block(
            first,
            vec![
                Box::new(|t: &mut Scenario| {
                    t.to = A;
                    t.value = U256::ZERO;
                }),
                Box::new(|t: &mut Scenario| {
                    t.to = EMPTY;
                    t.create = true;
                    t.value = U256::from(2u64);
                    t.data = vec![op::STOP];
                }),
                Box::new(|t: &mut Scenario| {
                    t.to = C;
                    t.create = false;
                    t.value = U256::ZERO;
                    t.data = vec![];
                }),
            ],
        );
        v.extend(blk);
    }
    v
}

/// The runtime a created account holds: with no calldata it destroys itself to `heir`, with
/// calldata it stores the first word in slot 1.
fn store_or_destroy(heir: Address) -> Vec<u8> {
    branch_on_data(
        destruct(Asm::new(), heir),
        Asm::new().push(0).op(op::CALLDATALOAD).push(1).op(op::SSTORE).stop(),
    )
}

fn destruction() -> Vec<Scenario> {
    let mut v = Vec::new();
    let runtime = store_or_destroy(NEWB);
    let init = init_deploying(Asm::new().sstore(1, 1), &runtime);
    let x = create2_addr(A, &init);
    let make = |c: Asm| create2_of(c, &init, 1).op(op::POP);
    let call_x = |c: Asm| call(c, x, 0, 300_000);
    // a kept destruction gives back the leaf, the code and the slot
    for growth in [None, Some(0u64)] {
        let a = call_x(make(Asm::new())).stop().build();
        let mut s = scn(
            format!("port_destr_kept_{}", growth.map_or("default", |_| "growth0")),
            &R_SD,
            vec![funded(A, a, 10)],
            5_000_000,
        );
        s.limits.growth = growth.map(|_| 1);
        v.push(s);
    }
    // a reverted inner frame discards the destruction; only the kept destruction gives back
    {
        let child = call_x(Asm::new()).revert0().build();
        let a = call_all(make(Asm::new()), C, 0).stop().build();
        v.push(scn(
            "port_destr_reverted_inner".to_string(),
            &R_SD,
            vec![funded(A, a, 10), acct(C, child)],
            5_000_000,
        ));
        let a = call_x(call_all(make(Asm::new()), C, 0)).stop().build();
        let child = call_x(Asm::new()).revert0().build();
        v.push(scn(
            "port_destr_only_kept_gives_back".to_string(),
            &R_SD,
            vec![funded(A, a, 10), acct(C, child)],
            5_000_000,
        ));
    }
    // a new beneficiary at a full budget stops the frame
    for n in [979u64, 980] {
        let child = destruct(write_loop(Asm::new(), 0, n), NEWB).build();
        let a = call_all(Asm::new(), C, 0).stop().build();
        v.push(scn(
            format!("port_destr_beneficiary_full_budget_{n}"),
            &R_SD,
            vec![acct(A, a), funded(C, child, 10)],
            400_000_000,
        ));
    }
    // stores after the mark, and a reverted enclosing frame
    for (tag, revert) in [("kept", false), ("reverted", true)] {
        let one = |w: u64| {
            let mut m = Asm::new().push(w).push(0).op(op::MSTORE);
            m = m
                .push(0)
                .push(32)
                .push(0)
                .push(0)
                .push(0)
                .push_addr(x)
                .op(op::GAS)
                .op(op::CALL)
                .op(op::POP);
            m
        };
        let mut child = call_x(Asm::new());
        child = one(2).0.into_iter().fold(child, |c, b| c.op(b));
        child = one(0).0.into_iter().fold(child, |c, b| c.op(b));
        child = call_x(child);
        let child = if revert { child.revert0().build() } else { child.stop().build() };
        let a = call_all(make(Asm::new()), C, 0).stop().build();
        v.push(scn(
            format!("port_destr_stores_after_mark_{tag}"),
            &R_SD,
            vec![funded(A, a, 10), acct(C, child)],
            5_000_000,
        ));
    }
    // a destruction gives back record history and keeps the logs
    {
        let rt = destruct(log3_word(Asm::new()), HEIR).build();
        let ini = init_deploying(Asm::new().sstore(1, 1), &rt);
        let xx = create2_addr(A, &ini);
        let a = create2_of(Asm::new(), &ini, 1).op(op::POP);
        let a = call(call(a, xx, 0, 300_000), xx, 1, 300_000).stop().build();
        v.push(scn(
            "port_destr_keeps_logs".to_string(),
            &R_SD,
            vec![funded(A, a, 10), funded(HEIR, vec![], 1)],
            5_000_000,
        ));
    }
    // a destroying child's negative net does not raise the parent's budget
    {
        let b = call_all(call_x(Asm::new()), C, 0).stop().build();
        let a = call_all(make(Asm::new()), B, 0).stop().build();
        let mut s = scn(
            "port_destr_negative_net_parent".to_string(),
            &R_SD,
            vec![
                funded(A, a, 10),
                acct(B, b),
                acct(C, write_loop(Asm::new(), 0, 959).stop().build()),
            ],
            400_000_000,
        );
        s.limits.growth = Some(1_000);
        v.push(s);
    }
    // a halt after a destroying child discards the mark
    {
        let b = endless(call_x(Asm::new())).build();
        let a = call(make(Asm::new()), B, 0, 2_000_000).stop().build();
        v.push(scn(
            "port_destr_halt_after_destroying_child".to_string(),
            &R_SD,
            vec![funded(A, a, 10), acct(B, b)],
            20_000_000,
        ));
    }
    // a second destruction with no balance subtracts nothing
    {
        let rt = destruct(Asm::new(), NEWB).build();
        let ini = init_deploying(Asm::new(), &rt);
        let xx = create2_addr(A, &ini);
        let a =
            call(call(create2_of(Asm::new(), &ini, 0).op(op::POP), xx, 0, 300_000), xx, 0, 300_000)
                .stop()
                .build();
        v.push(scn("port_destr_second_no_balance".to_string(), &R_SD, vec![acct(A, a)], 5_000_000));
    }
    // a beneficiary charge the frame cannot pay halts before the give-back
    for g in [150_000u64, 200_000, 250_000, 300_000, 350_000] {
        let ctor = destruct(write_loop(Asm::new(), 0, 3), NEWB).build();
        let a = Asm::new().create(&ctor, 1).op(op::POP).stop().build();
        v.push(scn(
            format!("port_destr_beneficiary_unpayable_{g}"),
            &R_SD,
            vec![funded(A, a, 10)],
            g,
        ));
    }
    // a cleared slot after the mark does not refill again; a fresh slot is neither charged nor
    // counted
    for (tag, value) in [("cleared", 0u64), ("fresh", 5)] {
        let key = if value == 0 { 1u64 } else { 9 };
        let mut data = vec![0u8; 32];
        data[31] = value as u8;
        let _ = (key, &data);
        let a = call(call_x(make(Asm::new())), x, 0, 300_000).stop().build();
        let a = {
            let _ = a;
            let w = Asm::new().push(value).push(0).op(op::MSTORE);
            let c = w
                .push(0)
                .push(0)
                .push(32)
                .push(0)
                .push(0)
                .push_addr(x)
                .op(op::GAS)
                .op(op::CALL)
                .op(op::POP);
            call_x(make(Asm::new())).ops(&c.0).stop().build()
        };
        v.push(scn(
            format!("port_destr_store_after_mark_{tag}"),
            &R_SD,
            vec![funded(A, a, 10)],
            5_000_000,
        ));
    }
    // a cleared account with no KV room stops at the frame budget
    for kv in [3u64, 4] {
        let child = destruct(Asm::new(), B).build();
        let a = call_all(vc(Asm::new(), B, 1), C, 0).stop().build();
        let mut s = scn(
            format!("port_destr_cleared_account_kv_{kv}"),
            &R_SD,
            vec![funded(A, a, 10), acct(B, vec![]), funded(C, child, 10)],
            5_000_000,
        );
        s.limits.kv = Some(kv);
        v.push(s);
    }
    // slots deleted with the account refill once
    {
        let ini = init_deploying(
            Asm::new().sstore(1, 1).sstore(2, 1).sstore(2, 0),
            &store_or_destroy(HEIR),
        );
        let xx = create2_addr(A, &ini);
        let a = call(create2_of(Asm::new(), &ini, 0).op(op::POP), xx, 0, 300_000).stop().build();
        v.push(scn(
            "port_destr_slots_refill_once".to_string(),
            &R_SD,
            vec![acct(A, a), funded(HEIR, vec![], 1)],
            5_000_000,
        ));
    }
    // a detention crossing at the destruction applies none of the table
    for cap in [1_000u64, 3_000, 5_000, 5_100, 8_000, 100_000] {
        let rt = destruct(Asm::new().op(op::TIMESTAMP).op(op::POP), HEIR).build();
        let ini = init_deploying(Asm::new().sstore(1, 1), &rt);
        let xx = create2_addr(A, &ini);
        let a = call(create2_of(Asm::new(), &ini, 1).op(op::POP), xx, 0, 300_000).stop().build();
        let mut s = scn(
            format!("port_destr_detention_crossing_{cap}"),
            &R_SD,
            vec![funded(A, a, 10), funded(HEIR, vec![], 1)],
            5_000_000,
        );
        s.limits.block_env_cap = Some(cap);
        v.push(s);
    }
    // a store after the mark is not charged what its frame cannot pay
    for (tag, before) in [("after_mark", false), ("before_destruction", true)] {
        let mut data_call = Asm::new().push(7).push(0).op(op::MSTORE);
        data_call = data_call
            .push(0)
            .push(0)
            .push(32)
            .push(0)
            .push(0)
            .push_addr(x)
            .push(60_000)
            .op(op::CALL)
            .op(op::POP);
        let a = if before {
            make(Asm::new()).ops(&data_call.0).ops(&call_x(Asm::new()).0).stop().build()
        } else {
            call_x(make(Asm::new())).ops(&data_call.0).stop().build()
        };
        v.push(scn(
            format!("port_destr_store_unpayable_{tag}"),
            &R_SD,
            vec![funded(A, a, 10)],
            10_000_000,
        ));
    }
    // a deposit after the mark: a constructor that destroys itself then returns code
    for (tag, destroys) in [("destroying", true), ("plain", false)] {
        let lib = destruct(Asm::new(), HEIR).build();
        let mut ctor = Asm::new();
        if destroys {
            ctor = ctor.call_with(op::DELEGATECALL, D, 0, 100_000, 0).op(op::POP);
        }
        let ini = init_deploying(ctor, &[op::STOP; 40]);
        let xx = create2_addr(C, &ini);
        let creator = create2_of(Asm::new(), &ini, 0).op(op::POP).stop().build();
        let a = call(Asm::new(), C, 0, 40_000).stop().build();
        v.push(scn(
            format!("port_destr_deposit_after_mark_{tag}"),
            &R_SD,
            vec![
                acct(A, a),
                acct(C, creator),
                acct(D, lib),
                funded(xx, vec![], 1),
                funded(HEIR, vec![], 1),
            ],
            200_000_000 + 10_000 + 100_000,
        ));
    }
    // code deposited after the constructor destroyed its own account keeps no state gas
    {
        let lib = destruct(Asm::new(), HEIR).build();
        let ctor = Asm::new().call_with(op::DELEGATECALL, D, 0, 100_000, 0).op(op::POP);
        let ini = init_deploying(ctor, &destruct(Asm::new(), HEIR).build());
        let xx = create2_addr(A, &ini);
        let a = call(create2_of(Asm::new(), &ini, 0).op(op::POP), xx, 0, 100_000).stop().build();
        v.push(scn(
            "port_destr_code_after_constructor_destroyed".to_string(),
            &R_SD,
            vec![acct(A, a), acct(D, lib), funded(HEIR, vec![], 1)],
            5_000_000,
        ));
    }
    // a creation at its growth limit that destroys itself to a new account succeeds
    for limit in [1_000u64, 1_001] {
        let mut s =
            scn(format!("port_destr_create_tx_selfdestruct_{limit}"), &R_SD, vec![], 400_000_000);
        s.create = true;
        s.value = U256::from(1u64);
        s.data = destruct(write_loop(Asm::new(), 0, 999), NEWB).build();
        s.limits.growth = Some(limit);
        v.push(s);
    }
    // a give-back onto an empty reservoir credits the spill first
    {
        let rt = destruct(Asm::new().sstore(5, 1).sstore(6, 1), HEIR).build();
        let ini = init_deploying(Asm::new().sstore(1, 1), &rt);
        let xx = create2_addr(A, &ini);
        let a = call(create2_of(Asm::new(), &ini, 0).op(op::POP), xx, 0, 3_000_000).stop().build();
        v.push(scn(
            "port_destr_give_back_credits_spill".to_string(),
            &R_SD,
            vec![acct(A, a), funded(HEIR, vec![], 1)],
            5_000_000,
        ));
    }
    v
}

fn log3_word(a: Asm) -> Asm {
    a.push(0).push(0).push(0).push(32).push(0).op(op::LOG3)
}

fn destruction_blocks() -> Vec<Scenario> {
    let mut v = Vec::new();
    let runtime = store_or_destroy(HEIR);
    let init = init_deploying(Asm::new().sstore(1, 1), &runtime);
    let x = create2_addr(A, &init);
    let make_and_destroy = create2_of(Asm::new(), &init, 0).op(op::POP);
    let a_code = call(make_and_destroy, x, 0, 300_000).stop().build();
    // a leaf an earlier transaction destroyed is absent at the next start (balance only / slot
    // only)
    for (tag, prefunded) in [("balance", true), ("empty", false)] {
        let mut accounts = vec![acct(A, a_code.clone()), funded(HEIR, vec![], 1)];
        if prefunded {
            accounts.push(funded(x, vec![], 1));
        }
        let first = scn(format!("port_destr_block_absent_{tag}"), &R_SD, accounts, 5_000_000);
        v.extend(block(first, vec![Box::new(|_: &mut Scenario| {})]));
    }
    // a leaf an earlier transaction funded is present at the next start
    {
        let first = {
            let mut s = scn(
                "port_destr_block_funded".to_string(),
                &R_SD,
                vec![acct(A, a_code.clone()), funded(HEIR, vec![], 1)],
                5_000_000,
            );
            s.to = x;
            s.value = U256::from(1u64);
            s
        };
        v.extend(block(
            first,
            vec![Box::new(|t: &mut Scenario| {
                t.to = A;
                t.value = U256::ZERO;
            })],
        ));
    }
    // a leaf funded after its destruction is present at the next start
    {
        let first = scn(
            "port_destr_block_funded_after".to_string(),
            &R_SD,
            vec![acct(A, a_code), funded(HEIR, vec![], 1)],
            5_000_000,
        );
        v.extend(block(
            first,
            vec![
                Box::new(move |t: &mut Scenario| {
                    t.to = x;
                    t.value = U256::from(1u64);
                }),
                Box::new(|t: &mut Scenario| {
                    t.to = A;
                    t.value = U256::ZERO;
                }),
            ],
        ));
    }
    v
}

fn salt() -> Vec<Scenario> {
    let mut v = Vec::new();
    // a transaction from the system address prices at the minimum bucket
    {
        let contract = Asm::new().sstore(7, 1).stop().build();
        let mut s = scn(
            "port_salt_protocol_caller_min_bucket".to_string(),
            &R_SALT,
            vec![
                acct(B, contract.clone()),
                funded(Address::new([0xfe; 20]), vec![], 1_000_000_000_000_000_000),
            ],
            1_000_000,
        );
        s.sender = Address::new([0xfe; 20]);
        s.to = B;
        s.salt = vec![(B, Some(7), 8)];
        v.push(s);
        let mut o = crate::edge::from_system(
            "port_salt_promoted_system_min_bucket",
            crate::edge::system_account(vec![], 0),
            super::ORACLE,
        );
        o.group = "port";
        o.rules = R_SALT.to_vec();
        o.chain_id = Some(1);
        o.accounts.retain(|x| x.address != super::ORACLE);
        o.accounts.push(acct(super::ORACLE, Asm::new().sstore(7, 1).stop().build()));
        o.salt = vec![(super::ORACLE, Some(7), 8)];
        v.push(o);
    }
    // writing over or clearing a slot charges no state gas; a slot written twice pays for one leaf
    for (tag, code, m) in [
        ("over", Asm::new().sstore(7, 2).stop().build(), 8u64),
        ("clear", Asm::new().sstore(7, 0).stop().build(), 8),
        ("twice", Asm::new().sstore(7, 1).sstore(7, 2).stop().build(), 2),
    ] {
        let mut a = acct(A, code);
        if tag != "twice" {
            a.storage = vec![(7, 1)];
        }
        let mut s = scn(format!("port_salt_slot_{tag}"), &R_SALT, vec![a], 1_000_000);
        s.salt = vec![(A, Some(7), m)];
        v.push(s);
    }
    // delegation cycles and self-delegation settle
    let deleg = |to: Address| {
        let mut c = vec![0xef, 0x01, 0x00];
        c.extend_from_slice(to.as_slice());
        c
    };
    let (p, q) = (Address::with_last_byte(0x91), Address::with_last_byte(0x92));
    let s_self = Address::with_last_byte(0x93);
    for (tag, kind) in [
        ("call", op::CALL),
        ("staticcall", op::STATICCALL),
        ("callcode", op::CALLCODE),
        ("delegatecall", op::DELEGATECALL),
    ] {
        for (target_tag, target) in [("self", s_self), ("cycle", p)] {
            let a = Asm::new().call_with(kind, target, 0, 100_000, 0).op(op::POP).stop().build();
            v.push(scn(
                format!("port_saltdel_{tag}_{target_tag}"),
                &["SATIN-STATE-010", "SATIN-AUTH-003"],
                vec![acct(A, a), acct(s_self, deleg(s_self)), acct(p, deleg(q)), acct(q, deleg(p))],
                1_000_000,
            ));
        }
    }
    {
        let mut s = scn(
            "port_saltdel_tx_to_self_delegating".to_string(),
            &["SATIN-STATE-010"],
            vec![acct(s_self, deleg(s_self))],
            1_000_000,
        );
        s.to = s_self;
        v.push(s);
    }
    // a create from a delegated frame settles; and is priced at the authority's address
    {
        let creator = Asm::new().create(&[], 0).op(op::POP).stop().build();
        let creator2 =
            Asm::new().push(0).push(0).push(0).push(0).op(op::CREATE2).op(op::POP).stop().build();
        for (tag, code) in [("create", creator.clone()), ("create2", creator2)] {
            let mut s = scn(
                format!("port_saltdel_{tag}_from_delegated"),
                &["SATIN-STATE-010", "SATIN-STATE-011"],
                vec![acct(B, deleg(D)), acct(D, code)],
                5_000_000,
            );
            s.to = B;
            v.push(s);
        }
        let mut x = crate::Account { nonce: 3, ..acct(B, deleg(D)) };
        x.nonce = 3;
        let mut dd = crate::Account { nonce: 7, ..acct(D, creator) };
        dd.nonce = 7;
        let mut s = scn(
            "port_saltdel_create_priced_at_authority".to_string(),
            &["SATIN-STATE-010", "SATIN-STATE-011"],
            vec![x, dd],
            5_000_000,
        );
        s.to = B;
        s.salt = vec![(B.create(3), None, 8)];
        v.push(s);
        // a callcode to a delegated account charges no state gas
        let a = Asm::new().call_with(op::CALLCODE, B, 1, 100_000, 0).op(op::POP).stop().build();
        let mut s = scn(
            "port_saltdel_callcode_no_state_gas".to_string(),
            &R_SALT,
            vec![funded(A, a, 1), acct(B, deleg(D)), acct(D, vec![op::STOP])],
            5_000_000,
        );
        s.salt = vec![(A, None, 8)];
        v.push(s);
    }
    // a deposit creation is priced where it deploys
    {
        for (tag, salt_at) in [
            ("where_deployed", 5u64),
            ("envelope_nonce_crowded", 0),
            ("envelope_nonce_invalid", 99),
        ] {
            let mut s = scn(
                format!("port_saltdep_{tag}"),
                &R_SALT,
                vec![crate::Account { nonce: 5, ..funded(D, vec![], 1_000_000_000) }],
                5_000_000,
            );
            s.sender = D;
            s.deposit = Some(0);
            s.create = true;
            s.data = crate::init_returning(0);
            match tag {
                "where_deployed" => s.salt = vec![(D.create(5), None, 8)],
                "envelope_nonce_crowded" => s.salt = vec![(D.create(salt_at), None, 8)],
                _ => s.salt_capacity = vec![(D.create(0), None, 0)],
            }
            v.push(s);
        }
        let mut s = scn(
            "port_saltdep_reverting_init".to_string(),
            &R_SALT,
            vec![crate::Account { nonce: 5, ..funded(D, vec![], 1_000_000_000) }],
            5_000_000,
        );
        s.sender = D;
        s.deposit = Some(0);
        s.create = true;
        s.data = vec![op::PUSH0, op::PUSH0, op::REVERT];
        s.salt = vec![(D.create(5), None, 8)];
        v.push(s);
    }
    // a refused code deposit pays no state gas; a kept charge and a restored one settle
    // independently
    {
        let mut s =
            scn("port_saltref_refused_code_deposit".to_string(), &R_SALT, vec![], 5_000_000);
        s.create = true;
        s.data = init_first(0xef, 1);
        s.salt = vec![(crate::SENDER.create(0), None, 8)];
        v.push(s);
        let a = Asm::new().sstore(1, 1).sstore(2, 1).sstore(2, 0).stop().build();
        let mut s =
            scn("port_saltref_kept_and_restored".to_string(), &R_SALT, vec![acct(A, a)], 5_000_000);
        s.salt = vec![(A, Some(1), 8), (A, Some(2), 2)];
        v.push(s);
        let init = Asm::new().sstore(1, 1).push(600_000).push(0).op(op::RETURN).build();
        let c = Asm::new().create(&init, 0).op(op::POP).stop().build();
        let mut s = scn(
            "port_saltref_code_deposit_unpayable".to_string(),
            &R_SALT,
            vec![acct(C, c)],
            5_000_000,
        );
        s.to = C;
        s.salt = vec![(C.create(0), Some(1), 8), (C.create(0), None, 8)];
        v.push(s);
        let rt = destruct(Asm::new(), HEIR).build();
        let ini = init_deploying(Asm::new().sstore(1, 1), &rt);
        let xx = A.create(0);
        let a = Asm::new()
            .create(&ini, 0)
            .op(op::POP)
            .call_with(op::CALL, xx, 0, 300_000, 0)
            .op(op::POP)
            .stop()
            .build();
        let mut s = scn(
            "port_saltref_destroyed_after_creation".to_string(),
            &R_SALT,
            vec![acct(A, a), funded(HEIR, vec![], 1)],
            5_000_000,
        );
        s.salt = vec![(xx, None, 8), (xx, Some(1), 8)];
        v.push(s);
    }
    let _ = sender;
    v
}
