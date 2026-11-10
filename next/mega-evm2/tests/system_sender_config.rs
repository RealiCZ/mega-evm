//! System-transaction sender validation follows the same configuration as user validation.
//! The differential scenario format cannot disable nonce validation or EIP-3607.
mod common;

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, Bytes, TxKind, U256, hex};
use evm2::{
    Evm, EvmFeatures,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    registry::HandlerError,
};
use mega_evm2::{
    MegaTypes, SEQUENCER_REGISTRY,
    factory::Simulation,
    tx::{MEGA_SYSTEM_TRANSACTION_SOURCE_HASH, ORACLE},
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SYSTEM: Address = Address::with_last_byte(0x50);

fn evm(disabled: EvmFeatures, code: &[u8]) -> Evm<'static, MegaTypes> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SYSTEM,
        AccountInfo::default()
            .with_nonce(7)
            .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code))),
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
        &U256::from_be_slice(SYSTEM.as_slice()),
    );
    // The simulation's switches of a node's configuration: the nonce check and EIP-3607.
    let switches = EvmFeatures::NONCE_CHECK | EvmFeatures::EIP3607;
    assert!(switches.contains(disabled), "only the simulation's switches can be off");
    let simulation = Simulation {
        skip_nonce_check: disabled.contains(EvmFeatures::NONCE_CHECK),
        skip_eip3607: disabled.contains(EvmFeatures::EIP3607),
        ..Simulation::default()
    };
    let block = BlockEnv::<MegaTypes> { gas_limit: U256::from(30_000_000), ..Default::default() };
    common::simulated_evm(block, Db::new(db), &simulation)
}

fn tx(nonce: u64, chain_id: Option<u64>) -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::new(
            TxLegacy {
                chain_id,
                nonce,
                gas_limit: 1_000_000,
                to: TxKind::Call(ORACLE),
                ..Default::default()
            },
            Some(Bytes::new()),
        ),
        SYSTEM,
    )
}

#[test]
fn test_system_sender_nonce_check_can_be_disabled() {
    let mut evm = evm(EvmFeatures::NONCE_CHECK, &[]);
    let out = evm.transact(&tx(0, Some(1))).expect("nonce check disabled").detach();
    assert!(out.result.status);
    assert_eq!(out.pending_state.account_info(&SYSTEM).unwrap().nonce, 8);
}

#[test]
fn test_system_sender_code_check_can_be_disabled() {
    let mut evm = evm(EvmFeatures::EIP3607, &[0x00]);
    let out = evm.transact(&tx(7, Some(1))).expect("code check disabled").detach();
    assert!(out.result.status);
    assert_eq!(out.pending_state.account_info(&SYSTEM).unwrap().nonce, 8);
}

#[test]
fn test_system_sender_checks_still_apply_when_enabled() {
    let mut nonce = evm(EvmFeatures::empty(), &[]);
    assert!(matches!(
        nonce.transact(&tx(0, Some(1))).err(),
        Some(HandlerError::InvalidNonce { expected: 7, got: 0 })
    ));
    let mut code = evm(EvmFeatures::empty(), &[0x00]);
    assert!(matches!(
        code.transact(&tx(7, Some(1))).err(),
        Some(HandlerError::RejectCallerWithCode)
    ));
    let mut both = evm(EvmFeatures::NONCE_CHECK | EvmFeatures::EIP3607, &[0x00]);
    assert!(matches!(both.transact(&tx(0, None)).err(), Some(HandlerError::MissingChainId)));
    assert!(matches!(
        both.transact(&tx(0, Some(2))).err(),
        Some(HandlerError::InvalidChainId { .. })
    ));
}

#[test]
fn test_protocol_source_from_live_system_sender_still_fails_without_a_frame() {
    let mut evm = evm(EvmFeatures::empty(), &[]);
    let tx = Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: MEGA_SYSTEM_TRANSACTION_SOURCE_HASH,
            from: SYSTEM,
            to: TxKind::Call(ORACLE),
            mint: 17,
            value: U256::ZERO,
            gas_limit: 1_000_000,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        SYSTEM,
    );
    let out = evm.transact(&tx).unwrap().detach();
    assert!(out.result.ext.failed_deposit);
    assert_eq!(out.result.tx_gas_used(), 1_000_000);
    assert!(out.pending_state.account_changes_for(&ORACLE).is_none());
    let registry = out.pending_state.account_changes_for(&SEQUENCER_REGISTRY).unwrap();
    assert!(registry.storage.slots.contains_key(&U256::ZERO));
    let caller = out.pending_state.account_info(&SYSTEM).unwrap();
    assert_eq!((caller.nonce, caller.balance), (8, U256::from(17)));
}
