//! Scenarios of group "rev": inputs found by reading the specification against the two
//! implementations, each executed on both. Every scenario names the rule it exercises and the
//! point it settles:
//! - the reservoir a start returns joins the caller's retained reservoir, once (`SATIN-POOL-012`);
//! - a value-bearing transaction's recipient has paid the account write the authorization of that
//!   recipient would charge (`SATIN-AUTH-015`);
//! - a record taken back is credited to the frame that takes it back (`SATIN-HIST-007`,
//!   `SATIN-HIST-012`);
//! - a call that never runs `NUMBER` is not a volatile read (`SATIN-DET-005`);
//! - a keyless deployment onto an address that holds a balance and no code deploys
//!   (`SATIN-KEY-008`);
//! - the Oracle authorizes its immediate caller (`SATIN-ORCL-011`);
//! - the account a deposit's caller needs is new when it is empty under EIP-161, an existing leaf
//!   included (`SATIN-DEP-003`, `SATIN-ACCT-015`).
#![allow(dead_code)]
use super::{
    A, ABOVE_CAP, Account, Asm, Auth, B, BELOW_CAP, C, D, EMPTY, IDENTITY, KEYLESS, LIMIT_CONTROL,
    Limits, ORACLE, SEQUENCER_REGISTRY, Scenario, acct, funded, op, scenario, spin_after,
};
use crate::{
    edge::{ONE_ETHER, registry_naming},
    q8::{keyless_call, mem_call, sel},
};
use alloy_primitives::{Address, B256, U256, hex, keccak256};

/// The real Oracle (version 2.0.0) and High-Precision Timestamp code, as the chain holds them.
const ORACLE_CODE: &str = include_str!("Oracle-2.0.0.hex");
const ORACLE_CODE_HASH: B256 =
    alloy_primitives::b256!("71a65239db8d0f1bb765fad36e34f57600420d103a4401ef7555bd50b229dc55");
const TIMESTAMP_CODE: &str = include_str!("HighPrecisionTimestamp.hex");
const TIMESTAMP_CODE_HASH: B256 =
    alloy_primitives::b256!("1b2df8ca5350cd7106d67ed95532584526f97a5ca267c8eef73a1831f53720f2");
/// The EIP-2935 history contract's address.
const HISTORY: Address = alloy_primitives::address!("0x0000F90827F1C53a10cb7A02335B175320002935");
const HIGH_PRECISION_TIMESTAMP: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02]);
/// An account the registry can name as the system address (a `u64` slot value), standing for a
/// contract the registry's admin made the system address.
const FORWARDER: Address = Address::with_last_byte(0x5c);

fn rev(
    name: &'static str,
    rules: &[&'static str],
    accounts: Vec<Account>,
    gas: u64,
    limits: Limits,
) -> Scenario {
    let mut s = scenario(name, accounts, gas, limits);
    s.group = "rev";
    s.rules = rules.to_vec();
    s
}

fn word(v: u64) -> [u8; 32] {
    U256::from(v).to_be_bytes::<32>()
}

/// `selector || words`.
fn call_data(selector: [u8; 4], words: &[[u8; 32]]) -> Vec<u8> {
    let mut c = selector.to_vec();
    for w in words {
        c.extend_from_slice(w);
    }
    c
}

pub fn oracle_account() -> Account {
    let code = hex::decode(ORACLE_CODE.trim()).unwrap();
    assert_eq!(keccak256(&code), ORACLE_CODE_HASH, "the Oracle's code");
    Account { address: ORACLE, code, ..Account::default() }
}

fn timestamp_account() -> Account {
    let code = hex::decode(TIMESTAMP_CODE.trim()).unwrap();
    assert_eq!(keccak256(&code), TIMESTAMP_CODE_HASH, "the High-Precision Timestamp's code");
    Account { address: HIGH_PRECISION_TIMESTAMP, code, ..Account::default() }
}

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(child_return());
    v.extend(prepaid_recipient());
    v.extend(giveback_frame());
    v.extend(history_contract());
    v.extend(keyless_balance_only());
    v.extend(oracle_immediate_caller());
    v.extend(deposit_empty_leaf());
    v
}

/// evm F1: a caller with a reservoir calls an account with no code, a precompile, an interceptor
/// and a code account that returns at once; the reservoir it ends with is what it kept plus what
/// the child returned, never more.
fn child_return() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-POOL-011", "SATIN-POOL-012", "SATIN-FWD-005"];
    let limit_control = sel("remainingComputeGas()");
    let targets: [(&'static str, Address, u8, Vec<u8>); 4] = [
        ("rev_child_return_no_code", EMPTY, op::CALL, Vec::new()),
        ("rev_child_return_precompile", IDENTITY, op::STATICCALL, vec![1, 2, 3]),
        ("rev_child_return_interceptor", LIMIT_CONTROL, op::STATICCALL, limit_control.to_vec()),
        ("rev_child_return_built_frame", B, op::CALL, Vec::new()),
    ];
    for (name, target, kind, input) in targets {
        let a = mem_call(Asm::new(), kind, target, 0, 100_000, &input)
            .record_call(1)
            .sstore(5, 1)
            .stop()
            .build();
        v.push(rev(
            name,
            &rules,
            vec![acct(A, a), acct(B, Asm::new().stop().build())],
            ABOVE_CAP,
            Limits::default(),
        ));
    }
    v
}

/// evm F2: an EIP-7702 transaction that sends one wei to an existing account which authorizes
/// itself: the account write was paid with the value, so the authorization does not pay it again.
fn prepaid_recipient() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-AUTH-015", "SATIN-SCHED-008", "SATIN-SCHED-006"];
    for (name, gas) in [
        ("rev_auth_recipient_prepaid_exact", 68_504u64),
        ("rev_auth_recipient_prepaid_one_short", 68_503),
        ("rev_auth_recipient_prepaid_ample", 400_000),
    ] {
        let mut s = rev(name, &rules, vec![funded(B, vec![], 5)], gas, Limits::default());
        s.to = B;
        s.value = U256::from(1u64);
        s.auths = vec![Auth { chain_id: 1, address: Address::ZERO, nonce: 0, authority: Some(B) }];
        v.push(s);
    }
    // The same authority when the transaction sends no value: the write is the authorization's.
    let mut s = rev(
        "rev_auth_recipient_no_value",
        &rules,
        vec![funded(B, vec![], 5)],
        400_000,
        Limits::default(),
    );
    s.to = B;
    s.auths = vec![Auth { chain_id: 1, address: Address::ZERO, nonce: 0, authority: Some(B) }];
    v.push(s);
    v
}

/// evm F3: a frame writes a slot (paying its record), delegates to a frame that logs, restores the
/// slot and reads `GAS`: the record taken back is credited to the delegate, which runs on it.
fn giveback_frame() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-HIST-007", "SATIN-HIST-012", "SATIN-POOL-008"];
    let c = Asm::new()
        .push(0)
        .push(0)
        .op(op::LOG0)
        .sstore(0, 7)
        .op(op::GAS)
        .push(0)
        .op(op::MSTORE)
        .push(32)
        .push(0)
        .op(op::RETURN)
        .build();
    let p = Asm::new()
        .sstore(0, 8)
        .call_all(op::DELEGATECALL, C, 0)
        .record_call(1)
        .store_returndata_word(2)
        .stop()
        .build();
    let mut pa = acct(A, p);
    pa.storage = vec![(0, 7)];
    for (name, gas) in [
        ("rev_giveback_to_restoring_frame", BELOW_CAP),
        ("rev_giveback_to_restoring_frame_res", ABOVE_CAP),
    ] {
        v.push(rev(name, &rules, vec![pa.clone(), acct(C, c.clone())], gas, Limits::default()));
    }
    v
}

/// evm F4: a call to the EIP-2935 history contract is a volatile read when the contract runs
/// `NUMBER`, not when the call fails before it runs; a transaction that only attempted the call is
/// not detained.
fn history_contract() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-DET-005", "SATIN-DET-003", "SATIN-DET-001"];
    // A stand-in with the contract's `get` path: it reads NUMBER first.
    let contract = acct(HISTORY, Asm::new().op(op::NUMBER).op(op::POP).stop().build());
    for (name, forwarded) in
        [("rev_history_call_no_gas", 0u64), ("rev_history_call_runs_number", 100_000)]
    {
        let prefix = Asm::new().call_with(op::STATICCALL, HISTORY, 0, forwarded, 0).op(op::POP);
        let a = spin_after(prefix, 2_000);
        let mut s =
            rev(name, &rules, vec![acct(A, a), contract.clone()], 1_000_000, Limits::default());
        s.limits.block_env_cap = Some(10_000);
        v.push(s);
    }
    v
}

/// blocks F1: the deploy address of a keyless deployment holds a balance and no code: the
/// deployment goes ahead.
fn keyless_balance_only() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-KEY-008", "SATIN-SD-016"];
    for (name, funded_deploy) in
        [("rev_keyless_balance_only_destination", true), ("rev_keyless_empty_destination", false)]
    {
        let (data, _signer, deploy) = keyless_call("ok", 1_000_000);
        let mut accounts = Vec::new();
        if funded_deploy {
            accounts.push(funded(deploy, vec![], 1));
        }
        let mut s = rev(name, &rules, accounts, 5_000_000, Limits::default());
        s.to = KEYLESS;
        s.data = data;
        v.push(s);
    }
    v
}

/// blocks F2: the registry names a contract as the system address; an ordinary sender calls it
/// and the contract writes the Oracle, which authorizes its immediate caller alone. The real
/// High-Precision Timestamp wrapper does the same from the address the registry names.
fn oracle_immediate_caller() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-ORCL-011", "SATIN-REG-002", "SATIN-BASE-008"];
    let set_slot = call_data(sel("setSlot(uint256,bytes32)"), &[word(7), word(123)]);
    let forwarder = mem_call(Asm::new(), op::CALL, ORACLE, 0, 500_000, &set_slot)
        .record_call(1)
        .store_returndata_word(3)
        .stop()
        .build();
    // The forwarder is the live system address: Alice's call writes the Oracle.
    let s = rev(
        "rev_oracle_write_through_system_contract",
        &rules,
        vec![registry_naming(FORWARDER), acct(FORWARDER, forwarder.clone()), oracle_account()],
        2_000_000,
        Limits::default(),
    );
    let mut s = s;
    s.to = FORWARDER;
    v.push(s);
    // The registry names someone else: the same call is refused by the Oracle.
    let mut s = rev(
        "rev_oracle_write_refused_for_other_caller",
        &rules,
        vec![
            registry_naming(Address::with_last_byte(0x5d)),
            acct(FORWARDER, forwarder),
            oracle_account(),
        ],
        2_000_000,
        Limits::default(),
    );
    s.to = FORWARDER;
    v.push(s);
    // The High-Precision Timestamp's wrapper (its real code, which names the Oracle) calls the
    // Oracle as itself. Installed at the address the registry names, it writes for any sender;
    // at its own address, which the registry does not name, the Oracle refuses it.
    let update = call_data(sel("update(uint256)"), &[word(1_000_000)]);
    let mut wrapper = timestamp_account();
    wrapper.address = FORWARDER;
    let mut s = rev(
        "rev_oracle_write_through_timestamp_wrapper_named",
        &rules,
        vec![registry_naming(FORWARDER), wrapper, oracle_account()],
        2_000_000,
        Limits::default(),
    );
    s.to = FORWARDER;
    s.data = update.clone();
    v.push(s);
    let mut s = rev(
        "rev_oracle_write_through_timestamp_wrapper_unnamed",
        &rules,
        vec![registry_naming(FORWARDER), timestamp_account(), oracle_account()],
        2_000_000,
        Limits::default(),
    );
    s.to = HIGH_PRECISION_TIMESTAMP;
    s.data = update;
    v.push(s);
    let _ = SEQUENCER_REGISTRY;
    v
}

/// blocks F10: a deposit from a caller whose leaf exists but is empty under EIP-161 (nonce 0,
/// balance 0, no code, a storage slot) creates its caller as an absent account does; a caller with
/// a nonce does not.
fn deposit_empty_leaf() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-DEP-003", "SATIN-DEP-005", "SATIN-ACCT-015", "SATIN-WIT-017"];
    let leaf = |nonce: u64, balance: u64| Account {
        address: D,
        nonce,
        balance: U256::from(balance),
        storage: vec![(1, 1)],
        ..Account::default()
    };
    let cases: [(&'static str, Option<Account>); 3] = [
        ("rev_deposit_caller_absent", None),
        ("rev_deposit_caller_empty_leaf", Some(leaf(0, 0))),
        ("rev_deposit_caller_nonempty_leaf", Some(leaf(1, 0))),
    ];
    for (name, caller) in cases {
        let mut accounts = vec![funded(B, vec![], 1)];
        accounts.extend(caller);
        let mut s = rev(name, &rules, accounts, 100_000, Limits::default());
        s.sender = D;
        s.deposit = Some(0);
        s.to = B;
        v.push(s);
    }
    // The same with a gas limit that pays the creation.
    for (name, caller) in [
        ("rev_deposit_caller_absent_ample", None),
        ("rev_deposit_caller_empty_leaf_ample", Some(leaf(0, 0))),
        ("rev_deposit_caller_nonempty_leaf_ample", Some(leaf(1, 0))),
    ] {
        let mut accounts = vec![funded(B, vec![], 1)];
        accounts.extend(caller);
        let mut s = rev(name, &rules, accounts, 1_000_000, Limits::default());
        s.sender = D;
        s.deposit = Some(0);
        s.to = B;
        v.push(s);
    }
    let _ = (C, ONE_ETHER);
    v
}
