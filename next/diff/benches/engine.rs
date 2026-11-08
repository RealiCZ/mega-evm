//! Engine benchmarks: a few fixed workloads on the Satin engine, each plain and with the
//! frame-recording inspector, so one run says whether a change slowed execution.
//! `cargo bench -p mega-evm2-diff --bench engine` (add `-- --save-baseline <name>` or
//! `-- --baseline <name>` to compare runs); the same target runs under `cargo codspeed`.
//!
//! Every workload is built outside the measured closure (`iter_batched`: a new database, EVM and
//! transaction per iteration, since a commit changes the state) and its input is fixed below.
//! Each workload is run once before measuring and must succeed, so a bench cannot measure a
//! halt by accident. All scenario workloads run as legacy transactions (a deposit where named)
//! from the shared sender in the synthetic block of `driver::evm`.
//!
//! | name          | what runs                                                                       |
//! | ------------- | ------------------------------------------------------------------------------- |
//! | `call_loop`   | 200 iterations of `CALL` to an account with no code, 5M gas                     |
//! | `detained`    | `TIMESTAMP` (a volatile read), then 1,000 iterations of arithmetic under a      |
//! |               | 100,000 detention cap, 5M gas                                                   |
//! | `sstore`      | slots 1..=8: set 0 -> 1, clear 1 -> 0, refill 0 -> 1; 10M gas                   |
//! | `create`      | `CREATE` whose init code returns 2,000 bytes of code (a code deposit); 50M gas  |
//! | `precompile`  | `NUMBER` (a volatile read), then a `CALL` of BLAKE2F with 40,000 rounds and a   |
//! |               | 3M gas forward, under a 25,000 detention cap (priced and held); 10M gas         |
//! | `deposit`     | an OP deposit (mint 0) calling a contract that writes slot 1; 1M gas            |
//! | `block`       | recorded block 26400004 of corpus `p9` (24 transactions, ten of them from the   |
//! |               | system address, which the executor promotes to deposits) through the block      |
//! |               | executor: pre-block changes, every transaction, commit                          |
//!
//! The block needs `zstd` on the path (the gate scripts need it too): it is unpacked once into
//! the Cargo target directory.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use evm2::Evm;
use mega_evm2::{MegaTypes, block::BlockExecutor};
use mega_evm2_diff::{
    driver::{self, Frames, scenarios::Scenario},
    replay::{self, Evm2Db, blockdb},
};
use std::{cell::RefCell, collections::HashMap, hint::black_box, rc::Rc};

#[path = "../shared/src/bench_recorded.rs"]
mod bench_recorded;
use bench_recorded::BLOCK_SET;

#[path = "../shared/src/bench_workloads.rs"]
mod bench_workloads;

/// A workload runs once outside the measurement and must have succeeded.
fn check(s: &Scenario) {
    let out = driver::run(s, false);
    assert_eq!(out["status"], "success", "{} did not succeed: {out}", s.name);
    println!("WORKLOAD {} gas_used={}", s.name, out["gas_used"]);
    if let Ok(dir) = std::env::var("BENCH_WORKLOAD_RECORDS") {
        let reference = std::fs::read_to_string(format!("{dir}/{}.json", s.name))
            .expect("reference workload record");
        let reference = serde_json::from_str(&reference).expect("workload json");
        let inspected = driver::run(s, true);
        let differences = mega_evm2_diff::gate::scenario_differences(&reference, &out, &inspected);
        assert!(differences.is_empty(), "{} differs: {differences:?}", s.name);
        println!("WORKLOAD_MATCH {}", s.name);
    }
}

fn bench_scenario(c: &mut Criterion, name: &str, s: Scenario) {
    check(&s);
    for inspected in [false, true] {
        let id = format!("{name}/{}", if inspected { "inspected" } else { "plain" });
        c.bench_function(&id, |b| {
            b.iter_batched(
                || {
                    let mut evm = driver::evm(&s, Default::default());
                    if inspected {
                        evm.set_inspector(Frames::default());
                    }
                    (evm, driver::tx(&s))
                },
                |(mut evm, tx): (Evm<'static, MegaTypes>, _)| {
                    let executed = evm.transact(&tx).expect("transact");
                    black_box(executed.commit())
                },
                BatchSize::LargeInput,
            )
        });
    }
}

/// Runs every transaction of the block through the executor and commits it.
fn run_block(b: blockdb::Block, raws: &[alloy_primitives::Bytes], inspected: bool) -> usize {
    let db = Rc::new(RefCell::new(b.db));
    let mut evm = replay::new_evm(
        &b.header,
        b.chain_id,
        Evm2Db { db, code_cache: Rc::new(RefCell::new(HashMap::new())) },
    );
    if inspected {
        evm.set_inspector(Frames::default());
    }
    let mut ex = BlockExecutor::new(evm, b.header.parent_hash, b.header.parent_beacon_block_root);
    ex.apply_pre_execution_changes().expect("pre-block");
    assert_eq!(b.txs.len(), raws.len());
    for (t, raw) in b.txs.iter().zip(raws) {
        let tx = replay::decode(raw, t).expect("decode");
        let out = ex.run_transaction(&tx, raw).expect("run transaction");
        ex.commit_transaction_outcome(out).expect("commit");
    }
    ex.receipts.len()
}

fn bench_block(c: &mut Criterion) {
    let rec = bench_recorded::recorded_block(env!("CARGO_MANIFEST_DIR"));
    for inspected in [false, true] {
        let included = run_block(blockdb::load_block(&rec.path, BLOCK_SET), &rec.raws, inspected);
        assert_eq!(included, rec.raws.len(), "every transaction of the block is included");
        let id = format!("block/{}", if inspected { "inspected" } else { "plain" });
        c.bench_function(&id, |b| {
            b.iter_batched(
                || blockdb::load_block(&rec.path, BLOCK_SET),
                |block| black_box(run_block(block, &rec.raws, inspected)),
                BatchSize::LargeInput,
            )
        });
    }
}

fn engine(c: &mut Criterion) {
    for (name, scenario) in bench_workloads::workloads() {
        bench_scenario(c, name, scenario);
    }
    bench_block(c);
}

/// Setup runs once per iteration (the state is committed), so a short, fixed measurement keeps a
/// run to minutes: 30 samples over 2 seconds after a 1-second warm-up.
fn config() -> Criterion {
    Criterion::default()
        .sample_size(30)
        .warm_up_time(std::time::Duration::from_secs(1))
        .measurement_time(std::time::Duration::from_secs(2))
}

criterion_group! {
    name = benches;
    config = config();
    targets = engine
}
criterion_main!(benches);
