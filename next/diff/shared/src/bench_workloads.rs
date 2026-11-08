//! Fixed interpreter workloads used unchanged by both benchmark workspaces.

use alloy_primitives::Address;
use mega_evm2_scenarios::{
    self as scenarios, A, Asm, EMPTY, Limits, Scenario, acct, init_returning, op, scenario,
};

const CALL_LOOP_ITERATIONS: u64 = 200;
const DETAINED_LOOP_ITERATIONS: u64 = 1_000;
const CREATE_CODE_BYTES: u64 = 2_000;
const BLAKE2F_ROUNDS: u32 = 40_000;

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

/// Returns the shared six synthetic workloads in a stable order.
pub fn workloads() -> Vec<(&'static str, Scenario)> {
    vec![
        ("call_loop", call_loop()),
        ("detained", detained()),
        ("sstore", sstore()),
        ("create", create()),
        ("precompile", precompile()),
        ("deposit", deposit()),
    ]
}
