//! Scenarios of group "port" and "blk": the system contracts' interception and access switch, the
//! remaining-compute-gas query, system transactions and the block-level predeploys (the other
//! implementation's `system/*`, `block/*` and `tracers` tests that no scenario did yet).
#![allow(dead_code)]
use super::{
    A, Asm, B, C, D, EMPTY, LIMIT_CONTROL, ORACLE, SEQUENCER_REGISTRY, Scenario, acct, block,
    funded, op, pp::*,
};
use crate::{
    edge::{from_system, registry_naming, system_account},
    q8::{ACCESS_CONTROL, mem_call, sel},
};
use alloy_primitives::{Address, U256};

const R_ACL: [&str; 4] = ["SATIN-MAC-001", "SATIN-OUT-008", "SATIN-DET-003", "SATIN-ICPT-001"];
const R_CTL: [&str; 3] = ["SATIN-ICPT-001", "SATIN-MLC-001", "SATIN-SYSC-001"];
const BEN: Address = Address::with_last_byte(0xbe);

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(access());
    v.extend(control());
    v.extend(remaining());
    v.extend(system_txs());
    v.extend(blocks());
    v.extend(tracers());
    v
}

fn off(a: Asm) -> Asm {
    a.mstore_selector(sel("disableVolatileDataAccess()"))
        .call_with(op::CALL, ACCESS_CONTROL, 0, 100_000, 4)
        .op(op::POP)
}

fn query(a: Asm, slot: u64) -> Asm {
    a.mstore_selector(sel("isVolatileDataAccessDisabled()"))
        .call_with(op::STATICCALL, ACCESS_CONTROL, 0, 100_000, 4)
        .op(op::POP)
        .store_returndata_word(slot)
}

fn reads_timestamp() -> Vec<u8> {
    Asm::new().op(op::TIMESTAMP).op(op::POP).sstore(1, 1).stop().build()
}

fn access() -> Vec<Scenario> {
    let mut v = Vec::new();
    let mk = |name: &str, accounts: Vec<crate::Account>, gas: u64| {
        scn(name.to_string(), &R_ACL, accounts, gas)
    };
    // a DELEGATECALL is also restricted; a STATICCALL switch is intercepted
    {
        let a = call_rec(
            off(Asm::new()).call_with(op::DELEGATECALL, B, 0, 100_000, 0).record_call(1),
            C,
            0,
            5,
        )
        .stop()
        .build();
        v.push(mk(
            "port_acl_delegatecall_restricted",
            vec![acct(A, a), acct(B, reads_timestamp()), acct(C, vec![])],
            5_000_000,
        ));
        let a = Asm::new()
            .mstore_selector(sel("disableVolatileDataAccess()"))
            .call_with(op::STATICCALL, ACCESS_CONTROL, 0, 100_000, 4)
            .record_call(1)
            .op(op::TIMESTAMP)
            .push(3)
            .op(op::SSTORE)
            .stop()
            .build();
        v.push(mk("port_acl_staticcall_disable_intercepted", vec![acct(A, a)], 5_000_000));
    }
    // a non-beneficiary account is not restricted
    for (tag, read) in [
        ("balance", Asm::new().push_addr(C).op(op::BALANCE).op(op::POP)),
        ("extcodesize", Asm::new().push_addr(C).op(op::EXTCODESIZE).op(op::POP)),
        ("selfdestruct", destruct(Asm::new(), C)),
    ] {
        let a = read_after_off(read);
        v.push(mk(
            &format!("port_acl_non_beneficiary_{tag}"),
            vec![funded(A, a, 5), acct(C, vec![])],
            5_000_000,
        ));
    }
    // a beneficiary frame with access off creates and moves value; its own account is refused
    {
        let b = off(Asm::new())
            .create(&[op::STOP], 1)
            .op(op::POP)
            .push(0)
            .push(0)
            .push(0)
            .push(1)
            .op(op::CREATE2)
            .op(op::POP)
            .stop()
            .build();
        let b = {
            let _ = b;
            off(Asm::new()).create(&[op::STOP], 1).op(op::POP).stop().build()
        };
        let mut s = mk(
            "port_acl_beneficiary_off_creates",
            vec![acct(A, call_rec(Asm::new(), B, 0, 1).stop().build()), funded(B, b, 5)],
            5_000_000,
        );
        s.beneficiary = B;
        v.push(s);
        let b = off(Asm::new())
            .push(0)
            .push(0)
            .push(0)
            .push(0)
            .push(0)
            .op(op::CREATE2)
            .op(op::POP)
            .stop()
            .build();
        let mut s = mk(
            "port_acl_beneficiary_off_create2",
            vec![acct(A, call_rec(Asm::new(), B, 0, 1).stop().build()), funded(B, b, 5)],
            5_000_000,
        );
        s.beneficiary = B;
        v.push(s);
        let b = vc2(off(Asm::new()), C, 1).stop().build();
        let mut s = mk(
            "port_acl_beneficiary_off_value_call",
            vec![
                acct(A, call_rec(Asm::new(), B, 0, 1).stop().build()),
                funded(B, b, 5),
                acct(C, vec![]),
            ],
            5_000_000,
        );
        s.beneficiary = B;
        v.push(s);
        for (tag, own) in [
            ("selfbalance", Asm::new().op(op::SELFBALANCE).op(op::POP)),
            ("balance_self", Asm::new().op(op::ADDRESS).op(op::BALANCE).op(op::POP)),
            ("extcodesize_self", Asm::new().op(op::ADDRESS).op(op::EXTCODESIZE).op(op::POP)),
        ] {
            let b = read_after_off(own);
            let mut s = mk(
                &format!("port_acl_beneficiary_off_own_{tag}"),
                vec![acct(A, call_rec(Asm::new(), B, 0, 1).stop().build()), funded(B, b, 5)],
                5_000_000,
            );
            s.beneficiary = B;
            v.push(s);
        }
    }
    // a creation at the beneficiary is no read
    for (tag, o) in [("on", false), ("off", true)] {
        let created = A.create(0);
        let base = if o { off(Asm::new()) } else { Asm::new() };
        let init = Asm::new().sstore(1, 1).stop().build();
        let a = base
            .create(&init, 0)
            .op(op::POP)
            .push(1)
            .op(op::SLOAD)
            .push(2)
            .op(op::SSTORE)
            .ops(&endless_tail())
            .stop()
            .build();
        let mut s =
            mk(&format!("port_acl_creation_at_beneficiary_{tag}"), vec![acct(A, a)], 5_000_000);
        s.beneficiary = created;
        s.limits.block_env_cap = Some(1_000);
        v.push(s);
    }
    // blocked access does not set the bitmap
    {
        let kids: Vec<Vec<u8>> = vec![
            Asm::new().op(op::TIMESTAMP).op(op::POP).stop().build(),
            Asm::new().push_addr(BEN).op(op::BALANCE).op(op::POP).stop().build(),
            call_all(Asm::new(), BEN, 0).stop().build(),
            destruct(Asm::new(), BEN).build(),
            Asm::new().push(0).op(op::SLOAD).op(op::POP).stop().build(),
        ];
        let mut a = off(Asm::new());
        let mut accts = vec![];
        for (i, code) in kids.into_iter().enumerate() {
            let at = Address::with_last_byte(0x60 + i as u8);
            a = call_all(a, at, 0);
            accts.push(funded(at, code, 5));
        }
        accts.push(acct(A, endless(a).build()));
        accts.push(funded(BEN, vec![], 1));
        accts.push(acct(ORACLE, vec![op::STOP]));
        let mut s = mk("port_acl_blocked_sets_no_bitmap", accts, 5_000_000);
        s.beneficiary = BEN;
        s.limits.block_env_cap = Some(1_000);
        s.limits.oracle_cap = Some(1_000);
        v.push(s);
    }
    // a create reverts when volatile access is disabled
    {
        let init = Asm::new().op(op::TIMESTAMP).op(op::POP).stop().build();
        let a = off(Asm::new()).create(&init, 0).push(1).op(op::SSTORE).stop().build();
        v.push(mk("port_acl_create_reads_while_disabled", vec![acct(A, a)], 5_000_000));
    }
    // a parent that read, then disabled: its child is restricted
    {
        let a = call_rec(off(Asm::new().op(op::TIMESTAMP).op(op::POP)), B, 0, 1).stop().build();
        v.push(mk(
            "port_acl_parent_read_child_restricted",
            vec![acct(A, a), acct(B, Asm::new().op(op::NUMBER).op(op::POP).stop().build())],
            5_000_000,
        ));
    }
    // a sibling is not restricted; a disable in a reverted or halted child does not affect a
    // sibling
    for (tag, b) in [
        ("returns", off(Asm::new()).stop().build()),
        ("reverts", off(Asm::new()).revert0().build()),
        ("halts", off(Asm::new()).op(op::INVALID).build()),
        ("runs_out_of_gas", endless(off(Asm::new())).build()),
    ] {
        let c = call_all(Asm::new().op(op::TIMESTAMP).op(op::POP), D, 0).stop().build();
        let a = call_rec(call_rec(Asm::new(), B, 0, 1), C, 0, 3).stop().build();
        let a = if tag == "runs_out_of_gas" {
            call_rec(call_rec_g(Asm::new(), B, 200_000, 1), C, 0, 3).stop().build()
        } else {
            a
        };
        v.push(mk(
            &format!("port_acl_sibling_{tag}"),
            vec![acct(A, a), acct(B, b), acct(C, c), acct(D, reads_timestamp())],
            5_000_000,
        ));
    }
    // the query answers for the caller at every depth
    {
        let c = query(Asm::new(), 1).stop().build();
        let b = query(call_all(Asm::new(), C, 0), 2).stop().build();
        let a = query(call_all(off(Asm::new()), B, 0), 3).stop().build();
        v.push(mk(
            "port_acl_query_every_depth",
            vec![acct(A, a), acct(B, b), acct(C, c)],
            5_000_000,
        ));
    }
    // the switch is the same below and above the execution cap
    for (tag, gas) in [("below", 5_000_000u64), ("above", 300_000_000)] {
        let a = query(call_all(off(Asm::new()), B, 0), 1).stop().build();
        v.push(mk(
            &format!("port_acl_cap_{tag}"),
            vec![acct(A, a), acct(B, reads_timestamp())],
            gas,
        ));
    }
    v
}

fn a_prefix() -> Asm {
    Asm::new()
}

fn call_rec_g(a: Asm, to: Address, gas: u64, slot: u64) -> Asm {
    a.call_with(op::CALL, to, 0, gas, 0).record_call(slot)
}

fn endless_tail() -> Vec<u8> {
    Vec::new()
}

fn vc2(a: Asm, to: Address, value: u64) -> Asm {
    call_all(a, to, value)
}

/// A frame that switches access off, runs `read`, and stops.
fn read_after_off(read: Asm) -> Vec<u8> {
    let mut a = off(Asm::new());
    a.0.extend_from_slice(&read.0);
    a.stop().build()
}

fn control() -> Vec<Scenario> {
    let mut v = Vec::new();
    let sels = [
        "disableVolatileDataAccess()",
        "enableVolatileDataAccess()",
        "isVolatileDataAccessDisabled()",
    ];
    // a static call is intercepted: answered, nothing written
    for (i, sig) in sels.iter().enumerate() {
        let a = Asm::new()
            .mstore_selector(sel(sig))
            .call_with(op::STATICCALL, ACCESS_CONTROL, 0, 100_000, 4)
            .record_call(1)
            .store_returndata_word(3)
            .stop()
            .build();
        v.push(scn(format!("port_ctl_staticcall_{i}"), &R_CTL, vec![acct(A, a)], 5_000_000));
    }
    // a value-bearing call to an unknown selector falls through to the bytecode
    {
        let a = Asm::new()
            .mstore_selector(sel("unknown()"))
            .call_with(op::CALL, ACCESS_CONTROL, 1, 100_000, 4)
            .record_call(1)
            .stop()
            .build();
        v.push(scn(
            "port_ctl_value_unknown_selector".to_string(),
            &R_CTL,
            vec![funded(A, a, 5), acct(ACCESS_CONTROL, vec![op::STOP])],
            5_000_000,
        ));
    }
    // a selector with trailing bytes is intercepted; admission is the selector alone
    for (to, tag, sig) in [
        (ACCESS_CONTROL, "access", "isVolatileDataAccessDisabled()"),
        (LIMIT_CONTROL, "limit", "remainingComputeGas()"),
    ] {
        for extra in [0usize, 28, 100] {
            let mut input = sel(sig).to_vec();
            input.extend(vec![0xaa; extra]);
            let a = mem_call(Asm::new(), op::CALL, to, 0, 100_000, &input)
                .record_call(1)
                .stop()
                .build();
            v.push(scn(
                format!("port_ctl_trailing_{tag}_{extra}"),
                &R_CTL,
                vec![acct(A, a)],
                5_000_000,
            ));
        }
        let short = &sel(sig)[..3];
        let a = mem_call(Asm::new(), op::CALL, to, 0, 100_000, short).record_call(1).stop().build();
        v.push(scn(
            format!("port_ctl_short_input_{tag}"),
            &R_CTL,
            vec![acct(A, a), acct(to, vec![op::STOP])],
            5_000_000,
        ));
    }
    // the address admits the dispatch
    for sig in ["remainingComputeGas()", "disableVolatileDataAccess()"] {
        let a = Asm::new()
            .mstore_selector(sel(sig))
            .call_with(op::CALL, B, 0, 100_000, 4)
            .record_call(1)
            .stop()
            .build();
        v.push(scn(
            format!("port_ctl_ordinary_contract_{}", sig.len()),
            &R_CTL,
            vec![acct(A, a), acct(B, Asm::new().sstore(9, 9).stop().build())],
            5_000_000,
        ));
    }
    // a nested static call is intercepted
    {
        let b = Asm::new()
            .mstore_selector(sel("remainingComputeGas()"))
            .call_with(op::STATICCALL, LIMIT_CONTROL, 0, 100_000, 4)
            .store_returndata_word(1)
            .stop()
            .build();
        let a =
            Asm::new().call_with(op::STATICCALL, B, 0, 200_000, 0).record_call(1).stop().build();
        v.push(scn(
            "port_ctl_nested_static".to_string(),
            &R_CTL,
            vec![acct(A, a), acct(B, b)],
            5_000_000,
        ));
    }
    v
}

fn remaining() -> Vec<Scenario> {
    let mut v = Vec::new();
    let q = |slot: u64| {
        Asm::new()
            .mstore_selector(sel("remainingComputeGas()"))
            .call_with(op::STATICCALL, LIMIT_CONTROL, 0, 100_000, 4)
            .op(op::POP)
            .store_returndata_word(slot)
    };
    let rules = ["SATIN-MLC-001", "SATIN-DET-013"];
    // a direct tx answers the frame's own gas, below and above the cap; a detained one hears the
    // cap
    for gas in [1_000_000u64, 300_000_000] {
        let mut s = scn(format!("port_rcg_direct_{gas}"), &rules, vec![], gas);
        s.to = LIMIT_CONTROL;
        s.data = sel("remainingComputeGas()").to_vec();
        v.push(s.clone());
        s.name = nm(format!("port_rcg_direct_padded_{gas}"));
        s.data.extend(vec![0x11; 40]);
        v.push(s.clone());
        s.name = nm(format!("port_rcg_direct_from_beneficiary_{gas}"));
        s.data = sel("remainingComputeGas()").to_vec();
        s.beneficiary = crate::SENDER;
        v.push(s);
    }
    // the answer decreases with work; sequential queries decrease; two-level nesting
    for n in [0u64, 100, 1_000] {
        let a = work(q(1), n);
        let a = a.ops(&q(2).0).stop().build();
        v.push(scn(format!("port_rcg_work_then_query_{n}"), &rules, vec![acct(A, a)], 5_000_000));
    }
    v.push(scn(
        "port_rcg_sequential_queries".to_string(),
        &rules,
        vec![acct(A, q(2).ops(&q(1).0).0.iter().fold(Asm::new(), |x, b| x.op(*b)).stop().build())],
        5_000_000,
    ));
    {
        let b = q(1).stop().build();
        v.push(scn(
            "port_rcg_two_level".to_string(),
            &rules,
            vec![acct(A, call_all(Asm::new(), B, 0).stop().build()), acct(B, b)],
            5_000_000,
        ));
        let b = work(Asm::new(), 500).revert0().build();
        let a = call_all(Asm::new(), B, 0).ops(&q(1).0).stop().build();
        v.push(scn(
            "port_rcg_after_inner_revert".to_string(),
            &rules,
            vec![acct(A, a), acct(B, b)],
            5_000_000,
        ));
    }
    v
}

fn system_txs() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-MST-001", "SATIN-MST-005", "SATIN-MST-008"];
    // two Mega System Transactions in a block both go through
    {
        let mut first = from_system("port_systx_two_in_block", system_account(vec![], 0), ORACLE);
        first.group = "port";
        first.rules = rules.to_vec();
        first.chain_id = Some(1);
        v.extend(block(
            first,
            vec![Box::new(|t: &mut Scenario| {
                t.nonce = 1;
            })],
        ));
    }
    // the callee sees the system address as its caller
    {
        let mut s = from_system("port_systx_callee_sees_system", system_account(vec![], 0), ORACLE);
        s.group = "port";
        s.rules = rules.to_vec();
        s.chain_id = Some(1);
        s.accounts.retain(|x| x.address != ORACLE);
        s.accounts
            .push(acct(ORACLE, Asm::new().op(op::CALLER).push(1).op(op::SSTORE).stop().build()));
        v.push(s);
    }
    // a deposit over the data-size limit that cannot pay for its caller is a failed deposit
    for gas in [21_000u64, 40_000, 60_000, 100_000] {
        let mut s = scn(
            format!("port_systx_deposit_over_limit_{gas}"),
            &["SATIN-DEP-004", "SATIN-DEP-006"],
            vec![acct(EMPTY, vec![])],
            gas,
        );
        s.deposit = Some(7);
        s.sender = Address::with_last_byte(0x77);
        s.to = EMPTY;
        s.limits.data = Some(309);
        v.push(s);
    }
    // the floor of an exempt transaction
    {
        let sink = Asm::new().push(0x7fe0).op(op::MLOAD).op(op::POP).stop().build();
        for (calldata_len, gas) in [
            (32usize, 10_000_000u64),
            (40_000, 10_000_000),
            (32, 300_000_000),
            (40_000, 300_000_000),
        ] {
            for (shape, to) in [("call", ORACLE), ("value", ORACLE), ("self", B)] {
                for origin in ["deposit", "system"] {
                    let mut s = if origin == "system" {
                        let mut s = from_system("x", system_account(vec![], 0), to);
                        s.accounts.retain(|x| x.address != ORACLE);
                        s.accounts.push(acct(ORACLE, sink.clone()));
                        s.chain_id = Some(1);
                        s
                    } else {
                        let mut s =
                            scn("x".to_string(), &[], vec![acct(ORACLE, sink.clone())], gas);
                        s.deposit = Some(0);
                        s.to = to;
                        s
                    };
                    s.name = nm(format!("port_floor_{origin}_{shape}_{calldata_len}_{gas}"));
                    s.group = "port";
                    s.rules = vec!["SATIN-HIST-017", "SATIN-MST-008", "SATIN-DEP-007"];
                    s.gas_limit = gas;
                    s.data = vec![0u8; calldata_len];
                    if shape == "value" {
                        s.value = U256::from(1u64);
                    }
                    if shape == "self" {
                        s.to = if origin == "system" { crate::edge::SYSTEM } else { s.sender };
                    }
                    v.push(s);
                }
            }
        }
        // a creation
        for origin in ["deposit", "system"] {
            if origin == "deposit" {
                let mut s = scn(
                    "port_floor_deposit_create".to_string(),
                    &["SATIN-HIST-017", "SATIN-DEP-007"],
                    vec![],
                    10_000_000,
                );
                s.deposit = Some(0);
                s.create = true;
                s.data = vec![0u8; 40_000];
                v.push(s);
            }
        }
    }
    v
}

fn blocks() -> Vec<Scenario> {
    let mut v = Vec::new();
    // an L1 attributes deposit fits the Satin limits (stand-in code writing the info slots)
    {
        let depositor = crate::q2::DEPOSITOR;
        for (tag, seq_only) in [("sequence_only", true), ("new_origin", false)] {
            let mut code = Asm::new().sstore(3, 5);
            if !seq_only {
                code = code
                    .sstore(1, 1)
                    .sstore(2, 2)
                    .sstore(4, 4)
                    .sstore(5, 5)
                    .sstore(6, 6)
                    .sstore(7, 7)
                    .sstore(8, 8);
            }
            let l1 = super::L1_BLOCK;
            let mut s = scn(
                format!("port_l1attr_{tag}"),
                &["SATIN-DEP-007", "SATIN-LIM-017"],
                vec![
                    acct(l1, code.stop().build()),
                    crate::Account {
                        storage: if seq_only {
                            vec![(1, 1), (2, 2), (3, 4), (4, 4), (5, 5), (6, 6), (7, 7), (8, 8)]
                        } else {
                            vec![]
                        },
                        ..acct(l1, vec![])
                    },
                ],
                1_000_000,
            );
            s.accounts.retain(|x| {
                !(x.address == l1 &&
                    x.code.is_empty() &&
                    x.storage.is_empty() &&
                    !seq_only &&
                    false)
            });
            s.accounts.dedup_by_key(|x| x.address);
            s.sender = depositor;
            s.deposit = Some(0);
            s.to = l1;
            s.data = vec![0x44; 526];
            v.push(s);
        }
    }
    // a CREATE2 round trip through the factory
    {
        let factory = Address::with_last_byte(0x12);
        // runtime: size = calldatasize - 32; calldatacopy(0, 32, size); create2(0, 0, size, salt);
        // return address
        let code = Asm::new()
            .push(32)
            .op(op::CALLDATASIZE)
            .op(op::SUB)
            .op(op::DUP1)
            .push(32)
            .push(0)
            .op(op::CALLDATACOPY)
            .push(0)
            .op(op::CALLDATALOAD)
            .op(op::SWAP1)
            .push(0)
            .push(0)
            .op(op::CREATE2)
            .push(0)
            .op(op::MSTORE)
            .push(32)
            .push(0)
            .op(op::RETURN)
            .build();
        let init = init_returning_runtime();
        let mut data = vec![0u8; 32];
        data[31] = 7;
        data.extend_from_slice(&init);
        let mut s = scn(
            "port_predeploy_create2_factory".to_string(),
            &["SATIN-BASE-001", "SATIN-SIZE-004"],
            vec![acct(factory, code)],
            5_000_000,
        );
        s.to = factory;
        s.data = data;
        v.push(s);
    }
    // the registry: an admin handoff over two transactions, and a schedule without a proof
    {
        let admin = Address::with_last_byte(0xad);
        let pending = Address::with_last_byte(0xae);
        let reg = {
            let mut r = registry_naming(Address::with_last_byte(0x5a));
            r.storage.push((1, 0xad));
            r
        };
        let call_data = |sig: &str, arg: Option<Address>| {
            let mut d = sel(sig).to_vec();
            if let Some(a) = arg {
                let mut w = [0u8; 32];
                w[12..].copy_from_slice(a.as_slice());
                d.extend_from_slice(&w);
            }
            d
        };
        let mut first = scn(
            "port_registry_admin_handoff".to_string(),
            &["SATIN-REG-001", "SATIN-REG-002"],
            vec![
                reg.clone(),
                funded(admin, vec![], 1_000_000_000_000_000_000),
                funded(pending, vec![], 1_000_000_000_000_000_000),
            ],
            1_000_000,
        );
        first.sender = admin;
        first.to = SEQUENCER_REGISTRY;
        first.data = call_data("transferAdmin(address)", Some(pending));
        v.extend(block(
            first,
            vec![Box::new(move |t: &mut Scenario| {
                t.sender = pending;
                t.nonce = u64::MAX;
                t.data = call_data("acceptAdmin()", None);
            })],
        ));
        let mut s = scn(
            "port_registry_schedule_without_proof".to_string(),
            &["SATIN-REG-001", "SATIN-REG-002"],
            vec![reg, funded(admin, vec![], 1_000_000_000_000_000_000)],
            1_000_000,
        );
        s.sender = admin;
        s.to = SEQUENCER_REGISTRY;
        s.data = call_data("scheduleSequencerChange(address,uint64,bytes)", Some(pending));
        v.push(s);
    }
    v
}

fn init_returning_runtime() -> Vec<u8> {
    super::pp::init_deploying(Asm::new(), &[op::STOP])
}

fn tracers() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-STOP-013", "SATIN-STOP-004", "SATIN-LIM-003"];
    // a data-size limit stops the first frame at its start
    {
        let mut s = scn(
            "port_trace_first_frame_start".to_string(),
            &rules,
            vec![acct(B, Asm::new().sstore(1, 1).stop().build())],
            1_000_000,
        );
        s.to = B;
        s.value = U256::from(1u64);
        s.limits.data = Some(310);
        v.push(s);
    }
    // the stop is caught by the parent, the frame budget reverts the child alone
    for (tag, limit) in [("tx_limit", 310 + 100u64), ("budget", 310 + 4_000)] {
        let a =
            call_rec(Asm::new(), B, 0, 1).sstore(5, 1).push(1).push(0).op(op::LOG0).stop().build();
        let b = log0(Asm::new(), if tag == "tx_limit" { 200 } else { 3_980 }).stop().build();
        let mut s = scn(
            format!("port_trace_child_log_{tag}"),
            &rules,
            vec![acct(A, a), acct(B, b)],
            5_000_000,
        );
        s.limits.data = Some(limit);
        v.push(s);
    }
    // a precompile child frame: the KZG point evaluation at depth 1
    {
        let a = mem_call(
            Asm::new(),
            op::STATICCALL,
            Address::with_last_byte(0x0a),
            0,
            200_000,
            &[0x01; 192],
        )
        .record_call(1)
        .stop()
        .build();
        v.push(scn(
            "port_trace_precompile_child".to_string(),
            &["SATIN-PCMP-001", "SATIN-OUT-011"],
            vec![acct(A, a)],
            1_000_000,
        ));
    }
    // a delegated sender that funds then creates reports the net change
    {
        let init = vec![op::PUSH0, op::SELFDESTRUCT];
        let _ = init;
        let ctor = destruct(Asm::new(), crate::SENDER).build();
        let x = create2_addr(B, &ctor);
        let b = create2_of(call_all(Asm::new(), x, 1), &ctor, 0).op(op::POP).stop().build();
        let mut deleg = vec![0xef, 0x01, 0x00];
        deleg.extend_from_slice(B.as_slice());
        let mut s = scn(
            "port_trace_delegated_sender_funds_then_creates".to_string(),
            &["SATIN-ACCT-006", "SATIN-AUTH-003"],
            vec![funded(B, b, 0)],
            5_000_000,
        );
        s.accounts[0] = funded(crate::SENDER, deleg, 1_000_000_000_000_000_000);
        s.to = crate::SENDER;
        s.value = U256::from(5u64);
        v.push(s);
    }
    // below the cap the gas limit is the compute budget
    for d in [-1i64, 0, 1, 100] {
        let g = (2_000i64 * 14 + 21_000 + 28_000 + 3_000 + d) as u64;
        let mut code = Asm::new();
        code = work(code, 2_000);
        v.push(scn(
            format!("port_cg_gas_limit_is_budget_{}", d + 1),
            &["SATIN-DET-009", "SATIN-POOL-007"],
            vec![acct(A, code.stop().build())],
            g,
        ));
    }
    let _ = (C, D);
    v
}
