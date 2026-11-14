//! Times the Satin block executor on the recorded corpus, after loading state and decoding
//! transactions in memory.
//!
//! - **Serial** (default): the gates' block timing (`bench_blocks::measure`, included unchanged):
//!   one first traversal of every block, then `BENCH_RUNS` timed traversals, each block's outcome
//!   checked against the first. One JSON line per block on stdout with its warm samples; the first
//!   traversal's per-block times as JSON lines on stderr.
//! - **Parallel** (`KEC_PARALLEL=<threads>:<split|shared>[,...]`): after one serial first
//!   traversal, `BENCH_RUNS` traversals per configuration on that many threads, which share the
//!   process-wide Keccak cache when the `cache` feature is on. `split` hands each block to one
//!   thread (the next free one); `shared` has every thread run every block, each from its own
//!   starting point, so the same inputs are hashed concurrently. One JSON line per traversal.
//!
//! With `KEC_PERF_CTL=<fifo>` and `KEC_PERF_ACK=<fifo>` (perf record's `--control`), parallel mode
//! enables a profiler started with its events disabled after its first traversal and disables it
//! after the last, so the samples cover the timed traversals only.
//!
//! The timed interval is the gates': it starts before the block session (the pre-block calls are
//! in it) and ends after the last commit; loading, decoding, cloning the prestate and dropping
//! the executor are outside it.

use alloy_consensus::transaction::Recovered;
use alloy_primitives::Bytes;
use kec_block::{
    bench_blocks, blockdb,
    replay::{self, Evm2Db},
};
use op_evm2::OpTxEnvelope;
use serde_json::json;
use std::{
    cell::RefCell,
    collections::HashMap,
    hint::black_box,
    rc::Rc,
    sync::{
        Barrier, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

/// The number of blocks of the first traversal, whose times go to stderr (serial mode).
static FIRST_PASS: OnceLock<usize> = OnceLock::new();
static SEEN: AtomicUsize = AtomicUsize::new(0);

fn run(
    block: &blockdb::Block,
    txs: &[(Recovered<OpTxEnvelope>, Bytes)],
) -> (Duration, usize, usize, u64) {
    let db = Rc::new(RefCell::new(bench_blocks::fresh_db(block)));
    let database =
        evm2::evm::Db::new(Evm2Db { db, code_cache: Rc::new(RefCell::new(HashMap::new())) });
    let factory = replay::factory(block.chain_id);
    let input = replay::block_input(&block.header);
    let (executor, elapsed, refused) = execute_block(&factory, &input, database, block, txs);
    if let Some(&n) = FIRST_PASS.get() &&
        SEEN.fetch_add(1, Ordering::Relaxed) < n
    {
        eprintln!(
            "{}",
            json!({"variant": kec_block::variant(), "set": block.set, "block": block.number,
                "first_ms": elapsed.as_secs_f64() * 1000.0})
        );
    }
    (elapsed, executor.receipts().len(), refused, executor.limiter().block_gas_used)
}

/// Exactly the measured interval, in a frame of its own so a profiler can find it.
#[inline(never)]
fn execute_block(
    factory: &mega_evm2::factory::SatinFactory,
    input: &mega_evm2::factory::BlockInput,
    database: impl evm2::evm::DynDatabase + 'static,
    block: &blockdb::Block,
    txs: &[(Recovered<OpTxEnvelope>, Bytes)],
) -> (mega_evm2::block::BlockSession<'static>, Duration, usize) {
    let mut refused = 0;
    let start = Instant::now();
    let services = replay::services(&block.header);
    let mut executor = factory.start_block(input, database, services).expect("pre-block changes");
    for (tx, raw) in txs {
        match executor.execute_candidate(tx, raw) {
            Ok(outcome) => {
                if executor.commit_candidate(outcome).is_err() {
                    refused += 1;
                }
            }
            Err(_) => refused += 1,
        }
    }
    black_box(&executor);
    let elapsed = start.elapsed();
    (executor, elapsed, refused)
}

/// Sends `command` to a profiler's control fifo and waits for its acknowledgement.
fn perf_control(command: &str) {
    use std::io::{BufRead, Write};
    let (Ok(ctl), Ok(ack)) = (std::env::var("KEC_PERF_CTL"), std::env::var("KEC_PERF_ACK")) else {
        return;
    };
    let mut c = std::fs::OpenOptions::new().write(true).open(ctl).expect("perf control fifo");
    writeln!(c, "{command}").expect("perf control");
    let mut line = String::new();
    std::io::BufReader::new(std::fs::File::open(ack).expect("perf ack fifo"))
        .read_line(&mut line)
        .expect("perf ack");
    assert_eq!(line.trim(), "ack", "perf did not acknowledge {command}");
}

fn parallel(spec: &str) {
    let runs: usize =
        std::env::var("BENCH_RUNS").unwrap_or_else(|_| "3".into()).parse().expect("BENCH_RUNS");
    let corpus: replay::Corpus = replay::load_corpus();
    let n = corpus.len();
    let first = Instant::now();
    let expected: Vec<_> = corpus
        .iter()
        .map(|(b, txs)| {
            let (_, included, refused, gas) = run(b, txs);
            (included, refused, gas)
        })
        .collect();
    eprintln!(
        "{}",
        json!({"variant": kec_block::variant(), "parallel_first_traversal_ms": first.elapsed().as_secs_f64() * 1000.0})
    );
    perf_control("enable");
    for config in spec.split(',').filter(|s| !s.is_empty()) {
        let (threads, mode) = config.split_once(':').expect("<threads>:<split|shared>");
        let threads: usize = threads.parse().expect("thread count");
        assert!(threads >= 1 && matches!(mode, "split" | "shared"), "bad configuration {config}");
        for r in 0..runs {
            let next = AtomicUsize::new(0);
            let barrier = Barrier::new(threads + 1);
            let (wall, per_thread) = std::thread::scope(|s| {
                let handles: Vec<_> = (0..threads)
                    .map(|t| {
                        let (corpus, expected, next, barrier) =
                            (&corpus, &expected, &next, &barrier);
                        std::thread::Builder::new()
                            .stack_size(64 << 20)
                            .spawn_scoped(s, move || {
                                let mut busy = Duration::ZERO;
                                let mut blocks = 0usize;
                                let mut one = |i: usize| {
                                    let (b, txs) = &corpus[i];
                                    let (elapsed, included, refused, gas) = run(b, txs);
                                    assert_eq!(
                                        (included, refused, gas),
                                        expected[i],
                                        "unstable block outcome"
                                    );
                                    busy += elapsed;
                                    blocks += 1;
                                };
                                barrier.wait();
                                if mode == "split" {
                                    loop {
                                        let i = next.fetch_add(1, Ordering::Relaxed);
                                        if i >= n {
                                            break;
                                        }
                                        one(i);
                                    }
                                } else {
                                    for k in 0..n {
                                        one((k + t * n / threads) % n);
                                    }
                                }
                                (busy, blocks)
                            })
                            .expect("spawn")
                    })
                    .collect();
                barrier.wait();
                let start = Instant::now();
                let per_thread: Vec<_> =
                    handles.into_iter().map(|h| h.join().expect("thread")).collect();
                (start.elapsed(), per_thread)
            });
            let busy: f64 = per_thread.iter().map(|(b, _)| b.as_secs_f64() * 1000.0).sum();
            let blocks: usize = per_thread.iter().map(|(_, k)| k).sum();
            println!(
                "{}",
                json!({"variant": kec_block::variant(), "threads": threads, "mode": mode, "run": r,
                    "wall_ms": wall.as_secs_f64() * 1000.0, "busy_ms": busy, "blocks": blocks,
                    "per_thread_blocks": per_thread.iter().map(|(_, k)| k).collect::<Vec<_>>()})
            );
        }
    }
    perf_control("disable");
}

fn main() {
    if cfg!(debug_assertions) {
        panic!("timing requires a release build");
    }
    if let Ok(spec) = std::env::var("KEC_PARALLEL") {
        parallel(&spec);
        return;
    }
    FIRST_PASS.set(blockdb::block_list().len()).expect("set once");
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
