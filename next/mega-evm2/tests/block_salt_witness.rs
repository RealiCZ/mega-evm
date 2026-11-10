//! SALT witness exports belong to the block execution, including dropped candidates.
#![cfg(feature = "witness")]

mod common;

use alloy_consensus::transaction::Recovered;
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, Db, InMemoryDB},
    version::StateGasSite,
};
use mega_evm2::{
    Cfg, SaltBelowMinimum,
    block::BlockSession,
    bucket_for,
    error::{BlockErrorKind, CandidateError, Disposition, Phase},
    factory::BlockServices,
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SENDER: Address = Address::with_last_byte(0x50);
const WRITER: Address = Address::with_last_byte(0xa0);
const NOOP: Address = Address::with_last_byte(0xb0);

fn bucket(slot: u64) -> u64 {
    bucket_for(StateGasSite::Storage(WRITER, U256::from(slot)))
}

fn db() -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&SENDER, AccountInfo::default().with_nonce(1));
    db.insert_account_info(&NOOP, AccountInfo::default().with_nonce(1));
    db.insert_account_info(
        &WRITER,
        AccountInfo::default()
            .with_code(Bytecode::new_legacy(Bytes::from(hex!("6001600155600160025500")))),
    );
    db
}

/// Block `number` over [`db`], the second slot's bucket of capacity zero when `fail_second`.
fn block(number: u64, fail_second: bool) -> BlockSession<'static> {
    let mut environment = Cfg::default();
    environment.buckets.insert(bucket(1), 512);
    environment.buckets.insert(bucket(2), if fail_second { 0 } else { 768 });
    let input = common::block_input(number, 1_000 + number, 30_000_000, Some(B256::ZERO));
    let services = BlockServices { environment, ..BlockServices::default() };
    common::factory().start_block(&input, Db::new(db()), services).expect("the block starts")
}

fn deposit(to: Address) -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(to),
            mint: 0,
            value: U256::ZERO,
            gas_limit: 1_000_000,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        SENDER,
    )
}

#[test]
fn test_block_keeps_buckets_from_committed_and_discarded_candidates() {
    for commit in [true, false] {
        let mut block = block(1, false);
        let out = block.execute_candidate(&deposit(WRITER), &[]).unwrap();
        assert!(out.result.status);
        if commit {
            block.commit_candidate(out).unwrap();
        } else {
            drop(out);
        }
        let out = block.execute_candidate(&deposit(NOOP), &[]).unwrap();
        block.commit_candidate(out).unwrap();
        assert_eq!(*block.bucket_reads(), [bucket(1), bucket(2)].into_iter().collect());
        let output = block.finish().expect("the block finishes");
        assert_eq!(output.bucket_reads, [bucket(1), bucket(2)].into_iter().collect());
        // The next block's union starts empty.
        let mut next = self::block(2, false);
        assert!(next.bucket_reads().is_empty());
        let out = next.execute_candidate(&deposit(NOOP), &[]).unwrap();
        next.commit_candidate(out).unwrap();
        assert!(next.bucket_reads().is_empty());
    }
}

/// A capacity below the minimum is an internal error of the node: the candidate has no outcome,
/// the block keeps the lookups that succeeded before it, and the session runs nothing after it.
#[test]
fn test_failed_candidate_keeps_only_successful_bucket_answers() {
    let mut block = block(1, true);
    let error = block
        .execute_candidate(&deposit(WRITER), &[])
        .expect_err("invalid capacity aborts execution");
    let CandidateError::Block(fault) = &error else { panic!("not the block's: {error:?}") };
    assert_eq!(fault.disposition(), Disposition::Internal);
    assert_eq!(fault.phase(), Phase::Transaction);
    assert_eq!(fault.context().bucket, Some(bucket(2)));
    assert!(fault.database_error().is_none());
    let BlockErrorKind::Execution(cause) = fault.kind() else { panic!("{fault:?}") };
    let evm2::registry::HandlerError::Fatal(any) = cause else { panic!("{cause:?}") };
    assert_eq!(
        any.downcast_ref::<SaltBelowMinimum>(),
        Some(&SaltBelowMinimum { bucket: bucket(2), capacity: 0 })
    );
    assert!(error.to_string().contains("below the minimum bucket"), "{error}");
    assert_eq!(*block.bucket_reads(), [bucket(1)].into_iter().collect());
    assert_eq!(block.fault(), Some(fault));
    let after = block.execute_candidate(&deposit(NOOP), &[]).expect_err("nothing runs after");
    assert_eq!(&after, &error);
    assert!(block.receipts().is_empty());
    assert_eq!(block.finish().expect_err("the block cannot finish"), *fault);
}

#[test]
fn test_committing_a_foreign_outcome_adds_no_bucket_reads() {
    let mut source = block(1, false);
    let out = source.execute_candidate(&deposit(WRITER), &[]).unwrap();
    let mut target = block(1, false);
    target.commit_candidate(out).unwrap();
    assert!(target.bucket_reads().is_empty());
}

/// An outcome executed for another block is not committed: the builder executes it again.
#[test]
fn test_an_outcome_of_another_block_is_not_committed() {
    let mut source = block(1, false);
    let out = source.execute_candidate(&deposit(WRITER), &[]).unwrap();
    let mut target = block(2, false);
    let error = target.commit_candidate(out).expect_err("another block's outcome");
    assert!(matches!(error, CandidateError::Foreign { .. }), "{error:?}");
    assert_eq!(error.disposition(), Disposition::Retry);
    assert!(target.receipts().is_empty());
    assert!(target.fault().is_none());
}
