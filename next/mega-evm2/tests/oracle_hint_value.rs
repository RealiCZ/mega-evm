//! A `sendHint` call is admitted whether it carries value or not (`SATIN-ORCL-006`): forwarded
//! some gas from a frame whose volatile-data access is on, its hint is counted on the transaction
//! and forwarded to the oracle service, a value-bearing call's included, though the Oracle's
//! non-payable method then reverts it.

mod common;

use alloy_primitives::{B256, Bytes, TxKind, U256, keccak256};
use common::*;
use evm2::evm::InMemoryDB;
use mega_evm2::ORACLE;

/// `sendHint(bytes32 topic, bytes data)` with a one-byte `data`.
fn send_hint() -> Vec<u8> {
    let mut input = keccak256("sendHint(bytes32,bytes)")[..4].to_vec();
    input.extend_from_slice(B256::repeat_byte(0x11).as_slice());
    input.extend_from_slice(&U256::from(0x40).to_be_bytes::<32>());
    input.extend_from_slice(&U256::from(1).to_be_bytes::<32>());
    let mut data = [0u8; 32];
    data[0] = 0xaa;
    input.extend_from_slice(&data);
    input
}

/// TOP copies the hint's input into memory and calls the Oracle with it, `value` wei and 50,000
/// of gas; the Oracle's code reverts when it receives value, as its non-payable method does.
fn hint_call(value: u8) -> (InMemoryDB, usize) {
    let input = send_hint();
    let len = input.len() as u8;
    let mut code = vec![op::PUSH1, len, op::PUSH1, 0, op::PUSH0, 0x39 /* CODECOPY */];
    // CALL(gas, ORACLE, value, 0, len, 0, 0)
    code.extend([op::PUSH0, op::PUSH0, op::PUSH1, len, op::PUSH0, op::PUSH1, value, op::PUSH20]);
    code.extend_from_slice(ORACLE.as_slice());
    code.extend([op::PUSH2, 0xc3, 0x50, op::CALL, op::STOP]);
    let start = code.len() as u8;
    code[3] = start;
    code.extend_from_slice(&input);
    let mut db = InMemoryDB::default();
    let mut top = contract(&code);
    top.balance = U256::from(10);
    db.insert_account_info(&TOP, top);
    // CALLVALUE; ISZERO; PUSH1 8; JUMPI; PUSH0; PUSH0; REVERT; JUMPDEST; STOP.
    let oracle = [0x34, 0x15, op::PUSH1, 8, 0x57, op::PUSH0, op::PUSH0, op::REVERT, 0x5b, op::STOP];
    db.insert_account_info(&ORACLE, contract(&oracle));
    (db, input.len())
}

/// A value-bearing `sendHint` with positive gas, access on, is admitted, forwarded and counted;
/// its frame then reverts in the bytecode, the hint staying counted.
#[test]
fn test_a_hint_with_value_is_admitted_forwarded_and_counted() {
    for value in [0u8, 1] {
        let (db, len) = hint_call(value);
        let mut evm = satin_evm(db);
        let r = run_on(&mut evm, TxKind::Call(TOP), U256::ZERO, Bytes::new(), 1_000_000);
        assert!(r.success, "{value}");
        let oracle_frame = r.frame(1);
        let expected = if value == 0 {
            evm2::interpreter::InstrStop::Stop
        } else {
            evm2::interpreter::InstrStop::Revert
        };
        assert_eq!(oracle_frame.stop, Some(expected), "{value}");
        let m = &evm.ext().mechanisms;
        assert_eq!(m.hint_bytes, len as u64, "the hint's bytes, value {value}");
        assert_eq!(m.hints.len(), 1, "the hint forwarded, value {value}");
        assert_eq!(m.hints[0].1, B256::repeat_byte(0x11));
        assert_eq!(m.hints[0].2.as_ref(), &[0xaa]);
    }
}
