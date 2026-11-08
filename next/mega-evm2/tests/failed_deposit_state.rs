//! Failed deposits undo writes while retaining every account and slot loaded in returned state.
//! Database-read instrumentation in the differential gates cannot observe discarded loaded entries.
use alloy_consensus::transaction::Recovered;
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    Evm, ExecutionConfig,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    MegaSpec, MegaTypes, SEQUENCER_REGISTRY, precompiles, satin_version, tx::registry,
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SENDER: Address = Address::with_last_byte(0x50);
const TARGET: Address = Address::with_last_byte(0xa0);

fn evm() -> Evm<'static, MegaTypes> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_nonce(7).with_balance(U256::from(100)),
    );
    // Read three slots, modify one, then halt with INVALID.
    db.insert_account_info(
        &TARGET,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from(hex!(
            "6001545060025450600354506009600255fe"
        )))),
    );
    for slot in 1..=3 {
        db.insert_account_storage(&TARGET, &U256::from(slot), &U256::from(slot + 10));
    }
    db.insert_account_info(&SEQUENCER_REGISTRY, AccountInfo::default().with_nonce(1));
    let config = ExecutionConfig::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> { gas_limit: U256::from(30_000_000), ..Default::default() };
    let mut evm = Evm::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(db),
        precompiles(),
    );
    evm.ext_mut().cfg.limits = [13_107_200, 500_000, 1_000];
    evm
}

fn deposit(gas_limit: u64) -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(TARGET),
            mint: 17,
            value: U256::from(3),
            gas_limit,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        SENDER,
    )
}

#[test]
fn test_failed_deposit_returns_loaded_slots_but_no_frame_writes() {
    let mut evm = evm();
    let out = evm.transact(&deposit(1_000_000)).unwrap().detach();
    assert!(out.result.ext.failed_deposit);
    assert_eq!(out.result.tx_gas_used(), 1_000_000);
    let state = out.pending_state;
    let target = state.account_changes_for(&TARGET).expect("loaded contract retained");
    assert!(!target.is_changed());
    assert_eq!(target.storage.slots.len(), 3);
    for slot in 1..=3 {
        let value = &target.storage.slots[&U256::from(slot)].value;
        assert_eq!(value.original, U256::from(slot + 10));
        assert_eq!(value.current, value.original);
    }
    let registry = state.account_changes_for(&SEQUENCER_REGISTRY).expect("registry retained");
    assert!(!registry.is_changed());
    assert!(registry.storage.slots.contains_key(&U256::ZERO));
    let sender = state.account_info(&SENDER).unwrap();
    assert_eq!((sender.nonce, sender.balance), (8, U256::from(117)));
    assert_eq!(state.account_changes().filter(|a| a.is_changed()).count(), 1);
}

#[test]
fn test_intrinsic_failure_keeps_registry_read_without_loading_recipient() {
    let mut evm = evm();
    let out = evm.transact(&deposit(1)).unwrap().detach();
    assert!(out.result.ext.failed_deposit);
    assert!(out.pending_state.account_changes_for(&TARGET).is_none());
    let registry =
        out.pending_state.account_changes_for(&SEQUENCER_REGISTRY).expect("registry retained");
    assert!(registry.storage.slots.contains_key(&U256::ZERO));
    let sender = out.pending_state.account_info(&SENDER).unwrap();
    assert_eq!((sender.nonce, sender.balance), (8, U256::from(117)));
}
