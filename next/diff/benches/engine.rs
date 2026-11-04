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
//! the system temp directory.

use alloy_primitives::Address;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use evm2::Evm;
use mega_evm2::{MegaTypes, block::BlockExecutor};
use mega_evm2_diff::{
    driver::{
        self, Frames,
        scenarios::{self, A, Asm, EMPTY, Limits, Scenario, acct, init_returning, op, scenario},
    },
    replay::{self, Evm2Db, blockdb},
};
use std::{cell::RefCell, collections::HashMap, hint::black_box, rc::Rc};

const CALL_LOOP_ITERATIONS: u64 = 200;
const DETAINED_LOOP_ITERATIONS: u64 = 1_000;
const CREATE_CODE_BYTES: u64 = 2_000;
const BLAKE2F_ROUNDS: u32 = 40_000;
const BLOCK_SET: &str = "p9";
const BLOCK_NUMBER: u64 = 26_400_004;

/// `<prefix>; PUSH2 n; JUMPDEST; <body>; PUSH1 1; SWAP1; SUB; DUP1; PUSH1 <JUMPDEST>; JUMPI; POP;
/// STOP`: `body` runs `n` times and must leave the stack as it found it.
fn counted_loop(prefix: &[u8], n: u64, body: &[u8]) -> Vec<u8> {
    let mut code = prefix.to_vec();
    code.extend_from_slice(&[0x61, (n >> 8) as u8, n as u8]);
    let jumpdest = code.len() as u8;
    code.push(op::JUMPDEST);
    code.extend_from_slice(body);
    code.extend_from_slice(&[
        op::PUSH1,
        1,
        op::SWAP1,
        op::SUB,
        op::DUP1,
        op::PUSH1,
        jumpdest,
        op::JUMPI,
        op::POP,
        op::STOP,
    ]);
    code
}

fn call_loop() -> Scenario {
    // CALL(gas, EMPTY, 0, 0, 0, 0, 0); POP
    let body = Asm::new()
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push_addr(EMPTY)
        .op(op::GAS)
        .op(op::CALL)
        .op(op::POP)
        .build();
    scenario(
        "bench_call_loop",
        vec![acct(A, counted_loop(&[], CALL_LOOP_ITERATIONS, &body))],
        5_000_000,
        Limits::default(),
    )
}

fn detained() -> Scenario {
    // body: 1 + 2, discarded
    let code = counted_loop(
        &[op::TIMESTAMP, op::POP],
        DETAINED_LOOP_ITERATIONS,
        &[op::PUSH1, 1, op::PUSH1, 2, op::ADD, op::POP],
    );
    scenario(
        "bench_detained",
        vec![acct(A, code)],
        5_000_000,
        Limits { block_env_cap: Some(100_000), ..Limits::default() },
    )
}

fn sstore() -> Scenario {
    let mut code = Asm::new();
    for slot in 1..=8 {
        code = code.sstore(slot, 1).sstore(slot, 0).sstore(slot, 1);
    }
    scenario("bench_sstore", vec![acct(A, code.stop().build())], 10_000_000, Limits::default())
}

fn create() -> Scenario {
    let code = Asm::new().create(&init_returning(CREATE_CODE_BYTES), 0).op(op::POP).stop().build();
    scenario("bench_create", vec![acct(A, code)], 50_000_000, Limits::default())
}

fn precompile() -> Scenario {
    let mut input = vec![0u8; 213];
    input[..4].copy_from_slice(&BLAKE2F_ROUNDS.to_be_bytes());
    let blake2f = Address::with_last_byte(9);
    let code = scenarios::q8::mem_call(
        Asm::new().op(op::NUMBER).op(op::POP),
        op::CALL,
        blake2f,
        0,
        3_000_000,
        &input,
    )
    .op(op::POP)
    .stop()
    .build();
    scenario(
        "bench_precompile",
        vec![acct(A, code)],
        10_000_000,
        Limits { block_env_cap: Some(25_000), ..Limits::default() },
    )
}

fn deposit() -> Scenario {
    let mut s = scenario(
        "bench_deposit",
        vec![acct(A, Asm::new().sstore(1, 1).stop().build())],
        1_000_000,
        Limits::default(),
    );
    s.deposit = Some(0);
    s
}

/// A workload runs once outside the measurement and must have succeeded.
fn check(s: &Scenario) {
    let out = driver::run(s, false);
    assert_eq!(out["status"], "success", "{} did not succeed: {out}", s.name);
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

/// The recorded block and the 2718 bytes of its transactions, unpacked once.
struct RecordedBlock {
    path: String,
    raws: Vec<alloy_primitives::Bytes>,
}

fn unpack(from: String, to: &std::path::Path) {
    let status = std::process::Command::new("zstd")
        .args(["-q", "-d", "-f", "-o"])
        .arg(to)
        .arg(from)
        .status()
        .expect("run zstd");
    assert!(status.success(), "unpack the recorded block");
}

fn recorded_block() -> RecordedBlock {
    let dir = std::env::temp_dir().join("mega-evm2-bench-blocks");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let data = env!("CARGO_MANIFEST_DIR");
    let block = dir.join(format!("{BLOCK_SET}-{BLOCK_NUMBER}.json"));
    unpack(format!("{data}/data/real-blocks/{BLOCK_SET}/{BLOCK_NUMBER}.json.zst"), &block);
    // The replay reference holds each transaction's 2718 bytes, which the block does not.
    let replay = dir.join(format!("{BLOCK_SET}-{BLOCK_NUMBER}.jsonl"));
    unpack(format!("{data}/data/replay/{BLOCK_SET}/{BLOCK_NUMBER}.jsonl.zst"), &replay);
    let raws = std::fs::read_to_string(replay)
        .expect("replay lines")
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("replay line"))
        .filter(|l| l["kind"] == "tx")
        .map(|l| blockdb::bytes(&l["raw"]))
        .collect();
    RecordedBlock { path: block.to_str().expect("utf-8 path").to_owned(), raws }
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
    let rec = recorded_block();
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
    bench_scenario(c, "call_loop", call_loop());
    bench_scenario(c, "detained", detained());
    bench_scenario(c, "sstore", sstore());
    bench_scenario(c, "create", create());
    bench_scenario(c, "precompile", precompile());
    bench_scenario(c, "deposit", deposit());
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
