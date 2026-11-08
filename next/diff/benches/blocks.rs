//! Times the block executor after loading state and decoding transactions in memory.

use alloy_consensus::transaction::Recovered;
use alloy_primitives::Bytes;
use mega_evm2::block::BlockExecutor;
use mega_evm2_diff::replay::{self, Evm2Db, blockdb};
use op_evm2::OpTxEnvelope;
use std::{
    cell::RefCell,
    collections::HashMap,
    hint::black_box,
    rc::Rc,
    time::{Duration, Instant},
};

#[path = "../shared/src/bench_blocks.rs"]
mod bench_blocks;

fn run(
    block: &blockdb::Block,
    txs: &[(Recovered<OpTxEnvelope>, Bytes)],
) -> (Duration, usize, usize, u64) {
    let db = Rc::new(RefCell::new(bench_blocks::fresh_db(block)));
    let evm = replay::new_evm(
        &block.header,
        block.chain_id,
        Evm2Db { db, code_cache: Rc::new(RefCell::new(HashMap::new())) },
    );
    let mut executor =
        BlockExecutor::new(evm, block.header.parent_hash, block.header.parent_beacon_block_root);
    let mut refused = 0;
    let start = Instant::now();
    executor.apply_pre_execution_changes().expect("pre-block changes");
    for (tx, raw) in txs {
        match executor.run_transaction(tx, raw) {
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
    (elapsed, executor.receipts.len(), refused, executor.limiter.block_gas_used)
}

fn main() {
    bench_blocks::measure(
        |block, raws| {
            block
                .txs
                .iter()
                .zip(raws)
                .map(|(tx, raw)| (replay::decode(raw, tx).expect("decode"), raw.clone()))
                .collect::<Vec<_>>()
        },
        |block, txs| run(block, txs),
    );
}
