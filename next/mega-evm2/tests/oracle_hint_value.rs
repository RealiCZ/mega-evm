//! A `sendHint` call is admitted whether it carries value or not (`SATIN-ORCL-006`): forwarded
//! some gas from a frame whose volatile-data access is on, its hint is counted on the transaction
//! and forwarded to the oracle service, a value-bearing call's included, though the Oracle's
//! non-payable method then reverts it. A value call's stipend is not forwarded gas; a
//! transaction's own `sendHint` has no stipend, and is forwarded its first frame's whole gas.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{B256, Bytes, Signature, TxKind, U256, hex, keccak256};
use common::*;
use evm2::evm::{AccountInfo, InMemoryDB};
use mega_evm2::{ORACLE, limit_exceeded_output};
use op_evm2::{OpTxEnvelope, TxDeposit};

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

/// TOP copies the hint's input into memory and calls the Oracle with it, `value` wei and `gas`
/// (at most 65,535); the Oracle's code reverts when it receives value, as its non-payable method
/// does.
fn hint_call(value: u8, gas: u16) -> (InMemoryDB, usize) {
    let input = send_hint();
    let len = input.len() as u8;
    let mut code = vec![op::PUSH1, len, op::PUSH1, 0, op::PUSH0, 0x39 /* CODECOPY */];
    // CALL(gas, ORACLE, value, 0, len, 0, 0)
    code.extend([op::PUSH0, op::PUSH0, op::PUSH1, len, op::PUSH0, op::PUSH1, value, op::PUSH20]);
    code.extend_from_slice(ORACLE.as_slice());
    code.push(op::PUSH2);
    code.extend(gas.to_be_bytes());
    code.extend([op::CALL, op::STOP]);
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
        let (db, len) = hint_call(value, 50_000);
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

/// A call forwarded no gas is not admitted, with value or without: a value call's 2,300 stipend is
/// not forwarded gas (`SATIN-DET-010`), so a `sendHint` sent with value and no gas runs the
/// Oracle's code on its stipend alone and counts nothing.
#[test]
fn test_a_hint_forwarded_no_gas_is_not_admitted_whatever_its_value() {
    for value in [0u8, 1] {
        let (db, _) = hint_call(value, 0);
        let mut evm = satin_evm(db);
        let r = run_on(&mut evm, TxKind::Call(TOP), U256::ZERO, Bytes::new(), 1_000_000);
        assert!(r.success, "{value}");
        let m = &evm.ext().mechanisms;
        assert_eq!(m.hint_bytes, 0, "no hint counted, value {value}");
        assert!(m.hints.is_empty(), "no hint forwarded, value {value}");
    }
}

/// The Oracle's deployed code, version 2.0.0, whose `sendHint` is not payable.
fn real_oracle() -> AccountInfo {
    contract(&hex::decode(include_str!("fixtures/Oracle-2.0.0.hex").trim()).unwrap())
}

/// `sendHint(bytes32(7), bytes)` with 64 bytes of `0xab`: 164 bytes.
fn send_hint_64() -> Vec<u8> {
    let mut input = keccak256("sendHint(bytes32,bytes)")[..4].to_vec();
    input.extend_from_slice(&U256::from(7).to_be_bytes::<32>());
    input.extend_from_slice(&U256::from(0x40).to_be_bytes::<32>());
    input.extend_from_slice(&U256::from(64).to_be_bytes::<32>());
    input.extend_from_slice(&[0xab; 64]);
    input
}

/// What a transaction whose own call is a `sendHint` left.
struct TopHint {
    status: bool,
    output: Bytes,
    /// The regular gas of the transaction's first frame, the Oracle's.
    frame_gas: u64,
    data_size: u64,
    hints: usize,
    hint_bytes: u64,
    stop: Option<(u8, u64)>,
}

/// A transaction from [`SENDER`] (10 wei) to the Oracle running `oracle`, with 1 wei, `input` and
/// `gas_limit`: a legacy transaction, or a deposit minting nothing; the transaction's data-size
/// limit `data_limit` when given.
fn top_hint(
    oracle: AccountInfo,
    deposit: bool,
    input: &[u8],
    gas_limit: u64,
    data_limit: Option<u64>,
) -> TopHint {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&SENDER, AccountInfo::default().with_balance(U256::from(10)));
    db.insert_account_info(&ORACLE, oracle);
    let mut evm = satin_evm(db);
    if let Some(limit) = data_limit {
        evm.ext_mut().limits.tx[0] = limit;
    }
    evm.set_inspector(Frames::default());
    let input = Bytes::copy_from_slice(input);
    let tx = if deposit {
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(ORACLE),
            mint: 0,
            value: U256::from(1),
            gas_limit,
            is_system_transaction: false,
            input,
        })
    } else {
        let tx = TxLegacy {
            chain_id: Some(1),
            nonce: 0,
            gas_price: 0,
            gas_limit,
            to: TxKind::Call(ORACLE),
            value: U256::from(1),
            input,
        };
        OpTxEnvelope::from_signed(tx.into_signed(Signature::new(
            U256::from(1),
            U256::from(1),
            false,
        )))
    };
    let result =
        evm.transact(&Recovered::new_unchecked(tx, SENDER)).expect("a valid transaction").commit();
    let frames = evm.clear_inspector_as::<Frames>().expect("the inspector").frames;
    let ext = evm.ext();
    TopHint {
        status: result.status,
        output: result.output.clone(),
        frame_gas: frames.iter().find(|f| f.depth == 0).expect("the first frame").gas_limit,
        data_size: ext.usage(result.status)[0],
        hints: ext.mechanisms.hints.len(),
        hint_bytes: ext.mechanisms.hint_bytes,
        stop: ext.top_stop,
    }
}

/// The gas a transaction of [`top_hint`] is charged before its first frame: the gas limit less the
/// first frame's gas, read from a run with plenty of gas.
fn before_first_frame(oracle: AccountInfo, deposit: bool, input: &[u8]) -> u64 {
    1_000_000 - top_hint(oracle, deposit, input, 1_000_000, None).frame_gas
}

/// A `sendHint` that is the transaction's own call, carrying 1 wei, is forwarded its first frame's
/// whole regular gas — a transaction's frame receives no stipend (`SATIN-FWD-003`) — so with any
/// positive frame gas it is admitted, counted and forwarded (`SATIN-ORCL-006`, `-007`, `-009`),
/// as a legacy transaction and as a deposit (`SATIN-ORCL-005`). The Oracle's code is a `STOP`, as
/// in the case found: the input of `sendHint(0x11…11, 0xaa)`, 132 bytes, then counts once in the
/// body (310 + 132 = 442) and once as the hint, with the transfer's records: 774 bytes of data
/// size. Before its first frame a legacy transaction pays its intrinsic gas (21,996), its body's
/// history (442 × 88 = 38,896) and the transfer's record (40 × 88 = 3,520): 64,412.
#[test]
fn test_a_transactions_own_value_hint_is_forwarded_its_first_frames_gas() {
    let input = send_hint();
    let stop_code = || contract(&[op::STOP]);
    assert_eq!(before_first_frame(stop_code(), false, &input), 64_412);
    for deposit in [false, true] {
        let pre = before_first_frame(stop_code(), deposit, &input);
        for frame_gas in [1, 2_000, 2_300, 2_301] {
            let r = top_hint(stop_code(), deposit, &input, pre + frame_gas, None);
            let at = format!("deposit {deposit}, frame gas {frame_gas}");
            assert_eq!(r.frame_gas, frame_gas, "{at}");
            assert!(r.status, "{at}");
            assert_eq!(r.hints, 1, "the hint forwarded, {at}");
            assert_eq!(r.hint_bytes, input.len() as u64, "{at}");
            assert_eq!(r.data_size, 774, "the hint counted, {at}");
        }
    }
}

/// The same with the Oracle's own code, whose `sendHint` is not payable, and a 164-byte input: the
/// hint is admitted, counted and forwarded before the bytecode runs, and stays counted after the
/// bytecode reverts the call (or, on one gas, runs out of it) and with it the transaction — the
/// transfer's records undone, its data size the body (310 + 164) and the hint (164): 638 bytes, a
/// failed deposit's included (`SATIN-DEP-006`).
#[test]
fn test_a_transactions_own_value_hint_to_the_oracles_code_stays_counted_after_its_revert() {
    let input = send_hint_64();
    for deposit in [false, true] {
        let pre = before_first_frame(real_oracle(), deposit, &input);
        for frame_gas in [1, 2_000, 2_300, 2_301] {
            let r = top_hint(real_oracle(), deposit, &input, pre + frame_gas, None);
            let at = format!("deposit {deposit}, frame gas {frame_gas}");
            assert_eq!(r.frame_gas, frame_gas, "{at}");
            assert!(!r.status, "the non-payable method fails the call, {at}");
            assert_eq!(r.hints, 1, "the hint forwarded, {at}");
            assert_eq!(r.hint_bytes, 164, "{at}");
            assert_eq!(r.data_size, 638, "the hint kept, {at}");
            assert_eq!(r.stop, None, "{at}");
        }
    }
}

/// A transaction's own value `sendHint` whose input would take the data size over the limit is
/// answered with the stop, before the Oracle's code runs, its first frame's gas untouched, and is
/// neither counted nor forwarded (`SATIN-ORCL-010`): the body's 474 bytes and the hint's 164 cross
/// a limit of 600.
#[test]
fn test_a_transactions_own_value_hint_over_the_data_size_limit_is_stopped() {
    let input = send_hint_64();
    for deposit in [false, true] {
        let pre = before_first_frame(real_oracle(), deposit, &input);
        let at = format!("deposit {deposit}");
        let r = top_hint(real_oracle(), deposit, &input, pre + 2_000, Some(600));
        assert!(!r.status, "{at}");
        assert_eq!(r.output.as_ref(), limit_exceeded_output(0, 600).as_slice(), "{at}");
        assert_eq!(r.stop, Some((0, 600)), "{at}");
        assert_eq!((r.hints, r.hint_bytes), (0, 0), "nothing counted or forwarded, {at}");
        assert_eq!(r.data_size, 474, "{at}");
        // Well below the limit the hint is admitted.
        let r = top_hint(real_oracle(), deposit, &input, pre + 2_000, Some(1_000));
        assert_eq!((r.hints, r.data_size, r.stop), (1, 638, None), "{at}");
    }
}
