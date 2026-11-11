//! A CREATE2 short of its salt makes Satin's checks in their order before it fails on the missing
//! salt (`SATIN-SIZE-005`, `SATIN-SIZE-006`): the static context first, and the offset's range only
//! for initcode that is not empty. The charges, which under gas detention decide the outcome, are
//! held to the reference's figures in `diff/tests/create2_salt_order.rs`.

mod common;

use alloy_primitives::{Bytes, TxKind, U256};
use common::*;
use evm2::{evm::InMemoryDB, interpreter::InstrStop};

const CREATE2: u8 = 0xf5;

/// `CREATE2` on three stack items: `len`, `offset` and a zero value, no salt.
fn short_create2(len: u8, offset: [u8; 32]) -> Vec<u8> {
    let mut code = vec![op::PUSH1, len, op::PUSH32];
    code.extend(offset);
    code.extend([op::PUSH0, CREATE2, op::STOP]);
    code
}

/// Runs a transaction to [`TOP`] running `top`, with [`C`] running `c`.
fn run_top(top: &[u8], c: &[u8]) -> Run {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&TOP, contract(top));
    db.insert_account_info(&C, contract(c));
    run_with(db, TxKind::Call(TOP), U256::ZERO, Bytes::new(), 1_000_000)
}

/// In a static frame the creation fails on the static context, before its operands are read.
#[test]
fn test_a_static_create2_short_of_its_salt_fails_on_the_static_context() {
    // STATICCALL(GAS, C, 0, 0, 0, 0)
    let mut top = vec![op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH20];
    top.extend_from_slice(C.as_slice());
    top.extend([op::GAS, op::STATICCALL, op::STOP]);
    let r = run_top(&top, &short_create2(32, [0; 32]));
    assert_eq!(r.frame(1).stop, Some(InstrStop::StateChangeDuringStaticCall));
}

/// Empty initcode reads no memory, so an offset out of range is not checked and the creation
/// fails on the missing salt; with one byte of initcode the same offset fails first.
#[test]
fn test_an_empty_create2_short_of_its_salt_checks_no_offset() {
    let r = run_top(&short_create2(0, [0xff; 32]), &[op::STOP]);
    assert_eq!(r.stop, InstrStop::StackUnderflow);
    let r = run_top(&short_create2(1, [0xff; 32]), &[op::STOP]);
    assert_eq!(r.stop, InstrStop::InvalidOperandOOG);
}
