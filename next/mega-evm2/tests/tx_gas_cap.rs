//! Satin does not apply the EIP-8037 total transaction gas cap (`TX_MAX_TOTAL_GAS_LIMIT` =
//! 2^32 − 1): a transaction's gas limit is bounded by the block's gas limit alone.
//!
//! Each transaction-type test declares a gas limit of 2^32 and calls a contract that stores 1 in
//! slot 0, so an accepted transaction is also seen to execute.

use alloy_consensus::{
    SignableTransaction, TxEip1559, TxEip2930, TxEip7702, TxLegacy, transaction::Recovered,
};
use alloy_eips::{
    Encodable2718,
    eip7702::{Authorization, SignedAuthorization},
};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256, hex, keccak256};
use evm2::{
    Evm, ExecutionConfig, TxResult,
    bytecode::Bytecode,
    env::BlockEnv,
    ethereum::LazyTxEip7702,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    MegaSpec, MegaTypes, SEQUENCER_REGISTRY,
    block::BlockExecutor,
    precompiles, satin_version,
    tx::{ORACLE, SEQUENCER_REGISTRY_CODE_HASH, registry},
};
use op_evm2::{OpTxEnvelope, TxDeposit};

/// One above EIP-8037's `TX_MAX_TOTAL_GAS_LIMIT` (2^32 − 1).
const ABOVE_TOTAL_CAP: u64 = 1 << 32;
const BLOCK_GAS_LIMIT: u64 = 10_000_000_000;
const CHAIN_ID: u64 = 1;

const SENDER: Address = Address::with_last_byte(0x50);
/// Stores 1 in slot 0.
const TARGET: Address = Address::with_last_byte(0xa0);
const AUTHORITY: Address = Address::with_last_byte(0xc0);
const DELEGATE: Address = Address::with_last_byte(0xd0);
/// The system address the `SequencerRegistry` names.
const SYSTEM: Address = Address::with_last_byte(0x5a);

/// `PUSH1 1 PUSH1 0 SSTORE STOP`.
const STORE_ONE: [u8; 6] = [0x60, 0x01, 0x60, 0x00, 0x55, 0x00];
/// `SequencerRegistry` v2.0.0's runtime code (`crates/system-contracts/artifacts`).
const SEQUENCER_REGISTRY_CODE: &str = include_str!("fixtures/SequencerRegistry-2.0.0.hex");

fn store_one() -> AccountInfo {
    AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from_static(&STORE_ONE)))
}

fn db() -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
    );
    db.insert_account_info(&TARGET, store_one());
    db
}

/// The database of a chain whose `SequencerRegistry` names [`SYSTEM`] as its system address.
fn db_with_system_address() -> InMemoryDB {
    let mut db = db();
    let code = hex::decode(SEQUENCER_REGISTRY_CODE.trim()).unwrap();
    assert_eq!(keccak256(&code), SEQUENCER_REGISTRY_CODE_HASH, "fixture is the trusted registry");
    db.insert_account_info(
        &SEQUENCER_REGISTRY,
        AccountInfo::default().with_code(Bytecode::new_legacy(code.into())),
    );
    db.insert_account_storage(
        &SEQUENCER_REGISTRY,
        &U256::ZERO,
        &U256::from_be_slice(SYSTEM.as_slice()),
    );
    db.insert_account_info(&ORACLE, store_one());
    db
}

/// A Satin EVM over `db`, configured as the replay configures a node.
fn satin_evm(db: InMemoryDB, block_gas_limit: u64) -> Evm<'static, MegaTypes> {
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(block_gas_limit),
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
    evm
}

fn signature() -> Signature {
    Signature::new(U256::from(1), U256::from(1), false)
}

fn legacy(gas_limit: u64) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy {
        chain_id: Some(CHAIN_ID),
        nonce: 0,
        gas_price: 0,
        gas_limit,
        to: TxKind::Call(TARGET),
        value: U256::ZERO,
        input: Bytes::new(),
    };
    Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature())), SENDER)
}

fn encoded(tx: &Recovered<OpTxEnvelope>) -> Bytes {
    tx.inner().enveloped_tx().cloned().unwrap()
}

/// Runs `tx`, commits it, and returns its result.
fn transact(evm: &mut Evm<'_, MegaTypes>, tx: &Recovered<OpTxEnvelope>) -> TxResult<MegaTypes> {
    match evm.transact(tx) {
        Ok(executed) => executed.commit(),
        Err(error) => panic!("transaction refused: {error:?}"),
    }
}

fn slot_zero(evm: &mut Evm<'_, MegaTypes>, address: Address) -> U256 {
    evm.state_mut().storage_slot_untracked(&address, &U256::ZERO).unwrap()
}

/// Runs `tx` from [`SENDER`] to [`TARGET`] and checks it executed.
fn assert_executes(tx: Recovered<OpTxEnvelope>) {
    let mut evm = satin_evm(db(), BLOCK_GAS_LIMIT);
    let result = transact(&mut evm, &tx);
    assert!(result.status, "{result:?}");
    assert!(!result.ext.failed_deposit);
    assert_eq!(slot_zero(&mut evm, TARGET), U256::from(1));
}

#[test]
fn test_legacy_tx_above_total_cap_executes() {
    assert_executes(legacy(ABOVE_TOTAL_CAP));
}

#[test]
fn test_eip2930_tx_above_total_cap_executes() {
    let tx = TxEip2930 {
        chain_id: CHAIN_ID,
        nonce: 0,
        gas_price: 0,
        gas_limit: ABOVE_TOTAL_CAP,
        to: TxKind::Call(TARGET),
        value: U256::ZERO,
        access_list: Default::default(),
        input: Bytes::new(),
    };
    assert_executes(Recovered::new_unchecked(
        OpTxEnvelope::from_signed(tx.into_signed(signature())),
        SENDER,
    ));
}

#[test]
fn test_eip1559_tx_above_total_cap_executes() {
    let tx = TxEip1559 {
        chain_id: CHAIN_ID,
        nonce: 0,
        gas_limit: ABOVE_TOTAL_CAP,
        max_fee_per_gas: 0,
        max_priority_fee_per_gas: 0,
        to: TxKind::Call(TARGET),
        value: U256::ZERO,
        access_list: Default::default(),
        input: Bytes::new(),
    };
    assert_executes(Recovered::new_unchecked(
        OpTxEnvelope::from_signed(tx.into_signed(signature())),
        SENDER,
    ));
}

#[test]
fn test_eip7702_tx_above_total_cap_executes() {
    let authorization =
        Authorization { chain_id: U256::from(CHAIN_ID), address: DELEGATE, nonce: 0 };
    let tx = TxEip7702 {
        chain_id: CHAIN_ID,
        nonce: 0,
        gas_limit: ABOVE_TOTAL_CAP,
        max_fee_per_gas: 0,
        max_priority_fee_per_gas: 0,
        to: TARGET,
        value: U256::ZERO,
        access_list: Default::default(),
        authorization_list: vec![SignedAuthorization::new_unchecked(
            authorization,
            0,
            U256::from(1),
            U256::from(1),
        )],
        input: Bytes::new(),
    };
    let enveloped = Bytes::from(tx.clone().into_signed(signature()).encoded_2718());
    let lazy = LazyTxEip7702::from_cached_recovered_authorizations(tx, vec![Some(AUTHORITY)]);
    let tx = Recovered::new_unchecked(OpTxEnvelope::new(lazy, Some(enveloped)), SENDER);

    let mut evm = satin_evm(db(), BLOCK_GAS_LIMIT);
    let result = transact(&mut evm, &tx);
    assert!(result.status, "{result:?}");
    assert_eq!(slot_zero(&mut evm, TARGET), U256::from(1));
    let authority = evm.read_account_info(&AUTHORITY).unwrap().unwrap();
    assert_eq!(authority.nonce, 1, "the authorization was applied");
}

#[test]
fn test_deposit_above_total_cap_executes() {
    assert_executes(Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(TARGET),
            mint: 0,
            value: U256::ZERO,
            gas_limit: ABOVE_TOTAL_CAP,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        SENDER,
    ));
}

/// A legacy call to the Oracle from the live system address runs as a deposit. The system address
/// holds no balance and the gas price is 1, so the same transaction from a user, which must fund
/// its whole gas limit up front, would be refused.
#[test]
fn test_system_address_tx_above_total_cap_executes() {
    let tx = TxLegacy {
        chain_id: Some(CHAIN_ID),
        nonce: 0,
        gas_price: 1,
        gas_limit: ABOVE_TOTAL_CAP,
        to: TxKind::Call(ORACLE),
        value: U256::ZERO,
        input: Bytes::new(),
    };
    let tx =
        Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature())), SYSTEM);

    let mut evm = satin_evm(db_with_system_address(), BLOCK_GAS_LIMIT);
    let result = transact(&mut evm, &tx);
    assert!(result.status, "{result:?}");
    assert_eq!(slot_zero(&mut evm, ORACLE), U256::from(1));
    let system = evm.read_account_info(&SYSTEM).unwrap().unwrap();
    assert_eq!((system.nonce, system.balance), (1, U256::ZERO), "no fee was charged");
}

#[test]
fn test_block_refuses_gas_limit_above_block_gas_limit() {
    let mut block = BlockExecutor::new(satin_evm(db(), ABOVE_TOTAL_CAP), B256::ZERO, None);
    let tx = legacy(ABOVE_TOTAL_CAP + 1);
    let error = block.run_transaction(&tx, &encoded(&tx)).unwrap_err();
    assert_eq!(
        error,
        "transaction gas limit 4294967297 is more than blocks available gas 4294967296"
    );
}

/// The block's gas limit is the only bound: a block whose gas limit is 2^32 includes a
/// transaction that declares all of it.
#[test]
fn test_block_includes_gas_limit_equal_to_block_gas_limit_above_total_cap() {
    let mut block = BlockExecutor::new(satin_evm(db(), ABOVE_TOTAL_CAP), B256::ZERO, None);
    let tx = legacy(ABOVE_TOTAL_CAP);
    let executed = block.run_transaction(&tx, &encoded(&tx)).unwrap();
    assert!(executed.result.status, "{:?}", executed.result);
    block.commit_transaction_outcome(executed).unwrap();
    assert_eq!(block.receipts.len(), 1);
    assert!(block.receipts[0].status);
    assert_eq!(slot_zero(&mut block.evm, TARGET), U256::from(1));
}
