//! Scenarios of group "port" and "blk": keyless deployment (the other implementation's
//! `system/keyless/*` and `limit_matrix/keyless` tests that no scenario did yet) and the Oracle's
//! interception (`system/oracle*`). The creation transactions are pre-signed in
//! [`crate::port_keyless_txs`].
#![allow(dead_code)]
use super::{
    A, Account, Asm, B, C, KEYLESS, LIMIT_CONTROL, ORACLE, Scenario, acct, block, funded, op, pp::*,
};
use crate::{
    port_keyless_txs::{BIG, KTXS},
    q8::{ACCESS_CONTROL, keyless_call_raw, keyless_tx, mem_call, sel, send_hint},
};
use alloy_primitives::{Address, U256, hex};

const R_KEY: [&str; 4] = ["SATIN-KEY-002", "SATIN-KEY-005", "SATIN-KEY-011", "SATIN-KEY-014"];
const R_KLIM: [&str; 3] = ["SATIN-KEY-012", "SATIN-KEY-016", "SATIN-LIM-003"];
const R_KDET: [&str; 3] = ["SATIN-KEY-010", "SATIN-KEY-018", "SATIN-DET-009"];
const R_ORCL: [&str; 4] = ["SATIN-ORCL-006", "SATIN-ORCL-008", "SATIN-ORCL-009", "SATIN-ORCL-010"];
const BEN: Address = Address::with_last_byte(0xbe);

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(deploys());
    v.extend(rules());
    v.extend(limits());
    v.extend(detention());
    v.extend(precedence());
    v.extend(oracle());
    v
}

/// The pre-signed transaction `name` of this module (raw bytes, signer, gas limit, value).
fn ktx(name: &str) -> (Vec<u8>, Address, u64, u64) {
    let t = KTXS.iter().find(|t| t.0 == name).unwrap_or_else(|| panic!("no keyless tx {name}"));
    (hex::decode(t.1).unwrap(), Address::from_slice(&hex::decode(t.2).unwrap()), t.3, t.4)
}

/// The big transactions: a zero init code of the stated length.
fn big(name: &str) -> (Vec<u8>, Address) {
    let t = BIG.iter().find(|t| t.0 == name).unwrap();
    let mut raw = hex::decode(t.1).unwrap();
    raw.extend(vec![0u8; t.3]);
    raw.extend(hex::decode(t.2).unwrap());
    (raw, Address::from_slice(&hex::decode(t.4).unwrap()))
}

/// A call of `keylessDeploy(raw, ovr)` at `gas`.
fn kl(name: String, rules: &[&'static str], raw: &[u8], ovr: u64, gas: u64) -> Scenario {
    let mut s = scn(name, rules, vec![], gas);
    s.to = KEYLESS;
    s.data = keyless_call_raw(raw, ovr);
    s
}

/// A call of one of this module's pre-signed transactions.
fn kt(name: String, rules: &[&'static str], tx: &str, ovr: u64, gas: u64) -> (Scenario, Address) {
    let (raw, signer, _, _) = ktx(tx);
    (kl(name, rules, &raw, ovr, gas), signer)
}

fn deploys() -> Vec<Scenario> {
    let mut v = Vec::new();
    // the canonical CREATE2 factory (Nick's method) and a registry-sized creation
    for (tag, gas) in [("below", 5_000_000u64), ("above", 300_000_000)] {
        let (s, _) = kt(
            format!("port_kdeploy_create2_factory_{tag}"),
            &R_KEY,
            "create2_factory",
            100_000,
            gas,
        );
        v.push(s);
    }
    let (s, _) = kt(
        "port_kdeploy_registry_sized".to_string(),
        &R_KEY,
        "erc1820_like",
        3_000_000,
        20_000_000,
    );
    v.push(s);
    // the call pays the CREATE opcode's regular gas: init code of the maximum size and none
    for name in ["init_max", "init_over"] {
        let (raw, _) = big(name);
        v.push(kl(format!("port_kdeploy_{name}"), &R_KEY, &raw, 5_000_000, 300_000_000));
    }
    for (tag, tx) in [("empty_init", "returns_empty"), ("ret1", "noop_ret1")] {
        let (s, _) =
            kt(format!("port_kdeploy_regular_gas_{tag}"), &R_KEY, tx, 1_000_000, 20_000_000);
        v.push(s);
    }
    // gas used counts the creation's spend before its refund
    {
        let (s, _) = kt(
            "port_kdeploy_refund_before_gas_used".to_string(),
            &R_KEY,
            "refund",
            1_000_000,
            20_000_000,
        );
        v.push(s);
    }
    // a failed deployment gives the created account back at its price (SALT on both)
    {
        let (raw, signer) = keyless_tx("revert");
        let mut s = kl(
            "port_kdeploy_failed_gives_back_at_price".to_string(),
            &R_KEY,
            &raw,
            1_000_000,
            5_000_000,
        );
        s.salt = vec![(signer.create(0), None, 8), (signer, None, 2)];
        v.push(s);
    }
    // a constructor out of gas is an execution halt, below and above the cap
    for (tag, gas) in [("below", 20_000_000u64), ("above", 300_000_000)] {
        let (raw, _) = keyless_tx("loop");
        v.push(kl(format!("port_kdeploy_constructor_oog_{tag}"), &R_KEY, &raw, 100_000, gas));
    }
    // an empty-code deployment leaves the address deployable (a block of two)
    {
        let (raw, _) =
            ktx("returns_empty").0.clone().into_iter().fold((Vec::new(), ()), |(mut a, ()), b| {
                a.push(b);
                (a, ())
            });
        let first =
            kl("port_kdeploy_empty_then_deploys".to_string(), &R_KEY, &raw, 5_000_000, 20_000_000);
        let raw2 = raw.clone();
        v.extend(block(
            first,
            vec![Box::new(move |t: &mut Scenario| {
                t.data = keyless_call_raw(&raw2, 1_000_000);
            })],
        ));
    }
    // the constructor's own creation may return empty code; an init that destroys itself
    for (name, tx, value) in [
        ("port_kdeploy_constructor_child_empty", "child_empty", 0u64),
        ("port_kdeploy_selfdestructs_to_beneficiary", "selfdestruct_beneficiary", 5),
        ("port_kdeploy_destroyed_via_delegatecall", "destroyed_delegate", 0),
        ("port_kdeploy_code_ef", "ef_code", 0),
        ("port_kdeploy_code_over_limit", "oversize_code", 0),
        ("port_kdeploy_modifies_other_state", "call_writes", 0),
        ("port_kdeploy_reads_existing", "reads_other", 0),
        ("port_kdeploy_failed_init_no_storage", "sstore_revert", 0),
        ("port_kdeploy_empty_code_no_log", "log_empty_code", 0),
        ("port_kdeploy_failed_no_log", "log_then_revert", 0),
        ("port_kdeploy_nested_keyless", "nested_keyless", 0),
        ("port_kdeploy_constructor_hint", "hint_ok", 0),
    ] {
        let (mut s, signer) =
            kt(nm(name.to_string()).to_string(), &R_KEY, tx, 5_000_000, 20_000_000);
        s.name = nm(name.to_string());
        s.accounts.push(acct(B, Asm::new().sstore(7, 9).stop().build()));
        s.accounts.push(acct(ORACLE, vec![op::STOP]));
        if value > 0 {
            s.accounts.push(funded(signer, vec![], value));
        }
        v.push(s);
    }
    // the beneficiary receives fees on an execution failure
    {
        let (raw, _) = keyless_tx("revert");
        let mut s = kl(
            "port_kdeploy_beneficiary_fees_on_failure".to_string(),
            &R_KEY,
            &raw,
            1_000_000,
            5_000_000,
        );
        s.gas_price = 1;
        s.beneficiary = BEN;
        v.push(s);
    }
    // a creation stopped on its transfer log before the value moves
    for (tag, limit) in [("holds_records_not_log", 310 + 3_000u64), ("ample", 1_000_000)] {
        let (raw, signer, _, _) = ktx("value5");
        let (raw, signer) = (raw, signer);
        let mut s = kl(
            format!("port_kdeploy_value_transfer_log_{tag}"),
            &R_KEY,
            &raw,
            1_000_000,
            20_000_000,
        );
        s.data = keyless_call_raw(&raw, 1_000_000);
        s.accounts.push(funded(signer, vec![], 5));
        s.limits.data = Some(limit);
        v.push(s);
    }
    // a destruction an earlier transaction made is not the deployment's
    {
        let probe = Address::with_last_byte(0x7a);
        let (raw, signer, _, _) = ktx("selfdestruct_self");
        let mut first =
            kl("port_kdeploy_earlier_destruction".to_string(), &R_KEY, &raw, 5_000_000, 20_000_000);
        first.accounts.push(acct(B, vec![]));
        let raw1 = raw.clone();
        let _ = (signer, probe);
        v.extend(block(
            first,
            vec![
                Box::new(move |t: &mut Scenario| {
                    t.to = probe;
                    t.value = U256::from(1u64);
                    t.data = vec![];
                }),
                Box::new(move |t: &mut Scenario| {
                    t.to = KEYLESS;
                    t.value = U256::ZERO;
                    t.data = keyless_call_raw(&raw1, 5_000_000);
                }),
            ],
        ));
    }
    v
}

fn rules() -> Vec<Scenario> {
    let mut v = Vec::new();
    // the override equal to the signed gas limit; a forward capped below the signed gas limit
    {
        let (raw, _, g, _) = ktx("short_spend");
        let _ = g;
        v.push(kl(
            "port_krule_override_equals_signed".to_string(),
            &R_KEY,
            &raw,
            60_000,
            20_000_000,
        ));
        for gas in [150_000u64, 200_000, 250_000, 300_000, 400_000] {
            let (raw, signer) = keyless_tx("ok");
            let mut s = kl(format!("port_krule_forward_{gas}"), &R_KEY, &raw, 200_000, gas);
            s.accounts.push(funded(signer, vec![], 1));
            v.push(s);
            let mut s =
                kl(format!("port_krule_forward_absent_signer_{gas}"), &R_KEY, &raw, 200_000, gas);
            s.name = nm(format!("port_krule_forward_absent_signer_{gas}"));
            v.push(s);
        }
    }
    // signatures and envelopes
    for (name, tx) in [
        ("port_krule_r_over_order", "r_over_order"),
        ("port_krule_r_zero", "r_zero"),
        ("port_krule_modified_s", "ok_other_s"),
        ("port_krule_typed_envelope", "typed_envelope"),
        ("port_krule_not_creation", "not_creation"),
    ] {
        let (s, _) = kt(name.to_string(), &R_KEY, tx, 1_000_000, 5_000_000);
        v.push(s);
    }
    // init code over / at the limit
    // (the zero init code of the limit deploys nothing; one byte more is refused)
    for name in ["init_max", "init_over"] {
        let (raw, _) = big(name);
        let mut s = kl(format!("port_krule_{name}_5m"), &R_KEY, &raw, 5_000_000, 20_000_000);
        s.gas_limit = 1_000_000_000;
        v.push(s);
    }
    // failing attempts by anybody leave the address deployable (a block)
    {
        let (raw, _) = keyless_tx("ok");
        let first = kl("port_krule_failing_attempts".to_string(), &R_KEY, &raw, 30_000, 5_000_000);
        let raw2 = raw.clone();
        let raw3 = raw.clone();
        v.extend(block(
            first,
            vec![
                Box::new(move |t: &mut Scenario| t.data = keyless_call_raw(&raw2, 30_000)),
                Box::new(move |t: &mut Scenario| t.data = keyless_call_raw(&raw3, 1_000_000)),
            ],
        ));
    }
    // a failure from nonce one keeps no record of the signer; every failure from nonce one leaves
    // it at one
    for (name, tx) in [
        ("port_krule_nonce_one_revert", "revert_n1"),
        ("port_krule_nonce_one_halt", "halt_n1"),
        ("port_krule_nonce_one_empty", "empty_n1"),
        ("port_krule_nonce_one_stopped_at_start", "noop_ret1_n1"),
    ] {
        let (mut s, signer) = kt(name.to_string(), &R_KEY, tx, 1_000_000, 5_000_000);
        s.accounts.push(crate::Account {
            address: signer,
            nonce: 1,
            balance: U256::from(3u64),
            ..Account::default()
        });
        if name.ends_with("stopped_at_start") {
            s.limits.data = Some(310 + 1_000 + 200);
        }
        v.push(s);
    }
    // a delegated signer's nonces stay spent
    for (tag, delegate) in [
        (
            "creates_twice",
            Asm::new()
                .create(&[op::STOP], 0)
                .op(op::POP)
                .create(&[op::STOP], 0)
                .op(op::POP)
                .stop()
                .build(),
        ),
        (
            "reverting_create",
            Asm::new().create(&[op::PUSH0, op::PUSH0, op::REVERT], 0).op(op::POP).stop().build(),
        ),
    ] {
        let (mut s, signer) = kt(
            format!("port_krule_delegated_signer_{tag}"),
            &R_KEY,
            "calls_signer",
            1_000_000,
            5_000_000,
        );
        let mut code = vec![0xef, 0x01, 0x00];
        code.extend_from_slice(C.as_slice());
        s.accounts.push(Account { address: signer, code, ..Account::default() });
        s.accounts.push(acct(C, delegate));
        v.push(s);
    }
    // the signer's balance and a call that cannot pay the signer's account
    for (tag, bal) in [("partial", 3u64), ("exact", 5)] {
        let (mut s, signer) =
            kt(format!("port_krule_balance_{tag}"), &R_KEY, "value5", 1_000_000, 5_000_000);
        s.accounts.push(funded(signer, vec![], bal));
        v.push(s);
    }
    for gas in [60_000u64, 80_000, 100_000, 120_000, 140_000] {
        let (raw, _) = keyless_tx("ok");
        v.push(kl(
            format!("port_krule_cannot_pay_signer_account_{gas}"),
            &R_KEY,
            &raw,
            200_000,
            gas,
        ));
    }
    for gas in [150_000u64, 170_000, 190_000, 210_000, 230_000] {
        let (raw, _) = keyless_tx("ok");
        v.push(kl(format!("port_krule_refused_after_charges_{gas}"), &R_KEY, &raw, 200_000, gas));
    }
    // an unknown selector is not dispatched; a 3-byte input is not either
    for (tag, data) in
        [("unknown", sel("unknown()").to_vec()), ("three_bytes", vec![0x01, 0x02, 0x03])]
    {
        let mut s = scn(
            format!("port_kframe_not_dispatched_{tag}"),
            &R_KEY,
            vec![acct(KEYLESS, vec![op::PUSH0, op::PUSH0, op::REVERT])],
            1_000_000,
        );
        s.to = KEYLESS;
        s.data = data;
        v.push(s);
    }
    // the contract's bytecode never runs for a dispatched call
    {
        let (raw, _) = keyless_tx("ok");
        let code = Asm::new().sstore(0, 1).stop().build();
        let mut s = kl(
            "port_kframe_bytecode_not_run_dispatched".to_string(),
            &R_KEY,
            &raw,
            1_000_000,
            5_000_000,
        );
        s.accounts.push(acct(KEYLESS, code.clone()));
        v.push(s);
        let mut s = kl(
            "port_kframe_bytecode_not_run_refused".to_string(),
            &R_KEY,
            &raw,
            100_000,
            5_000_000,
        );
        s.accounts.push(acct(KEYLESS, code.clone()));
        v.push(s);
        let mut s = scn(
            "port_kframe_bytecode_runs_other_selector".to_string(),
            &R_KEY,
            vec![acct(KEYLESS, code.clone())],
            1_000_000,
        );
        s.to = KEYLESS;
        s.data = sel("other()").to_vec();
        v.push(s);
        let caller = call_all(Asm::new(), KEYLESS, 0).stop().build();
        let mut s = scn(
            "port_kframe_contract_call_not_dispatched".to_string(),
            &R_KEY,
            vec![acct(A, caller), acct(KEYLESS, code)],
            1_000_000,
        );
        s.data = vec![];
        v.push(s);
    }
    // a call carrying value is refused before its start is counted
    {
        let (raw, _) = keyless_tx("ok");
        for (tag, limit) in [("holds_body", 310 + raw.len() as u64 + 200), ("ample", 1_000_000)] {
            let mut s =
                kl(format!("port_kframe_value_refused_{tag}"), &R_KEY, &raw, 1_000_000, 5_000_000);
            s.value = U256::from(1u64);
            s.limits.data = Some(limit);
            v.push(s);
        }
        // the call touches the contract as any call does
        let mut s =
            kl("port_kframe_touches_contract".to_string(), &R_KEY, &raw, 1_000_000, 5_000_000);
        s.accounts.push(acct(KEYLESS, vec![op::STOP]));
        v.push(s);
        // a rejected transaction is seen as nothing
        let mut s = kl("port_kframe_rejected_tx".to_string(), &R_KEY, &raw, 1_000_000, 5_000_000);
        s.limits.data = Some(100);
        v.push(s);
        // a value-bearing call that does not decode is refused for the value
        let mut s =
            kl("port_kprec_value_not_decoding".to_string(), &R_KEY, &raw, 1_000_000, 5_000_000);
        s.data.truncate(40);
        s.value = U256::from(1u64);
        v.push(s);
        let mut s = kl(
            "port_kprec_value_bad_tx".to_string(),
            &R_KEY,
            &[0x01, 0x02, 0x03],
            1_000_000,
            5_000_000,
        );
        s.value = U256::from(1u64);
        v.push(s);
    }
    v
}

fn limits() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = R_KLIM;
    // a keyless creation receives the reservoir share and a 980 budget; from nonce 0 and nonce 1
    for (tag, tx, nonce) in [("n0", "slots_980", 0u64), ("n1", "slots_980_n1", 1)] {
        for (gtag, gas) in [("above", 400_000_000u64), ("below", 150_000_000)] {
            let (mut s, signer) =
                kt(format!("port_klim_980_{tag}_{gtag}"), &rules, tx, 140_000_000, gas);
            if nonce == 1 {
                s.accounts.push(Account {
                    address: signer,
                    nonce: 1,
                    balance: U256::from(3u64),
                    ..Account::default()
                });
            }
            v.push(s);
        }
    }
    // the signer's record and the deploy address are state growth (growth limit 10)
    for (tag, tx, present) in [
        ("8", "slots_8", false),
        ("9", "slots_9", false),
        ("8_existing", "slots_8", true),
        ("9_existing", "slots_9", true),
    ] {
        let (mut s, signer) =
            kt(format!("port_klim_growth_{tag}"), &rules, tx, 3_000_000, 20_000_000);
        if present {
            s.accounts.push(funded(signer, vec![], 3));
        }
        s.limits.growth = Some(10);
        v.push(s);
    }
    // a deployment crossing its data-size budget reverts alone; the start crossing stops the
    // creation
    {
        let (raw, _) = keyless_tx("writes");
        for (tag, limit) in [
            ("log_room", 310 + 1_500u64),
            ("no_room", 310 + 300),
            ("start_only_one_record", 310 + raw.len() as u64 + 60),
        ] {
            let mut s = kl(format!("port_klim_data_{tag}"), &rules, &raw, 1_000_000, 5_000_000);
            s.limits.data = Some(limit);
            v.push(s);
        }
        let (raw, _) = keyless_tx("ok");
        let mut s = kl("port_klim_start_crossing".to_string(), &rules, &raw, 1_000_000, 5_000_000);
        s.limits.data = Some(310 + raw.len() as u64 + 60);
        v.push(s);
        let (raw, _) = keyless_tx("revert");
        for d in [0u64, 20, 40, 80] {
            let mut s = kl(
                format!("port_klim_signer_record_budget_{d}"),
                &rules,
                &raw,
                1_000_000,
                5_000_000,
            );
            s.limits.data = Some(310 + raw.len() as u64 + 40 + d);
            v.push(s);
        }
    }
    // a hint the creation sent leaves the signer's record over the transaction limit
    for d in [0u64, 40, 400] {
        let (mut s, _) = kt(
            format!("port_klim_hint_then_fail_{d}"),
            &rules,
            "hint_then_fail",
            1_000_000,
            5_000_000,
        );
        s.accounts.push(acct(ORACLE, vec![op::STOP]));
        s.limits.data = Some(310 + 1_700 + d);
        v.push(s);
    }
    // a refusal after the charges gives the reservoir back (above the cap)
    for gas in [250_000_000u64, 300_000_000, 400_000_000] {
        let (raw, _) = keyless_tx("ok");
        v.push(kl(format!("port_klim_refusal_gives_back_{gas}"), &rules, &raw, 199_999_000, gas));
    }
    // every limit stops a deployment from either nonce at either tier
    for (dim, tx) in [
        ("data", "slots_9"),
        ("kv", "slots_9"),
        ("growth", "slots_9"),
        ("compute", "timestamp_fill"),
    ] {
        for (nonce, tx_n) in
            [(0u64, tx), (1, if tx == "slots_9" { "slots_9_n1" } else { "timestamp_fill" })]
        {
            for (tier, gas) in [("below", 20_000_000u64), ("above", 300_000_000)] {
                let (mut s, signer) = kt(
                    format!("port_klim_every_{dim}_n{nonce}_{tier}"),
                    &rules,
                    tx_n,
                    3_000_000,
                    gas,
                );
                if nonce == 1 && tx_n != "timestamp_fill" {
                    s.accounts.push(Account {
                        address: signer,
                        nonce: 1,
                        balance: U256::from(3u64),
                        ..Account::default()
                    });
                }
                match dim {
                    "data" => s.limits.data = Some(310 + 1_500 + 200),
                    "kv" => s.limits.kv = Some(5),
                    "growth" => s.limits.growth = Some(5),
                    _ => s.limits.block_env_cap = Some(1_000),
                }
                v.push(s);
            }
        }
    }
    // a keyless constructor that self-destructs answers empty code
    {
        let (mut s, _) = kt(
            "port_klim_constructor_selfdestructs".to_string(),
            &rules,
            "destroyed_delegate",
            1_000_000,
            5_000_000,
        );
        s.accounts.push(acct(B, destruct(Asm::new(), BEN).build()));
        v.push(s);
    }
    v
}

fn detention() -> Vec<Scenario> {
    let mut v = Vec::new();
    // a sender that is the beneficiary holds the whole deployment to the cap
    for cap in [20_000u64, 60_000, 90_000, 120_000, 160_000, 1_000_000] {
        let (raw, _) = keyless_tx("ok");
        let mut s = kl(
            format!("port_kdet_sender_is_beneficiary_{cap}"),
            &R_KDET,
            &raw,
            1_000_000,
            5_000_000,
        );
        s.beneficiary = crate::SENDER;
        s.limits.block_env_cap = Some(cap);
        v.push(s);
    }
    // the constructor's own reads: switching the access off, the remaining gas, the Oracle, the
    // block
    for (name, tx) in [
        ("disable_then_timestamp", "disable_then_timestamp"),
        ("remaining_gas", "remaining_gas"),
        ("remaining_gas_after_read", "remaining_gas_after_read"),
        ("oracle_read", "oracle_read"),
        ("reads_block", "reads_block"),
        ("timestamp_empty", "timestamp_empty"),
        ("timestamp_then_fill_no_room", "timestamp_fill"),
    ] {
        for (tag, ben, cap) in [
            ("plain", None, None),
            ("capped", None, Some(5_000u64)),
            ("as_beneficiary", Some(crate::SENDER), Some(300_000)),
        ] {
            let (mut s, _) =
                kt(format!("port_kdet_ctor_{name}_{tag}"), &R_KDET, tx, 2_000_000, 20_000_000);
            s.accounts.push(acct(
                ORACLE,
                Asm::new()
                    .push(0)
                    .op(op::SLOAD)
                    .push(0)
                    .op(op::MSTORE)
                    .push(32)
                    .push(0)
                    .op(op::RETURN)
                    .build(),
            ));
            s.accounts.push(acct(ACCESS_CONTROL, vec![op::STOP]));
            s.accounts.push(acct(LIMIT_CONTROL, vec![op::STOP]));
            s.oracle = vec![(0, 7)];
            if let Some(b) = ben {
                s.beneficiary = b;
            }
            s.limits.block_env_cap = cap;
            s.limits.oracle_cap = cap;
            if name.ends_with("no_room") {
                s.limits.data = Some(310 + 1_700);
            }
            v.push(s);
        }
    }
    v
}

fn precedence() -> Vec<Scenario> {
    let mut v = Vec::new();
    // a call two rules refuse reports the first of the legacy order
    {
        let (raw, signer) = keyless_tx("ok");
        let deploy = signer.create(0);
        let combos: Vec<(&'static str, Vec<Account>, u64)> = vec![
            (
                "nonce_and_address",
                vec![
                    Account { address: signer, nonce: 2, ..Account::default() },
                    acct(deploy, vec![op::STOP]),
                ],
                0,
            ),
            ("code_and_unfunded", vec![acct(signer, vec![op::STOP])], 1),
            (
                "nonce_and_address_2",
                vec![
                    Account {
                        address: signer,
                        nonce: 1,
                        balance: U256::from(5u64),
                        ..Account::default()
                    },
                    acct(deploy, vec![op::STOP]),
                ],
                0,
            ),
        ];
        for (tag, accounts, value) in combos {
            for (gtag, gas) in [("5m", 5_000_000u64), ("300m", 300_000_000)] {
                let (vraw, vsigner) = if value > 0 { ktx_value() } else { (raw.clone(), signer) };
                let mut s =
                    kl(format!("port_kprec_two_rules_{tag}_{gtag}"), &R_KEY, &vraw, 1_000_000, gas);
                s.accounts.extend(accounts.iter().cloned());
                if value > 0 {
                    s.accounts.retain(|a| a.address != signer);
                    s.accounts.push(acct(vsigner, vec![op::STOP]));
                }
                v.push(s);
            }
        }
        // the signer's account and the forward come before the address and the balance
        for (tag, gas) in [("below", 160_000u64), ("above", 300_000_000)] {
            for (extra, accounts) in
                [("occupied", vec![acct(deploy, vec![op::STOP])]), ("plain", vec![])]
            {
                let mut s = kl(
                    format!("port_kprec_forward_first_{tag}_{extra}"),
                    &R_KEY,
                    &raw,
                    199_999_900,
                    gas,
                );
                s.accounts.extend(accounts);
                v.push(s);
            }
        }
        // the creation's charges can refuse a call no rule refuses; an out of gas consumes the
        // regular gas alone
        for gas in [140_000u64, 160_000, 180_000, 200_000, 215_000, 225_000] {
            let mut s =
                kl(format!("port_kprec_creation_charges_{gas}"), &R_KEY, &raw, 200_000, gas);
            s.accounts.push(funded(signer, vec![], 1));
            v.push(s);
        }
        for gas in [30_000u64, 40_000, 48_000, 52_000, 56_000, 60_000] {
            for (stag, present) in [("absent", false), ("one_wei", true)] {
                let mut s =
                    kl(format!("port_kprec_oog_creation_{stag}_{gas}"), &R_KEY, &raw, 200_000, gas);
                if present {
                    s.accounts.push(funded(signer, vec![], 1));
                }
                v.push(s);
            }
        }
    }
    v
}

fn ktx_value() -> (Vec<u8>, Address) {
    let (raw, signer, _, _) = ktx("value5");
    (raw, signer)
}

fn oracle() -> Vec<Scenario> {
    let mut v = Vec::new();
    // the hint payload is counted on the transaction, trailing bytes included
    for extra in [0usize, 7, 64] {
        let mut data = send_hint(1, &[0xab; 40]);
        data.extend(vec![0xee; extra]);
        let mut s = scn(
            format!("port_orcl_payload_counted_{extra}"),
            &R_ORCL,
            vec![acct(ORACLE, vec![op::STOP])],
            1_000_000,
        );
        s.to = ORACLE;
        s.data = data;
        v.push(s);
    }
    // a call with one gas forwards the hint it cannot deliver
    {
        let hint = send_hint(2, &[0x12; 32]);
        let a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 1, &hint).record_call(1).stop().build();
        v.push(scn(
            "port_orcl_one_gas_hint".to_string(),
            &R_ORCL,
            vec![acct(A, a), acct(ORACLE, vec![op::STOP])],
            1_000_000,
        ));
    }
    // an unknown selector and a slot read run the real bytecode
    {
        let code = crate::rev::oracle_account().code;
        let mut s = scn(
            "port_orcl_unknown_selector".to_string(),
            &R_ORCL,
            vec![acct(ORACLE, code.clone())],
            1_000_000,
        );
        s.to = ORACLE;
        s.data = sel("unknown()").to_vec();
        v.push(s);
        let mut input = sel("getSlot(uint256)").to_vec();
        input.extend_from_slice(&U256::from(5u64).to_be_bytes::<32>());
        let a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 500_000, &input)
            .record_call(1)
            .store_returndata_word(3)
            .stop()
            .build();
        let mut s = scn(
            "port_orcl_get_slot_runs_bytecode".to_string(),
            &R_ORCL,
            vec![acct(A, a), acct(ORACLE, code.clone())],
            5_000_000,
        );
        s.oracle = vec![(5, 99)];
        v.push(s);
        // the Oracle's code elsewhere forwards no hint
        let elsewhere = Address::with_last_byte(0x0c);
        let mut s = scn(
            "port_orcl_code_elsewhere".to_string(),
            &R_ORCL,
            vec![acct(elsewhere, code)],
            1_000_000,
        );
        s.to = elsewhere;
        s.data = send_hint(1, &[0x33; 32]);
        v.push(s);
    }
    // a hint that crosses the data-size limit is not forwarded; one at the limit is
    for (tag, delta) in [("crossing", -1i64), ("at_limit", 0), ("ample", 10_000)] {
        let data = send_hint(1, &[0xaa; 100]);
        let mut s = scn(
            format!("port_orcl_limit_{tag}"),
            &R_ORCL,
            vec![acct(ORACLE, vec![op::STOP])],
            1_000_000,
        );
        s.to = ORACLE;
        s.limits.data = Some((310 + data.len() as u64).wrapping_add(delta as u64));
        s.data = data;
        v.push(s);
    }
    // a malformed or padded hint is counted whole
    {
        let mut malformed = send_hint(1, &[0x77; 64]);
        malformed.truncate(100);
        let mut padded = send_hint(1, &[0x77; 64]);
        padded.extend(vec![0u8; 96]);
        for (tag, data) in [("malformed", malformed), ("padded", padded)] {
            for (ltag, limit) in [("plain", None), ("half", Some(310u64 + 70))] {
                let a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 500_000, &data)
                    .record_call(1)
                    .stop()
                    .build();
                let mut s = scn(
                    format!("port_orcl_{tag}_{ltag}"),
                    &R_ORCL,
                    vec![acct(A, a), acct(ORACLE, vec![op::STOP])],
                    2_000_000,
                );
                s.limits.data = limit;
                v.push(s);
            }
        }
    }
    // a child of a frame that switched access off sends no hint; a withheld hint is not counted
    {
        let hint = send_hint(1, &[0x44; 64]);
        let b =
            mem_call(Asm::new(), op::CALL, ORACLE, 0, 500_000, &hint).op(op::POP).stop().build();
        let a = call_all(
            Asm::new()
                .mstore_selector(sel("disableVolatileDataAccess()"))
                .call_with(op::CALL, ACCESS_CONTROL, 0, 100_000, 4)
                .op(op::POP),
            B,
            0,
        )
        .stop()
        .build();
        v.push(scn(
            "port_orcl_child_after_disable".to_string(),
            &R_ORCL,
            vec![acct(A, a), acct(B, b), acct(ORACLE, vec![op::STOP])],
            2_000_000,
        ));
        let big_hint = send_hint(1, &[0x45; 2_000]);
        let a = mem_call(
            Asm::new()
                .mstore_selector(sel("disableVolatileDataAccess()"))
                .call_with(op::CALL, ACCESS_CONTROL, 0, 100_000, 4)
                .op(op::POP),
            op::CALL,
            ORACLE,
            0,
            500_000,
            &big_hint,
        )
        .record_call(1)
        .stop()
        .build();
        let mut s = scn(
            "port_orcl_withheld_hint_not_counted".to_string(),
            &R_ORCL,
            vec![acct(A, a), acct(ORACLE, vec![op::STOP])],
            5_000_000,
        );
        s.limits.data = Some(400);
        v.push(s);
    }
    // oracle storage: reads in the Oracle's own frame ask the service; elsewhere they are ordinary
    let reader = Asm::new()
        .push(5)
        .op(op::SLOAD)
        .push(0)
        .op(op::MSTORE)
        .push(32)
        .push(0)
        .op(op::RETURN)
        .build();
    {
        let mut s = scn(
            "port_ostore_direct_call".to_string(),
            &["SATIN-ORCL-002", "SATIN-ORCL-003"],
            vec![acct(ORACLE, reader.clone())],
            1_000_000,
        );
        s.to = ORACLE;
        s.oracle = vec![(5, 77)];
        v.push(s);
        let a = Asm::new()
            .call_with(op::DELEGATECALL, ORACLE, 0, 100_000, 0)
            .op(op::POP)
            .stop()
            .build();
        let mut s = scn(
            "port_ostore_delegatecall_ordinary_read".to_string(),
            &["SATIN-ORCL-002"],
            vec![acct(A, a), acct(ORACLE, reader.clone())],
            1_000_000,
        );
        s.oracle = vec![(5, 77)];
        v.push(s);
        for (tag, ans, gas) in [
            ("answer_below", true, 5_000_000u64),
            ("noanswer_below", false, 5_000_000),
            ("answer_above", true, 300_000_000),
            ("noanswer_above", false, 300_000_000),
        ] {
            let code = Asm::new().push(5).op(op::SLOAD).op(op::POP).sstore(5, 1).stop().build();
            let mut s = scn(
                format!("port_ostore_read_then_write_{tag}"),
                &["SATIN-ORCL-002", "SATIN-ORCL-003"],
                vec![acct(ORACLE, code)],
                gas,
            );
            s.to = ORACLE;
            if ans {
                s.oracle = vec![(5, 77)];
            }
            v.push(s);
            // a multicall read then write on the real code (the write is the system's)
            let mut s = scn(
                format!("port_ostore_multicall_{tag}"),
                &["SATIN-ORCL-002", "SATIN-ORCL-011"],
                vec![crate::rev::oracle_account()],
                gas,
            );
            s.to = ORACLE;
            s.data = {
                let mut d = sel("multiCall(bytes[])").to_vec();
                d.extend_from_slice(&U256::from(32u64).to_be_bytes::<32>());
                d.extend_from_slice(&U256::from(0u64).to_be_bytes::<32>());
                d
            };
            if ans {
                s.oracle = vec![(5, 77)];
            }
            v.push(s);
        }
        // the service's value wins over the frame's own write
        for (tag, ans) in [("answer", true), ("none", false)] {
            let code =
                Asm::new().sstore(5, 3).push(5).op(op::SLOAD).push(6).op(op::SSTORE).stop().build();
            let mut s = scn(
                format!("port_ostore_service_wins_{tag}"),
                &["SATIN-ORCL-002", "SATIN-ORCL-004"],
                vec![acct(ORACLE, code)],
                1_000_000,
            );
            s.to = ORACLE;
            if ans {
                s.oracle = vec![(5, 9)];
            }
            v.push(s);
        }
        // reads in a nested Oracle frame are cold and ask
        let code = branch_on_data(
            Asm::new()
                .push(5)
                .op(op::SLOAD)
                .op(op::POP)
                .call_with(op::CALL, ORACLE, 0, 200_000, 1)
                .op(op::POP)
                .stop(),
            Asm::new().push(5).op(op::SLOAD).op(op::POP).push(5).op(op::SLOAD).op(op::POP).stop(),
        );
        let mut s = scn(
            "port_ostore_nested_frame_cold".to_string(),
            &["SATIN-ORCL-002", "SATIN-ORCL-003"],
            vec![acct(ORACLE, code)],
            1_000_000,
        );
        s.to = ORACLE;
        s.oracle = vec![(5, 77)];
        v.push(s);
    }
    v
}
