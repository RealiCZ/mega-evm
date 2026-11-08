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
//!   beneficiary): priced and held as a child call is, also when the precompile's address holds an
//!   EIP-7702 delegation (the precompile runs, not its delegate);
//! - a SALT bucket that reports a capacity below the minimum bucket: the transaction fails;
//! - per-transaction limits at or above 2^63: compared over the whole `u64` range;
//! - a `keylessDeploy` call whose `bytes` offset is the largest machine word: malformed encoding;
//! - a deposit whose first frame cannot pay the new-account state gas of its value's recipient;
//! - the order of a transaction's gas checks: the intrinsic gas without the body's history, the
//!   calldata floor, then the intrinsic gas with the history; the environment's rules (the block's
//!   gas limit) before the body's data size;
//! - a deposit refused for its body: nothing of it runs, no start is predicted or settled;
//! - a failed deposit from the live system address, and its permitted legacy call, from an absent
//!   account: the account the nonce bump creates is counted outside the frames, whose budgets stay
//!   unlimited for the protocol's own work;
//! - a deposit from an account with code or a delegation: its code is read;
//! - a value sent to, or a creation onto, an account the transaction created and destroyed: it
//!   counts nothing more, so the start prices no record of it;
//! - a frame stopped by a limit as it resumes from its child (a creator's nonce record): it resumes
//!   first, so the detention of the transaction holds its caller as before;
//! - a creation transaction whose constructor destroys the new contract (itself, or through a frame
//!   it delegates to): the start record that comes back with the leaf goes back to the regular gas
//!   the record took, the reservoir keeps the rest;
//! - a keyless deployment whose signer is the transaction's sender: the sender's account is never
//!   recorded again, so the deployment charges no record of the signer's nonce and gives none back
//!   when the nonce is set back;
//! - a first frame that starts with the transaction already over a limit (a deposit's created
//!   caller under a state-growth limit of 0): built or answered with a success, it is held to the
//!   limits as its start is drawn, dimension by dimension, so the first dimension crossed in the
//!   order data size, KV updates, state growth names the stop; an interceptor's answer is not held;
//! - a failed `DUPN`, `DUP1` on an empty stack and `DUP16` on a one-item stack, without entries:
//!   the class of their label;
//! - a value the recipient returns to the sender (a call, a destruction): the start record is taken
//!   back in the first frame, and the reservoir it returns unwinds the transaction's spill.
//! - a creation transaction whose created address is the block beneficiary: a creation has no
//!   recipient, so it is not detained from its start (its sender still is);
//! - `SLOTNUM` returned as the output: it pushes zero, which the class `slotnum_zero` accepts until
//!   the reference makes the same change.
#![allow(dead_code)]
use super::{
    A, Account, Asm, Auth, B, BELOW_CAP, BLOCK_GAS_LIMIT, D, EMPTY, IDENTITY, KEYLESS, Limits,
    ORACLE, SENDER, SEQUENCER_REGISTRY, Scenario, acct, funded, init_returning, op, scenario,
};
use alloy_primitives::{Address, B256, U256, address, b256, hex, keccak256};

/// The system address the registry names in these scenarios.
pub const SYSTEM: Address = Address::with_last_byte(0x5a);
/// An address no scenario gives an account: an EIP-7702 authority, created by its authorization.
pub(crate) const AUTHORITY: Address = Address::with_last_byte(0xc1);
/// The `SequencerRegistry` v2.0.0 runtime code, the code both engines trust.
const REGISTRY_CODE: &str = include_str!("SequencerRegistry-2.0.0.hex");
const REGISTRY_CODE_HASH: B256 =
    b256!("abd7e8f1c8f0f9ca0346df585b65d9791a7e9ba5c431cc4eda5d2c2f1f5e9c43");
const RIPEMD160: Address = Address::with_last_byte(0x03);
const BLAKE2F: Address = Address::with_last_byte(0x09);
const P256VERIFY: Address = address!("0x0000000000000000000000000000000000000100");
pub(crate) const ONE_ETHER: u64 = 1_000_000_000_000_000_000;

fn edge(name: &'static str, accounts: Vec<Account>, gas: u64, limits: Limits) -> Scenario {
    let mut s = scenario(name, accounts, gas, limits);
    s.group = "edge";
    s
}

/// The registry with its trusted code, naming `system` in `_currentSystemAddress` (slot 0).
pub(crate) fn registry_naming(system: Address) -> Account {
    let code = hex::decode(REGISTRY_CODE.trim()).unwrap();
    assert_eq!(keccak256(&code), REGISTRY_CODE_HASH, "the registry's trusted code");
    let word = u64::from_be_bytes(system.0[12..].try_into().unwrap());
    assert_eq!(Address::with_last_byte(0).0[..12], system.0[..12], "a slot value of u64");
    Account { address: SEQUENCER_REGISTRY, code, storage: vec![(0, word)], ..Account::default() }
}

/// SSTORE(1, 1); STOP.
pub(crate) fn store_one() -> Vec<u8> {
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
    // A precompile address that holds an EIP-7702 delegation (to B) still runs the precompile, so
    // it is priced and held as the precompile, not as its delegate: RIPEMD160 (an input-only price)
    // and P256VERIFY (priced by a run), under a cap of 10 with the sender as the beneficiary.
    for (name, precompile, data) in [
        ("edge_top_delegated_precompile_ripemd160", RIPEMD160, vec![0; 32]),
        ("edge_top_delegated_precompile_p256verify", P256VERIFY, vec![0; 160]),
    ] {
        let mut s = edge(name, vec![acct(precompile, delegation(B))], BELOW_CAP, cap);
        s.to = precompile;
        s.data = data;
        s.beneficiary = SENDER;
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

/// The order of a user transaction's gas checks. A call of the identity precompile with 213 zero
/// bytes: its intrinsic gas without the body's history is 15,852, its calldata floor 28,632 and
/// its intrinsic gas with the history 61,876; each gas limit below sits in one window. Then a gas
/// limit above the block's, alone and with a body over a data-size limit of 0 (the block's rule is
/// the environment's, checked first), and the body alone.
fn gas_check_order() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, gas) in [
        ("edge_gas_below_intrinsic", 15_000u64),
        ("edge_gas_below_floor", 23_000),
        ("edge_gas_below_history", 40_000),
    ] {
        let mut s = edge(name, vec![], gas, Limits::default());
        s.to = IDENTITY;
        s.data = vec![0; 213];
        v.push(s);
    }
    let body_limit = Limits { data: Some(0), ..Limits::default() };
    for (name, gas, limits) in [
        ("edge_gas_above_block", BLOCK_GAS_LIMIT + 1, Limits::default()),
        ("edge_gas_above_block_body_over_limit", BLOCK_GAS_LIMIT + 1, body_limit),
        ("edge_body_over_limit", BELOW_CAP, body_limit),
    ] {
        v.push(edge(name, vec![acct(A, store_one())], gas, limits));
    }
    v
}

/// Deposits: one whose body is over the data-size limit (from a caller it creates, with a value
/// for an account that exists), and ones from an account with a delegation (with an admitted body
/// and with one over the limit) and with code.
fn deposit_rules() -> Vec<Scenario> {
    let depositor = super::q2::DEPOSITOR;
    let mut v = Vec::new();
    let mut s = edge(
        "edge_deposit_body_over_limit",
        vec![acct(A, store_one())],
        BELOW_CAP,
        Limits { data: Some(100), ..Limits::default() },
    );
    s.sender = depositor;
    s.deposit = Some(1_000_000);
    s.value = U256::from(1);
    v.push(s);
    // The same delegated caller with its body over the limit: it is not validated, its code is
    // not read.
    for (name, code, limits) in [
        ("edge_deposit_delegated_sender", delegation(A), Limits::default()),
        ("edge_deposit_sender_with_code", vec![op::STOP], Limits::default()),
        (
            "edge_deposit_delegated_sender_body_over_limit",
            delegation(A),
            Limits { data: Some(100), ..Limits::default() },
        ),
    ] {
        let mut s =
            edge(name, vec![acct(A, store_one()), funded(depositor, code, 5)], BELOW_CAP, limits);
        s.sender = depositor;
        s.deposit = Some(0);
        v.push(s);
    }
    v
}

/// From the live system address without an account: a deposit (a failed deposit, whose nonce bump
/// creates the account) and the permitted legacy call to the Oracle (promoted; the created
/// account is counted outside its frame, whose budgets stay unlimited).
fn system_absent_sender() -> Vec<Scenario> {
    let accounts = vec![registry_naming(SYSTEM), acct(ORACLE, store_one()), acct(A, store_one())];
    let mut v = Vec::new();
    let mut s =
        edge("edge_system_deposit_absent_sender", accounts.clone(), BELOW_CAP, Limits::default());
    s.sender = SYSTEM;
    s.deposit = Some(0);
    v.push(s);
    let mut s =
        edge("edge_system_legacy_oracle_absent_sender", accounts, BELOW_CAP, Limits::default());
    s.sender = SYSTEM;
    s.to = ORACLE;
    s.chain_id = Some(1);
    v.push(s);
    v
}

/// A (nonce 1, balance 10) funds the address T of its next creation, creates there a contract
/// whose constructor destroys it, then sends T another wei, or creates onto T again with CREATE2
/// (a collision): T counts nothing more, so neither start prices a record of it.
fn after_destruction() -> Vec<Scenario> {
    let mut v = Vec::new();
    let a = |code: Vec<u8>| Account {
        address: A,
        code,
        balance: U256::from(10),
        nonce: 1,
        ..Account::default()
    };
    let destroys_to_a = Asm::new().push_addr(A).op(op::SELFDESTRUCT).build();
    let t = A.create(1);
    let code = Asm::new()
        .call_all(op::CALL, t, 1)
        .op(op::POP)
        .create(&destroys_to_a, 0)
        .op(op::POP)
        .call_all(op::CALL, t, 1)
        .record_call(0x10)
        .stop()
        .build();
    v.push(edge(
        "edge_transfer_after_create_destroyed",
        vec![a(code)],
        BELOW_CAP,
        Limits::default(),
    ));
    // CREATE2 twice at the same address: the first contract destroys itself in its constructor,
    // the second creation collides with what is left of it.
    let mut c = Asm::new();
    for i in 0..2u64 {
        for (j, chunk) in destroys_to_a.chunks(32).enumerate() {
            let mut w = [0u8; 32];
            w[..chunk.len()].copy_from_slice(chunk);
            c = c.push_bytes(&w).push(32 * j as u64).op(op::MSTORE);
        }
        c = c
            .push(0)
            .push(destroys_to_a.len() as u64)
            .push(0)
            .push(0)
            .op(op::CREATE2)
            .push(0x20 + i)
            .op(op::SSTORE);
    }
    v.push(edge(
        "edge_create2_after_create2_destroyed",
        vec![a(c.stop().build())],
        BELOW_CAP,
        Limits::default(),
    ));
    v
}

/// A calls C, whose CREATE's constructor reads TIMESTAMP (the transaction is detained from there
/// on, under a cap of 500 or the default) and writes a slot; the creation's records leave C less
/// data-size room than its own nonce record needs, so C stops as the creation returns. A then
/// reads one of its slots (cold) and loops: the detention stops it there.
fn creator_stopped_on_resume() -> Vec<Scenario> {
    let mut v = Vec::new();
    let mut init = Asm::new().sstore(1, 1).op(op::TIMESTAMP).op(op::POP).build();
    init.extend(init_returning(1));
    let c = Asm::new().create(&init, 0).op(op::POP).sstore(1, 1).revert0().build();
    let a = Asm::new().call_with(op::CALL, super::C, 0, 200_000, 0).op(op::POP);
    let a = a.push(3).op(op::SLOAD).op(op::POP);
    let jd = a.0.len() as u64;
    let a = a.op(op::JUMPDEST).push(jd).op(op::JUMP).build();
    for (name, cap) in [
        ("edge_creator_stopped_on_resume_detained", Some(500)),
        ("edge_creator_stopped_on_resume_default_cap", None),
    ] {
        let c = Account {
            address: super::C,
            code: c.clone(),
            storage: vec![(1, 1)],
            ..Account::default()
        };
        let limits = Limits { data: Some(410), block_env_cap: cap, ..Limits::default() };
        v.push(edge(name, vec![acct(A, a.clone()), c], 200_300_000, limits));
    }
    v
}

/// A creation transaction whose constructor destroys the new contract to A: by itself, below the
/// execution cap (the regular gas paid its start record) and above it by less than the body's
/// history and the record (the reservoir paid part of the record), and through a DELEGATECALL of
/// D, whose code destroys its caller's account.
fn creation_destroyed_in_constructor() -> Vec<Scenario> {
    let destroy = Asm::new().push_addr(A).op(op::SELFDESTRUCT).build();
    let mut v = Vec::new();
    // 22 bytes of init code: a body of 332 bytes, whose history is 29,216; the record is 3,520.
    for (name, gas) in [
        ("edge_create_tx_destroyed_in_constructor", 5_000_000u64),
        ("edge_create_tx_destroyed_in_constructor_part_reservoir", 200_000_000 + 30_000),
    ] {
        let mut s = edge(name, vec![], gas, Limits::default());
        s.create = true;
        s.data = destroy.clone();
        v.push(s);
    }
    let mut s = edge(
        "edge_create_tx_destroyed_by_delegate",
        vec![acct(D, destroy)],
        5_000_000,
        Limits::default(),
    );
    s.create = true;
    s.data = Asm::new().call_all(op::DELEGATECALL, D, 0).op(op::POP).stop().build();
    v.push(s);
    v
}

/// `CREATE2` with three operands, its salt missing: the init-code charge and the memory expansion
/// come before the salt is popped. In a frame detained by `TIMESTAMP` (cap 100, in a child, and at
/// Satin's default cap after 19.99M of compute) the expansion is the crossing, so the frame stops
/// at the compute limit and keeps its gas; with four operands (the control) it crosses the same
/// way. Undetained, an expansion of 1 MiB cannot be paid: the frame halts out of gas, not on the
/// stack.
fn create2_salt_order() -> Vec<Scenario> {
    // TIMESTAMP POP; PUSH2 0x8000 PUSH0 PUSH0 (len, offset, value); CREATE2.
    let missing_salt = hex!("42506180005f5ff5").to_vec();
    let mut v = Vec::new();
    let detained = Limits { block_env_cap: Some(100), ..Limits::default() };
    v.push(edge(
        "edge_create2_missing_salt_detained",
        vec![acct(A, missing_salt.clone())],
        BELOW_CAP,
        detained,
    ));
    // TIMESTAMP POP; PUSH0 (the salt) PUSH2 0x8000 PUSH0 PUSH0; CREATE2.
    v.push(edge(
        "edge_create2_with_salt_detained",
        vec![acct(A, hex!("42505f6180005f5ff5").to_vec())],
        BELOW_CAP,
        detained,
    ));
    // A calls C with 100,000 gas, then stores the call's result, RETURNDATASIZE and GAS.
    let a = Asm::new()
        .call_with(op::CALL, super::C, 0, 100_000, 0)
        .record_call(1)
        .op(op::GAS)
        .push(3)
        .op(op::SSTORE)
        .stop()
        .build();
    v.push(edge(
        "edge_create2_missing_salt_detained_child",
        vec![acct(A, a), acct(super::C, missing_salt)],
        BELOW_CAP,
        detained,
    ));
    // TIMESTAMP POP; burns compute up to 19.99M after the read (GAS loop), then the 1 MiB CREATE2
    // with its salt missing.
    v.push(edge(
        "edge_create2_missing_salt_default_cap",
        vec![acct(A, hex!("42505a63013105f090035b805a11600a5750621000005f5ff5").to_vec())],
        150_000_000,
        Limits::default(),
    ));
    // PUSH3 0x100000 PUSH0 PUSH0; CREATE2.
    v.push(edge(
        "edge_create2_missing_salt_unpayable",
        vec![acct(A, hex!("621000005f5ff5").to_vec())],
        BELOW_CAP,
        Limits::default(),
    ));
    v
}

/// The first frame's start record is a runtime charge, not part of validation: a gas limit that
/// covers the intrinsic gas and the body's history (21,000 + 27,280 for a value call) but not the
/// recipient's record (3,520) is an included out-of-gas that runs no frame and reads nothing of
/// its recipient; one below the validation minimum is refused. Every envelope that prices
/// history, a creation, an absent recipient; at 51,800 the record is paid (the control).
fn start_record_at_runtime() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, tx_type, gas) in [
        ("edge_start_record_unpaid_legacy", 0u8, 50_000u64),
        ("edge_start_record_unpaid_2930", 1, 50_000),
        ("edge_start_record_unpaid_1559", 2, 50_000),
        ("edge_start_record_at_validation_minimum", 0, 48_280),
        ("edge_start_record_below_validation_minimum", 0, 48_279),
        ("edge_start_record_one_below", 0, 51_799),
        ("edge_start_record_paid", 0, 51_800),
    ] {
        let mut s = edge(name, vec![funded(A, Vec::new(), 1)], gas, Limits::default());
        s.tx_type = tx_type;
        s.value = U256::from(1);
        s.gas_price = 1;
        v.push(s);
    }
    let mut s =
        edge("edge_start_record_unpaid_absent_recipient", vec![], 50_000, Limits::default());
    s.to = EMPTY;
    s.value = U256::from(1);
    s.gas_price = 1;
    v.push(s);
    // A creation: 24,000 + 27,280, its account's record unpaid at 52,000.
    let mut s = edge("edge_start_record_unpaid_create", vec![], 52_000, Limits::default());
    s.create = true;
    s.gas_price = 1;
    v.push(s);
    v
}

/// A deposit loads its recipient only once it passed its intrinsic check and paid its created
/// caller: one that cannot pay the caller (absent recipient, existing recipient), or is refused
/// below its intrinsic gas with a mint that covers its value, reads nothing of it; one refused
/// below its intrinsic gas settles no start.
fn deposit_before_start() -> Vec<Scenario> {
    let mut v: Vec<Scenario> = deposit_first_frame()
        .into_iter()
        .filter(|s| s.name == "edge_deposit_new_caller_value_new_recipient_runtime_oog")
        .map(|mut s| {
            s.name = "edge_deposit_new_caller_unpaid_absent_recipient";
            s.gas_limit = 25_000;
            s
        })
        .collect();
    let fresh = Address::with_last_byte(0xd1);
    for (name, gas, mint, value) in [
        ("edge_deposit_new_caller_unpaid_existing_recipient", 200_000u64, 1u64, 1u64),
        ("edge_deposit_new_caller_below_intrinsic", 11_999, 0, 0),
    ] {
        let mut s = edge(name, vec![acct(B, vec![op::STOP])], gas, Limits::default());
        s.accounts.retain(|a| a.address != super::SENDER);
        s.sender = fresh;
        s.to = B;
        s.deposit = Some(mint);
        s.value = U256::from(value);
        s.salt = vec![(fresh, None, 1)];
        v.push(s);
    }
    for (name, mint, value) in
        [("edge_deposit_below_intrinsic", 0u64, 0u64), ("edge_deposit_below_intrinsic_value", 5, 5)]
    {
        let mut s = edge(name, vec![], 12_000, Limits::default());
        s.deposit = Some(mint);
        s.value = U256::from(value);
        v.push(s);
    }
    v
}

/// SELFDESTRUCT to the block beneficiary (C destroys itself to B) whose completed write then fails:
/// its records cross the frame's KV budget, or the frame cannot pay their history (directly, and
/// in a frame reached by DELEGATECALL). Its read of the beneficiary counts only once the opcode
/// completed, so the caller A goes on undetained (cap 6,000, and at Satin's default cap with 50M
/// of compute after). Controls: another heir, and the history paid.
fn selfdestruct_read_after_write() -> Vec<Scenario> {
    let destroy_to_b = hex!("7310000000000000000000000000000000000000b0ff").to_vec();
    let mut v = Vec::new();
    // (name, A's code, KV limit, detention cap, gas limit)
    type Case = (&'static str, &'static [u8], Option<u64>, Option<u64>, u64);
    let cases: [Case; 6] = [
        (
            "edge_sd_beneficiary_kv_stop",
            &hex!(
                "5f5f5f5f60037310000000000000000000000000000000000000c05af16001553d6002556001600555600160065500"
            ),
            Some(3),
            Some(6_000),
            BELOW_CAP,
        ),
        (
            "edge_sd_other_heir_kv_stop",
            &hex!(
                "5f5f5f5f60037310000000000000000000000000000000000000c05af16001553d6002556001600555600160065500"
            ),
            Some(3),
            Some(6_000),
            BELOW_CAP,
        ),
        (
            "edge_sd_beneficiary_history_unpaid",
            &hex!(
                "5f5f5f5f5f7310000000000000000000000000000000000000c0611770f16001553d6002556001600555600160065500"
            ),
            None,
            Some(6_000),
            BELOW_CAP,
        ),
        (
            "edge_sd_beneficiary_history_paid",
            &hex!(
                "5f5f5f5f5f7310000000000000000000000000000000000000c061ea60f16001553d6002556001600555600160065500"
            ),
            None,
            Some(6_000),
            BELOW_CAP,
        ),
        (
            "edge_sd_beneficiary_history_unpaid_default_cap",
            &hex!(
                "5f5f5f5f5f7310000000000000000000000000000000000000c0611770f16001553d6002555b6305f5e1005a11602557600160055500"
            ),
            None,
            None,
            150_000_000,
        ),
        (
            "edge_sd_beneficiary_history_unpaid_delegated",
            &hex!(
                "5f5f5f5f5f7310000000000000000000000000000000000000c0612328f16001553d6002556001600555600160065500"
            ),
            None,
            Some(6_000),
            BELOW_CAP,
        ),
    ];
    for (name, a, kv, cap, gas) in cases {
        let heir_b = name != "edge_sd_other_heir_kv_stop";
        let c = if name.ends_with("_delegated") {
            hex!("5f5f5f5f7310000000000000000000000000000000000000d05af46001553d60025500").to_vec()
        } else if heir_b {
            destroy_to_b.clone()
        } else {
            hex!("7310000000000000000000000000000000000000d0ff").to_vec()
        };
        let mut accounts = vec![
            funded(A, a.to_vec(), if kv.is_some() { 100 } else { 0 }),
            funded(super::C, c, 7),
            funded(B, Vec::new(), 1),
        ];
        if name.ends_with("_delegated") {
            accounts.push(acct(D, destroy_to_b.clone()));
        } else if !heir_b {
            accounts.push(funded(D, Vec::new(), 1));
        }
        let limits = Limits { kv, block_env_cap: cap, ..Limits::default() };
        let mut s = edge(name, accounts, gas, limits);
        s.beneficiary = B;
        v.push(s);
    }
    v
}

/// A keyless call with a value, detained from its start (its sender or recipient is the block
/// beneficiary) under a cap below its 100,000 overhead: the overhead is the crossing, so the call
/// stops at the compute limit with none of it billed, before its value is refused. At a cap of
/// 100,000 it is refused for its value (the control); as a deposit, the same.
fn keyless_value_detained() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, beneficiary, cap, deposit) in [
        ("edge_keyless_value_detained_sender", super::SENDER, 3_000u64, false),
        ("edge_keyless_value_detained_recipient", KEYLESS, 99_999, false),
        ("edge_keyless_value_detained_at_overhead", KEYLESS, 100_000, false),
        ("edge_keyless_value_detained_deposit", super::SENDER, 3_000, true),
    ] {
        let (data, _, _) = super::q8::keyless_call("ok", 1_000_000);
        let limits = Limits { block_env_cap: Some(cap), ..Limits::default() };
        let mut s = edge(name, vec![], 5_000_000, limits);
        s.to = KEYLESS;
        s.data = data;
        s.value = U256::from(1);
        s.beneficiary = beneficiary;
        if deposit {
            s.deposit = Some(1);
        }
        v.push(s);
    }
    v
}

/// A failed deposit (its recipient halts on INVALID) whose mint the sender's balance cannot hold:
/// the credit is a checked add that keeps the balance (op-revm), at `U256::MAX` and one below it;
/// a mint that fits exactly (the control) is credited.
fn failed_deposit_mint_overflow() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, below_max) in [
        ("edge_deposit_failed_mint_overflow", 0u64),
        ("edge_deposit_failed_mint_overflow_max_minus_one", 1),
        ("edge_deposit_failed_mint_fits", 100),
    ] {
        let mut s = edge(name, vec![acct(A, vec![op::INVALID])], 1_000_000, Limits::default());
        s.accounts.retain(|a| a.address != super::SENDER);
        s.accounts.push(Account {
            address: super::SENDER,
            balance: U256::MAX - U256::from(below_max),
            ..Account::default()
        });
        s.deposit = Some(100);
        v.push(s);
    }
    v
}

/// A pre-EIP-155 creation (nonce 0, gas price 100 gwei, gas limit 200,000, value 5 wei, init code
/// returning one byte) signed by Nick's method (`r` = `s` = 0x22..22), and its signer.
const KEYLESS_WITH_VALUE: &str = "f8568085174876e80083030d4080058560016000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222";
const KEYLESS_WITH_VALUE_SIGNER: Address =
    alloy_primitives::address!("0xb6a4e6e5019de4b8dae955a61c926d35c8f4bcc8");

/// A keyless deployment whose signer is the transaction's sender (at nonce 0, which validation
/// bumps to 1, so the creation's bump is taken back): no record of the signer's nonce is charged,
/// and none is given back, whether the creation succeeds, reverts or halts, carries a value or the
/// transaction a gas price. The control: the same signer at nonce 1, from another sender, whose
/// nonce record is charged and given back.
fn keyless_signer_is_sender() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, tx, gas_price) in [
        ("edge_keyless_signer_is_sender_ok", "ok", 0u64),
        ("edge_keyless_signer_is_sender_revert", "revert", 0),
        ("edge_keyless_signer_is_sender_halt", "halt", 0),
        ("edge_keyless_signer_is_sender_value", "value", 0),
        ("edge_keyless_signer_is_sender_gas_price", "ok", 1),
        ("edge_keyless_signer_at_nonce_1_other_sender", "ok", 0),
    ] {
        let (raw, signer) = if tx == "value" {
            (hex::decode(KEYLESS_WITH_VALUE).unwrap(), KEYLESS_WITH_VALUE_SIGNER)
        } else {
            super::q8::keyless_tx(tx)
        };
        let other_sender = name.ends_with("other_sender");
        let signer_account = Account {
            address: signer,
            nonce: other_sender as u64,
            balance: U256::from(ONE_ETHER),
            ..Account::default()
        };
        let mut s = edge(name, vec![signer_account], 1_000_000, Limits::default());
        s.to = KEYLESS;
        s.data = super::q8::keyless_call_raw(&raw, 1_000_000);
        s.gas_price = gas_price;
        if !other_sender {
            s.accounts.retain(|a| a.address != super::SENDER);
            s.sender = signer;
        }
        v.push(s);
    }
    v
}

/// A deposit from an absent caller (its created caller is the transaction's one unit of state
/// growth) under a state-growth limit of 0, whose first frame starts with the transaction already
/// over that limit: answered with a success (a value to an account without code, the identity
/// precompile, an absent account), the answer is replaced by the stop `3:0`; built with a start
/// that crosses an earlier dimension (a data-size budget of 100 under a 200-byte start, a KV
/// limit of 0), that dimension is the stop. Controls: a built frame without a value (`3:0`) and an
/// interceptor's answer (not held).
fn first_frame_over_limit() -> Vec<Scenario> {
    let depositor = super::q2::DEPOSITOR;
    let contract = || acct(A, vec![op::STOP]);
    let growth0 = Limits { growth: Some(0), ..Limits::default() };
    let mut v = Vec::new();
    for (name, to, value, mint, data, accounts, limits) in [
        (
            "edge_first_frame_over_growth_value_to_eoa",
            A,
            1u64,
            5u64,
            vec![],
            vec![funded(A, vec![], 1)],
            growth0,
        ),
        ("edge_first_frame_over_growth_precompile", IDENTITY, 0, 0, vec![0, 0xff], vec![], growth0),
        ("edge_first_frame_over_growth_absent", EMPTY, 0, 1, vec![], vec![], growth0),
        (
            "edge_first_frame_data_budget_before_growth",
            A,
            1,
            1_000_000,
            vec![],
            vec![contract()],
            Limits { data: Some(410), ..growth0 },
        ),
        (
            "edge_first_frame_kv_before_growth",
            A,
            1,
            1_000_000,
            vec![],
            vec![contract()],
            Limits { kv: Some(0), ..growth0 },
        ),
        (
            "edge_first_frame_over_growth_contract",
            A,
            0,
            1_000_000,
            vec![],
            vec![contract()],
            growth0,
        ),
        (
            "edge_first_frame_over_growth_interceptor",
            super::q8::ACCESS_CONTROL,
            0,
            0,
            super::q8::sel("isVolatileDataAccessDisabled()").to_vec(),
            vec![],
            growth0,
        ),
    ] {
        let mut s = edge(name, accounts, 1_000_000, limits);
        s.sender = depositor;
        s.deposit = Some(mint);
        s.to = to;
        s.value = U256::from(value);
        s.data = data;
        v.push(s);
    }
    v
}

/// Failed DUPs without entries of their own, cases of the label class `dup_stack_label`:
/// `PUSH1 1; DUPN 0x00` (a `DUPN` deeper than the one-item stack), `DUP1` on an empty stack and
/// `PUSH1 1; DUP16` (16 deep on a one-item stack).
fn dupn_deeper_than_stack() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, code) in [
        ("edge_dupn_deeper_than_stack", vec![op::PUSH1, 1, op::DUPN, 0x00, op::STOP]),
        ("edge_dup1_empty_stack", vec![op::DUP1, op::STOP]),
        ("edge_dup16_one_item_stack", vec![op::PUSH1, 1, op::DUP1 + 15, op::STOP]),
    ] {
        v.push(edge(name, vec![acct(A, code)], 1_000_000, Limits::default()));
    }
    v
}

/// A deposit creating from a sender at nonce `u64::MAX`, whose nonce cannot be bumped: revm
/// answers the creation with a success, nothing created, its gas untouched, after the balance
/// check (a value the sender cannot fund is out of funds, a failed deposit). The control is a
/// sender at `u64::MAX - 1`, which creates.
fn deposit_creation_nonce_max() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, nonce, value, balance) in [
        ("edge_deposit_create_nonce_max", u64::MAX, 0u64, ONE_ETHER),
        ("edge_deposit_create_nonce_max_value", u64::MAX, 5, ONE_ETHER),
        ("edge_deposit_create_nonce_max_unfunded_value", u64::MAX, 5, 0),
        ("edge_deposit_create_nonce_max_minus_one", u64::MAX - 1, 0, ONE_ETHER),
    ] {
        let depositor = super::q2::DEPOSITOR;
        let sender = Account {
            address: depositor,
            nonce,
            balance: U256::from(balance),
            ..Account::default()
        };
        let mut s = edge(name, vec![sender], 1_000_000, Limits::default());
        s.accounts.retain(|a| a.address != super::SENDER);
        s.sender = depositor;
        s.deposit = Some(0);
        s.create = true;
        s.value = U256::from(value);
        s.data = init_returning(1);
        v.push(s);
    }
    v
}

/// A value of 1 the recipient returns to the sender, by a call or by destroying itself: the first
/// frame's start record is taken back inside the frame and comes back as reservoir, which at the
/// transaction's settlement unwinds the spill the record made (gas below the execution cap: the
/// reservoir was empty). Control: above the cap the reservoir paid the record, nothing spilled.
fn value_returned_to_sender() -> Vec<Scenario> {
    let call_back =
        Asm::new().call_with(op::CALL, super::SENDER, 1, 0, 0).op(op::POP).stop().build();
    let destroy = Asm::new().push_addr(super::SENDER).op(op::SELFDESTRUCT).build();
    let mut v = Vec::new();
    for (name, code, gas) in [
        ("edge_value_returned_to_sender", call_back.clone(), 1_000_000u64),
        ("edge_value_returned_by_selfdestruct", destroy, 1_000_000),
        ("edge_value_returned_reservoir_pays_the_record", call_back, super::ABOVE_CAP),
    ] {
        let mut s = edge(name, vec![acct(A, code)], gas, Limits::default());
        s.value = U256::from(1);
        v.push(s);
    }
    v
}

/// A ModExp call after a read of volatile data (the default cap), a counted loop spending compute
/// first: the lengths (base, exponent, modulus) and the forward, from the code
/// `TIMESTAMP POP <loop> <lengths> CALL(forward, 0x05, 0, 0, 0x60, 0, 0) POP STOP`.
fn modexp_over_bound() -> Vec<Scenario> {
    let detained = |iterations: u32, base: u16, forward: u32| {
        let mut code = hex::decode("4250").unwrap();
        code.push(op::PUSH1 + 2);
        code.extend_from_slice(&iterations.to_be_bytes()[1..]);
        code.extend(hex::decode("5b600190038061000657507f").unwrap());
        let mut word = [0u8; 32];
        word[30..].copy_from_slice(&base.to_be_bytes());
        code.extend_from_slice(&word);
        code.extend(hex::decode("5f527f").unwrap());
        word = [0u8; 32];
        word[31] = 1;
        code.extend_from_slice(&word);
        code.extend(hex::decode("6020527f").unwrap());
        code.extend_from_slice(&word);
        code.extend(
            hex::decode("6040525f5f60605f5f730000000000000000000000000000000000000005").unwrap(),
        );
        code.push(op::PUSH1 + 2);
        code.extend_from_slice(&forward.to_be_bytes()[1..]);
        code.extend(hex::decode("f15000").unwrap());
        code
    };
    let mut v = Vec::new();
    for (name, iterations, base, forward, gas) in [
        (
            "edge_modexp_over_bound_detained_allowance_above_500",
            0x0b_b9f4u32,
            1_025u16,
            1_000_000u32,
            30_000_000u64,
        ),
        (
            "edge_modexp_over_bound_detained_allowance_below_500",
            0x0b_bcbf,
            1_025,
            20_000,
            30_000_000,
        ),
        ("edge_modexp_at_bound_detained", 0x0b_b9f4, 1_024, 1_000_000, 30_000_000),
        ("edge_modexp_over_bound_base_20000_detained", 0x04_b1ec, 20_000, 15_000_000, 60_000_000),
    ] {
        v.push(edge(
            name,
            vec![acct(A, detained(iterations, base, forward))],
            gas,
            Limits::default(),
        ));
    }
    let lengths = |base: u8| {
        let mut input = vec![0u8; 96];
        input[30] = 0x04;
        input[31] = base;
        input[63] = 1;
        input[95] = 1;
        input
    };
    for (name, data, gas) in [
        ("edge_modexp_over_bound_undetained", lengths(0x01), 200_000u64),
        ("edge_modexp_huge_lengths_forward_below_formula", vec![0x12, 0x34, 0x56, 0x78], 152_696),
    ] {
        let mut s = edge(name, vec![], gas, Limits::default());
        s.to = Address::with_last_byte(5);
        s.data = data;
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
    v.extend(gas_check_order());
    v.extend(deposit_rules());
    v.extend(system_absent_sender());
    v.extend(after_destruction());
    v.extend(creator_stopped_on_resume());
    v.extend(creation_destroyed_in_constructor());
    v.extend(create2_salt_order());
    v.extend(start_record_at_runtime());
    v.extend(deposit_before_start());
    v.extend(selfdestruct_read_after_write());
    v.extend(keyless_value_detained());
    v.extend(failed_deposit_mint_overflow());
    v.extend(keyless_signer_is_sender());
    v.extend(first_frame_over_limit());
    v.extend(dupn_deeper_than_stack());
    v.extend(value_returned_to_sender());
    v.extend(modexp_over_bound());
    v.extend(deposit_creation_nonce_max());
    v.extend(creation_onto_beneficiary());
    v.extend(slotnum_returned());
    v.extend(protocol_caller());
    v
}

/// `SLOTNUM` returned as the transaction's output (`SLOTNUM PUSH0 MSTORE PUSH1 32 PUSH0 RETURN`):
/// the pushed word is the only thing the value reaches.
fn slotnum_returned() -> Vec<Scenario> {
    let code = hex!("4b5f5260205ff3").to_vec();
    vec![edge("edge_slotnum_returned", vec![acct(A, code)], 1_000_000, Limits::default())]
}

/// A creation transaction whose created address (`SENDER.create(0)`) is the block beneficiary: a
/// creation has no recipient, and its loads of the created address are its own checks, so nothing
/// detains it from its start. At a cap of 1 (legacy, EIP-1559 and deposit envelopes) BASEFEE
/// detains the constructor and the POP after it is the crossing; at the default cap the
/// constructor stores what `remainingComputeGas()` answers, which a detention from the start would
/// lower to the cap. Controls: another beneficiary at the default cap, and a sender that is the
/// beneficiary, which detains the creation from its start.
fn creation_onto_beneficiary() -> Vec<Scenario> {
    let created = super::SENDER.create(0);
    // BASEFEE POP PUSH0 PUSH0 RETURN.
    let basefee = hex!("48505f5ff3").to_vec();
    let cap_one = Limits { block_env_cap: Some(1), ..Limits::default() };
    let mut v = Vec::new();
    for (name, tx_type, deposit) in [
        ("edge_create_tx_beneficiary_is_created_address_legacy", 0u8, None),
        ("edge_create_tx_beneficiary_is_created_address_1559", 2, None),
        ("edge_create_tx_beneficiary_is_created_address_deposit", 0, Some(0)),
    ] {
        let mut s = edge(name, vec![], 1_000_000, cap_one);
        s.create = true;
        s.data = basefee.clone();
        s.tx_type = tx_type;
        s.deposit = deposit;
        s.beneficiary = created;
        v.push(s);
    }
    let mut s = edge("edge_create_tx_sender_is_beneficiary", vec![], 1_000_000, cap_one);
    s.create = true;
    s.data = basefee;
    s.beneficiary = super::SENDER;
    v.push(s);
    // The constructor stores the call's success and RETURNDATASIZE at 0x10 and 0x11, and the
    // remaining compute gas it is answered at 0x20.
    let remaining = Asm::new()
        .mstore_selector(super::remaining_compute_gas_selector())
        .call_with(op::CALL, super::LIMIT_CONTROL, 0, 500_000, 4)
        .record_call(0x10)
        .store_returndata_word(0x20)
        .stop()
        .build();
    for (name, beneficiary) in [
        ("edge_create_tx_beneficiary_is_created_address_default_cap", created),
        ("edge_create_tx_beneficiary_is_another_address_default_cap", B),
    ] {
        let mut s = edge(name, vec![], 30_000_000, Limits::default());
        s.create = true;
        s.data = remaining.clone();
        s.beneficiary = beneficiary;
        v.push(s);
    }
    v
}

/// The reserved protocol caller is exempt regardless of envelope type, even outside a block.
fn protocol_caller() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, ty, deposit) in [
        ("edge_protocol_caller_legacy", 0, None),
        ("edge_protocol_caller_access_list", 1, None),
        ("edge_protocol_caller_dynamic_fee", 2, None),
        ("edge_protocol_caller_deposit", 0, Some(0)),
    ] {
        let mut s = edge(
            name,
            vec![acct(A, hex!("425060015f5500").to_vec())],
            BELOW_CAP,
            Limits { data: Some(0), block_env_cap: Some(0), ..Limits::default() },
        );
        s.sender = address!("fffffffffffffffffffffffffffffffffffffffe");
        s.tx_type = ty;
        s.deposit = deposit;
        s.salt = vec![(A, Some(0), 3)];
        v.push(s);
    }
    v
}
