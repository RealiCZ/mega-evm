//! A pre-block system call reads the Oracle's storage from the chain: it asks the oracle
//! service nothing and records no answer, so the pre-block state is a function of the state alone.

mod common;

use alloy_primitives::{Address, B256, Bytes, U256};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    Cfg,
    block::{BEACON_ROOTS, BlockSession},
    factory::BlockServices,
    system_contracts::ORACLE_CODE,
    tx::ORACLE,
};

/// The Oracle's slot 1 on the chain, and the service's answer for it.
const ON_CHAIN: u64 = 5;
const ANSWER: u64 = 7;

/// A block whose beacon-roots system call (the last pre-block call when no registry change is
/// due) calls the Oracle's `getSlot(1)`, the Oracle holding its own code (which the block's
/// deployment leaves as it is), and stores the returned word in its own slot 0.
fn block() -> BlockSession<'static> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &ORACLE,
        AccountInfo::default().with_code(Bytecode::new_legacy(ORACLE_CODE)),
    );
    db.insert_account_storage(&ORACLE, &U256::from(1), &U256::from(ON_CHAIN));
    // MSTORE(0, getSlot's selector << 224); MSTORE(4, 1);
    // CALL(GAS, ORACLE, 0, 0, 36, 0, 32) POP; SSTORE(0, MLOAD(0)); STOP
    let mut caller = vec![0x63, 0x7e, 0xba, 0x7b, 0xa6, 0x60, 0xe0, 0x1b, 0x5f, 0x52];
    caller.extend_from_slice(&[0x60, 0x01, 0x60, 0x04, 0x52]);
    caller.extend_from_slice(&[0x60, 0x20, 0x5f, 0x60, 0x24, 0x5f, 0x5f, 0x73]);
    caller.extend_from_slice(ORACLE.as_slice());
    caller.extend_from_slice(&[0x5a, 0xf1, 0x50, 0x5f, 0x51, 0x5f, 0x55, 0x00]);
    db.insert_account_info(
        &BEACON_ROOTS,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from(caller))),
    );
    let mut environment = Cfg::default();
    environment.oracle.insert(U256::from(1), U256::from(ANSWER));
    let input = common::block_input(1, 1_000, 30_000_000, Some(B256::repeat_byte(0x11)));
    let services = BlockServices { environment, ..common::services(B256::ZERO) };
    common::factory().start_block(&input, Db::new(db), services).expect("pre-block calls")
}

#[test]
fn test_a_system_call_reads_the_oracle_from_the_chain() {
    let block = block();
    let stored = common::committed_slot(&block, BEACON_ROOTS, U256::ZERO);
    assert_eq!(
        stored,
        Some(U256::from(ON_CHAIN)),
        "the Oracle's read answered the service's value"
    );
    assert_eq!(block.evm().ext().oracle_reads, 0, "a system call recorded an Oracle answer");
}

/// The control: the same call as a transaction's (a call from an account) gets the service's
/// answer, so the test above sees the system call's rule and not a fixture without an answer.
#[test]
fn test_a_transaction_reads_the_oracle_from_the_service() {
    use alloy_consensus::{TxLegacy, transaction::Recovered};
    use op_evm2::OpTxEnvelope;
    let sender = Address::with_last_byte(0x50);
    let mut block = block();
    let tx = TxLegacy {
        chain_id: Some(common::CHAIN_ID),
        gas_limit: 1_000_000,
        to: alloy_primitives::TxKind::Call(BEACON_ROOTS),
        ..TxLegacy::default()
    };
    let tx = Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), sender);
    let executed = block.execute_candidate(&tx, &[]).expect("the call runs");
    assert!(executed.result().status, "the call succeeds: {:?}", executed.result().stop);
    assert_eq!(block.evm().ext().oracle_reads, 1);
    block.commit_candidate(executed).expect("committed");
    let stored = common::committed_slot(&block, BEACON_ROOTS, U256::ZERO);
    assert_eq!(stored, Some(U256::from(ANSWER)));
}
