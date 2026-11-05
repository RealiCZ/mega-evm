//! Scenarios of group "edge": edge inputs of single rules, each next to a control input that
//! takes the ordinary path:
//! - the live system address (the `SequencerRegistry`'s trusted code names it): only a legacy call
//!   to a whitelisted contract from it is a system-address transaction; every other envelope from
//!   it — a call off the whitelist, a creation, a typed transaction — is refused, and a deposit
//!   from it is a failed deposit. A permitted envelope must carry the chain id; its sender check is
//!   EIP-3607's with the EIP-7702 exception (a delegated sender is accepted), the code checked
//!   before the nonce;
//! - a contract created onto an address an earlier transfer of the same transaction materialized,
//!   then destroyed: its leaf was absent at the transaction's start, so the destruction gives back
//!   the leaf's state gas, its unit of state growth and its write record, whichever charge made it;
//!   an account that existed empty at the start keeps its leaf;
//! - a transaction-level precompile call detained from the start (sender or recipient is the
//!   beneficiary): priced and held as a child call is;
//! - a SALT bucket that reports a capacity below the minimum bucket: the transaction fails;
//! - per-transaction limits at or above 2^63: compared over the whole `u64` range;
//! - a `keylessDeploy` call whose `bytes` offset is the largest machine word: malformed encoding;
//! - a deposit whose first frame cannot pay the new-account state gas of its value's recipient.
#![allow(dead_code)]
use super::{
    A, Account, Asm, Auth, B, BELOW_CAP, D, EMPTY, KEYLESS, Limits, ORACLE, SENDER,
    SEQUENCER_REGISTRY, Scenario, acct, funded, init_returning, op, scenario,
};
use alloy_primitives::{Address, B256, U256, b256, hex, keccak256};

/// The system address the registry names in these scenarios.
pub const SYSTEM: Address = Address::with_last_byte(0x5a);
/// An address no scenario gives an account: an EIP-7702 authority, created by its authorization.
const AUTHORITY: Address = Address::with_last_byte(0xc1);
/// The `SequencerRegistry` v2.0.0 runtime code, the code both engines trust.
const REGISTRY_CODE: &str = include_str!("SequencerRegistry-2.0.0.hex");
const REGISTRY_CODE_HASH: B256 =
    b256!("abd7e8f1c8f0f9ca0346df585b65d9791a7e9ba5c431cc4eda5d2c2f1f5e9c43");
const RIPEMD160: Address = Address::with_last_byte(0x03);
const BLAKE2F: Address = Address::with_last_byte(0x09);
const ONE_ETHER: u64 = 1_000_000_000_000_000_000;

fn edge(name: &'static str, accounts: Vec<Account>, gas: u64, limits: Limits) -> Scenario {
    let mut s = scenario(name, accounts, gas, limits);
    s.group = "edge";
    s
}

/// The registry with its trusted code, naming `system` in `_currentSystemAddress` (slot 0).
fn registry_naming(system: Address) -> Account {
    let code = hex::decode(REGISTRY_CODE.trim()).unwrap();
    assert_eq!(keccak256(&code), REGISTRY_CODE_HASH, "the registry's trusted code");
    let word = u64::from_be_bytes(system.0[12..].try_into().unwrap());
    assert_eq!(Address::with_last_byte(0).0[..12], system.0[..12], "a slot value of u64");
    Account { address: SEQUENCER_REGISTRY, code, storage: vec![(0, word)], ..Account::default() }
}

/// SSTORE(1, 1); STOP.
fn store_one() -> Vec<u8> {
    Asm::new().sstore(1, 1).stop().build()
}

fn delegation(target: Address) -> Vec<u8> {
    let mut c = vec![0xef, 0x01, 0x00];
    c.extend_from_slice(target.as_slice());
    c
}

/// A transaction from [`SYSTEM`] to `to` on a chain whose registry names it; the system account
/// is `system` (funded), the Oracle and A store 1 in slot 1.
fn from_system(name: &'static str, system: Account, to: Address) -> Scenario {
    let accounts =
        vec![registry_naming(SYSTEM), system, acct(ORACLE, store_one()), acct(A, store_one())];
    let mut s = edge(name, accounts, BELOW_CAP, Limits::default());
    s.sender = SYSTEM;
    s.to = to;
    s
}

fn system_account(code: Vec<u8>, nonce: u64) -> Account {
    Account { nonce, ..funded(SYSTEM, code, ONE_ETHER) }
}

fn system_address() -> Vec<Scenario> {
    let mut v = Vec::new();
    // The permitted shape: a legacy call to the Oracle, promoted (control).
    let mut s =
        from_system("edge_system_legacy_oracle_promoted", system_account(vec![], 0), ORACLE);
    s.chain_id = Some(1);
    v.push(s);
    // The permitted shape without a chain id is refused for the missing chain id.
    v.push(from_system(
        "edge_system_legacy_oracle_without_chain_id",
        system_account(vec![], 0),
        ORACLE,
    ));
    // Every other envelope from the live system address is refused.
    v.push(from_system("edge_system_legacy_off_whitelist", system_account(vec![], 0), A));
    let mut s = from_system("edge_system_creation", system_account(vec![], 0), A);
    s.create = true;
    s.data = Asm::new().sstore(1, 1).build();
    s.data.extend(init_returning(1));
    v.push(s);
    for (name, ty) in [("edge_system_eip2930_oracle", 1u8), ("edge_system_eip1559_oracle", 2)] {
        let mut s = from_system(name, system_account(vec![], 0), ORACLE);
        s.tx_type = ty;
        v.push(s);
    }
    // The same typed envelopes from an ordinary sender run (controls).
    for (name, ty) in [("edge_eip2930_ordinary_call", 1u8), ("edge_eip1559_ordinary_call", 2)] {
        let mut s = edge(name, vec![acct(A, store_one())], BELOW_CAP, Limits::default());
        s.tx_type = ty;
        v.push(s);
    }
    let mut s = from_system("edge_system_eip7702_oracle", system_account(vec![], 0), ORACLE);
    s.auths = vec![Auth { chain_id: 1, address: A, nonce: 0, authority: Some(AUTHORITY) }];
    v.push(s);
    // A deposit from it is never of the system shape: a failed deposit, whatever it calls.
    for (name, to) in [("edge_system_deposit", A), ("edge_system_deposit_to_oracle", ORACLE)] {
        let mut s = from_system(name, system_account(vec![], 0), to);
        s.deposit = Some(0);
        v.push(s);
    }
    // The sender check of a permitted envelope: EIP-3607 accepts a delegation, refuses other code,
    // and is made before the nonce.
    let mut s =
        from_system("edge_system_sender_delegated", system_account(delegation(A), 0), ORACLE);
    s.chain_id = Some(1);
    v.push(s);
    let mut s =
        from_system("edge_system_sender_with_code", system_account(vec![op::STOP], 0), ORACLE);
    s.chain_id = Some(1);
    v.push(s);
    let mut s = from_system(
        "edge_system_sender_with_code_and_stale_nonce",
        system_account(vec![op::STOP], 1),
        ORACLE,
    );
    s.chain_id = Some(1);
    v.push(s);
    v
}

/// A (nonce 1, balance 10) materializes the address of one of its later creations with a value
/// transfer, then creates there a contract whose constructor destroys it to A.
fn destroyed_leaf() -> Vec<Scenario> {
    let mut v = Vec::new();
    let a = |code: Vec<u8>| Account {
        address: A,
        code,
        balance: U256::from(10),
        nonce: 1,
        ..Account::default()
    };
    let destroys_to_a = Asm::new().push_addr(A).op(op::SELFDESTRUCT).build();
    // A CALL with value 1 materializes T, then CREATE (A's nonce 1) lands on it.
    let t = A.create(1);
    let code = Asm::new()
        .call_all(op::CALL, t, 1)
        .op(op::POP)
        .create(&destroys_to_a, 0)
        .op(op::POP)
        .stop()
        .build();
    v.push(edge(
        "edge_transfer_then_create_destroyed",
        vec![a(code)],
        BELOW_CAP,
        Limits::default(),
    ));
    // A first creates (value 1) a contract that destroys itself to T, which materializes T; the
    // second CREATE (A's nonce 2) lands on it.
    let t = A.create(2);
    let destroys_to_t = Asm::new().push_addr(t).op(op::SELFDESTRUCT).build();
    let code = Asm::new()
        .create(&destroys_to_t, 1)
        .op(op::POP)
        .create(&destroys_to_a, 0)
        .op(op::POP)
        .stop()
        .build();
    v.push(edge(
        "edge_selfdestruct_balance_then_create_destroyed",
        vec![a(code)],
        BELOW_CAP,
        Limits::default(),
    ));
    // The address of A's creation holds an account with no balance, nonce or code but a slot: it
    // existed at the transaction's start, so its leaf stands when the creation onto it is
    // destroyed.
    let t = A.create(1);
    let existing_empty = Account { address: t, storage: vec![(5, 9)], ..Account::default() };
    let code = Asm::new().create(&destroys_to_a, 0).op(op::POP).stop().build();
    v.push(edge(
        "edge_existing_empty_account_then_create_destroyed",
        vec![a(code), existing_empty],
        BELOW_CAP,
        Limits::default(),
    ));
    v
}

fn top_level_precompile() -> Vec<Scenario> {
    let mut v = Vec::new();
    // BLAKE2F with 20,000,001 rounds (a malformed final flag, which it checks after its charge):
    // priced past the default cap and within the gas limit.
    let mut input = 20_000_001u32.to_be_bytes().to_vec();
    input.extend([0u8; 208]);
    input.push(2);
    let mut s =
        edge("edge_top_blake2f_sender_is_beneficiary", vec![], 100_000_000, Limits::default());
    s.to = BLAKE2F;
    s.data = input.clone();
    s.beneficiary = SENDER;
    v.push(s);
    // The same call priced past its forward: not held, it runs out of gas.
    let mut s = edge("edge_top_blake2f_price_above_forward", vec![], BELOW_CAP, Limits::default());
    s.to = BLAKE2F;
    s.data = input;
    s.beneficiary = SENDER;
    v.push(s);
    // RIPEMD160 of one word (720) within the default cap: not held, it runs.
    let mut s = edge("edge_top_ripemd160_within_allowance", vec![], BELOW_CAP, Limits::default());
    s.to = RIPEMD160;
    s.data = vec![0; 32];
    s.beneficiary = SENDER;
    v.push(s);
    // RIPEMD160 of one word (720) under a cap of 10.
    let cap = Limits { block_env_cap: Some(10), ..Limits::default() };
    for (name, beneficiary) in [
        ("edge_top_ripemd160_sender_is_beneficiary", SENDER),
        ("edge_top_ripemd160_recipient_is_beneficiary", RIPEMD160),
        ("edge_top_ripemd160_undetained", B),
    ] {
        let mut s = edge(name, vec![], BELOW_CAP, cap);
        s.to = RIPEMD160;
        s.data = vec![0; 32];
        s.beneficiary = beneficiary;
        v.push(s);
    }
    v
}

fn salt_below_minimum() -> Vec<Scenario> {
    let mut v = Vec::new();
    // A slot's bucket.
    let mut s = edge(
        "edge_salt_capacity_zero_slot",
        vec![acct(A, Asm::new().sstore(0, 1).stop().build())],
        BELOW_CAP,
        Limits::default(),
    );
    s.salt = vec![(A, Some(0), 0)];
    v.push(s);
    // A new account's bucket: the transaction's value, a CALL's, a CREATE's.
    let mut s =
        edge("edge_salt_capacity_zero_tx_new_account", vec![], BELOW_CAP, Limits::default());
    s.to = EMPTY;
    s.value = U256::from(1);
    s.salt = vec![(EMPTY, None, 0)];
    v.push(s);
    let code = Asm::new().call_all(op::CALL, EMPTY, 1).op(op::POP).stop().build();
    let mut s = edge(
        "edge_salt_capacity_zero_call_new_account",
        vec![funded(A, code, 10)],
        BELOW_CAP,
        Limits::default(),
    );
    s.salt = vec![(EMPTY, None, 0)];
    v.push(s);
    let code = Asm::new().create(&init_returning(1), 0).op(op::POP).stop().build();
    let mut s =
        edge("edge_salt_capacity_zero_create", vec![acct(A, code)], BELOW_CAP, Limits::default());
    s.salt = vec![(A.create(0), None, 0)];
    v.push(s);
    // A deposit's created caller.
    let fresh = Address::with_last_byte(0xd2);
    let mut s =
        edge("edge_salt_capacity_zero_deposit_caller", vec![], BELOW_CAP, Limits::default());
    s.sender = fresh;
    s.deposit = Some(0);
    s.to = A;
    s.accounts.push(acct(A, store_one()));
    s.salt = vec![(fresh, None, 0)];
    v.push(s);
    v
}

fn wide_limits() -> Vec<Scenario> {
    let mut v = Vec::new();
    let max = u64::MAX;
    for (name, limits) in [
        ("edge_data_limit_u64_max", Limits { data: Some(max), ..Limits::default() }),
        ("edge_kv_limit_u64_max", Limits { kv: Some(max), ..Limits::default() }),
        ("edge_growth_limit_u64_max", Limits { growth: Some(max), ..Limits::default() }),
        ("edge_growth_limit_2p63", Limits { growth: Some(1 << 63), ..Limits::default() }),
    ] {
        v.push(edge(name, vec![acct(A, store_one())], BELOW_CAP, limits));
    }
    // The EIP-7702 list rule: the room the KV and state-growth limits leave the list.
    let mut s = edge(
        "edge_eip7702_limits_u64_max",
        vec![acct(A, store_one()), acct(D, vec![op::STOP])],
        BELOW_CAP,
        Limits { kv: Some(max), growth: Some(max), ..Limits::default() },
    );
    s.auths = vec![Auth { chain_id: 1, address: D, nonce: 0, authority: Some(AUTHORITY) }];
    v.push(s);
    // An Oracle hint, held to the data-size limit as it is sent; and the transaction's limits
    // checked as a caller resumes from the failed child that sent one.
    let hint = super::q8::send_hint(1, &[0xcd; 32]);
    let oracle = acct(ORACLE, vec![op::STOP]);
    let mut s = edge(
        "edge_hint_data_limit_u64_max",
        vec![oracle.clone()],
        BELOW_CAP,
        Limits { data: Some(max), ..Limits::default() },
    );
    s.to = ORACLE;
    s.data = hint.clone();
    v.push(s);
    let b = super::q8::mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint)
        .op(op::POP)
        .revert0()
        .build();
    let a = Asm::new().call_all(op::CALL, B, 0).record_call(1).sstore(7, 7).stop().build();
    v.push(edge(
        "edge_hint_hold_caller_data_limit_u64_max",
        vec![acct(A, a), acct(B, b), oracle],
        BELOW_CAP,
        Limits { data: Some(max), ..Limits::default() },
    ));
    v
}

/// `keylessDeploy(bytes, uint256)` whose `bytes` offset word is `u64::MAX` (the largest `usize` of
/// a 64-bit host), with nothing after the two head words, and the same call whose offset points
/// past the input: both malformed encodings, refused after the call's overhead.
fn keyless_offsets() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, offset) in
        [("edge_keyless_offset_u64_max", u64::MAX), ("edge_keyless_offset_past_input", 0x40)]
    {
        let mut data = alloy_primitives::keccak256("keylessDeploy(bytes,uint256)")[..4].to_vec();
        data.extend_from_slice(&U256::from(offset).to_be_bytes::<32>());
        data.extend_from_slice(&U256::from(1_000_000u64).to_be_bytes::<32>());
        let mut s = edge(name, vec![], 5_000_000, Limits::default());
        s.to = KEYLESS;
        s.data = data;
        v.push(s);
    }
    v
}

/// A deposit (mint 10) of 5 wei to an empty recipient: its first frame's runtime gas phase cannot
/// pay the recipient's new-account state gas at gas 150,000 (and 300,000 with a created caller),
/// and can at 1,000,000 (control).
fn deposit_first_frame() -> Vec<Scenario> {
    let depositor = super::q2::DEPOSITOR;
    let mut v = Vec::new();
    for (name, gas, funded_sender) in [
        ("edge_deposit_value_new_recipient_runtime_oog", 150_000u64, true),
        ("edge_deposit_value_new_recipient_ok", 1_000_000, true),
        ("edge_deposit_new_caller_value_new_recipient_runtime_oog", 300_000, false),
    ] {
        let accounts =
            if funded_sender { vec![funded(depositor, Vec::new(), 1_000)] } else { Vec::new() };
        let mut s = edge(name, accounts, gas, Limits::default());
        s.sender = depositor;
        s.deposit = Some(10);
        s.to = EMPTY;
        s.value = U256::from(5);
        v.push(s);
    }
    v
}

pub fn all() -> Vec<Scenario> {
    let mut v = system_address();
    v.extend(destroyed_leaf());
    v.extend(top_level_precompile());
    v.extend(salt_below_minimum());
    v.extend(wide_limits());
    v.extend(keyless_offsets());
    v.extend(deposit_first_frame());
    v
}
