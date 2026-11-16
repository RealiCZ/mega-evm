//! Scenarios of group "q8": the later mechanisms, ported from the Satin implementation's tests
//! into the shared format so both engines run them, plain and inspected:
//! - EIP-7702 (`tests/satin/{state_growth,write_records,history_bytes,data_size,salt,
//!   pool_validation,kv_updates,volatile_access}.rs`): authority records, growth, state gas, SALT,
//!   the list rule, the beneficiary authority, validation, runtime out-of-gas;
//! - keyless deployment (`tests/system/keyless/{deploy,rules,charges,limits,detention}.rs`);
//! - `MegaAccessControl` (`tests/system/access_control.rs`);
//! - Oracle hints (`tests/system/oracle.rs`);
//! - the precompile price-and-hold catalogue (`tests/satin/precompile_gas.rs`, detained calls).
#![allow(dead_code)]
use super::{
    A, ABOVE_CAP, Account, Asm, Auth, B, BELOW_CAP, C, D, KEYLESS, Limits, ORACLE, Scenario, acct,
    funded, op, scenario,
};
use alloy_primitives::{Address, U256, hex, keccak256};

pub const ACCESS_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04]);
pub const X1: Address = Address::new([0x71; 20]);
pub const X2: Address = Address::new([0x72; 20]);
pub const X3: Address = Address::new([0x73; 20]);
pub const BENEFICIARY: Address = Address::ZERO;

/// Keyless deployment transactions: name, signed RLP, signer.
pub const KEYLESS_TXS: &[(&str, &str, &str)] = &[
    (
        "ok",
        "f8568085174876e80083030d4080808560016000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0x25a3eF92221C997F6abE3b393C5bA020c5E7EcD3",
    ),
    (
        "revert",
        "f85e8085174876e80083030d4080808d63deadbeef6000526004601cfd1ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0x4673cB38D39A60E84cD979644b9f510f4ECF829c",
    ),
    (
        "halt",
        "f8528085174876e80083030d40808081fe1ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0xC400c193Dc9A6a1b5415fEaC66103beea39F6C7b",
    ),
    (
        "empty",
        "f8568085174876e80083030d4080808560006000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0xcD8cd55C6BD52eE4Bcee33EC48db29163240faDF",
    ),
    (
        "writes",
        "f8678085174876e80083061a8080809660016001556007600052604260206000a160016000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0xCe7E10C4B3f635c4a2cd6C60358836F023200199",
    ),
    (
        "volatile",
        "f85a8085174876e800830493e08080894360015560016000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0x6165820aE16Fb6d6e8eA39429BdD53A107c35754",
    ),
    (
        "loop",
        "f8638085174876e800832dc6c08080926107d05b60019003806003575060016000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0xE71fFD845Dfd661F4AA6413011Ac7e97a81F338a",
    ),
    (
        "value",
        "f8568085174876e80083030d4080018560016000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0xA278483bC03ABfD4D94816B0d402b03cd7085F7e",
    ),
    (
        "nonce1",
        "f8560185174876e80083030d4080808560016000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0xF6c83B061A80Bcc071f71b0f051C94F178741423",
    ),
    (
        "child",
        "f8678085174876e800830927c08080966460016000f36000526005601b6000f05060016000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222",
        "0x21597e7d9204c8931b9987eBcc2bfc093063a2D5",
    ),
];

pub fn sel(sig: &str) -> [u8; 4] {
    keccak256(sig)[..4].try_into().unwrap()
}

fn word(v: u64) -> [u8; 32] {
    U256::from(v).to_be_bytes::<32>()
}

fn abi_bytes(selector: [u8; 4], head: &[[u8; 32]], data: &[u8]) -> Vec<u8> {
    let mut c = selector.to_vec();
    for h in head {
        c.extend_from_slice(h);
    }
    c.extend_from_slice(&word(32 * (head.len() as u64 + 1)));
    c.extend_from_slice(&word(data.len() as u64));
    let mut d = data.to_vec();
    d.resize(data.len().div_ceil(32) * 32, 0);
    c.extend_from_slice(&d);
    c
}

pub fn keyless_tx(name: &str) -> (Vec<u8>, Address) {
    let (_, raw, signer) = KEYLESS_TXS.iter().find(|t| t.0 == name).unwrap();
    (hex::decode(raw).unwrap(), signer.parse().unwrap())
}

/// `keylessDeploy(bytes, uint256)` with keyless transaction `name`, the deployment `(signer,
/// deploy address)`.
pub fn keyless_call(name: &str, gas_override: u64) -> (Vec<u8>, Address, Address) {
    let (raw, signer) = keyless_tx(name);
    (keyless_call_raw(&raw, gas_override), signer, signer.create(0))
}

/// `keylessDeploy(bytes, uint256)` with the signed keyless transaction `raw`.
pub fn keyless_call_raw(raw: &[u8], gas_override: u64) -> Vec<u8> {
    let mut c = sel("keylessDeploy(bytes,uint256)").to_vec();
    c.extend_from_slice(&word(0x40));
    c.extend_from_slice(&word(gas_override));
    c.extend_from_slice(&word(raw.len() as u64));
    let mut d = raw.to_vec();
    d.resize(raw.len().div_ceil(32) * 32, 0);
    c.extend_from_slice(&d);
    c
}

pub fn send_hint(topic: u64, data: &[u8]) -> Vec<u8> {
    abi_bytes(sel("sendHint(bytes32,bytes)"), &[word(topic)], data)
}

/// Writes `input` into memory at 0 (MSTOREs), then a call of `kind` to `to` with it.
pub fn mem_call(a: Asm, kind: u8, to: Address, value: u64, gas: u64, input: &[u8]) -> Asm {
    let mut a = a;
    for (i, chunk) in input.chunks(32).enumerate() {
        let mut w = [0u8; 32];
        w[..chunk.len()].copy_from_slice(chunk);
        if w != [0u8; 32] {
            a = a.push_bytes(&w).push(32 * i as u64).op(op::MSTORE);
        }
    }
    a.call_with(kind, to, value, gas, input.len() as u64)
}

fn auth(address: Address, nonce: u64, authority: Address) -> Auth {
    Auth { chain_id: 1, address, nonce, authority: Some(authority) }
}

fn delegation(target: Address) -> Vec<u8> {
    let mut c = vec![0xef, 0x01, 0x00];
    c.extend_from_slice(target.as_slice());
    c
}

fn q8(name: &'static str, accounts: Vec<Account>, gas: u64, limits: Limits) -> Scenario {
    let mut s = scenario(name, accounts, gas, limits);
    s.group = "q8";
    s
}

fn eip7702() -> Vec<Scenario> {
    let mut v = Vec::new();
    // D: SSTORE(1, 1), LOG0 of 32 bytes, STOP. Code a delegated authority runs.
    let d_code = Asm::new().sstore(1, 1).push(32).push(0).op(op::LOG0).stop().build();
    let d = acct(D, d_code.clone());
    // e01: one new authority delegating to D; the transaction calls the authority (D's code runs at
    // X1).
    let mut s =
        q8("e01_new_authority_runs_delegate", vec![d.clone()], BELOW_CAP, Limits::default());
    s.to = X1;
    s.auths = vec![auth(D, 0, X1)];
    v.push(s.clone());
    // e02: an existing (funded) authority, the transaction calls A.
    let mut e = s.clone();
    e.name = "e02_existing_authority";
    e.to = A;
    e.accounts.push(funded(X1, vec![], 1_000));
    e.accounts.push(acct(A, Asm::new().sstore(2, 2).stop().build()));
    v.push(e);
    // e03: two entries for one new authority (nonce 0 then 1): created once, one record, one
    // indicator.
    let mut e = s.clone();
    e.name = "e03_two_entries_one_authority";
    e.auths = vec![auth(D, 0, X1), auth(A, 1, X1)];
    v.push(e);
    // e04: wrong nonce (skipped after the load); e05: another chain id (not loaded);
    // e06: an invalid signature; e07: an authority holding code (skipped).
    let mut e = s.clone();
    e.name = "e04_wrong_nonce_skipped";
    e.auths = vec![auth(D, 5, X1), auth(D, 0, X2)];
    v.push(e);
    let mut e = s.clone();
    e.name = "e05_other_chain_skipped";
    e.auths = vec![
        Auth { chain_id: 2, address: D, nonce: 0, authority: Some(X1) },
        Auth { chain_id: 0, address: D, nonce: 0, authority: Some(X2) },
    ];
    v.push(e);
    let mut e = s.clone();
    e.name = "e06_invalid_signature";
    e.auths = vec![Auth { chain_id: 1, address: D, nonce: 0, authority: None }, auth(D, 0, X2)];
    v.push(e);
    let mut e = s.clone();
    e.name = "e07_authority_with_code_skipped";
    e.accounts.push(acct(X1, vec![0x00]));
    e.auths = vec![auth(D, 0, X1), auth(D, 0, X2)];
    v.push(e);
    // e08: clears a delegation the authority held at the start, then sets one again.
    let mut e = s.clone();
    e.name = "e08_clear_then_set";
    e.to = A;
    e.accounts.push(Account {
        address: X1,
        code: delegation(D),
        nonce: 3,
        balance: U256::from(5),
        storage: vec![],
    });
    e.accounts.push(acct(A, Asm::new().stop().build()));
    e.auths = vec![auth(Address::ZERO, 3, X1), auth(D, 4, X1)];
    v.push(e);
    // e09: the sender is an authority (its nonce after validation's bump): no record of its own.
    let mut e = s.clone();
    e.name = "e09_sender_authority";
    e.to = super::SENDER;
    e.auths = vec![auth(D, 1, super::SENDER), auth(D, 0, X2)];
    v.push(e);
    // e10 / e11: the list rule: KV limit 1 with two authorities; growth limit 1 with two new ones.
    let mut e = s.clone();
    e.name = "e10_list_skipped_kv";
    e.limits = Limits { kv: Some(1), ..Limits::default() };
    e.accounts.push(funded(X1, vec![], 1));
    e.accounts.push(funded(X2, vec![], 1));
    e.auths = vec![auth(D, 0, X1), auth(D, 0, X2)];
    v.push(e);
    let mut e = s.clone();
    e.name = "e11_list_skipped_growth";
    e.limits = Limits { growth: Some(1), ..Limits::default() };
    e.auths = vec![auth(D, 0, X1), auth(D, 0, X2), auth(D, 7, X3)];
    v.push(e);
    let mut e = s.clone();
    e.name = "e11b_list_fits_growth";
    e.limits = Limits { growth: Some(3), ..Limits::default() };
    e.auths = vec![auth(D, 0, X1), auth(D, 0, X2)];
    v.push(e);
    // e12: SALT m = 4 on the authority's leaf.
    let mut e = s.clone();
    e.name = "e12_authority_salt_m4";
    e.salt = vec![(X1, None, 4)];
    v.push(e);
    // e13: above the cap: the reservoir pays; the first frame reverts after a value transfer.
    let mut e = s.clone();
    e.name = "e13_above_cap_first_frame_reverts";
    e.gas_limit = ABOVE_CAP;
    e.to = B;
    e.value = U256::from(1);
    e.accounts.push(acct(B, Asm::new().sstore(1, 1).revert0().build()));
    v.push(e);
    let mut e = s.clone();
    e.name = "e13b_above_cap_value_to_new";
    e.gas_limit = ABOVE_CAP;
    e.to = super::EMPTY;
    e.value = U256::from(1);
    v.push(e);
    // e14: the authority is the block beneficiary: detained from the start under a small cap.
    let mut e = s.clone();
    e.name = "e14_beneficiary_authority_detained";
    e.limits = Limits { block_env_cap: Some(30_000), ..Limits::default() };
    e.to = A;
    let lp = Asm::new().op(op::JUMPDEST).push(1).push(0).op(op::SSTORE).push(0).op(op::JUMP);
    e.accounts.push(acct(A, lp.build()));
    e.auths = vec![auth(D, 0, BENEFICIARY)];
    v.push(e);
    // e15: value to the authority itself (changed already: no recipient record).
    let mut e = s.clone();
    e.name = "e15_value_to_authority";
    e.value = U256::from(7);
    v.push(e);
    // e16: the body and a record per entry over the data-size limit: refused at validation.
    let mut e = s.clone();
    e.name = "e16_body_records_over_data_limit";
    e.limits = Limits { data: Some(310 + 101 * 3 + 40 * 2), ..Limits::default() };
    e.auths = vec![auth(D, 0, X1), auth(D, 0, X2), auth(D, 0, X3)];
    v.push(e);
    let mut e = s.clone();
    e.name = "e16b_body_records_fit";
    e.limits = Limits { data: Some(310 + 101 * 3 + 40 * 3 + 2_000), ..Limits::default() };
    e.auths = vec![auth(D, 0, X1), auth(D, 0, X2), auth(D, 0, X3)];
    v.push(e);
    // e17: gas enough for the intrinsic and the body but not the authorities' state gas.
    let mut e = s.clone();
    e.name = "e17_authority_charges_out_of_gas";
    e.gas_limit = 120_000;
    e.auths = vec![auth(D, 0, X1), auth(D, 0, X2)];
    v.push(e);
    // e18: the delegate halts: authorities stay, the first frame's record goes.
    let mut e = s.clone();
    e.name = "e18_delegate_halts";
    e.accounts[1] = acct(D, vec![0xfe]);
    e.value = U256::from(3);
    v.push(e);
    // e19: the delegate's code reads its own (absent) storage and writes three slots under a growth
    // limit.
    let mut e = s.clone();
    e.name = "e19_delegate_writes_growth_stop";
    e.limits = Limits { growth: Some(3), ..Limits::default() };
    e.accounts[1] = acct(D, Asm::new().sstore(1, 1).sstore(2, 1).sstore(3, 1).stop().build());
    v.push(e);
    // e20: a contract calls an authority with value (a delegate that logs and writes).
    let mut e = s.clone();
    e.name = "e20_call_into_authority_with_value";
    e.to = A;
    e.value = U256::from(10);
    e.accounts.push(acct(A, Asm::new().call_all(op::CALL, X1, 4).record_call(5).stop().build()));
    v.push(e);
    v
}

fn keyless() -> Vec<Scenario> {
    let mut v = Vec::new();
    let mk = |name: &'static str, tx: &str, ovr: u64, gas: u64| {
        let (data, signer, deploy) = keyless_call(tx, ovr);
        let mut s = q8(name, vec![], gas, Limits::default());
        s.to = KEYLESS;
        s.data = data;
        (s, signer, deploy)
    };
    for (name, tx) in [
        ("k01_ok", "ok"),
        ("k02_revert", "revert"),
        ("k03_halt", "halt"),
        ("k04_empty_code", "empty"),
        ("k05_writes_logs", "writes"),
        ("k17_child_creation", "child"),
    ] {
        v.push(mk(name, tx, 1_000_000, 5_000_000).0);
    }
    let (mut s, _, _) = mk("k06_volatile_detained", "volatile", 1_000_000, 5_000_000);
    s.limits = Limits { block_env_cap: Some(60_000), ..Limits::default() };
    v.push(s);
    let (mut s, _, _) = mk("k06b_volatile_crossing", "volatile", 1_000_000, 5_000_000);
    s.limits = Limits { block_env_cap: Some(5_000), ..Limits::default() };
    v.push(s);
    v.push(mk("k07_loop", "loop", 5_000_000, 20_000_000).0);
    let (s, _, _) = mk("k07b_loop_override_capped", "loop", 50_000_000, 4_000_000);
    v.push(s);
    v.push(mk("k08_value_unfunded_signer", "value", 1_000_000, 5_000_000).0);
    let (mut s, signer, _) = mk("k09_value_funded_signer", "value", 1_000_000, 5_000_000);
    s.accounts.push(funded(signer, vec![], 1_000));
    v.push(s);
    v.push(mk("k10_nonzero_tx_nonce", "nonce1", 1_000_000, 5_000_000).0);
    v.push(mk("k11_override_below_tx_gas", "ok", 100_000, 5_000_000).0);
    let (mut s, _, _) = mk("k12_malformed_call", "ok", 1_000_000, 5_000_000);
    s.data.truncate(40);
    v.push(s);
    let (mut s, _, _) = mk("k13_call_value_refused", "ok", 1_000_000, 5_000_000);
    s.value = U256::from(1);
    v.push(s);
    let (mut s, _, deploy) = mk("k14_contract_already_exists", "ok", 1_000_000, 5_000_000);
    s.accounts.push(acct(deploy, vec![0x00]));
    v.push(s);
    let (mut s, signer, _) = mk("k15_signer_nonce_too_high", "ok", 1_000_000, 5_000_000);
    s.accounts.push(Account { address: signer, nonce: 2, ..Account::default() });
    v.push(s);
    let (mut s, signer, _) = mk("k16_signer_nonce_one_taken_back", "ok", 1_000_000, 5_000_000);
    s.accounts.push(Account {
        address: signer,
        nonce: 1,
        balance: U256::from(9),
        ..Account::default()
    });
    v.push(s);
    let (mut s, signer, _) = mk("k16b_signer_funded_nonce_zero", "ok", 1_000_000, 5_000_000);
    s.accounts.push(funded(signer, vec![], 5));
    v.push(s);
    let (mut s, signer, _) = mk("k16c_signer_has_code", "ok", 1_000_000, 5_000_000);
    s.accounts.push(acct(signer, vec![0x00]));
    v.push(s);
    // A contract calls keylessDeploy (depth 1): not dispatched, the contract's own code runs.
    let (data, _, _) = keyless_call("ok", 1_000_000);
    let mut s = q8(
        "k18_from_a_contract_not_dispatched",
        vec![acct(
            A,
            mem_call(Asm::new(), op::CALL, KEYLESS, 0, 2_000_000, &data)
                .record_call(1)
                .stop()
                .build(),
        )],
        5_000_000,
        Limits::default(),
    );
    s.to = A;
    v.push(s);
    let (mut s, _, _) = mk("k19_above_cap", "writes", 1_000_000, ABOVE_CAP);
    s.salt = vec![];
    v.push(s);
    v.push(mk("k20_overhead_out_of_gas", "ok", 1_000_000, 90_000).0);
    let (mut s, _, _) = mk("k21_growth_limit_one", "ok", 1_000_000, 5_000_000);
    s.limits = Limits { growth: Some(1), ..Limits::default() };
    v.push(s);
    let (mut s, _, _) = mk("k22_kv_limit_one", "writes", 1_000_000, 5_000_000);
    s.limits = Limits { kv: Some(1), ..Limits::default() };
    v.push(s);
    let (mut s, signer, deploy) = mk("k23_salt_m8", "writes", 1_000_000, 5_000_000);
    s.salt = vec![(signer, None, 8), (deploy, None, 2), (deploy, Some(1), 4)];
    v.push(s);
    let (mut s, _, _) = mk("k24_not_preeip155", "ok", 1_000_000, 5_000_000);
    s.data = {
        let raw = hex::decode("f856808504a817c800830186a0808085608060405226a0fceb37453e90ac5ec2780748b7a4907b1dcfb87708697de2e6be19831938c77ba0224ee4c1aaa6a1490b4e3a1fbed7c5151668a12b6f6e3227c2692a64cf79e81f").unwrap();
        abi_bytes(sel("keylessDeploy(bytes,uint256)"), &[], &raw).to_vec()
    };
    // abi_bytes puts the offset first; keylessDeploy wants (offset, gas override): rebuild.
    let raw = hex::decode("f856808504a817c800830186a0808085608060405226a0fceb37453e90ac5ec2780748b7a4907b1dcfb87708697de2e6be19831938c77ba0224ee4c1aaa6a1490b4e3a1fbed7c5151668a12b6f6e3227c2692a64cf79e81f").unwrap();
    let mut c = sel("keylessDeploy(bytes,uint256)").to_vec();
    c.extend_from_slice(&word(0x40));
    c.extend_from_slice(&word(1_000_000));
    c.extend_from_slice(&word(raw.len() as u64));
    let mut d = raw.clone();
    d.resize(raw.len().div_ceil(32) * 32, 0);
    c.extend_from_slice(&d);
    s.data = c;
    v.push(s);
    v
}

fn access_control() -> Vec<Scenario> {
    let mut v = Vec::new();
    let disable = sel("disableVolatileDataAccess()");
    let enable = sel("enableVolatileDataAccess()");
    let is_disabled = sel("isVolatileDataAccessDisabled()");
    let ac = |a: Asm, s: [u8; 4], slot: u64| {
        mem_call(a, op::CALL, ACCESS_CONTROL, 0, 100_000, &s).record_call(slot)
    };
    // a01: A switches its own access off, then reads TIMESTAMP: refused.
    let a = ac(Asm::new(), disable, 1).op(op::TIMESTAMP).push(3).op(op::SSTORE).stop().build();
    v.push(q8(
        "a01_disable_then_timestamp_refused",
        vec![acct(A, a)],
        BELOW_CAP,
        Limits::default(),
    ));
    // a02: A switches off, B reads NUMBER (refused, B reverts), A switches back on and reads.
    let b = Asm::new().op(op::NUMBER).push(1).op(op::SSTORE).stop().build();
    let a =
        ac(Asm::new(), disable, 1).call_all(op::CALL, B, 0).record_call(3).store_returndata_word(5);
    let a = ac(a, enable, 6).op(op::TIMESTAMP).push(8).op(op::SSTORE).stop().build();
    v.push(q8(
        "a02_child_refused_parent_reenables",
        vec![acct(A, a), acct(B, b)],
        BELOW_CAP,
        Limits::default(),
    ));
    // a03: A switches off; B tries to switch back on: DisabledByParent.
    let b = ac(Asm::new(), enable, 1).store_returndata_word(3).stop().build();
    let a = ac(Asm::new(), disable, 1).call_all(op::CALL, B, 0).record_call(3).stop().build();
    v.push(q8(
        "a03_disabled_by_parent",
        vec![acct(A, a), acct(B, b)],
        BELOW_CAP,
        Limits::default(),
    ));
    // a04: B switches off and returns: A reads afterwards.
    let b = ac(Asm::new(), disable, 1).op(op::COINBASE).push(3).op(op::SSTORE).stop().build();
    let a = Asm::new()
        .call_all(op::CALL, B, 0)
        .record_call(1)
        .op(0x45)
        .push(3)
        .op(op::SSTORE)
        .stop()
        .build();
    v.push(q8(
        "a04_switch_cleared_on_return",
        vec![acct(A, a), acct(B, b)],
        BELOW_CAP,
        Limits::default(),
    ));
    // a05: isVolatileDataAccessDisabled before and after.
    let a = ac(Asm::new(), is_disabled, 1).store_returndata_word(3);
    let a = ac(a, disable, 4);
    let a = ac(a, is_disabled, 5).store_returndata_word(7).stop().build();
    v.push(q8("a05_is_disabled", vec![acct(A, a)], BELOW_CAP, Limits::default()));
    // a06: BALANCE of the beneficiary refused; a07: the Oracle's storage refused in its frame.
    let a = ac(Asm::new(), disable, 1)
        .push_addr(BENEFICIARY)
        .op(op::BALANCE)
        .push(3)
        .op(op::SSTORE)
        .stop()
        .build();
    v.push(q8("a06_beneficiary_balance_refused", vec![acct(A, a)], BELOW_CAP, Limits::default()));
    let oracle = Account {
        address: ORACLE,
        code: Asm::new()
            .push(5)
            .op(op::SLOAD)
            .push(0)
            .op(op::MSTORE)
            .push(32)
            .push(0)
            .op(op::RETURN)
            .build(),
        storage: vec![(5, 9)],
        ..Account::default()
    };
    let a = ac(Asm::new(), disable, 1)
        .call_all(op::CALL, ORACLE, 0)
        .record_call(3)
        .store_returndata_word(5)
        .stop()
        .build();
    let mut s = q8(
        "a07_oracle_read_refused",
        vec![acct(A, a), oracle.clone()],
        BELOW_CAP,
        Limits::default(),
    );
    s.oracle = vec![(5, 77)];
    v.push(s);
    // a08: the transaction calls the contract directly; a09: with value; a10: DELEGATECALL
    // (not intercepted); a11: BLOCKHASH and SLOTNUM refused; a13: an unknown selector.
    let mut s = q8("a08_direct_disable", vec![], BELOW_CAP, Limits::default());
    s.to = ACCESS_CONTROL;
    s.data = disable.to_vec();
    v.push(s);
    let a = mem_call(Asm::new(), op::CALL, ACCESS_CONTROL, 1, 100_000, &disable)
        .record_call(1)
        .store_returndata_word(3)
        .stop()
        .build();
    v.push(q8("a09_value_refused", vec![funded(A, a, 10)], BELOW_CAP, Limits::default()));
    let a = mem_call(Asm::new(), op::DELEGATECALL, ACCESS_CONTROL, 0, 100_000, &disable)
        .record_call(1)
        .op(op::TIMESTAMP)
        .push(3)
        .op(op::SSTORE)
        .stop()
        .build();
    v.push(q8("a10_delegatecall_not_intercepted", vec![acct(A, a)], BELOW_CAP, Limits::default()));
    let a =
        ac(Asm::new(), disable, 1).push(0).op(op::BLOCKHASH).push(3).op(op::SSTORE).stop().build();
    v.push(q8("a11_blockhash_refused", vec![acct(A, a)], BELOW_CAP, Limits::default()));
    let a = ac(Asm::new(), disable, 1).op(op::SLOTNUM).push(3).op(op::SSTORE).stop().build();
    v.push(q8("a11b_slotnum_refused", vec![acct(A, a)], BELOW_CAP, Limits::default()));
    let a = ac(Asm::new(), sel("unknown()"), 1).stop().build();
    v.push(q8(
        "a13_unknown_selector_falls_through",
        vec![acct(A, a)],
        BELOW_CAP,
        Limits::default(),
    ));
    // a12: detained (a read first), then switched off, then a child loops to the crossing.
    let b = Asm::new().op(op::JUMPDEST).push(1).push(0).op(op::SSTORE).push(0).op(op::JUMP).build();
    let a = Asm::new().op(op::NUMBER).op(op::POP);
    let a = ac(a, disable, 1).call_all(op::CALL, B, 0).record_call(3).stop().build();
    v.push(q8(
        "a12_detained_and_disabled",
        vec![acct(A, a), acct(B, b)],
        BELOW_CAP,
        Limits { block_env_cap: Some(80_000), ..Limits::default() },
    ));
    // a14: B (switched off by A) calls C which reads the beneficiary through EXTCODEHASH.
    let c = Asm::new().push_addr(BENEFICIARY).op(0x3f).push(1).op(op::SSTORE).stop().build();
    let b = Asm::new().call_all(op::CALL, C, 0).record_call(1).stop().build();
    let a = ac(Asm::new(), disable, 1).call_all(op::CALL, B, 0).record_call(3).stop().build();
    v.push(q8(
        "a14_grandchild_refused",
        vec![acct(A, a), acct(B, b), acct(C, c)],
        BELOW_CAP,
        Limits::default(),
    ));
    v
}

fn hints() -> Vec<Scenario> {
    let mut v = Vec::new();
    let oracle = |code: Vec<u8>| Account {
        address: ORACLE,
        code,
        storage: vec![(5, 9)],
        ..Account::default()
    };
    let oracle_reader = oracle(
        Asm::new()
            .push(5)
            .op(op::SLOAD)
            .push(0)
            .op(op::MSTORE)
            .push(32)
            .push(0)
            .op(op::RETURN)
            .build(),
    );
    let oracle_stop = oracle(Asm::new().push(1).push(0).op(op::LOG0).stop().build());
    let hint = send_hint(7, &[0xab; 64]);
    // h01: a hint counted and forwarded, the Oracle's bytecode runs.
    let a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint).record_call(1).stop().build();
    v.push(q8(
        "h01_hint_forwarded",
        vec![acct(A, a.clone()), oracle_stop.clone()],
        BELOW_CAP,
        Limits::default(),
    ));
    // h02: a hint over the transaction's data-size limit: answered with the stop, A catches it.
    let a2 = mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint)
        .record_call(1)
        .store_returndata_word(3)
        .stop()
        .build();
    v.push(q8(
        "h02_hint_over_data_limit",
        vec![acct(A, a2), oracle_stop.clone()],
        BELOW_CAP,
        Limits { data: Some(310 + 100), ..Limits::default() },
    ));
    // h03: with value (not intercepted); h07: STATICCALL; h09: a payload that does not decode.
    let a = mem_call(Asm::new(), op::CALL, ORACLE, 1, 200_000, &hint).record_call(1).stop().build();
    v.push(q8(
        "h03_hint_with_value_not_intercepted",
        vec![funded(A, a, 5), oracle_stop.clone()],
        BELOW_CAP,
        Limits::default(),
    ));
    let a = mem_call(Asm::new(), op::STATICCALL, ORACLE, 0, 200_000, &hint)
        .record_call(1)
        .stop()
        .build();
    v.push(q8(
        "h07_hint_staticcall",
        vec![acct(A, a), oracle(Asm::new().stop().build())],
        BELOW_CAP,
        Limits::default(),
    ));
    let a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint[..4])
        .record_call(1)
        .stop()
        .build();
    v.push(q8(
        "h09_hint_undecodable_counted",
        vec![acct(A, a), oracle_stop.clone()],
        BELOW_CAP,
        Limits::default(),
    ));
    // h04: from a frame whose access is off: dropped, the bytecode runs.
    let disable = sel("disableVolatileDataAccess()");
    let a = mem_call(Asm::new(), op::CALL, ACCESS_CONTROL, 0, 100_000, &disable).record_call(1);
    let a = mem_call(a, op::CALL, ORACLE, 0, 200_000, &hint).record_call(3).stop().build();
    v.push(q8(
        "h04_hint_dropped_when_disabled",
        vec![acct(A, a), oracle_stop.clone()],
        BELOW_CAP,
        Limits::default(),
    ));
    // h05: B sends the hint and reverts: the hint's bytes stay on the transaction.
    let b = mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint).op(op::POP).revert0().build();
    let a = Asm::new().call_all(op::CALL, B, 0).record_call(1).stop().build();
    v.push(q8(
        "h05_hint_in_reverted_child",
        vec![acct(A, a), acct(B, b), oracle_stop.clone()],
        BELOW_CAP,
        Limits::default(),
    ));
    // h06: a hint, then the Oracle's storage read through the service.
    let a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint)
        .record_call(1)
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push_addr(ORACLE)
        .op(op::GAS)
        .op(op::CALL)
        .record_call(3)
        .store_returndata_word(5)
        .stop()
        .build();
    let mut s = q8(
        "h06_hint_then_oracle_read",
        vec![acct(A, a), oracle_reader.clone()],
        BELOW_CAP,
        Limits::default(),
    );
    s.oracle = vec![(5, 1234)];
    v.push(s);
    // h08: the transaction calls the Oracle's sendHint directly.
    let mut s = q8("h08_direct_hint", vec![oracle_stop.clone()], BELOW_CAP, Limits::default());
    s.to = ORACLE;
    s.data = hint.clone();
    v.push(s);
    // h10: two hints fill the limit, a write after them is stopped.
    let a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint).record_call(1);
    let a =
        mem_call(a, op::CALL, ORACLE, 0, 200_000, &hint).record_call(3).sstore(7, 7).stop().build();
    v.push(q8(
        "h10_hints_fill_limit_then_write",
        vec![acct(A, a), oracle_stop.clone()],
        BELOW_CAP,
        Limits { data: Some(310 + 2 * 164 + 150), ..Limits::default() },
    ));
    v
}

/// Precompile calls with an input of `len` bytes whose leading words are `words`.
fn precompile_input(words: &[(usize, [u8; 32])], len: usize) -> Vec<u8> {
    let mut v = vec![0u8; len];
    for (off, w) in words {
        v[*off..*off + 32].copy_from_slice(w);
    }
    v
}

fn precompiles() -> Vec<Scenario> {
    let mut v = Vec::new();
    let pc = |b: u16| {
        let mut a = [0u8; 20];
        a[18] = (b >> 8) as u8;
        a[19] = b as u8;
        Address::new(a)
    };
    let mut rounds = [0u8; 32];
    rounds[..4].copy_from_slice(&40_000u32.to_be_bytes());
    let cases: Vec<(&'static str, u16, Vec<u8>)> = vec![
        (
            "modexp_big_exponent",
            5,
            precompile_input(
                &[(0, word(1)), (32, word(64)), (64, word(1)), (96, [0xff; 32])],
                96 + 1 + 64 + 1,
            ),
        ),
        ("bn_pairing_one_pair", 8, vec![0u8; 192]),
        ("bn_pairing_over_size_cap", 8, vec![0u8; 57_600 + 192]),
        ("blake2f_many_rounds", 9, precompile_input(&[(0, rounds)], 213)),
        ("bls_g1_add", 0x0b, vec![0u8; 256]),
        ("bls_g1_msm_four", 0x0c, vec![0u8; 160 * 4]),
        ("bls_g2_add", 0x0d, vec![0u8; 512]),
        ("bls_g2_msm_two", 0x0e, vec![0u8; 288 * 2]),
        ("bls_pairing_one", 0x0f, vec![0u8; 384]),
        ("bls_map_fp_to_g1", 0x10, vec![0u8; 64]),
        ("bls_map_fp2_to_g2", 0x11, vec![0u8; 128]),
        ("p256verify", 0x100, vec![0u8; 160]),
        ("bls_g1_msm_bad_length", 0x0c, vec![0u8; 100]),
        // Malformed inputs, priced by the probe: BLAKE2F refuses a length other than 213 before
        // any gas (price 0, though the rounds word asks for 40,000); the BN254 pairing charges
        // for the whole 192-byte pairs (113,000) before it checks the length.
        ("blake2f_bad_length", 9, precompile_input(&[(0, rounds)], 212)),
        ("bn_pairing_odd_length", 8, vec![0u8; 192 * 2 + 5]),
    ];
    let cases =
        cases.into_iter().map(|(name, addr, input)| (name, addr, input, 3_000_000, 10_000_000));
    // Over op-evm2's Karst size cap with a forward that covers the pairing formula (45,000 + 34,000
    // per pair, 10,279,000 here): refused before any gas, it is priced at nothing, so detention
    // lets it run (and halt) instead of holding it.
    let big = (
        "bn_pairing_over_size_cap_big_forward",
        8,
        vec![0u8; 57_600 + 192],
        12_000_000,
        20_000_000,
    );
    for (name, addr, input, forward, gas) in cases.chain([big]) {
        for (tag, cap) in [("plain", None), ("detained", Some(25_000u64))] {
            let read =
                if cap.is_some() { Asm::new().op(op::NUMBER).op(op::POP) } else { Asm::new() };
            let a = mem_call(read, op::CALL, pc(addr), 0, forward, &input)
                .record_call(1)
                .stop()
                .build();
            let full: &'static str = Box::leak(format!("p_{name}_{tag}").into_boxed_str());
            v.push(q8(
                full,
                vec![acct(A, a)],
                gas,
                Limits { block_env_cap: cap, ..Limits::default() },
            ));
        }
    }
    v
}

fn q8_more() -> Vec<Scenario> {
    let mut v = Vec::new();
    let disable = sel("disableVolatileDataAccess()");
    let ac = |a: Asm| mem_call(a, op::CALL, ACCESS_CONTROL, 0, 100_000, &disable).record_call(1);
    // a15: a CALL of the beneficiary refused; a16: SELFDESTRUCT to the beneficiary refused;
    // a17: the beneficiary is B (a contract): a CALL of it is a volatile read (detained).
    let a = ac(Asm::new()).call_all(op::CALL, BENEFICIARY, 0).record_call(3).stop().build();
    v.push(q8("a15_call_beneficiary_refused", vec![acct(A, a)], BELOW_CAP, Limits::default()));
    let a = ac(Asm::new()).push_addr(BENEFICIARY).op(op::SELFDESTRUCT).build();
    v.push(q8(
        "a16_selfdestruct_to_beneficiary_refused",
        vec![acct(A, a)],
        BELOW_CAP,
        Limits::default(),
    ));
    let b = Asm::new().op(op::JUMPDEST).push(1).push(0).op(op::SSTORE).push(0).op(op::JUMP).build();
    let a = Asm::new().call_all(op::CALL, B, 0).record_call(1).stop().build();
    let mut s = q8(
        "a17_call_beneficiary_contract_detained",
        vec![acct(A, a.clone()), acct(B, b.clone())],
        BELOW_CAP,
        Limits { block_env_cap: Some(40_000), ..Limits::default() },
    );
    s.beneficiary = B;
    v.push(s);
    let a = ac(Asm::new()).call_all(op::CALL, B, 0).record_call(3).stop().build();
    let mut s = q8(
        "a18_call_beneficiary_contract_refused",
        vec![acct(A, a), acct(B, b)],
        BELOW_CAP,
        Limits::default(),
    );
    s.beneficiary = B;
    v.push(s);
    // a19-a21: EXTCODECOPY of the beneficiary in a frame whose access is off. It charges its copy
    // and memory expansion before its load: when it can pay them, the load is refused and the
    // frame reverts on the gas it had after the static gas (a19); when it cannot, it halts on the
    // charge before any load (a20 on the memory expansion, a21 on the copy).
    for (name, mem_offset, len) in [
        ("a19_extcodecopy_beneficiary_refused_on_static_gas", 0u64, 1_024u64),
        ("a20_extcodecopy_beneficiary_memory_unpayable_halts", 1 << 24, 32),
        ("a21_extcodecopy_beneficiary_copy_unpayable_halts", 0, 1 << 27),
    ] {
        let a = ac(Asm::new())
            .push(len)
            .push(0)
            .push(mem_offset)
            .push_addr(BENEFICIARY)
            .op(0x3c)
            .stop()
            .build();
        v.push(q8(name, vec![acct(A, a)], BELOW_CAP, Limits::default()));
    }
    // a22-a27: a call of the beneficiary itself in a frame whose access is off. Satin's Host
    // refuses the load of the target, after the call's pops, the static check, the memory
    // expansion of its ranges and the value-transfer charge, and before the skip-cold check; a
    // call that fails earlier halts on that charge. `cold_burn` runs the frame's gas below a cold
    // access (2,500) or below the value-transfer charge (8,000) first.
    let call_kinds = [
        ("call", op::CALL),
        ("callcode", op::CALLCODE),
        ("delegatecall", op::DELEGATECALL),
        ("staticcall", op::STATICCALL),
    ];
    let beneficiary_call = |kind: u8, out_off: u64, value: u64, burn_below: Option<u64>| {
        let mut a = ac(Asm::new());
        if let Some(below) = burn_below {
            // JUMPDEST; GAS; PUSH2 below; LT; PUSH2 start; JUMPI: spin while GAS > below.
            let start = a.clone().build().len() as u64;
            a = a.op(op::JUMPDEST).op(op::GAS).push(below).op(op::LT).push(start).op(op::JUMPI);
        }
        a = a.push(32).push(out_off).push(0).push(0);
        if kind == op::CALL || kind == op::CALLCODE {
            a = a.push(value);
        }
        a.push_addr(BENEFICIARY).push(0).op(kind).stop().build()
    };
    for (kname, kind) in call_kinds {
        for (tag, out_off) in [("refused", 0u64), ("memory_unpayable_halts", 1 << 24)] {
            let name: &'static str =
                Box::leak(format!("a22_{kname}_beneficiary_{tag}").into_boxed_str());
            let a = beneficiary_call(kind, out_off, 0, None);
            v.push(q8(name, vec![acct(A, a)], BELOW_CAP, Limits::default()));
        }
    }
    // The value-transfer charge (9,000) unpayable: the call halts on it, before the load.
    let a = beneficiary_call(op::CALL, 0, 1, Some(8_000));
    v.push(q8(
        "a23_call_beneficiary_value_unpayable_halts",
        vec![acct(A, a)],
        BELOW_CAP,
        Limits::default(),
    ));
    // The same with the value-transfer charge payable: refused all the same, the load comes first.
    let a = beneficiary_call(op::CALL, 0, 1, Some(40_000));
    v.push(q8(
        "a23b_call_beneficiary_value_payable_refused",
        vec![acct(A, a)],
        BELOW_CAP,
        Limits::default(),
    ));
    // A cold access unpayable (the frame holds less than 2,600): the Host refuses before its
    // skip-cold check.
    let a = beneficiary_call(op::CALL, 0, 0, Some(2_500));
    v.push(q8(
        "a24_call_beneficiary_cold_unpayable",
        vec![acct(A, a)],
        BELOW_CAP,
        Limits::default(),
    ));
    // a25-a26: BLOCKHASH in a frame whose access is off. revm pops its operand before the Host
    // answers, so an empty stack halts with a stack underflow; with an operand the read is refused.
    let a = ac(Asm::new()).op(op::BLOCKHASH).stop().build();
    v.push(q8(
        "a25_blockhash_empty_stack_underflows",
        vec![acct(A, a)],
        BELOW_CAP,
        Limits::default(),
    ));
    let a = ac(Asm::new()).push(0).op(op::BLOCKHASH).stop().build();
    v.push(q8("a26_blockhash_refused", vec![acct(A, a)], BELOW_CAP, Limits::default()));
    // k25: the keyless signer is the block beneficiary: the call reads it (detained).
    for (name, cap) in
        [("k25_signer_is_beneficiary", 60_000u64), ("k25b_signer_is_beneficiary_tight", 2_000)]
    {
        let (data, signer, _) = keyless_call("writes", 1_000_000);
        let mut s =
            q8(name, vec![], 5_000_000, Limits { block_env_cap: Some(cap), ..Limits::default() });
        s.to = KEYLESS;
        s.data = data;
        s.beneficiary = signer;
        v.push(s);
    }
    // t01-t04: transaction-level volatile reads (satin.md §12 "What is volatile"): the recipient
    // is the beneficiary; the sender is; the recipient delegates (EIP-7702) to it; a CALL
    // follows a delegation to it.
    let lp =
        Asm::new().op(op::JUMPDEST).push(1).push(0).op(op::SSTORE).push(0).op(op::JUMP).build();
    let mut s = q8(
        "t01_recipient_is_beneficiary",
        vec![acct(B, lp.clone())],
        BELOW_CAP,
        Limits { block_env_cap: Some(40_000), ..Limits::default() },
    );
    s.to = B;
    s.beneficiary = B;
    v.push(s);
    let mut s = q8(
        "t02_sender_is_beneficiary",
        vec![acct(B, lp.clone())],
        BELOW_CAP,
        Limits { block_env_cap: Some(40_000), ..Limits::default() },
    );
    s.to = B;
    s.beneficiary = super::SENDER;
    v.push(s);
    let mut s = q8(
        "t03_recipient_delegates_to_beneficiary",
        vec![acct(B, lp.clone()), acct(X1, delegation(B))],
        BELOW_CAP,
        Limits { block_env_cap: Some(40_000), ..Limits::default() },
    );
    s.to = X1;
    s.beneficiary = B;
    v.push(s);
    let a = Asm::new().call_all(op::CALL, X1, 0).record_call(1).stop().build();
    let mut s = q8(
        "t04_call_follows_delegation_to_beneficiary",
        vec![acct(A, a), acct(B, lp.clone()), acct(X1, delegation(B))],
        BELOW_CAP,
        Limits { block_env_cap: Some(40_000), ..Limits::default() },
    );
    s.beneficiary = B;
    v.push(s);
    let a = ac(Asm::new()).call_all(op::CALL, X1, 0).record_call(3).stop().build();
    let mut s = q8(
        "t05_call_follows_delegation_refused",
        vec![acct(A, a), acct(B, lp), acct(X1, delegation(B))],
        BELOW_CAP,
        Limits::default(),
    );
    s.beneficiary = B;
    v.push(s);
    // h11: B restores its own slot (a take-back frees 40 bytes), sends a hint into that room and
    // reverts: the record comes back, the hint stays, so A resumes over the data-size limit.
    let hint = send_hint(1, &[0xcd; 32]);
    let other = mem_call(Asm::new().sstore(1, 0), op::CALL, ORACLE, 0, 200_000, &hint)
        .op(op::POP)
        .revert0()
        .build();
    let head_len = 2 + 1 + 1 + 3 + 1;
    let dest = head_len + other.len();
    let mut b =
        vec![op::PUSH1, 1, op::SLOAD, op::ISZERO, 0x61, (dest >> 8) as u8, dest as u8, op::JUMPI];
    b.extend(other);
    b.extend(Asm::new().op(op::JUMPDEST).sstore(1, 1).stop().build());
    let a = Asm::new()
        .call_all(op::CALL, B, 0)
        .record_call(1)
        .call_all(op::CALL, B, 0)
        .record_call(3)
        .sstore(7, 7)
        .stop()
        .build();
    let oracle_stop = Account {
        address: ORACLE,
        code: Asm::new().stop().build(),
        storage: vec![(5, 9)],
        ..Account::default()
    };
    for limit in [430u64, 440, 450, 460, 470, 480, 490, 520] {
        let name: &'static str =
            Box::leak(format!("h11_hold_caller_limit_{limit}").into_boxed_str());
        v.push(q8(
            name,
            vec![acct(A, a.clone()), acct(B, b.clone()), oracle_stop.clone()],
            BELOW_CAP,
            Limits { data: Some(limit), ..Limits::default() },
        ));
    }
    v
}

pub fn all() -> Vec<Scenario> {
    let mut v = eip7702();
    v.extend(q8_more());
    v.extend(keyless());
    v.extend(access_control());
    v.extend(hints());
    v.extend(precompiles());
    v
}
