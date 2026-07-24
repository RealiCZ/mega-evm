//! REX6 checkpoint compute-gas accounting (prototype).
//!
//! Under checkpoint accounting, plain opcodes run the raw revm instructions with no per-opcode
//! recording; compute gas settles as an interpreter-gas delta at each checkpoint (storage-gas
//! opcodes, CALL/CREATE family, volatile opcodes, frame entry/exit). These tests pin the two
//! sides of that trade:
//!
//! - **Precision invariant**: per-transaction accounting totals are bit-identical to per-opcode
//!   recording (the interpreter gas counter telescopes over the unwrapped segment).
//! - **Coarsened enforcement**: a limit crossing inside a plain-opcode segment surfaces at the
//!   *next checkpoint*, not at the crossing opcode, so limit-exceeding transactions overshoot by up
//!   to one segment.

use crate::common::{transact, transact_default, CALLER, CONTRACT};
use alloy_primitives::{Bytes, U256};
use mega_evm::{
    test_utils::{BytecodeBuilder, MemoryDatabase},
    EvmTxRuntimeLimits, MegaSpecId,
};
use revm::bytecode::opcode::{POP, SSTORE, STOP, TIMESTAMP};

const ONE_ETH: u128 = 1_000_000_000_000_000_000;

fn db_with_code(code: Bytes) -> MemoryDatabase {
    MemoryDatabase::default()
        .account_balance(CALLER, U256::from(10 * ONE_ETH))
        .account_code(CONTRACT, code)
}

/// A countdown loop of cheap opcodes with no checkpoint inside the loop body:
///
/// ```text
/// PUSH2 iterations; loop: JUMPDEST; PUSH1 1; SWAP1; SUB; DUP1; PUSH1 loop; JUMPI; STOP
/// ```
///
/// Each iteration executes 7 plain opcodes for 26 gas. `prefix` is prepended verbatim and
/// participates in the jump-target offset.
fn countdown_loop_code_with_prefix(prefix: &[u8], iterations: u16) -> Bytes {
    let mut code = prefix.to_vec();
    code.push(0x61); // PUSH2
    code.extend_from_slice(&iterations.to_be_bytes());
    let loop_target = code.len() as u8;
    code.push(0x5b); // JUMPDEST
    code.extend_from_slice(&[0x60, 0x01]); // PUSH1 1
    code.push(0x90); // SWAP1
    code.push(0x03); // SUB
    code.push(0x80); // DUP1
    code.extend_from_slice(&[0x60, loop_target]); // PUSH1 loop
    code.push(0x57); // JUMPI
    code.push(0x00); // STOP
    Bytes::from(code)
}

fn countdown_loop_code(iterations: u16) -> Bytes {
    countdown_loop_code_with_prefix(&[], iterations)
}

/// A straight line of `pairs` PUSH1/POP pairs (5 gas, 2 plain opcodes each), then
/// `SSTORE(7, 99)` (the first checkpoint in the code), then STOP.
fn plain_run_then_sstore_code(pairs: usize) -> Bytes {
    let mut builder = BytecodeBuilder::default();
    for _ in 0..pairs {
        builder = builder.push_number(1u64);
        builder = builder.append(POP);
    }
    builder
        .push_u256(U256::from(99u64)) // value
        .push_u256(U256::from(7u64)) // slot
        .append(SSTORE)
        .append(STOP)
        .build()
}

/// Precision invariant on a plain-opcode hot loop: with no limit crossing, checkpoint
/// accounting (REX6) must produce the same compute-gas total and receipt `gas_used` as
/// per-opcode accounting (REX5) — the segment delta telescopes over exactly the opcodes the
/// per-opcode wrappers would have recorded, and the loop's only settlement is the frame-end
/// checkpoint.
#[test]
fn test_checkpoint_plain_loop_totals_match_rex5() {
    let code = countdown_loop_code(500);
    let r5 = transact_default(MegaSpecId::REX5, db_with_code(code.clone()));
    let r6 = transact_default(MegaSpecId::REX6, db_with_code(code));

    assert!(r5.is_success(), "REX5 loop must succeed: {:?}", r5.result);
    assert!(r6.is_success(), "REX6 loop must succeed: {:?}", r6.result);
    assert_eq!(
        r5.compute_gas, r6.compute_gas,
        "checkpoint totals must telescope to the per-opcode sum"
    );
    assert_eq!(r5.gas_used, r6.gas_used, "receipt gas must be unchanged");
}

/// A compute-gas crossing inside a plain-opcode segment surfaces at the next checkpoint.
///
/// The limit is placed mid-way through a straight plain-opcode run that ends in an SSTORE.
/// REX5 (per-opcode) halts at the crossing opcode; REX6 (checkpoint) runs the rest of the
/// segment and halts at the SSTORE settlement, recording the full segment — the same total a
/// generous-limit run records up to that point. Both halt; the checkpoint run overshoots.
#[test]
fn test_checkpoint_halt_lands_at_next_checkpoint() {
    let code = plain_run_then_sstore_code(200); // 200 * 5 = 1,000 gas of plain opcodes

    // Intrinsic compute gas (tx base cost) measured with a STOP-only contract.
    let intrinsic = transact_default(
        MegaSpecId::REX6,
        db_with_code(BytecodeBuilder::default().append(STOP).build()),
    )
    .compute_gas;

    // Trip the limit ~300 gas into the 1,000-gas plain run, well before the SSTORE.
    let compute_limit = intrinsic + 300;
    let limits =
        |spec| EvmTxRuntimeLimits::from_spec(spec).with_tx_compute_gas_limit(compute_limit);

    let r5 = transact(MegaSpecId::REX5, db_with_code(code.clone()), limits(MegaSpecId::REX5));
    let r6 = transact(MegaSpecId::REX6, db_with_code(code.clone()), limits(MegaSpecId::REX6));
    let r6_full = transact_default(MegaSpecId::REX6, db_with_code(code));

    assert!(!r5.is_success(), "REX5 must stop on the tight compute limit: {:?}", r5.result);
    assert!(!r6.is_success(), "REX6 must stop on the tight compute limit: {:?}", r6.result);

    // REX5 stops within one opcode of the crossing.
    assert!(
        r5.compute_gas <= compute_limit + 12,
        "REX5 must stop at the crossing opcode; compute={} limit={compute_limit}",
        r5.compute_gas
    );
    // REX6 records the whole segment up to (and including) the SSTORE checkpoint: identical
    // to what a run without the tight limit records at that point (SSTORE is the last
    // gas-consuming opcode, so the full-run total equals the at-checkpoint total).
    assert_eq!(
        r6.compute_gas, r6_full.compute_gas,
        "REX6 must settle the full segment at the checkpoint"
    );
    assert!(
        r6.compute_gas > r5.compute_gas,
        "checkpoint enforcement overshoots per-opcode enforcement; REX5={} REX6={}",
        r5.compute_gas,
        r6.compute_gas
    );
}

/// Pins the V1 checkpoint-set gap: a loop of plain opcodes contains **no checkpoint**, so a
/// detention cap crossing inside it is only enforced at the frame-end settlement — the
/// overshoot is bounded by the interpreter gas budget, NOT by the ~73k straight-line
/// code-size bound from the design spec ("a straight line cannot loop" only holds when
/// JUMP/JUMPI are checkpoints, which V1 deliberately excludes).
///
/// A ~14-byte contract overshoots the detention cap by >100k gas here. If the V1 set is kept
/// for the final spec, the overshoot bound must be stated as the remaining interpreter gas;
/// bounding it by code size requires JUMP/JUMPI checkpoints (V2) or another backstop.
#[test]
fn test_checkpoint_v1_loop_segment_is_not_code_size_bounded() {
    // TIMESTAMP marks volatile access (detention cap = usage + 1,000), then a 10,000-iteration
    // countdown loop (~260k gas) with no checkpoint inside.
    let code = countdown_loop_code_with_prefix(&[TIMESTAMP, POP], 10_000);

    let mut limits = EvmTxRuntimeLimits::from_spec(MegaSpecId::REX6);
    limits.block_env_access_compute_gas_limit = 1_000;

    let r5 = transact(MegaSpecId::REX5, db_with_code(code.clone()), {
        let mut l = EvmTxRuntimeLimits::from_spec(MegaSpecId::REX5);
        l.block_env_access_compute_gas_limit = 1_000;
        l
    });
    let r6 = transact(MegaSpecId::REX6, db_with_code(code), limits);

    assert!(!r5.is_success(), "REX5 must halt on the detention cap: {:?}", r5.result);
    assert!(!r6.is_success(), "REX6 must halt on the detention cap: {:?}", r6.result);

    // REX5 enforces within one opcode of the cap crossing (cap ≈ intrinsic + 1,000).
    assert!(
        r5.compute_gas < 30_000,
        "REX5 must stop near the detention cap; compute={}",
        r5.compute_gas
    );
    // REX6 runs the entire loop before any checkpoint settles: >100k gas past the cap from a
    // 16-byte contract — the overshoot is not bounded by code size under V1.
    assert!(
        r6.compute_gas > 100_000,
        "REX6 V1 loop segment must overshoot far past the code-size bound; compute={}",
        r6.compute_gas
    );
}
