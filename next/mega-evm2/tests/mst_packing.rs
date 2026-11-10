//! A Mega System Transaction is held to the block's three packing budgets — data size, KV updates
//! and state growth — as any other non-deposit transaction is, and counts toward each; a deposit
//! is not (`SATIN-BLK-003`, `SATIN-BLK-006`, `SATIN-BLK-007`, `SATIN-MST-009`). A block reaches a
//! budget when its counter is at or above it.

mod common;

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    SEQUENCER_REGISTRY,
    block::{BLOCK_BUDGETS, BlockLimiter, BlockSession},
    error::{Budget, CandidateError, Disposition, Refusal},
    tx::ORACLE,
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SYSTEM: Address = Address::with_last_byte(0x51);
const USER: Address = Address::with_last_byte(0x50);
/// Writes 1 into slots 1 to 1,000, all fresh: the state growth budget's worth.
const FILLER: Address = Address::with_last_byte(0xf1);

fn filler_code() -> Bytes {
    let mut code = Vec::new();
    for slot in 1..=BLOCK_BUDGETS[2] as u16 {
        code.extend_from_slice(&[0x60, 0x01, 0x61]);
        code.extend_from_slice(&slot.to_be_bytes());
        code.push(0x55);
    }
    code.push(0x00);
    code.into()
}

fn executor() -> BlockSession<'static> {
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
        &FILLER,
        AccountInfo::default().with_nonce(1).with_code(Bytecode::new_legacy(filler_code())),
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
    let input = common::block_input(1, 1_000, 10_000_000_000, Some(B256::ZERO));
    common::start_block(&input, Db::new(db))
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

fn deposit(to: Address, gas_limit: u64) -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: USER,
            to: TxKind::Call(to),
            mint: 0,
            value: U256::ZERO,
            gas_limit,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        USER,
    )
}

fn run(ex: &mut BlockSession<'static>, tx: &Recovered<OpTxEnvelope>) -> Result<(), CandidateError> {
    // The envelope's bytes only size the transaction for the builder's limits, none here.
    ex.execute_candidate(tx, &[0u8; 110]).map(|_| ())
}

/// With each budget reached in turn, a transaction held to the budgets is refused for it and an
/// exempt one is not.
#[test]
fn test_each_reached_budget_refuses_a_transaction_held_to_it() {
    for (i, budget) in Budget::ALL.into_iter().enumerate() {
        let mut limiter = BlockLimiter { block_gas_limit: 30_000_000, ..Default::default() };
        limiter.usage[i] = BLOCK_BUDGETS[i];
        let refusal = Refusal::Packing { budget, used: BLOCK_BUDGETS[i], limit: BLOCK_BUDGETS[i] };
        assert_eq!(limiter.pre_execution_check(1_000_000, false), Err(refusal));
        assert_eq!(limiter.pre_execution_check(1_000_000, true), Ok(()));
        limiter.usage[i] = BLOCK_BUDGETS[i] - 1;
        assert_eq!(limiter.pre_execution_check(1_000_000, false), Ok(()));
    }
}

/// Once a deposit has filled the block's state growth budget, a Mega System Transaction is refused
/// for it, as a user's transaction is; a deposit is not. Below the budgets the Mega System
/// Transaction runs and counts its body toward the block's data size.
#[test]
fn test_a_mega_system_transaction_is_held_to_the_packing_budgets() {
    let mut ex = executor();
    let fill = ex.execute_candidate(&deposit(FILLER, 1_000_000_000), &[]).expect("the filler");
    assert!(fill.result().status, "{:?}", fill.result());
    assert_eq!(fill.usage()[2], BLOCK_BUDGETS[2]);
    ex.commit_candidate(fill).expect("committed");
    let full = Refusal::Packing {
        budget: Budget::StateGrowth,
        used: BLOCK_BUDGETS[2],
        limit: BLOCK_BUDGETS[2],
    };
    let refused = run(&mut ex, &legacy(SYSTEM, ORACLE)).expect_err("the system transaction");
    assert_eq!(refused, CandidateError::Refused(full.clone()));
    assert_eq!(refused.disposition(), Disposition::Refused);
    assert!(refused.to_string().starts_with("Block state growth limit"), "{refused}");
    let user = run(&mut ex, &legacy(USER, ORACLE)).expect_err("the user's transaction");
    assert_eq!(user, CandidateError::Refused(full));
    run(&mut ex, &deposit(ORACLE, 1_000_000)).expect("a deposit is not held to the budgets");
    assert!(ex.fault().is_none(), "a refusal decides nothing about the block");

    let mut ex = executor();
    let tx = legacy(SYSTEM, ORACLE);
    let out = ex.execute_candidate(&tx, &[0u8; 110]).expect("below the budgets");
    assert!(out.is_system_transaction());
    ex.commit_candidate(out).expect("committed");
    assert_eq!(ex.limiter().usage[0], 310);
}
