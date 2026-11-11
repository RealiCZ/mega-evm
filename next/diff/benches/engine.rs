//! Engine benchmarks: a few fixed workloads on the Satin engine, each plain and with the
//! frame-recording inspector, so one run says whether a change slowed execution.
//! `cargo bench -p mega-evm2-diff --bench engine` (add `-- --save-baseline <name>` or
//! `-- --baseline <name>` to compare runs); the same target runs under `cargo codspeed`.
//!
//! Every workload is built outside the measured closure (`iter_batched`: a new database, EVM and
//! transaction per iteration, since a commit changes the state) and its input is fixed below.
//! Each workload is run once before measuring and checked against its expected status, so a
//! success workload cannot measure a halt by accident. All scenario workloads run as legacy
//! transactions (a deposit where named) in the synthetic block of `driver::evm`; the protocol
//! workload uses the reserved caller and the others use the shared sender.
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
//! | `failed_deposit` | a deposit that writes a slot then halts, retaining its reads after rollback |
//! | `protocol_caller` | the reserved caller writing storage under zero resource limits |
//! | `block`       | recorded block 26400004 of corpus `p9` (24 transactions, ten of them from the   |
//! |               | system address, which the executor promotes to deposits) through the block      |
//! |               | executor: pre-block changes, every transaction, commit                          |
//!
//! The first six workloads (`bench_workloads`) and the block are shared with the reference's
//! benchmark; `bench-engine.sh` runs both and compares the six's outcomes with the reference's
//! records before timing. `failed_deposit` and `protocol_caller` are the engine's own.
//!
//! The block needs `zstd` on the path (the gate scripts need it too): it is unpacked once into
//! the Cargo target directory.

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use evm2::Evm;
use mega_evm2::{MegaTypes, factory::BlockServices};
use mega_evm2_diff::{
    driver::{
        self, Frames,
        scenarios::{self, A, Scenario},
    },
    replay::{self, Evm2Db, blockdb},
};
use std::{cell::RefCell, collections::HashMap, hint::black_box, rc::Rc};

#[path = "../shared/src/bench_recorded.rs"]
mod bench_recorded;
use bench_recorded::BLOCK_SET;

#[path = "../shared/src/bench_workloads.rs"]
mod bench_workloads;

/// Checks the workload once outside the measurement against its expected status (the
/// deliberately failed deposit halts). A workload `shared` with the reference's benchmark is also
/// compared with the reference's record of it when `BENCH_WORKLOAD_RECORDS` names them.
fn check(s: &Scenario, expected_status: &str, shared: bool) {
    let out = driver::run(s, false);
    assert_eq!(out["status"], expected_status, "{} unexpected outcome: {out}", s.name);
    println!("WORKLOAD {} gas_used={}", s.name, out["gas_used"]);
    if !shared {
        return;
    }
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

fn bench_scenario(c: &mut Criterion, name: &str, s: Scenario, expected_status: &str, shared: bool) {
    check(&s, expected_status, shared);
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
    let database =
        evm2::evm::Db::new(Evm2Db { db, code_cache: Rc::new(RefCell::new(HashMap::new())) });
    let inspector =
        inspected.then(|| Box::new(Frames::default()) as Box<dyn evm2::Inspector<MegaTypes>>);
    let services = BlockServices { inspector, ..replay::services(&b.header) };
    let mut ex = replay::factory(b.chain_id)
        .start_block(&replay::block_input(&b.header), database, services)
        .expect("pre-block");
    assert_eq!(b.txs.len(), raws.len());
    for (t, raw) in b.txs.iter().zip(raws) {
        let tx = replay::decode(raw, t).expect("decode");
        let out = ex.execute_candidate(&tx, raw).expect("run transaction");
        ex.commit_candidate(out).expect("commit");
    }
    ex.receipts().len()
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
        bench_scenario(c, name, scenario, "success", true);
    }
    let (_, mut failed) =
        bench_workloads::workloads().into_iter().find(|(name, _)| *name == "deposit").unwrap();
    // Replace STOP with INVALID so the slot written before the halt is returned as a read.
    let code = &mut failed.accounts.iter_mut().find(|a| a.address == A).unwrap().code;
    *code.last_mut().unwrap() = 0xfe;
    bench_scenario(c, "failed_deposit", failed, "halt", false);
    bench_scenario(
        c,
        "protocol_caller",
        scenarios::by_name("edge_protocol_caller_legacy").unwrap(),
        "success",
        false,
    );
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
