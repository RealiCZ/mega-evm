//! The per-transaction limits and gas detention's caps are Satin's protocol values, not
//! configuration: an engine nobody configured holds every transaction to 13,107,200 bytes of data
//! size, 500,000 write records and 1,000 new accounts and slots, and caps its compute at 20,000,000
//! past a volatile read; a block session runs under those values, and only a simulation, named as
//! one, runs under others.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256, keccak256};
use evm2::{
    Evm, TxResult,
    bytecode::Bytecode,
    evm::{AccountInfo, Db, InMemoryDB},
    interpreter::InstrStop,
};
use mega_evm2::{
    MegaTypes, TxLimits,
    error::{CandidateError, Refusal},
    factory::{BlockServices, Simulation},
};
use op_evm2::OpTxEnvelope;

const SENDER: Address = Address::with_last_byte(0x50);
const CONTRACT: Address = Address::with_last_byte(0xc0);

fn db(code: &[u8]) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(24))),
    );
    db.insert_account_info(
        &CONTRACT,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code))),
    );
    db
}

/// An engine as the factory builds it, no simulation switch given.
fn satin_evm(code: &[u8]) -> Evm<'static, MegaTypes> {
    common::evm(common::block(), Db::new(db(code)))
}

fn call(gas_limit: u64) -> Recovered<OpTxEnvelope> {
    let tx =
        TxLegacy { chain_id: Some(1), gas_limit, to: TxKind::Call(CONTRACT), ..Default::default() };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER)
}

fn run(code: &[u8], gas_limit: u64) -> TxResult<MegaTypes> {
    let mut evm = satin_evm(code);
    evm.transact(&call(gas_limit)).expect("the transaction is valid").detach().result
}

/// `MegaLimitExceeded(kind, limit)`: the stop's revert data.
fn limit_exceeded(kind: u8, limit: u64) -> Bytes {
    let mut out = keccak256("MegaLimitExceeded(uint8,uint64)")[..4].to_vec();
    out.extend_from_slice(&B256::from(U256::from(kind)).0);
    out.extend_from_slice(&B256::from(U256::from(limit)).0);
    Bytes::from(out)
}

#[test]
fn test_satins_limits_are_the_protocol_values() {
    assert_eq!(
        TxLimits::SATIN,
        TxLimits { tx: [13_107_200, 500_000, 1_000], cap: 20_000_000, oracle_cap: 20_000_000 }
    );
    assert_eq!(TxLimits::default(), TxLimits::SATIN);
}

/// 1,001 fresh slots: the 1,001st crosses the state-growth limit of 1,000.
#[test]
fn test_an_unconfigured_engine_holds_a_transaction_to_satins_state_growth_limit() {
    // PUSH2 1001; loop: JUMPDEST PUSH1 1 DUP2 SSTORE PUSH1 1 SWAP1 SUB DUP1 PUSH1 3 JUMPI; STOP.
    let code = [
        0x61, 0x03, 0xe9, 0x5b, 0x60, 0x01, 0x81, 0x55, 0x60, 0x01, 0x90, 0x03, 0x80, 0x60, 0x03,
        0x57, 0x00,
    ];
    let result = run(&code, 150_000_000);
    assert_eq!(result.stop, InstrStop::Revert, "{result:?}");
    assert_eq!(result.output, limit_exceeded(3, 1_000));
    // 1,000 fresh slots fit.
    let mut code = code;
    code[1..3].copy_from_slice(&1_000u16.to_be_bytes());
    let result = run(&code, 150_000_000);
    assert!(result.status, "{result:?}");
}

/// TIMESTAMP, then a loop that spends compute until detention stops it: the limit is the compute
/// at the read plus 20,000,000.
#[test]
fn test_an_unconfigured_engine_detains_at_satins_cap() {
    // TIMESTAMP POP; loop: JUMPDEST PUSH1 2 JUMP.
    let code = [0x42, 0x50, 0x5b, 0x60, 0x02, 0x56];
    let result = run(&code, 30_000_000);
    assert_eq!(result.stop, InstrStop::Revert, "{result:?}");
    let out = result.output.to_vec();
    assert_eq!(out[..4], keccak256("MegaLimitExceeded(uint8,uint64)")[..4]);
    assert_eq!(U256::from_be_slice(&out[4..36]), U256::from(2), "the compute kind");
    // TIMESTAMP is the frame's first instruction: the compute at the read is its 2 gas.
    assert_eq!(U256::from_be_slice(&out[36..68]), U256::from(20_000_002u64));
}

/// A block session runs every transaction under Satin's limits: the factory's consensus entry
/// point takes no limit at all. A simulation that names limits of nothing runs under them, so its
/// 310 body bytes are refused, and it says it is a simulation.
#[test]
fn test_a_block_session_runs_every_transaction_under_satins_limits() {
    let nothing = TxLimits { tx: [0; 3], cap: 0, oracle_cap: 0 };
    let input = common::block_input(1, 1_000, 10_000_000_000, Some(B256::repeat_byte(0x11)));
    let tx = call(100_000);
    let encoded = tx.inner().enveloped_tx().cloned().unwrap();
    let mut block = common::start_block(&input, Db::new(db(&[0x00])));
    assert!(!block.is_simulation());
    let executed = block.execute_candidate(&tx, &encoded).expect("under Satin's limits");
    assert!(executed.result.status, "{:?}", executed.result);
    assert_eq!(block.evm().ext().limits, TxLimits::SATIN);

    let simulation = Simulation { limits: Some(nothing), ..Simulation::default() };
    let mut simulated = common::factory()
        .start_simulated_block(&input, Db::new(db(&[0x00])), BlockServices::default(), &simulation)
        .expect("the simulated block starts");
    assert!(simulated.is_simulation());
    let refused = simulated.execute_candidate(&tx, &encoded).expect_err("under no room");
    assert!(matches!(refused, CandidateError::Refused(Refusal::Transaction(_))), "{refused:?}");
    assert!(refused.to_string().contains("data-size limit"), "{refused}");
    assert_eq!(simulated.evm().ext().limits, nothing);
}
