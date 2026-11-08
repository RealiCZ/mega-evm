//! In-memory inputs and repeated measurements shared by the two block timing binaries.

use crate::blockdb::{self, Block, BlockDb};
use alloy_primitives::Bytes;
use serde_json::{Value, json};
use std::{hint::black_box, time::Duration};

/// Loads a block's exact transaction bytes and checks their identities against the block.
pub fn raw_transactions(block: &Block) -> Vec<Bytes> {
    let root = std::env::var("MEGA_EVM2_REPLAY_REF").expect("MEGA_EVM2_REPLAY_REF");
    let text = std::fs::read_to_string(format!("{root}/{}/{}.jsonl", block.set, block.number))
        .expect("replay file");
    let rows: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("replay row"))
        .filter(|row| row["kind"] == "tx")
        .collect();
    assert_eq!(rows.len(), block.txs.len());
    rows.iter()
        .zip(&block.txs)
        .enumerate()
        .map(|(i, (row, tx))| {
            assert_eq!(row["index"], json!(i));
            assert_eq!(row["hash"], tx["hash"]);
            assert!(row.get("envelope_error").is_none());
            blockdb::bytes(&row["raw"])
        })
        .collect()
}

/// Copies only the initial database contents, outside the measured executor interval.
pub fn fresh_db(block: &Block) -> BlockDb {
    BlockDb {
        accounts: block.db.accounts.clone(),
        codes: block.db.codes.clone(),
        block_hashes: block.db.block_hashes.clone(),
        ..Default::default()
    }
}

/// Reports timed block execution and checks that every repetition includes the same work.
///
/// `prepare` decodes transactions once; `run` creates fresh execution state before starting its
/// timer and returns elapsed time, included transactions, refused transactions and receipt gas.
pub fn measure<T>(
    prepare: impl Fn(&Block, &[Bytes]) -> T,
    run: impl Fn(&Block, &T) -> (Duration, usize, usize, u64),
) {
    if cfg!(debug_assertions) {
        panic!("timing requires a release build");
    }
    let runs: usize = std::env::var("BENCH_RUNS")
        .unwrap_or_else(|_| "7".into())
        .parse()
        .expect("BENCH_RUNS must be an integer");
    assert!(runs >= 3, "at least three measured repetitions are required");
    let blocks: Vec<_> = blockdb::block_list()
        .into_iter()
        .map(|(set, path)| {
            let block = blockdb::load_block(&path, &set);
            let txs = prepare(&block, &raw_transactions(&block));
            (block, txs)
        })
        .collect();
    let identities: Vec<_> =
        blocks.iter().map(|(b, _)| format!("{}:{}", b.set, b.number)).collect();
    assert!(blockdb::check_processed_identities(&identities));
    assert!(!blocks.is_empty());
    let expected: Vec<_> = blocks
        .iter()
        .map(|(b, txs)| {
            let (_, included, refused, gas) = black_box(run(b, txs));
            assert_eq!(included + refused, b.txs.len());
            (included, refused, gas)
        })
        .collect();
    let mut samples = vec![Vec::with_capacity(runs); blocks.len()];
    for _ in 0..runs {
        for (i, (b, txs)) in blocks.iter().enumerate() {
            let (elapsed, included, refused, gas) = black_box(run(b, txs));
            assert_eq!((included, refused, gas), expected[i], "unstable block outcome");
            samples[i].push(elapsed.as_secs_f64() * 1_000.0);
        }
    }
    for ((b, _), (times, (included, refused, gas))) in
        blocks.iter().zip(samples.into_iter().zip(expected))
    {
        println!(
            "{}",
            json!({"set": b.set, "block": b.number, "txs": b.txs.len(),
            "included": included, "refused": refused, "gas": gas, "ms": times})
        );
    }
}
