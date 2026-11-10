//! Deposits and Mega System Transactions pay history gas as any transaction does — their body and
//! calldata in their validation, their start record before their first frame, their logs, records
//! and deposited code as they run — and a failed deposit reports no history gas but its body and
//! calldata as history bytes (`SATIN-HIST-016`, `SATIN-HIST-022`, `SATIN-DEP-001`,
//! `SATIN-DEP-005`, `SATIN-MST-008`, `SATIN-MST-010`, `SATIN-TAKE-012`). The figures are the
//! specification's worked ones.

mod common;

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    Evm, TxResult,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{MegaTypes, SEQUENCER_REGISTRY, tx::ORACLE};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SENDER: Address = Address::with_last_byte(0x50);
const TARGET: Address = Address::with_last_byte(0xa0);
const NEW: Address = Address::with_last_byte(0xa1);
/// The body's history: 310 bytes at 88.
const BODY: u64 = 27_280;
/// A write record's history: 40 bytes at 88.
const RECORD: u64 = 3_520;
const INVALID: u8 = 0xfe;
const STOP: u8 = 0x00;
const REVERT: [u8; 3] = [0x5f, 0x5f, 0xfd];

fn satin_evm(db: InMemoryDB) -> Evm<'static, MegaTypes> {
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    common::evm(block, Db::new(db))
}

/// An existing funded sender (nonce 3) and an existing target running `code`.
fn world(code: &[u8]) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_nonce(3).with_balance(U256::from(10)),
    );
    db.insert_account_info(
        &TARGET,
        AccountInfo::default()
            .with_nonce(1)
            .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code))),
    );
    db
}

fn deposit(to: Address, value: u64, mint: u128, gas_limit: u64) -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(to),
            mint,
            value: U256::from(value),
            gas_limit,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        SENDER,
    )
}

/// What a transaction reports: its result, its (history gas, history bytes) and its usage (data
/// size, KV updates, state growth), and the sender's nonce and balance after it.
struct Reported {
    result: TxResult<MegaTypes>,
    history: (u64, u64),
    usage: [u64; 3],
    sender: (u64, U256),
}

fn run(db: InMemoryDB, tx: Recovered<OpTxEnvelope>) -> Reported {
    let mut evm = satin_evm(db);
    let out = evm.transact(&tx).expect("a valid transaction").detach();
    let history = evm.ext().history(out.result.status);
    let usage = evm.ext().usage(out.result.status);
    let from: Address = tx.signer();
    let info = out.pending_state.account_info(&from).cloned().unwrap_or_default();
    Reported { result: out.result, history, usage, sender: (info.nonce, info.balance) }
}

/// The state gas a transaction reports, its history gas apart (the record's `state`).
fn state(r: &Reported) -> u64 {
    r.result.state_gas_spent.saturating_sub(r.history.0)
}

/// Expected (1): a deposit calling an existing account with no value and empty calldata on a
/// 30,000 gas limit cannot pay its intrinsic regular gas and the body's 27,280 of history: a
/// failed deposit — nonce bumped, mint credited, 30,000 of regular gas, no state or history gas,
/// 310 history bytes.
#[test]
fn test_a_deposit_short_of_its_bodys_history_is_a_failed_deposit() {
    let r = run(world(&[STOP]), deposit(TARGET, 0, 5, 30_000));
    assert!(r.result.ext.failed_deposit);
    assert_eq!(r.result.total_gas_spent, 30_000);
    assert_eq!(r.result.state_gas_spent, 0);
    assert_eq!(r.history, (0, 310));
    assert_eq!(r.sender, (4, U256::from(15)));
}

/// Expected (2) and (3): on a 100,000 gas limit the deposit pays the body's history; a frame that
/// halts makes it a failed deposit reporting 100,000 of regular gas, no history gas and 310
/// history bytes, a frame that returns has it pay and report 27,280 of history gas for its body
/// and 310 history bytes, the 310 also its data size (`SATIN-HIST-022`'s example).
#[test]
fn test_a_deposit_pays_its_bodys_history_unless_it_fails() {
    let halted = run(world(&[INVALID]), deposit(TARGET, 0, 0, 100_000));
    assert!(halted.result.ext.failed_deposit);
    assert_eq!(halted.result.total_gas_spent, 100_000);
    assert_eq!(state(&halted), 0);
    assert_eq!(halted.history, (0, 310));
    let returned = run(world(&[STOP]), deposit(TARGET, 0, 0, 100_000));
    assert!(returned.result.status);
    assert!(!returned.result.ext.failed_deposit);
    assert_eq!(returned.history, (BODY, 310));
    assert_eq!(state(&returned), 0);
    assert_eq!(returned.usage[0], 310);
}

/// A deposit moving value to an account the state does not hold makes its start record before its
/// first frame, priced as any transaction's (3,520, its 40 bytes kept); a frame that reverts gives
/// the record back into the transaction's reservoir, the body's history staying.
#[test]
fn test_a_deposits_start_record_is_priced_and_given_back_on_a_revert() {
    let r = run(world(&[STOP]), deposit(NEW, 1, 1, 400_000));
    assert!(r.result.status);
    assert_eq!(r.history, (BODY + RECORD, 350));
    let mut db = world(&REVERT);
    db.insert_account_info(&NEW, AccountInfo::default());
    let reverted = run(db, deposit(TARGET, 1, 1, 400_000));
    assert!(!reverted.result.status);
    assert!(!reverted.result.ext.failed_deposit);
    assert_eq!(reverted.history, (BODY, 310));
}

/// `SATIN-TAKE-012`'s failed-deposit example: from a sender empty under EIP-161, on a 1,000,000
/// gas limit, a deposit whose first frame halts after its caller's 183,600 of state gas was paid
/// reports 1,000,000 of regular gas, no state and no history gas, one unit of state growth, zero
/// KV updates, the data size of its body and calldata and the same bytes as its history bytes;
/// the sender keeps the nonce bump and the mint.
#[test]
fn test_a_failed_deposit_from_an_empty_sender() {
    let mut db = world(&[INVALID]);
    db.insert_account_info(&SENDER, AccountInfo::default());
    let r = run(db, deposit(TARGET, 0, 9, 1_000_000));
    assert!(r.result.ext.failed_deposit);
    assert_eq!(r.result.total_gas_spent, 1_000_000);
    assert_eq!(r.result.state_gas_spent, 0);
    assert_eq!(r.history, (0, 310));
    assert_eq!(r.usage, [310, 0, 1]);
    assert_eq!(r.sender, (1, U256::from(9)));
}

/// A Mega System Transaction — a legacy call of the Oracle from the live system address — pays
/// history gas as any transaction does: its body's 27,280, and 310 history bytes.
#[test]
fn test_a_mega_system_transaction_pays_history_gas() {
    let system = Address::with_last_byte(0x51);
    let mut db = world(&[STOP]);
    db.insert_account_info(&system, AccountInfo::default().with_balance(U256::from(1)));
    db.insert_account_info(
        &ORACLE,
        AccountInfo::default()
            .with_nonce(1)
            .with_code(Bytecode::new_legacy(Bytes::from_static(&[STOP]))),
    );
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
    let tx = TxLegacy {
        chain_id: Some(1),
        gas_limit: 1_000_000,
        to: TxKind::Call(ORACLE),
        ..Default::default()
    };
    let tx = Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), system);
    let r = run(db, tx);
    assert!(r.result.status);
    assert_eq!(r.history, (BODY, 310));
    assert_eq!(state(&r), 0);
}
