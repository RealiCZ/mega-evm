//! SALT witness exports belong to the block execution, including dropped candidates.
#![cfg(feature = "witness")]
use alloy_consensus::transaction::Recovered;
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    Evm, ExecutionConfig,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    version::StateGasSite,
};
use mega_evm2::{
    MegaSpec, MegaTypes, block::BlockExecutor, bucket_for, precompiles, satin_version, tx::registry,
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SENDER: Address = Address::with_last_byte(0x50);
const WRITER: Address = Address::with_last_byte(0xa0);
const NOOP: Address = Address::with_last_byte(0xb0);

fn bucket(slot: u64) -> u64 {
    bucket_for(StateGasSite::Storage(WRITER, U256::from(slot)))
}

fn block(fail_second: bool) -> BlockExecutor<'static> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&SENDER, AccountInfo::default().with_nonce(1));
    db.insert_account_info(&NOOP, AccountInfo::default().with_nonce(1));
    db.insert_account_info(
        &WRITER,
        AccountInfo::default()
            .with_code(Bytecode::new_legacy(Bytes::from(hex!("6001600155600160025500")))),
    );
    let config = ExecutionConfig::for_spec_and_version(MegaSpec::Satin, satin_version());
    let env = BlockEnv::<MegaTypes> { gas_limit: U256::from(30_000_000), ..Default::default() };
    let mut evm = Evm::new_with_execution_config(
        config,
        MegaSpec::Satin,
        env,
        registry(),
        Db::new(db),
        precompiles(),
    );
    evm.ext_mut().cfg.limits = [13_107_200, 500_000, 1_000];
    evm.ext_mut().cfg.buckets.insert(bucket(1), 512);
    evm.ext_mut().cfg.buckets.insert(bucket(2), if fail_second { 0 } else { 768 });
    BlockExecutor::new(evm, B256::ZERO, Some(B256::ZERO))
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
        let mut block = block(false);
        let out = block.run_transaction(&deposit(WRITER), &[]).unwrap();
        assert!(out.result.status);
        if commit {
            block.commit_transaction_outcome(out).unwrap();
        } else {
            drop(out);
        }
        let out = block.run_transaction(&deposit(NOOP), &[]).unwrap();
        block.commit_transaction_outcome(out).unwrap();
        assert_eq!(block.bucket_reads, [bucket(1), bucket(2)].into_iter().collect());
        let mut next = BlockExecutor::new(block.evm, B256::ZERO, Some(B256::ZERO));
        assert!(next.bucket_reads.is_empty());
        let out = next.run_transaction(&deposit(NOOP), &[]).unwrap();
        next.commit_transaction_outcome(out).unwrap();
        assert!(next.bucket_reads.is_empty());
    }
}

#[test]
fn test_failed_candidate_keeps_only_successful_bucket_answers() {
    let mut block = block(true);
    let error = block
        .run_transaction(&deposit(WRITER), &[])
        .expect_err("invalid capacity aborts execution");
    assert!(error.contains("below the minimum bucket"), "{error}");
    let out = block.run_transaction(&deposit(NOOP), &[]).unwrap();
    block.commit_transaction_outcome(out).unwrap();
    assert_eq!(block.bucket_reads, [bucket(1)].into_iter().collect());
}

#[test]
fn test_committing_a_foreign_outcome_adds_no_bucket_reads() {
    let mut source = block(false);
    let out = source.run_transaction(&deposit(WRITER), &[]).unwrap();
    let mut target = block(false);
    target.commit_transaction_outcome(out).unwrap();
    assert!(target.bucket_reads.is_empty());
}
