//! Scenarios of group "port": gas detention and volatile reads (the other implementation's
//! `detention`, `detained_transfers`, `volatile_access`, `withheld_gas` and `remaining_compute_gas`
//! tests that no scenario did yet).
#![allow(dead_code)]
use super::{A, Asm, B, C, D, EMPTY, LIMIT_CONTROL, ORACLE, Scenario, acct, funded, op, pp::*};
use crate::q8::{mem_call, sel, send_hint};
use alloy_primitives::{Address, U256, hex};

const R_DET: [&str; 4] = ["SATIN-DET-001", "SATIN-DET-009", "SATIN-DET-012", "SATIN-DET-014"];
const R_ORCL: [&str; 3] = ["SATIN-ORCL-002", "SATIN-DET-012", "SATIN-DET-013"];
const R_WH: [&str; 4] = ["SATIN-DET-011", "SATIN-DET-016", "SATIN-DET-017", "SATIN-DET-018"];
const R_REM: [&str; 3] = ["SATIN-DET-013", "SATIN-DET-014", "SATIN-MLC-001"];
const HISTORY: Address = alloy_primitives::address!("0x0000F90827F1C53a10cb7A02335B175320002935");
const BEACON: Address = alloy_primitives::address!("0x000F3df6D732807Ef1319fB7B8bB8522d0Beac02");
const HISTORY_CODE: &str = "3373fffffffffffffffffffffffffffffffffffffffe14604657602036036042575f35600143038111604257611fff81430311604257611fff9006545f5260205ff35b5f5ffd5b5f35611fff60014303065500";
const BEACON_CODE: &str = "3373fffffffffffffffffffffffffffffffffffffffe14604d57602036146024575f5ffd5b5f35801560495762001fff810690815414603c575f5ffd5b62001fff01545f5260205ff35b5f5ffd5b62001fff42064281555f359062001fff015500";
const BEN: Address = Address::with_last_byte(0xbe);

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(detention());
    v.extend(transfers());
    v.extend(volatile());
    v.extend(withheld());
    v.extend(remaining());
    v
}

/// The Oracle's program: read slot 0 and stop.
fn oracle_reader() -> Vec<u8> {
    Asm::new().push(0).op(op::SLOAD).op(op::POP).stop().build()
}

/// One volatile read, as code that leaves nothing on the stack.
fn reads() -> Vec<(&'static str, Asm)> {
    let one = |o: u8| Asm::new().op(o).op(op::POP);
    vec![
        ("timestamp", one(op::TIMESTAMP)),
        ("number", one(op::NUMBER)),
        ("coinbase", one(op::COINBASE)),
        ("gaslimit", one(0x45)),
        ("basefee", one(0x48)),
        ("blobbasefee", one(0x4a)),
        ("prevrandao", one(0x44)),
        ("slotnum", one(op::SLOTNUM)),
        ("blockhash", Asm::new().push(0).op(op::BLOCKHASH).op(op::POP)),
        ("balance_ben", Asm::new().push_addr(BEN).op(op::BALANCE).op(op::POP)),
        ("extcodesize_ben", Asm::new().push_addr(BEN).op(op::EXTCODESIZE).op(op::POP)),
        ("extcodehash_ben", Asm::new().push_addr(BEN).op(0x3f).op(op::POP)),
        ("call_ben", call_all(Asm::new(), BEN, 0)),
    ]
}

fn detention() -> Vec<Scenario> {
    let mut v = Vec::new();
    let base = |name: String, code: Vec<u8>, gas: u64| {
        let mut s = scn(
            name,
            &R_DET,
            vec![acct(A, code), funded(BEN, vec![], 1_000), acct(ORACLE, oracle_reader())],
            gas,
        );
        s.beneficiary = BEN;
        s
    };
    // every volatile read caps the transaction from where it read
    for (tag, read) in reads() {
        for gas in [100_000_000u64, 300_000_000] {
            let code =
                endless(read.clone().0.iter().fold(work(Asm::new(), 1_700), |a, b| a.op(*b)))
                    .build();
            v.push(base(format!("port_det_read_{tag}_{gas}"), code, gas));
        }
    }
    for gas in [100_000_000u64, 300_000_000] {
        let code = endless(call_all(work(Asm::new(), 1_700), ORACLE, 0)).build();
        v.push(base(format!("port_det_read_oracle_{gas}"), code, gas));
    }
    // the cap counts from a spend larger than itself
    for (tag, tail_stop) in [("loop", false), ("stop", true)] {
        let head = spin_loop(Asm::new(), 800_000).op(op::TIMESTAMP).op(op::POP);
        let code = if tail_stop { head.stop().build() } else { endless(head).build() };
        v.push(base(format!("port_det_cap_after_larger_spend_{tag}"), code, 100_000_000));
    }
    // other accounts and storage are not volatile
    {
        let mut x = Asm::new()
            .push_addr(C)
            .op(op::BALANCE)
            .op(op::POP)
            .push_addr(C)
            .op(op::EXTCODESIZE)
            .op(op::POP)
            .push_addr(C)
            .op(0x3f)
            .op(op::POP);
        x = x.push(1).op(op::SLOAD).op(op::POP);
        x = call_all(x, C, 0);
        x = x.call_with(op::STATICCALL, C, 0, 100_000, 0).op(op::POP);
        let mut s = scn(
            "port_det_other_accounts_not_volatile".to_string(),
            &R_DET,
            vec![acct(A, endless(x).build()), funded(C, vec![op::STOP], 5)],
            5_000_000,
        );
        s.limits.block_env_cap = Some(1_000);
        s.beneficiary = EMPTY;
        v.push(s);
    }
    // BLOBHASH is not volatile
    {
        let code = endless(Asm::new().push(0).op(0x49).op(op::POP)).build();
        let mut s = scn(
            "port_det_blobhash_not_volatile".to_string(),
            &R_DET,
            vec![acct(A, code)],
            5_000_000,
        );
        s.limits.block_env_cap = Some(1_000);
        v.push(s);
    }
    // the history contract reads the block number, the beacon roots contract reads state
    {
        let hist = hex::decode(HISTORY_CODE).unwrap();
        let beac = hex::decode(BEACON_CODE).unwrap();
        let mut word = |n: u64| {
            let mut w = [0u8; 32];
            w[24..].copy_from_slice(&n.to_be_bytes());
            w
        };
        let get =
            |to: Address, arg: [u8; 32]| mem_call(Asm::new(), op::STATICCALL, to, 0, 200_000, &arg);
        let _ = &mut word;
        let a1 = endless(get(HISTORY, word(0)).op(op::POP)).build();
        let a2 = endless(get(BEACON, word(1)).op(op::POP)).build();
        let off = crate::q8::sel("disableVolatileDataAccess()");
        let _ = off;
        for (tag, code, to) in [("history", a1, HISTORY), ("beacon", a2, BEACON)] {
            let mut s = scn(
                format!("port_det_real_system_contract_{tag}"),
                &["SATIN-DET-005", "SATIN-DET-004"],
                vec![acct(A, code), acct(HISTORY, hist.clone()), acct(BEACON, beac.clone())],
                100_000_000,
            );
            s.limits.block_env_cap = Some(5_000);
            let _ = to;
            v.push(s);
        }
    }
    // a read answers with the block's field
    {
        let mut a = Asm::new();
        for (i, o) in [op::NUMBER, op::TIMESTAMP, op::COINBASE, 0x45, 0x48, 0x4a, op::SLOTNUM, 0x44]
            .into_iter()
            .enumerate()
        {
            a = a.op(o).push(i as u64).op(op::SSTORE);
        }
        a = a
            .push(0)
            .op(op::BLOCKHASH)
            .push(8)
            .op(op::SSTORE)
            .push_addr(BEN)
            .op(op::BALANCE)
            .push(9)
            .op(op::SSTORE)
            .stop();
        let mut s = scn(
            "port_det_read_answers_field".to_string(),
            &R_DET,
            vec![acct(A, a.build()), funded(BEN, vec![], 77)],
            5_000_000,
        );
        s.beneficiary = BEN;
        v.push(s);
    }
    // the most restrictive limit binds whatever the order
    for (tag, oracle_first) in [("block_then_oracle", false), ("oracle_then_block", true)] {
        let mut a = work(Asm::new(), 200);
        if oracle_first {
            a = call_all(a, ORACLE, 0).op(op::TIMESTAMP).op(op::POP);
        } else {
            a = call_all(a.op(op::TIMESTAMP).op(op::POP), ORACLE, 0);
        }
        v.push(base(format!("port_det_most_restrictive_{tag}"), endless(a).build(), 100_000_000));
    }
    // spending exactly the cap completes; one gas more crosses
    for k in 10u64..=20 {
        let a = spin_loop(Asm::new().op(op::TIMESTAMP).op(op::POP), 769_230);
        let mut a = a;
        for _ in 0..k {
            a = a.op(op::JUMPDEST);
        }
        v.push(base(format!("port_det_exact_cap_{k}"), a.stop().build(), 100_000_000));
    }
    // a value-called child reads from its caller's whole charges
    {
        let child = endless(Asm::new().op(op::TIMESTAMP).op(op::POP)).build();
        let a = call_all(Asm::new(), B, 1).stop().build();
        v.push({
            let mut s = base("port_det_value_child_reads".to_string(), a, 100_000_000);
            s.accounts[0].balance = U256::from(1u64);
            s.accounts.push(acct(B, child.clone()));
            s
        });
        let mut s = base("port_det_value_tx_reads".to_string(), vec![], 100_000_000);
        s.to = B;
        s.value = U256::from(1u64);
        s.accounts.push(acct(B, child));
        v.push(s);
    }
    // stipend-funded work is compute; a returned value call counts what its callee ran
    {
        let child = endless(Asm::new()).build();
        let a = Asm::new()
            .op(op::TIMESTAMP)
            .op(op::POP)
            .call_with(op::CALL, B, 1, 0, 0)
            .op(op::POP)
            .stop()
            .build();
        let mut s = base("port_det_stipend_work_compute".to_string(), a, 10_000_000);
        s.accounts[0].balance = U256::from(1u64);
        s.accounts.push(acct(B, child));
        s.limits.block_env_cap = Some(1_000);
        v.push(s);
        let child = work(Asm::new(), 40).stop().build();
        for (tag, cap) in [("cap_low", 100u64), ("cap_mid", 400), ("cap_high", 100_000)] {
            let a = endless(
                Asm::new()
                    .op(op::TIMESTAMP)
                    .op(op::POP)
                    .call_with(op::CALL, B, 1, 0, 0)
                    .op(op::POP),
            )
            .build();
            let mut s = base(format!("port_det_returned_value_call_{tag}"), a, 10_000_000);
            s.accounts[0].balance = U256::from(1u64);
            s.accounts.push(acct(B, child.clone()));
            s.limits.block_env_cap = Some(cap);
            v.push(s);
        }
    }
    // a call that reads counts from what its caller paid
    for cap in [0u64, 1_000, 10_000, 100_000] {
        let a = endless(call_all(Asm::new(), BEN, 1)).build();
        let mut s = base(format!("port_det_call_beneficiary_value_{cap}"), a, 10_000_000);
        s.accounts[0].balance = U256::from(10u64);
        s.limits.block_env_cap = Some(cap);
        v.push(s);
    }
    // writes after a read stop at the cap or run out of their own gas
    for gas in [60_000_000u64, 200_000_000 + 1_700 * (SLOT_STATE + RECORD_HISTORY)] {
        let a = write_loop(Asm::new().op(op::TIMESTAMP).op(op::POP), 0, 1_700).stop().build();
        let mut s = base(format!("port_det_writes_after_read_{gas}"), a, gas);
        s.limits.growth = Some(u64::MAX);
        v.push(s);
    }
    // gas spilled before the read is not compute; a refill after the read does not lift the cap
    for gas in [100_000_000u64, 300_000_000] {
        let a = endless(write_loop(Asm::new(), 0, 100).op(op::TIMESTAMP).op(op::POP)).build();
        let mut s = base(format!("port_det_spill_before_read_{gas}"), a, gas);
        s.limits.block_env_cap = Some(30_000);
        s.limits.growth = Some(u64::MAX);
        v.push(s);
    }
    {
        let a = endless(Asm::new().sstore(1, 1).op(op::TIMESTAMP).op(op::POP).sstore(1, 0)).build();
        let mut s = base("port_det_refill_after_read".to_string(), a, 100_000_000);
        s.limits.block_env_cap = Some(30_000);
        v.push(s);
        let a = endless(Asm::new().sstore(1, 2).op(op::TIMESTAMP).op(op::POP).sstore(1, 1)).build();
        let mut s = base("port_det_history_refill_after_read".to_string(), a, 100_000_000);
        s.accounts[0].storage = vec![(1, 1)];
        s.limits.block_env_cap = Some(30_000);
        v.push(s);
    }
    // a start its caller cannot pay for counts none of its forward
    {
        let mids: [(&str, Vec<u8>); 3] = [
            ("value_call", call_all(Asm::new(), EMPTY, 1).stop().build()),
            ("create", Asm::new().create(&[op::STOP], 1).op(op::POP).stop().build()),
            (
                "create2",
                Asm::new()
                    .push(0)
                    .push(1)
                    .push(0)
                    .push(0)
                    .op(op::CREATE2)
                    .op(op::POP)
                    .stop()
                    .build(),
            ),
        ];
        for (kind, mid) in mids {
            for gas in [60_000u64, 120_000, 180_000, 240_000, 5_000_000] {
                let a = call_all(Asm::new().op(op::TIMESTAMP).op(op::POP), B, 0).stop().build();
                let mut s = base(format!("port_det_unpayable_start_{kind}_{gas}"), a, gas);
                s.accounts.push(funded(B, mid.clone(), 1));
                v.push(s);
            }
        }
    }
    // the most compute a transaction can spend bounds the caps
    for cap in [200_000_000u64, 199_999_000, 150_000_000] {
        let mut delegated = vec![0xef, 0x01, 0x00];
        delegated.extend_from_slice(BEN.as_slice());
        let mut s = scn(format!("port_det_max_compute_bounds_{cap}"), &R_DET, vec![], 400_000_000);
        s.accounts[0] = funded(crate::SENDER, delegated, 1_000_000_000_000_000_000);
        s.accounts.push(acct(BEN, endless(Asm::new()).build()));
        s.to = crate::SENDER;
        s.beneficiary = BEN;
        s.limits.block_env_cap = Some(cap);
        v.push(s);
    }
    v
}

fn transfers() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-DET-022", "SATIN-LOG-006", "SATIN-LIM-013"];
    // a precompile answered from its price at a data-size boundary
    {
        let mut input = vec![0u8; 192];
        input[31] = 32;
        input[63] = 32;
        input[95] = 32;
        input[96..128].fill(0xff);
        input[128..160].fill(0xff);
        let n = input.len() as u64;
        let a = Asm::new()
            .op(op::TIMESTAMP)
            .op(op::POP)
            .push(n)
            .push(0)
            .push(0)
            .op(op::CALLDATACOPY)
            .call_with(op::CALL, Address::with_last_byte(5), 1_000, 0, n)
            .op(op::POP)
            .stop()
            .build();
        let _ = a;
        let a = {
            let x = Asm::new()
                .op(op::TIMESTAMP)
                .op(op::POP)
                .push(n)
                .push(0)
                .push(0)
                .op(op::CALLDATACOPY);
            let x = x
                .push(0)
                .push(0)
                .push(n)
                .push(0)
                .push(1_000)
                .push_addr(Address::with_last_byte(5))
                .op(op::GAS)
                .op(op::CALL);
            x.op(op::POP).stop().build()
        };
        for (tag, extra) in [
            ("boundary_short", 310 + n + 240 - 1),
            ("boundary_fits", 310 + n + 240),
            ("ample", 1_000_000),
        ] {
            let mut s = scn(
                format!("port_dettr_precompile_price_{tag}"),
                &rules,
                vec![funded(A, a.clone(), 1_000)],
                100_000_000,
            );
            s.data = input.clone();
            s.limits.data = Some(extra);
            v.push(s);
        }
    }
    // a value call's own transfer crossing the cap stops its caller before anything moves
    for bal in [1_000u64, 999] {
        for target in ["receiver", "ecrecover"] {
            for cap in [100u64, 140, 160, 170, 180, 200, 10_000] {
                let to = if target == "receiver" { B } else { Address::with_last_byte(1) };
                let a = Asm::new()
                    .op(op::TIMESTAMP)
                    .op(op::POP)
                    .call_with(op::CALL, to, 1_000, 0, 0)
                    .op(op::POP)
                    .stop()
                    .build();
                let parent = call_rec(Asm::new(), A, 0, 1).stop().build();
                let mut s = scn(
                    format!("port_dettr_value_call_{bal}_{target}_{cap}"),
                    &rules,
                    vec![acct(C, parent), funded(A, a, bal), acct(B, vec![])],
                    10_000_000,
                );
                s.to = C;
                s.limits.block_env_cap = Some(cap);
                v.push(s);
            }
        }
    }
    // a SELFDESTRUCT's transfer log goes with the frame the cap stops
    for cap in [4_999u64, 5_004, 5_005, 5_006, 5_100, 8_000, 100_000] {
        let a = destruct(Asm::new().op(op::TIMESTAMP).op(op::POP), B).build();
        let mut s = scn(
            format!("port_dettr_selfdestruct_{cap}"),
            &rules,
            vec![funded(A, a, 1_000), acct(B, vec![op::STOP])],
            10_000_000,
        );
        s.limits.block_env_cap = Some(cap);
        v.push(s);
        let mut s = scn(
            format!("port_dettr_selfdestruct_after_call_{cap}"),
            &rules,
            vec![
                funded(
                    A,
                    endless(call_all(Asm::new().op(op::TIMESTAMP).op(op::POP), C, 0)).build(),
                    1_000,
                ),
                funded(C, destruct(Asm::new(), B).build(), 5),
                acct(B, vec![op::STOP]),
            ],
            10_000_000,
        );
        s.limits.block_env_cap = Some(cap);
        v.push(s);
    }
    v
}

fn volatile() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-DET-001", "SATIN-DET-002", "SATIN-DET-003"];
    // the transaction's own fields record nothing
    {
        let mut a = Asm::new();
        for o in [0x32u8, op::CALLER, op::CALLVALUE, 0x3a, 0x46, op::ADDRESS, op::SELFBALANCE] {
            a = a.op(o).op(op::POP);
        }
        let mut s = scn(
            "port_vol_own_fields_nothing".to_string(),
            &rules,
            vec![acct(A, endless(a).build())],
            5_000_000,
        );
        s.limits.block_env_cap = Some(1_000);
        v.push(s);
    }
    // a read two frames down caps every caller
    {
        let a = endless(call_all(Asm::new(), B, 0)).build();
        let b = call_all(Asm::new(), C, 0).build();
        let b = {
            let mut x = b;
            x.push(op::STOP);
            x
        };
        let c = Asm::new().op(op::TIMESTAMP).op(op::POP).stop().build();
        let mut s = scn(
            "port_vol_two_frames_down".to_string(),
            &rules,
            vec![acct(A, a), acct(B, b), acct(C, c)],
            100_000_000,
        );
        v.push(s.clone());
        let b2 = endless(call_all(Asm::new(), C, 0)).build();
        s.name = "port_vol_two_frames_down_middle_loops";
        s.accounts[2].code = b2;
        v.push(s);
    }
    // a read in a frame that reverts still caps
    {
        let a = endless(call_all(Asm::new(), B, 0)).build();
        let b = Asm::new().op(op::TIMESTAMP).op(op::POP).revert0().build();
        v.push(scn(
            "port_vol_read_in_reverted_frame".to_string(),
            &rules,
            vec![acct(A, a), acct(B, b)],
            100_000_000,
        ));
    }
    // the Oracle without reading its storage detains nothing; reading it, it does
    {
        let a = endless(call_all(Asm::new(), ORACLE, 0)).build();
        let mut s = scn(
            "port_vol_oracle_no_read".to_string(),
            &R_ORCL,
            vec![acct(A, a.clone()), acct(ORACLE, vec![op::STOP])],
            5_000_000,
        );
        s.limits.oracle_cap = Some(1_000);
        v.push(s);
        let reader = endless(Asm::new().push(0).op(op::SLOAD).op(op::POP)).build();
        let direct = |name: &'static str, kind: u8| {
            let a =
                endless(Asm::new().call_with(kind, ORACLE, 0, 1_000_000, 0).op(op::POP)).build();
            let mut s =
                sc(name, &R_ORCL, vec![acct(A, a), acct(ORACLE, reader.clone())], 100_000_000);
            s.limits.oracle_cap = Some(100_000);
            s
        };
        v.push(direct("port_vol_oracle_reads_call", op::CALL));
        v.push(direct("port_vol_oracle_reads_staticcall", op::STATICCALL));
        let mut s = sc(
            "port_vol_oracle_reads_direct",
            &R_ORCL,
            vec![acct(ORACLE, reader.clone())],
            100_000_000,
        );
        s.to = ORACLE;
        s.limits.oracle_cap = Some(100_000);
        v.push(s);
        // the Oracle's code on another account's storage reads nothing volatile
        let reader_stop = Asm::new().push(0).op(op::SLOAD).op(op::POP).stop().build();
        for (tag, kind) in [("delegatecall", op::DELEGATECALL), ("callcode", op::CALLCODE)] {
            let a = endless(Asm::new().call_with(kind, ORACLE, 0, 100_000, 0).op(op::POP)).build();
            let mut s = scn(
                format!("port_vol_oracle_code_elsewhere_{tag}"),
                &R_ORCL,
                vec![acct(A, a), acct(ORACLE, reader_stop.clone())],
                5_000_000,
            );
            s.limits.oracle_cap = Some(1_000);
            v.push(s);
        }
        // a hint detains nothing
        let hint = send_hint(1, &[0x33; 32]);
        let a =
            endless(mem_call(Asm::new(), op::CALL, ORACLE, 0, 100_000, &hint).op(op::POP)).build();
        let mut s = scn(
            "port_vol_hint_detains_nothing".to_string(),
            &R_ORCL,
            vec![acct(A, a), acct(ORACLE, vec![op::STOP])],
            5_000_000,
        );
        s.limits.oracle_cap = Some(1_000);
        s.limits.block_env_cap = Some(1_000);
        v.push(s);
    }
    // the system address is not detained; the same body as a deposit is
    {
        let body = work(Asm::new().push(0).op(op::SLOAD).op(op::POP), 400).stop().build();
        let mut s = crate::edge::from_system(
            "port_vol_system_not_detained",
            crate::edge::system_account(vec![], 0),
            ORACLE,
        );
        s.group = "port";
        s.rules = vec!["SATIN-DET-026", "SATIN-MST-008"];
        s.chain_id = Some(1);
        s.accounts.retain(|x| x.address != ORACLE);
        s.accounts.push(acct(ORACLE, body.clone()));
        s.limits.oracle_cap = Some(1_000);
        v.push(s);
        let mut s = scn(
            "port_vol_deposit_detained".to_string(),
            &["SATIN-DET-026", "SATIN-MST-010"],
            vec![acct(ORACLE, body)],
            1_000_000,
        );
        s.to = ORACLE;
        s.deposit = Some(0);
        s.limits.oracle_cap = Some(1_000);
        v.push(s);
    }
    // a cap below the intrinsic gas stops nothing before the first frame
    for (tag, code) in [
        ("completes", Asm::new().sstore(1, 1).stop().build()),
        ("loops", endless(Asm::new()).build()),
    ] {
        let mut s = scn(
            format!("port_vol_cap_below_intrinsic_{tag}"),
            &["SATIN-DET-008", "SATIN-AUTH-007"],
            vec![acct(A, code)],
            1_000_000,
        );
        s.auths = vec![crate::Auth {
            chain_id: 1,
            address: D,
            nonce: 0,
            authority: Some(Address::new([0x71; 20])),
        }];
        s.limits.block_env_cap = Some(1_000);
        v.push(s);
    }
    v
}

fn withheld() -> Vec<Scenario> {
    let mut v = Vec::new();
    // a bounded callee writes what it writes without the read
    for (tag, gas) in [("below", 5_000_000u64), ("above", 300_000_000)] {
        for read in [true, false] {
            let pre = if read {
                Asm::new().op(op::TIMESTAMP).op(op::POP)
            } else {
                Asm::new().push(0).op(op::POP)
            };
            let a = call_rec(pre, B, 0, 1);
            let a = {
                let _ = a;
                let pre = if read {
                    Asm::new().op(op::TIMESTAMP).op(op::POP)
                } else {
                    Asm::new().push(0).op(op::POP)
                };
                pre.call_with(op::CALL, B, 0, 3 * 400 + 5_000, 0).record_call(1).stop().build()
            };
            let mut s = scn(
                format!("port_wh_bounded_callee_{tag}_{}", if read { "read" } else { "noread" }),
                &R_WH,
                vec![acct(A, a), acct(B, fresh3())],
                gas,
            );
            s.limits.block_env_cap = Some(100_000);
            v.push(s);
        }
    }
    // the settlement counts the withheld gas as unspent (refund capped at a fifth)
    for read in [true, false] {
        let mut a = if read {
            Asm::new().op(op::TIMESTAMP).op(op::POP)
        } else {
            Asm::new().push(0).op(op::POP)
        };
        for i in 0..16u64 {
            a = a.sstore(i + 1, 1).sstore(i + 1, 0);
        }
        let mut s = scn(
            format!("port_wh_settlement_unspent_{}", if read { "read" } else { "noread" }),
            &R_WH,
            vec![acct(A, a.stop().build())],
            5_000_000,
        );
        s.limits.block_env_cap = Some(1_000_000);
        s.gas_price = 1;
        v.push(s);
    }
    // what a halting callee burns is not compute
    {
        let halts: Vec<(&'static str, Vec<u8>)> = vec![
            ("invalid", vec![op::INVALID]),
            (
                "memory_16mib",
                Asm::new().push(1).push_bytes(&[1, 0, 0, 0]).op(op::MSTORE).stop().build(),
            ),
            ("operand_huge", Asm::new().push_bytes(&[0xff; 9]).op(op::MLOAD).stop().build()),
            (
                "log_huge",
                Asm::new().push_bytes(&[1, 0, 0, 0, 0, 0]).push(0).op(op::LOG0).stop().build(),
            ),
            (
                "keccak_huge",
                Asm::new().push_bytes(&[1, 0, 0, 0, 0, 0]).push(0).op(0x20).stop().build(),
            ),
            (
                "calldatacopy_huge",
                Asm::new()
                    .push_bytes(&[1, 0, 0, 0, 0, 0])
                    .push(0)
                    .push(0)
                    .op(op::CALLDATACOPY)
                    .stop()
                    .build(),
            ),
            (
                "codecopy_huge",
                Asm::new()
                    .push_bytes(&[1, 0, 0, 0, 0, 0])
                    .push(0)
                    .push(0)
                    .op(op::CODECOPY)
                    .stop()
                    .build(),
            ),
            ("value_call_records", call_all(Asm::new(), EMPTY, 1).stop().build()),
            (
                "precompile_rejects",
                Asm::new()
                    .call_with(op::CALL, Address::with_last_byte(0x0a), 0, 100_000, 0)
                    .op(op::POP)
                    .op(op::INVALID)
                    .build(),
            ),
        ];
        for (kind, code) in halts {
            let a = call_rec(Asm::new().op(op::TIMESTAMP).op(op::POP), B, 0, 1);
            let a = {
                let _ = a;
                work(Asm::new().op(op::TIMESTAMP).op(op::POP), 100)
                    .call_with(op::CALL, B, 0, 400_000, 0)
                    .record_call(1)
                    .stop()
                    .build()
            };
            let mut s = scn(
                format!("port_wh_halt_burn_{kind}"),
                &R_WH,
                vec![acct(A, a), funded(B, code, 1)],
                5_000_000,
            );
            s.limits.block_env_cap = Some(2_000_000);
            v.push(s);
        }
    }
    // an EXP / RETURNDATACOPY / static charge that cannot be paid burns what its frame had
    {
        let exp = Asm::new().push_bytes(&[0xff; 32]).push(2).op(0x0a).stop().build();
        let rdc = Asm::new()
            .call_with(op::STATICCALL, C, 0, 100_000, 0)
            .op(op::POP)
            .push(500 * 32)
            .push(0)
            .push(0)
            .op(op::RETURNDATACOPY)
            .stop()
            .build();
        let ret = Asm::new().push(500 * 32).push(0).op(op::RETURN).build();
        let sd = destruct(Asm::new(), Address::with_last_byte(0x66)).build();
        let cases: [(&'static str, Vec<u8>, u64, u64); 3] = [
            ("exp", exp, 1_500, 100_000),
            ("returndatacopy", rdc, 3_500, 300_000),
            ("static_selfdestruct", sd, 5_002, 200_000),
        ];
        for (kind, code, fwd, cap) in cases {
            let mut a = Asm::new().op(op::TIMESTAMP).op(op::POP);
            for _ in 0..100 {
                a = call(a, B, 0, fwd);
            }
            let mut s = scn(
                format!("port_wh_unpayable_{kind}"),
                &R_WH,
                vec![acct(A, a.stop().build()), funded(B, code, 5), acct(C, ret.clone())],
                10_000_000,
            );
            s.limits.block_env_cap = Some(cap);
            v.push(s);
        }
        for cap in [3_400u64, 6_040] {
            for (tag, gas) in [("below", 5_000_000u64), ("above", 300_000_000)] {
                let sd = destruct(Asm::new(), Address::with_last_byte(0x66)).build();
                let a = call(Asm::new().op(op::TIMESTAMP).op(op::POP), B, 0, 5_002).stop().build();
                let mid = call(Asm::new(), C, 0, 5_002).stop().build();
                let chain =
                    call(Asm::new().op(op::TIMESTAMP).op(op::POP), D, 0, 12_000).stop().build();
                let mut s = scn(
                    format!("port_wh_static_halt_{cap}_{tag}"),
                    &R_WH,
                    vec![
                        acct(A, a),
                        funded(B, sd.clone(), 5),
                        funded(C, sd.clone(), 5),
                        acct(D, destruct(Asm::new(), C).build()),
                    ],
                    gas,
                );
                s.limits.block_env_cap = Some(cap);
                v.push(s.clone());
                s.name = nm(format!("port_wh_static_halt_chain_{cap}_{tag}"));
                s.accounts[0].code = chain;
                s.accounts[3].code = mid;
                v.push(s);
            }
        }
    }
    // a halt burns what its frame had when it halted
    {
        let b = {
            let mut x = Asm::new().push(1000).push(0).op(0x20).op(op::POP);
            x = work(x, 5_000);
            x.op(op::INVALID).build()
        };
        let a = endless(call_all(Asm::new().op(op::TIMESTAMP).op(op::POP), B, 0)).build();
        let mut s = scn(
            "port_wh_halt_burn_at_invalid".to_string(),
            &R_WH,
            vec![acct(A, a), acct(B, b)],
            20_000_000,
        );
        s.limits.block_env_cap = Some(5_000_000);
        v.push(s);
    }
    // a call that reads the beneficiary costs what a warm account costs
    for (tag, gas_arg, value) in
        [("zero", 0u64, 0u64), ("stipend", 2_300, 1), ("all", 0, 1), ("value_zero_all", 0, 0)]
    {
        for who in ["beneficiary", "warm_empty"] {
            let target = if who == "beneficiary" { BEN } else { EMPTY };
            let mut a = Asm::new().push_addr(EMPTY).op(op::BALANCE).op(op::POP);
            a = if gas_arg == 0 && tag.contains("all") {
                a.call_all(op::CALL, target, value)
            } else {
                a.call_with(op::CALL, target, value, gas_arg, 0)
            };
            let mut s = scn(
                format!("port_wh_call_beneficiary_{tag}_{who}"),
                &R_WH,
                vec![funded(A, a.op(op::POP).stop().build(), 5), funded(BEN, vec![], 1)],
                5_000_000,
            );
            s.beneficiary = BEN;
            v.push(s);
        }
    }
    // a precompile priced past the cap computes nothing
    for (tag, big) in [("58m", 0x30u8), ("125m", 0x40)] {
        let mut input = vec![0u8; 96 + 64];
        input[31] = 32;
        input[63] = 32;
        input[95] = 32;
        input[96..128].fill(0xff);
        input[128..160].fill(big);
        let a = Asm::new()
            .op(op::TIMESTAMP)
            .op(op::POP)
            .push(160)
            .push(0)
            .push(0)
            .op(op::CALLDATACOPY)
            .push(0)
            .push(0)
            .push(160)
            .push(0)
            .push_addr(Address::with_last_byte(5))
            .op(op::GAS)
            .op(op::CALL)
            .push(1)
            .op(op::SSTORE)
            .stop()
            .build();
        for (gtag, gas) in [("below", 5_000_000u64), ("above", 300_000_000)] {
            let mut s = scn(
                format!("port_wh_precompile_past_cap_{tag}_{gtag}"),
                &R_WH,
                vec![acct(A, a.clone())],
                gas,
            );
            s.data = input.clone();
            v.push(s);
        }
    }
    // the KZG point evaluation is priced and keeps no clamp
    {
        let a = Asm::new()
            .op(op::TIMESTAMP)
            .op(op::POP)
            .call_with(op::CALL, Address::with_last_byte(0x0a), 0, 80_000, 0)
            .push(1)
            .op(op::SSTORE)
            .stop()
            .build();
        let mut s = scn("port_wh_kzg_priced".to_string(), &R_WH, vec![acct(A, a)], 5_000_000);
        s.limits.block_env_cap = Some(50_000);
        v.push(s);
    }
    // a refused keyless call's spilled state gas is not compute
    for cap in [50_000u64, 100_000, 400_000] {
        for (tag, gas) in [("below", 5_000_000u64), ("above", 300_000_000)] {
            let (data, _signer, _) = crate::q8::keyless_call("ok", 0);
            let mut s = scn(format!("port_wh_keyless_refused_{cap}_{tag}"), &R_WH, vec![], gas);
            s.to = crate::KEYLESS;
            s.data = data;
            s.sender = BEN;
            s.accounts.push(funded(BEN, vec![], 1_000_000_000_000_000_000));
            s.beneficiary = BEN;
            s.limits.block_env_cap = Some(cap);
            v.push(s);
        }
    }
    v
}

fn fresh3() -> Vec<u8> {
    writes(Asm::new(), 0, 3).stop().build()
}

fn remaining() -> Vec<Scenario> {
    let mut v = Vec::new();
    let query = |slot: u64| {
        Asm::new()
            .mstore_selector(sel("remainingComputeGas()"))
            .call_with(op::STATICCALL, LIMIT_CONTROL, 0, 100_000, 4)
            .op(op::POP)
            .store_returndata_word(slot)
    };
    // a contract's query below and above the execution cap
    for (tag, gas) in [("below", 1_000_000u64), ("above", 300_000_000)] {
        let a = query(1).stop().build();
        v.push(scn(format!("port_rcg_contract_query_{tag}"), &R_REM, vec![acct(A, a)], gas));
        let a = call_all(Asm::new().op(op::TIMESTAMP).op(op::POP), B, 0).stop().build();
        v.push(scn(
            format!("port_rcg_detained_contract_query_{tag}"),
            &R_REM,
            vec![acct(A, a), acct(B, query(1).stop().build())],
            gas,
        ));
    }
    // a detained caller with less than the allowance hears its own gas
    {
        let a = Asm::new().op(op::TIMESTAMP).op(op::POP).ops(&query(1).0).stop().build();
        v.push(scn(
            "port_rcg_detained_less_than_allowance".to_string(),
            &R_REM,
            vec![acct(A, a)],
            200_000,
        ));
    }
    // a child's read binds its caller's answer
    {
        let b = Asm::new().op(op::TIMESTAMP).op(op::POP).stop().build();
        let a = call_all(Asm::new(), B, 0).ops(&query(1).0).stop().build();
        let mut s = scn(
            "port_rcg_child_read_binds_caller".to_string(),
            &R_REM,
            vec![acct(A, a), acct(B, b)],
            10_000_000,
        );
        s.limits.block_env_cap = Some(50_000);
        v.push(s);
    }
    // the answer is what the caller resumes with
    {
        let shapes: Vec<(&'static str, Vec<u8>, bool)> = vec![
            ("undetained", query(1).stop().build(), false),
            (
                "after_read",
                Asm::new().op(op::TIMESTAMP).op(op::POP).ops(&query(1).0).stop().build(),
                false,
            ),
            (
                "after_halting_reader",
                Asm::new()
                    .call_with(op::CALL, C, 0, 100_000, 0)
                    .op(op::POP)
                    .ops(&query(1).0)
                    .stop()
                    .build(),
                false,
            ),
            (
                "after_value_call",
                Asm::new()
                    .call_with(op::CALL, EMPTY, 1, 50_000, 0)
                    .op(op::POP)
                    .ops(&query(1).0)
                    .stop()
                    .build(),
                false,
            ),
            ("beneficiary_sender", query(1).stop().build(), true),
        ];
        let halting_reader = Asm::new().op(op::TIMESTAMP).op(op::POP).op(op::INVALID).build();
        for (tag, code, ben_sender) in shapes {
            let mut s = scn(
                format!("port_rcg_answer_resume_{tag}"),
                &R_REM,
                vec![funded(A, code, 5), acct(C, halting_reader.clone())],
                10_000_000,
            );
            if ben_sender {
                s.beneficiary = crate::SENDER;
            }
            s.limits.block_env_cap = Some(100_000);
            v.push(s);
        }
    }
    // a value-called callee hears its stipend whether or not detained
    for read in [false, true] {
        for fwd in [0u64, 10_000, 50_000] {
            let b = query(1).stop().build();
            let pre = if read { Asm::new().op(op::TIMESTAMP).op(op::POP) } else { Asm::new() };
            let a = pre.call_with(op::CALL, B, 1, fwd, 0).op(op::POP).stop().build();
            let mut s = scn(
                format!(
                    "port_rcg_value_callee_stipend_{}_{fwd}",
                    if read { "read" } else { "noread" }
                ),
                &R_REM,
                vec![funded(A, a, 5), acct(B, b)],
                10_000_000,
            );
            s.limits.block_env_cap = Some(100_000);
            v.push(s);
        }
    }
    // a caller can spend the answer and no more
    for (tag, delta) in [("under", -20i64), ("exact_ish", -1), ("over", 40)] {
        let a = Asm::new().op(op::TIMESTAMP).op(op::POP).ops(&query(1).0);
        // spin for (answer + delta) gas: the answer is the cap less what ran since the read
        let spin = (50_000i64 + delta * 26) / 26;
        let a = spin_loop(a, spin.max(1) as u64).stop().build();
        let mut s =
            scn(format!("port_rcg_spend_the_answer_{tag}"), &R_REM, vec![acct(A, a)], 10_000_000);
        s.limits.block_env_cap = Some(50_000);
        v.push(s);
    }
    v
}
