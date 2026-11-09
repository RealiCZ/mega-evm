//! The Oracle's storage reads: made before their cold access is charged (`SATIN-ORCL-004`,
//! `SATIN-DET-003`), and served by the oracle service only for a transaction that is neither a
//! deposit nor a Mega System Transaction (`SATIN-ORCL-005`; a system call's reads are
//! `system_call_oracle.rs`'s).

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    Evm, ExecutionConfig,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    MegaSpec, MegaTypes, SEQUENCER_REGISTRY, precompiles, satin_version,
    tx::{ORACLE, registry},
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SENDER: Address = Address::with_last_byte(0x50);
const TOP: Address = Address::with_last_byte(0xa0);
/// The Oracle's slot 5 on the chain, and the service's answer for it.
const ON_CHAIN: u64 = 7;
const ANSWER: u64 = 42;

fn code(c: &[u8]) -> AccountInfo {
    AccountInfo::default().with_nonce(1).with_code(Bytecode::new_legacy(Bytes::copy_from_slice(c)))
}

fn satin_evm(db: InMemoryDB) -> Evm<'static, MegaTypes> {
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(db),
        precompiles(),
    );
    evm.state_mut().set_read_absent_storage(true);
    evm.ext_mut().cfg.oracle.insert(U256::from(5), U256::from(ANSWER));
    evm
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

/// An Oracle frame forwarded 2,102 of gas has 1,999 left at its `SLOAD` once `PUSH1` (3) and the
/// opcode's warm 100 are paid, short of the cold access's 2,000 more: the slot is loaded, the
/// service asked and the read marked all the same, at the compute before the cold access — TOP's
/// 3,016 (five `PUSH0`s, `PUSH20`, `PUSH2` and the cold `CALL`'s 3,000) and the Oracle frame's 103
/// — so the limit is 3,119 + 20,000,000; then the frame runs out of gas (the call fails), the mark
/// standing. With 2,000 more of gas the read completes, at the same limit.
#[test]
fn test_an_oracle_read_short_of_its_cold_access_loads_asks_and_marks() {
    for (forward, completed) in [(2_102, 0), (4_102, 1)] {
        let mut evm = satin_evm(oracle_called_with(forward));
        let out = evm.transact(&legacy(SENDER, TOP, 1_000_000)).expect("a valid call").detach();
        assert!(out.result.status, "{forward}");
        assert_eq!(word(&out.result.output), completed, "the call's success, {forward}");
        assert_eq!(evm.ext().oracle_reads, 1, "the service asked, {forward}");
        assert_eq!(evm.ext().detention.limit, Some(20_003_119), "the read marked, {forward}");
        let oracle = out.pending_state.account_changes_for(&ORACLE).expect("the Oracle loaded");
        assert!(oracle.storage.slots.contains_key(&U256::from(5)), "slot 5 loaded, {forward}");
    }
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
