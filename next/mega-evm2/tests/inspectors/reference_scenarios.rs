//! The fifteen scenarios the tracer tests of the Satin implementation under review pin, built the
//! same way: the same accounts and addresses, the same code byte for byte, the same values,
//! limits and transaction kinds, so the call, prestate-diff and struct-log output of the two
//! implementations can be compared. Only the gas limits differ: these use round figures with room
//! to spare instead of the other side's derived ones, which nothing a scenario does depends on;
//! the figures a gas limit sets (a frame's `gas`) are what that comparison leaves out.
//!
//! Each test pins every tracer's output for its scenario (`harness::pin`, every traced run held to
//! the plain one) and asserts by name what the scenario must show.

use crate::{
    common,
    harness::{Asm, Case, NO_LIMITS, Traces, balance, call, code, create, db, deposit, pin},
};
use alloy_consensus::{SignableTransaction, TxLegacy};
use alloy_primitives::{Address, Bytes, Signature, TxKind, U256, address, hex};
use evm2::{evm::InMemoryDB, interpreter::op};
use mega_evm2::{
    TX_BODY_SIZE, TxLimits,
    system_contracts::{FLAT_SYSTEM_CONTRACTS, SEQUENCER_REGISTRY_CONTRACT},
};
use serde_json::{Value, json};

pub const CALLER: Address = address!("0x0000000000000000000000000000000000400000");
pub const PAYEE: Address = address!("0x0000000000000000000000000000000000400001");
pub const CONTRACT: Address = address!("0x0000000000000000000000000000000000400002");
pub const CHILD: Address = address!("0x0000000000000000000000000000000000400003");

/// A gas limit with room for every scenario below the execution cap.
pub const GAS: u64 = 10_000_000;
/// Satin's execution cap: gas above it goes to the reservoir.
pub const EXECUTION_CAP: u64 = 200_000_000;
/// The gas limit the keyless deployments are signed with.
const SIGNED_GAS_LIMIT: u64 = 1_000_000;

/// The caller's balance, 10^15 wei.
pub fn funded() -> Vec<(Address, evm2::evm::AccountInfo)> {
    vec![(CALLER, balance(U256::from(1_000_000_000_000_000u64)))]
}

/// [`funded`] plus `accounts`.
pub fn state(accounts: impl IntoIterator<Item = (Address, evm2::evm::AccountInfo)>) -> InMemoryDB {
    db(funded().into_iter().chain(accounts))
}

/// A call from [`CALLER`].
pub fn tx(
    to: Address,
    value: u64,
    input: Bytes,
    gas: u64,
) -> alloy_consensus::transaction::Recovered<op_evm2::OpTxEnvelope> {
    call(CALLER, to, U256::from(value), input, gas)
}

/// The value-call shape the scenarios share: `CALL` of `target` with all the gas left.
fn calling(target: Address, value: u64) -> Asm {
    Asm::default().call(target, U256::from(value))
}

/// A parent that calls [`CHILD`] with all its gas and no value, then, if it resumes, writes a
/// slot and stops.
fn calling_child() -> Bytes {
    calling(CHILD, 0).op(op::POP).sstore(U256::ZERO, U256::from(1)).stop().build()
}

/// A frame that emits one `LOG0` of `len` bytes of memory, then stops.
fn logging(len: u8) -> Bytes {
    Asm::default().push_u8(len).ops([op::PUSH0, op::LOG0]).stop().build()
}

/// The runtime the creation scenarios deploy: it returns nothing.
pub fn runtime() -> Bytes {
    Asm::default().return_empty().build()
}

/// Init code that copies [`runtime`] from its own tail and deposits it.
pub fn deploying_runtime() -> Bytes {
    let runtime = runtime();
    let len = runtime.len() as u8;
    let prefix = Asm::default()
        .push_u8(len)
        .push_u16(0)
        .op(op::PUSH0)
        .op(op::CODECOPY)
        .push_u8(len)
        .op(op::PUSH0)
        .op(op::RETURN);
    let tail = prefix.len() as u16;
    Asm::default()
        .push_u8(len)
        .push_u16(tail)
        .op(op::PUSH0)
        .op(op::CODECOPY)
        .push_u8(len)
        .op(op::PUSH0)
        .op(op::RETURN)
        .ops(runtime)
        .build()
}

/// Every system contract installed, as the Satin implementation's test does for a keyless
/// deployment.
pub fn system_state() -> InMemoryDB {
    let mut contracts = FLAT_SYSTEM_CONTRACTS.to_vec();
    contracts.push(SEQUENCER_REGISTRY_CONTRACT);
    state(contracts.iter().map(|c| (c.address, code(&c.code))))
}

/// A pre-EIP-155 signed creation of `init` carrying `value`, as Nick's Method makes one, and the
/// address its signature recovers to.
pub fn signed_deployment(value: u64, init: Bytes) -> (Vec<u8>, Address) {
    let tx = TxLegacy {
        chain_id: None,
        nonce: 0,
        gas_price: 100_000_000_000,
        gas_limit: SIGNED_GAS_LIMIT,
        to: TxKind::Create,
        value: U256::from(value),
        input: init,
    };
    let word = U256::from_be_bytes([0x22; 32]);
    let signed = tx.into_signed(Signature::new(word, word, false));
    let signer = mega_evm2::keyless::recover(&signed).expect("Nick's-Method signature recovers");
    let mut raw = Vec::new();
    alloy_eips::eip2718::Encodable2718::encode_2718(&signed, &mut raw);
    (raw, signer)
}

/// `keylessDeploy` of `raw` with the gas limit override the Satin implementation's test uses.
pub fn keyless_call(raw: &[u8]) -> Bytes {
    common::keyless_deploy(raw, 10_000_000_000)
}

/// The KZG point evaluation input of the c-kzg vector `verify_kzg_proof_case_correct_proof_4_4`:
/// `versioned_hash ++ z ++ y ++ commitment ++ proof`.
pub fn kzg_input() -> Vec<u8> {
    let mut input =
        hex!("01e798154708fe7789429634053cbf9f99b619f9f084048927333fce637f549b").to_vec();
    input.extend(hex!("73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000000"));
    input.extend(hex!("1522a4a7f34e1ea350ae07c29c96c7e79655aa926122e95fe69fcbd932ca49e9"));
    input.extend(hex!(
        "8f59a8d2a1a625a17f3fea0fe5eb8c896db3764f3185481bc22f91b4aaffcca25f26936857bc3a7c2539ea8ec3a952b7"
    ));
    input.extend(hex!(
        "a62ad71d14c5719385c0686f1871430475bf3a00f0aa3f7b8dd99a9abc2160744faf0070725e00b60ad9a026a15b1a8c"
    ));
    input
}

pub const KZG: Address = Address::with_last_byte(0x0a);

/// The detention cap of the scenario that crosses again in the parent: enough for the parent's
/// call and 32 iterations of the child's loop, the last two gas short of a round.
fn crossing_cap() -> u64 {
    let params = &mega_evm2::satin_version().gas_params;
    let cold_account = params.get(evm2::version::GasId::WarmStorageReadCost) +
        params.cold_account_additional_cost();
    // BASE + (four PUSH0, PUSH32, PUSH20, GAS, CALL's cold access) + 32 * (JUMPDEST, PUSH1, JUMP) -
    // 2.
    2 + (4 * 2 + 2 * 3 + 2 + cold_account) + 32 * (1 + 3 + 8) - 2
}

/// Every scenario, by name.
pub fn cases() -> Vec<Case> {
    let limits = |f: fn(&mut TxLimits)| {
        let mut l = NO_LIMITS;
        f(&mut l);
        l
    };
    vec![
        Case::new("eth_transfer", state([]), tx(PAYEE, 1, Bytes::new(), GAS)),
        Case::new(
            "sstore_new_slot",
            state([(
                CONTRACT,
                code(&Asm::default().sstore(U256::from(1), U256::from(1)).stop().build()),
            )]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        ),
        Case::new("create", state([]), create(CALLER, deploying_runtime(), GAS)),
        {
            let child = Asm::default()
                .sstore(U256::from(1), U256::from(1))
                .push_u8(0x42)
                .ops([op::PUSH0, op::PUSH0, op::LOG1])
                .revert()
                .build();
            let parent =
                calling(CHILD, 1).ops([op::POP, op::PUSH0, op::PUSH0, op::LOG0]).stop().build();
            let mut contract = code(&parent);
            contract.balance = U256::from(1);
            Case::new(
                "nested_inner_revert",
                state([(CONTRACT, contract), (CHILD, code(&child))]),
                tx(CONTRACT, 0, Bytes::new(), GAS),
            )
        },
        Case::new(
            "data_size_first_frame_start_stop",
            state([(CONTRACT, code(&Asm::default().stop().build()))]),
            tx(CONTRACT, 1, Bytes::new(), GAS),
        )
        .limits(limits(|l| l.tx[0] = TX_BODY_SIZE)),
        Case::new(
            "data_size_stop_caught_by_the_parent",
            state([(CONTRACT, code(&calling_child())), (CHILD, code(&logging(128)))]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        )
        .limits(limits(|l| l.tx[0] = TX_BODY_SIZE + 100)),
        Case::new(
            "frame_budget_child_revert",
            state([
                (
                    CONTRACT,
                    code(
                        &calling(CHILD, 0)
                            .ops([op::POP, op::PUSH0, op::PUSH0, op::LOG0])
                            .stop()
                            .build(),
                    ),
                ),
                (CHILD, code(&logging(166))),
            ]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        )
        .limits(limits(|l| l.tx[0] = TX_BODY_SIZE + 200)),
        Case::new(
            "timestamp_detention",
            state([(
                CONTRACT,
                code(
                    &Asm::default()
                        .ops([op::TIMESTAMP, op::POP, op::JUMPDEST])
                        .push_u64(2)
                        .op(op::JUMP)
                        .build(),
                ),
            )]),
            tx(CONTRACT, 0, Bytes::new(), GAS),
        )
        .limits(limits(|l| l.cap = 80)),
        {
            let parent = Asm::default()
                .ops([op::PUSH0, op::POP, op::TIMESTAMP, op::POP])
                .call(CHILD, U256::ZERO)
                .op(op::POP)
                .sstore(U256::ZERO, U256::from(1))
                .stop()
                .build();
            let child = Asm::default().op(op::JUMPDEST).push_u8(0).op(op::JUMP).build();
            let mut l = NO_LIMITS;
            l.cap = crossing_cap();
            Case::new(
                "detention_stop_crosses_again_in_the_parent",
                state([(CONTRACT, code(&parent)), (CHILD, code(&child))]),
                tx(CONTRACT, 0, Bytes::new(), GAS),
            )
            .limits(l)
        },
        Case::new(
            "reservoir_sstore",
            state([(
                CONTRACT,
                code(&Asm::default().sstore(U256::from(1), U256::from(1)).stop().build()),
            )]),
            tx(CONTRACT, 0, Bytes::new(), EXECUTION_CAP + 2_000_000),
        ),
        Case::new(
            "identity_precompile",
            state([]),
            tx(Address::with_last_byte(4), 0, Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef]), GAS),
        ),
        {
            let input = kzg_input();
            let code_ = Asm::default()
                .mstore(0, &input)
                .ops([op::PUSH0, op::PUSH0])
                .push_u8(input.len() as u8)
                .op(op::PUSH0)
                .push_address(KZG)
                .ops([op::GAS, op::STATICCALL, op::POP])
                .stop()
                .build();
            Case::new(
                "precompile_child",
                state([(CONTRACT, code(&code_))]),
                tx(CONTRACT, 0, Bytes::new(), GAS),
            )
        },
        Case::new(
            "deposit",
            state([(CONTRACT, code(&Asm::default().stop().build()))]),
            deposit(CALLER, TxKind::Call(CONTRACT), 1_000, U256::from(7), Bytes::new(), GAS),
        ),
        {
            let (raw, _) = signed_deployment(0, deploying_runtime());
            Case::new(
                "keyless_success",
                system_state(),
                tx(common::KEYLESS, 0, keyless_call(&raw), GAS),
            )
        },
        {
            let (raw, _) = signed_deployment(1, deploying_runtime());
            Case::new(
                "keyless_refused",
                system_state(),
                tx(common::KEYLESS, 0, keyless_call(&raw), GAS),
            )
        },
    ]
}

/// The scenario `name`, pinned.
fn pinned(name: &str) -> (Case, Traces) {
    let case = cases().into_iter().find(|c| c.name == name).expect("a listed scenario");
    let traces = pin(&case);
    (case, traces)
}

/// The call tracer's frames, depth first: `(type, from, to, error)`.
pub fn frames(call: &Value) -> Vec<(String, String, String, Option<String>)> {
    fn walk(f: &Value, out: &mut Vec<(String, String, String, Option<String>)>) {
        let s = |k: &str| f[k].as_str().unwrap_or_default().to_string();
        out.push((s("type"), s("from"), s("to"), f["error"].as_str().map(str::to_string)));
        for c in f["calls"].as_array().into_iter().flatten() {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(call, &mut out);
    out
}

/// The struct logger's steps: `(depth, op)`.
pub fn steps(struct_log: &Value) -> Vec<(u64, String)> {
    struct_log["structLogs"]
        .as_array()
        .expect("struct logs")
        .iter()
        .map(|s| (s["depth"].as_u64().unwrap(), s["op"].as_str().unwrap().to_string()))
        .collect()
}

#[test]
fn inspector_ref_eth_transfer() {
    let (_, t) = pinned("eth_transfer");
    assert!(t.plain.success());
    // The EIP-7708 transfer log of the value, in the call tracer's root frame.
    let logs = t["call"]["logs"].as_array().unwrap();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0]["address"], json!("0xfffffffffffffffffffffffffffffffffffffffe"));
}

#[test]
fn inspector_ref_sstore_new_slot() {
    let (_, t) = pinned("sstore_new_slot");
    assert!(t.plain.success());
    let post = &t["prestate_diff"]["post"][CONTRACT.to_string().to_lowercase()]["storage"];
    assert_eq!(post.as_object().map(|s| s.len()), Some(1), "the diff shows the slot written");
}

#[test]
fn inspector_ref_create() {
    let (_, t) = pinned("create");
    assert!(t.plain.success());
    assert_eq!(t["call"]["type"], json!("CREATE"));
    let created = CALLER.create(0).to_string().to_lowercase();
    assert_eq!(t["prestate_diff"]["post"][&created]["code"], json!("0x5f5ff3"));
}

#[test]
fn inspector_ref_nested_inner_revert_reverted_log() {
    let (_, t) = pinned("nested_inner_revert");
    assert!(t.plain.success());
    let calls = t["call"]["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["error"], json!("execution reverted"));
    assert!(calls[0].get("logs").is_none(), "a reverted frame's logs are not shown");
    assert_eq!(t["call"]["logs"].as_array().unwrap().len(), 1, "the caller's own log");
}

#[test]
fn inspector_ref_data_size_first_frame_start_stop_caught_stop() {
    let (_, t) = pinned("data_size_first_frame_start_stop");
    assert!(!t.plain.success());
    assert_eq!(t["call"]["error"], json!("execution reverted"));
    assert_eq!(steps(&t["struct_log"]).len(), 0, "no instruction ran");
}

#[test]
fn inspector_ref_data_size_caught_stop_by_the_parent() {
    let (_, t) = pinned("data_size_stop_caught_by_the_parent");
    assert!(t.plain.success());
    let calls = t["call"]["calls"].as_array().unwrap();
    assert_eq!(calls[0]["error"], json!("execution reverted"));
}

#[test]
fn inspector_ref_frame_budget_caught_stop_child_revert() {
    let (_, t) = pinned("frame_budget_child_revert");
    assert!(t.plain.success());
    assert_eq!(t["call"]["logs"].as_array().unwrap().len(), 1, "the parent's log");
}

#[test]
fn inspector_ref_timestamp_detention() {
    let (_, t) = pinned("timestamp_detention");
    assert!(!t.plain.success());
    assert_eq!(t["struct_log"]["failed"], json!(true));
}

#[test]
fn inspector_ref_detention_stop_crosses_again_in_the_parent() {
    let (_, t) = pinned("detention_stop_crosses_again_in_the_parent");
    assert!(!t.plain.success());
    let s = steps(&t["struct_log"]);
    let call = s.iter().position(|(d, o)| *d == 1 && o == "CALL").expect("the parent's call");
    assert!(s[call + 1..].iter().any(|(d, _)| *d == 1), "the parent resumed after the stop");
}

#[test]
fn inspector_ref_reservoir_sstore() {
    let (_, t) = pinned("reservoir_sstore");
    assert!(t.plain.success());
}

#[test]
fn inspector_ref_identity_precompile() {
    let (_, t) = pinned("identity_precompile");
    assert!(t.plain.success());
    assert_eq!(t["call"]["output"], json!("0xdeadbeef"));
}

#[test]
fn inspector_ref_precompile_child() {
    let (_, t) = pinned("precompile_child");
    assert!(t.plain.success());
    let calls = t["call"]["calls"].as_array().unwrap();
    assert_eq!(calls[0]["type"], json!("STATICCALL"));
    assert_eq!(calls[0]["to"], json!(KZG.to_string().to_lowercase()));
}

#[test]
fn inspector_ref_deposit() {
    let (_, t) = pinned("deposit");
    assert!(t.plain.success());
    assert_eq!(t["call"]["logs"].as_array().unwrap().len(), 1, "the value's transfer log");
}

#[test]
fn inspector_ref_keyless_success() {
    let (_, t) = pinned("keyless_success");
    assert!(t.plain.success());
    let f = frames(&t["call"]);
    assert_eq!(f.len(), 2, "the keyless call and its creation: {f:?}");
    assert_eq!(f[1].0, "CREATE");
}

#[test]
fn inspector_ref_keyless_refused() {
    let (_, t) = pinned("keyless_refused");
    assert!(!t.plain.success());
    assert_eq!(frames(&t["call"]).len(), 1, "a refused deployment starts no creation");
}
