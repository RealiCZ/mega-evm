//! A Mega System Transaction is held to the block's three packing budgets — data size, KV updates
//! and state growth — as any other non-deposit transaction is, and counts toward each; a deposit
//! is not (`SATIN-BLK-003`, `SATIN-BLK-006`, `SATIN-BLK-007`, `SATIN-MST-009`). A block reaches a
//! budget when its counter is at or above it.

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    Evm, ExecutionConfig,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    MegaSpec, MegaTypes, SEQUENCER_REGISTRY,
    block::{BLOCK_BUDGETS, BlockExecutor},
    precompiles, satin_version,
    tx::{ORACLE, registry},
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SYSTEM: Address = Address::with_last_byte(0x51);
const USER: Address = Address::with_last_byte(0x50);

fn executor() -> BlockExecutor<'static> {
    let mut db = InMemoryDB::default();
    for a in [SYSTEM, USER] {
        db.insert_account_info(
            &a,
            AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
        );
    }
    db.insert_account_info(
        &ORACLE,
        AccountInfo::default()
            .with_nonce(1)
            .with_code(Bytecode::new_legacy(Bytes::from_static(&[0x00]))),
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
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    let evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(db),
        precompiles(),
    );
    BlockExecutor::new(evm, B256::ZERO, Some(B256::ZERO))
}

fn legacy(from: Address, to: Address) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy {
        chain_id: Some(1),
        gas_limit: 1_000_000,
        to: TxKind::Call(to),
        ..Default::default()
    };
    Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), from)
}

fn deposit() -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: USER,
            to: TxKind::Call(ORACLE),
            mint: 0,
            value: U256::ZERO,
            gas_limit: 1_000_000,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        USER,
    )
}

fn run(ex: &mut BlockExecutor<'static>, tx: &Recovered<OpTxEnvelope>) -> Result<(), String> {
    // The envelope's bytes only size the transaction for the builder's limits, none here.
    ex.run_transaction(tx, &[0u8; 110]).map(|_| ())
}

/// With each budget reached in turn, a Mega System Transaction is refused for it, as a user's
/// transaction is; a deposit is not. Below the budgets the Mega System Transaction runs and counts
/// its body toward the block's data size.
#[test]
fn test_a_mega_system_transaction_is_held_to_the_packing_budgets() {
    const NAMES: [&str; 3] =
        ["Block transactions data limit", "Block KV update limit", "Block state growth limit"];
    for i in 0..3 {
        let mut ex = executor();
        ex.limiter.usage[i] = BLOCK_BUDGETS[i];
        let refused = run(&mut ex, &legacy(SYSTEM, ORACLE)).expect_err("the system transaction");
        assert!(refused.starts_with(NAMES[i]), "{refused}");
        let user = run(&mut ex, &legacy(USER, ORACLE)).expect_err("the user's transaction");
        assert!(user.starts_with(NAMES[i]), "{user}");
        run(&mut ex, &deposit()).expect("a deposit is not held to the budgets");
    }
    let mut ex = executor();
    let tx = legacy(SYSTEM, ORACLE);
    let out = ex.run_transaction(&tx, &[0u8; 110]).expect("below the budgets");
    assert!(out.is_system_transaction);
    ex.commit_transaction_outcome(out).expect("committed");
    assert_eq!(ex.limiter.usage[0], 310);
}
