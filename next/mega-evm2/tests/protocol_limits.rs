//! The per-transaction limits and gas detention's caps are Satin's protocol values, not
//! configuration: an engine nobody configured holds every transaction to 13,107,200 bytes of data
//! size, 500,000 write records and 1,000 new accounts and slots, and caps its compute at 20,000,000
//! past a volatile read; a block session runs under those values, and only a simulation, named as
//! one, runs under others. The conformance suite's cases run on the engine the factory makes and
//! hold the stops at those values; these tests hold the values and the block session.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    TxLimits,
    error::{CandidateError, Refusal},
    factory::Simulation,
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

fn call(gas_limit: u64) -> Recovered<OpTxEnvelope> {
    let tx =
        TxLegacy { chain_id: Some(1), gas_limit, to: TxKind::Call(CONTRACT), ..Default::default() };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER)
}

#[test]
fn test_satins_limits_are_the_protocol_values() {
    assert_eq!(
        TxLimits::SATIN,
        TxLimits { tx: [13_107_200, 500_000, 1_000], cap: 20_000_000, oracle_cap: 20_000_000 }
    );
    assert_eq!(TxLimits::default(), TxLimits::SATIN);
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
    assert!(executed.result().status, "{:?}", executed.result());
    assert_eq!(block.evm().ext().limits, TxLimits::SATIN);

    let simulation = Simulation { limits: Some(nothing), ..Simulation::default() };
    let mut simulated = common::factory()
        .start_simulated_block(
            &input,
            Db::new(db(&[0x00])),
            common::services(B256::ZERO),
            &simulation,
        )
        .expect("the simulated block starts");
    assert!(simulated.is_simulation());
    let refused = simulated.execute_candidate(&tx, &encoded).expect_err("under no room");
    assert!(matches!(refused, CandidateError::Refused(Refusal::Transaction(_))), "{refused:?}");
    assert!(refused.to_string().contains("data-size limit"), "{refused}");
    assert_eq!(simulated.evm().ext().limits, nothing);
}
