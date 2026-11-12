//! Times the reference block executor using the same in-memory inputs as the engine.

use alloy_consensus::transaction::Recovered;
use alloy_eips::Decodable2718;
use mega_evm::{
    AHashBucketHasher, BlockLimits, MegaBlockExecutionCtx, MegaBlockExecutor, MegaEvmFactory,
    MegaHardfork, MegaSpecId, MegaTxEnvelope, TestExternalEnvs,
    alloy_evm::{EvmEnv, EvmFactory, block::BlockExecutor},
    alloy_hardforks::ForkCondition,
    alloy_op_evm::block::OpAlloyReceiptBuilder,
};
use mega_evm2_reference::{SatinDb, block_env, blockdb};
use revm::{context::CfgEnv, database::State};
use std::{
    cell::RefCell,
    convert::Infallible,
    hint::black_box,
    rc::Rc,
    time::{Duration, Instant},
};

#[path = "../../diff/shared/src/bench_blocks.rs"]
mod bench_blocks;

fn run(block: &blockdb::Block, txs: &[Recovered<MegaTxEnvelope>]) -> (Duration, usize, usize, u64) {
    let db = Rc::new(RefCell::new(bench_blocks::fresh_db(block)));
    let hardforks = mega_evm::hardfork_schedule(block.chain_id)
        .unwrap_or_default()
        .with(MegaHardfork::Satin, ForkCondition::Timestamp(0));
    let mut cfg = CfgEnv::new_with_spec(MegaSpecId::SATIN);
    cfg.chain_id = block.chain_id;
    let factory = MegaEvmFactory::new()
        .with_external_env_factory(TestExternalEnvs::<Infallible, AHashBucketHasher>::new());
    let context = MegaBlockExecutionCtx::new(
        block.header.parent_hash,
        block.header.parent_beacon_block_root,
        Default::default(),
        BlockLimits::default(),
        block.parent_timestamp,
    );
    let mut state = State::builder().with_database(SatinDb(db)).with_bundle_update().build();
    let evm = factory.create_evm(&mut state, EvmEnv::new(cfg, block_env(block)));
    let mut executor =
        MegaBlockExecutor::new(evm, context, &hardforks, OpAlloyReceiptBuilder::default());
    let mut refused = 0;
    let start = Instant::now();
    executor.apply_pre_execution_changes().expect("pre-block changes");
    for tx in txs {
        match executor.run_transaction(tx.as_recovered_ref()) {
            Ok(outcome) => {
                if executor.commit_transaction_outcome(outcome).is_err() {
                    refused += 1;
                }
            }
            Err(_) => refused += 1,
        }
    }
    black_box(&executor);
    let elapsed = start.elapsed();
    (elapsed, executor.receipts().len(), refused, executor.limiter().block_gas_used)
}

fn main() {
    bench_blocks::measure(
        |block, raws| {
            block
                .txs
                .iter()
                .zip(raws)
                .map(|(tx, raw)| {
                    let envelope = MegaTxEnvelope::decode_2718(&mut raw.as_ref()).expect("decode");
                    black_box(envelope.tx_hash());
                    Recovered::new_unchecked(envelope, blockdb::addr(&tx["from"]))
                })
                .collect::<Vec<_>>()
        },
        |block, txs| run(block, txs),
    );
}
