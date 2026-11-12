//! The Oracle's storage reads: made before their cold access is charged (`SATIN-ORCL-004`,
//! `SATIN-DET-003`), and served by the oracle service only for a transaction that is neither a
//! deposit nor a Mega System Transaction (`SATIN-ORCL-005`; a system call's reads are
//! `system_call_oracle.rs`'s).

mod common;

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    Evm,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    MegaTypes, SEQUENCER_REGISTRY,
    oracle::{OracleAnswers, OracleServices},
    tx::ORACLE,
};
use op_evm2::{OpTxEnvelope, TxDeposit};
use std::sync::Arc;

const SENDER: Address = Address::with_last_byte(0x50);
const TOP: Address = Address::with_last_byte(0xa0);
/// The Oracle's slot 5 on the chain, and the service's answer for it.
const ON_CHAIN: u64 = 7;
const ANSWER: u64 = 42;

fn code(c: &[u8]) -> AccountInfo {
    AccountInfo::default().with_nonce(1).with_code(Bytecode::new_legacy(Bytes::copy_from_slice(c)))
}

fn satin_evm(db: InMemoryDB) -> Evm<'static, MegaTypes> {
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    let answers = OracleAnswers::from_iter([(U256::from(5), U256::from(ANSWER))]);
    common::oracle_evm(block, Db::new(db), OracleServices::service(Arc::new(answers)))
}

fn legacy(from: Address, to: Address, gas_limit: u64) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy { chain_id: Some(1), gas_limit, to: TxKind::Call(to), ..Default::default() };
    Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), from)
}

/// TOP calls the Oracle with `forward` of gas and returns the call's success word; the Oracle runs
/// `PUSH1 5; SLOAD; STOP`.
fn oracle_called_with(forward: u16) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&SENDER, AccountInfo::default().with_balance(U256::from(1)));
    let mut top = vec![0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x73];
    top.extend_from_slice(ORACLE.as_slice());
    top.push(0x61);
    top.extend_from_slice(&forward.to_be_bytes());
    top.extend([0xf1, 0x5f, 0x52, 0x60, 0x20, 0x5f, 0xf3]);
    db.insert_account_info(&TOP, code(&top));
    db.insert_account_info(&ORACLE, code(&[0x60, 0x05, 0x54, 0x00]));
    db.insert_account_storage(&ORACLE, &U256::from(5), &U256::from(ON_CHAIN));
    db
}

/// An Oracle frame's read is made before its cold access, `COLD_SLOAD_COST` (2,100, the warm base
/// included), is charged (`SATIN-ORCL-004`): forwarded 53 of gas the frame has 50 left at its
/// `SLOAD` once `PUSH1` (3) is paid, short even of the warm base, and forwarded 2,102 it has 2,099,
/// short of the cold access by one; either way the slot is loaded, the service asked and the read
/// marked, at the compute before the cold access (`SATIN-DET-003`) — TOP's 3,016 (five `PUSH0`s,
/// `PUSH20`, `PUSH2` and the cold `CALL`'s 3,000) and the Oracle frame's 3 — so the limit is
/// 3,019 + 20,000,000; then the frame runs out of gas (the call fails), the mark standing. With
/// 2,100 at the `SLOAD` the read completes, at the same limit.
#[test]
fn test_an_oracle_read_short_of_its_cold_access_loads_asks_and_marks() {
    for (forward, completed) in [(53, 0), (2_102, 0), (2_103, 1), (4_102, 1)] {
        let mut evm = satin_evm(oracle_called_with(forward));
        let out = evm.transact(&legacy(SENDER, TOP, 1_000_000)).expect("a valid call").detach();
        assert!(out.result.status, "{forward}");
        assert_eq!(word(&out.result.output), completed, "the call's success, {forward}");
        assert_eq!(evm.ext().oracle_reads, 1, "the service asked, {forward}");
        assert_eq!(evm.ext().detention.limit, Some(20_003_019), "the read marked, {forward}");
        let oracle = out.pending_state.account_changes_for(&ORACLE).expect("the Oracle loaded");
        assert!(oracle.storage.slots.contains_key(&U256::from(5)), "slot 5 loaded, {forward}");
    }
}

/// The Oracle's own code (version 2.0.0) reached with too little gas for its read: TOP calls
/// `getSlot(5)` with 250 of gas, which reaches the Oracle's `SLOAD` with 29 left, then loops. The
/// read is made and marked all the same before the child runs out of gas, so the loop, on the
/// transaction's 30,000,000 of gas, is stopped at the compute limit the mark set, the rest of its
/// gas unspent, and does not run out of gas.
#[test]
fn test_an_oracle_read_below_its_warm_base_marks_and_the_caller_reaches_the_compute_stop() {
    let selector = alloy_primitives::keccak256("getSlot(uint256)");
    let mut input = selector[..4].to_vec();
    input.extend_from_slice(&U256::from(5).to_be_bytes::<32>());
    // CODECOPY(0, input, 36); CALL(250, ORACLE, 0, 0, 36, 0, 0); POP; JUMPDEST; PUSH1 38; JUMP
    let mut top = vec![0x60, 36, 0x60, 42, 0x5f, 0x39, 0x5f, 0x5f, 0x60, 36, 0x5f, 0x5f, 0x73];
    top.extend_from_slice(ORACLE.as_slice());
    top.extend([0x61, 0x00, 0xfa, 0xf1, 0x50, 0x5b, 0x60, 38, 0x56]);
    assert_eq!(top.len(), 42);
    top.extend_from_slice(&input);
    let mut db = InMemoryDB::default();
    db.insert_account_info(&SENDER, AccountInfo::default().with_balance(U256::from(1)));
    db.insert_account_info(&TOP, code(&top));
    let oracle = hex::decode(include_str!("fixtures/Oracle-2.0.0.hex").trim()).unwrap();
    db.insert_account_info(&ORACLE, code(&oracle));
    db.insert_account_storage(&ORACLE, &U256::from(5), &U256::from(ON_CHAIN));
    let mut evm = satin_evm(db);
    let out = evm.transact(&legacy(SENDER, TOP, 30_000_000)).expect("a valid call").detach();
    assert_eq!(evm.ext().oracle_reads, 1, "the service asked");
    let limit = evm.ext().detention.limit.expect("the read marked");
    assert!(!out.result.status);
    assert_eq!(
        out.result.output.as_ref(),
        mega_evm2::limit_exceeded_output(2, limit).as_slice(),
        "the compute stop"
    );
    assert_eq!(evm.ext().top_stop, Some((2, limit)));
    assert!(
        out.result.tx_gas_used() < 21_000_000,
        "gas left unspent: {}",
        out.result.tx_gas_used()
    );
    let oracle = out.pending_state.account_changes_for(&ORACLE).expect("the Oracle loaded");
    assert!(oracle.storage.slots.contains_key(&U256::from(5)), "slot 5 loaded");
}

/// The Oracle's code returning its slot 5: `PUSH1 5; SLOAD; PUSH0; MSTORE; PUSH1 32; PUSH0;
/// RETURN`.
const RETURNS_SLOT_5: [u8; 9] = [0x60, 0x05, 0x54, 0x5f, 0x52, 0x60, 0x20, 0x5f, 0xf3];

fn oracle_db() -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&SENDER, AccountInfo::default().with_balance(U256::from(1)));
    db.insert_account_info(&ORACLE, code(&RETURNS_SLOT_5));
    db.insert_account_storage(&ORACLE, &U256::from(5), &U256::from(ON_CHAIN));
    db
}

fn word(output: &[u8]) -> u64 {
    U256::from_be_slice(output).to::<u64>()
}

/// A user's transaction reading the Oracle is served (control); a deposit's read and a Mega
/// System Transaction's are not: each pushes the slot's value on the chain, asks nothing, and is
/// loaded, priced and marked as any Oracle read is.
#[test]
fn test_deposits_and_mega_system_transactions_ask_the_oracle_service_nothing() {
    // A user's call of the Oracle: served.
    let mut evm = satin_evm(oracle_db());
    let user = evm.transact(&legacy(SENDER, ORACLE, 1_000_000)).expect("valid").detach();
    assert_eq!(word(&user.result.output), ANSWER);
    assert_eq!(evm.ext().oracle_reads, 1);
    // A deposit's call of the Oracle: the slot's value.
    let mut evm = satin_evm(oracle_db());
    let deposit = Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(ORACLE),
            mint: 0,
            value: U256::ZERO,
            gas_limit: 1_000_000,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        SENDER,
    );
    let out = evm.transact(&deposit).expect("valid").detach();
    assert!(out.result.status);
    assert_eq!(word(&out.result.output), ON_CHAIN);
    assert_eq!(evm.ext().oracle_reads, 0);
    assert!(evm.ext().detention.limit.is_some(), "a deposit's Oracle read is marked");
    // A Mega System Transaction: a legacy call of the Oracle from the live system address.
    let system = Address::with_last_byte(0x51);
    let mut db = oracle_db();
    db.insert_account_info(&system, AccountInfo::default().with_balance(U256::from(1)));
    db.insert_account_info(
        &SEQUENCER_REGISTRY,
        AccountInfo::default().with_code(Bytecode::new_legacy(
            hex::decode(include_str!("fixtures/SequencerRegistry-2.0.0.hex").trim())
                .unwrap()
                .into(),
        )),
    );
    db.insert_account_storage(
        &SEQUENCER_REGISTRY,
        &U256::ZERO,
        &U256::from_be_slice(system.as_slice()),
    );
    let mut evm = satin_evm(db);
    let out = evm.transact(&legacy(system, ORACLE, 1_000_000)).expect("valid").detach();
    assert!(out.result.status);
    assert_eq!(word(&out.result.output), ON_CHAIN);
    assert_eq!(evm.ext().oracle_reads, 0);
}
