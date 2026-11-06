//! A pre-block system call reads the Oracle's storage from the chain: it asks the oracle
//! service nothing and records no answer, so the pre-block state is a function of the state alone.

use alloy_primitives::{Address, B256, Bytes, U256};
use evm2::{
    Evm, ExecutionConfig,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    MegaSpec, MegaTypes,
    block::{BEACON_ROOTS, BlockExecutor},
    precompiles, satin_version,
    tx::{ORACLE, registry},
};

/// The Oracle's slot 1 on the chain, and the service's answer for it.
const ON_CHAIN: u64 = 5;
const ANSWER: u64 = 7;

/// A block whose beacon-roots system call (the last pre-block call when no registry change is
/// due) calls the Oracle, whose code returns its slot 1, and stores the returned word in its own
/// slot 0.
fn block() -> BlockExecutor<'static> {
    let mut db = InMemoryDB::default();
    // PUSH1 1 SLOAD PUSH0 MSTORE PUSH1 32 PUSH0 RETURN
    let oracle = [0x60, 0x01, 0x54, 0x5f, 0x52, 0x60, 0x20, 0x5f, 0xf3];
    db.insert_account_info(
        &ORACLE,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::copy_from_slice(&oracle))),
    );
    db.insert_account_storage(&ORACLE, &U256::from(1), &U256::from(ON_CHAIN));
    // CALL(GAS, ORACLE, 0, 0, 0, 0, 32) POP; SSTORE(0, MLOAD(0)); STOP
    let mut caller = vec![0x60, 0x20, 0x5f, 0x5f, 0x5f, 0x5f, 0x73];
    caller.extend_from_slice(ORACLE.as_slice());
    caller.extend_from_slice(&[0x5a, 0xf1, 0x50, 0x5f, 0x51, 0x5f, 0x55, 0x00]);
    db.insert_account_info(
        &BEACON_ROOTS,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from(caller))),
    );
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let env = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        env,
        registry(),
        Db::new(db),
        precompiles(),
    );
    let ext = evm.ext_mut();
    ext.cfg.limits = [13_107_200, 500_000, 1_000];
    ext.cfg.cap = 20_000_000;
    ext.cfg.oracle_cap = 20_000_000;
    ext.cfg.oracle.insert(U256::from(1), U256::from(ANSWER));
    BlockExecutor::new(evm, B256::ZERO, Some(B256::repeat_byte(0x11)))
}

#[test]
fn test_a_system_call_reads_the_oracle_from_the_chain() {
    let mut block = block();
    block.apply_pre_execution_changes().expect("pre-block calls");
    let stored = block
        .evm
        .state_mut()
        .storage_slot_untracked(&BEACON_ROOTS, &U256::ZERO)
        .expect("the caller's slot");
    assert_eq!(stored, U256::from(ON_CHAIN), "the Oracle's read answered the service's value");
    assert_eq!(block.evm.ext().oracle_reads, 0, "a system call recorded an Oracle answer");
}

/// The control: the same call as a transaction's (a call from an account) gets the service's
/// answer, so the test above sees the system call's rule and not a fixture without an answer.
#[test]
fn test_a_transaction_reads_the_oracle_from_the_service() {
    use alloy_consensus::{TxLegacy, transaction::Recovered};
    use op_evm2::OpTxEnvelope;
    let sender = Address::with_last_byte(0x50);
    let mut block = block();
    block.evm.state_mut().set_read_absent_storage(true);
    let tx = TxLegacy {
        chain_id: Some(satin_version().chain_id),
        gas_limit: 1_000_000,
        to: alloy_primitives::TxKind::Call(BEACON_ROOTS),
        ..TxLegacy::default()
    };
    let tx = Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), sender);
    let executed = block.evm.transact(&tx).expect("the call runs");
    let result = executed.commit();
    assert!(result.status, "the call succeeds: {:?}", result.stop);
    let stored = block
        .evm
        .state_mut()
        .storage_slot_untracked(&BEACON_ROOTS, &U256::ZERO)
        .expect("the caller's slot");
    assert_eq!(stored, U256::from(ANSWER));
    assert_eq!(block.evm.ext().oracle_reads, 1);
}
