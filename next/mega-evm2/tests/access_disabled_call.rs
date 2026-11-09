//! In a frame whose volatile-data access `MegaAccessControl` switched off, a read of the block
//! beneficiary's account is refused before the opcode charges or checks anything, and costs
//! nothing (`SATIN-MAC-003`, `SATIN-MAC-004`): a CALL of an account whose EIP-7702 delegate is the
//! beneficiary is refused, its delegate found by reading the target, before its memory expansion
//! (`SATIN-MAC-007`), so one whose return range ends near 2^40 bytes (a memory cost whose
//! quadratic term does not fit in a `u64`) is refused rather than halting on the memory limit; and
//! an `EXTCODECOPY` of the beneficiary with a 1 MiB length on 1,000 of regular gas is refused
//! keeping all of its gas.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, Bytes, Signature, TxKind, U256, keccak256};
use evm2::{
    Evm, ExecutionConfig, TxResult,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    interpreter::InstrStop,
};
use mega_evm2::{MegaSpec, MegaTypes, precompiles, satin_version, tx::registry};
use op_evm2::OpTxEnvelope;

const SENDER: Address = Address::with_last_byte(0x50);
/// Switches its access off, then calls [`TARGET`].
const CALLER: Address = Address::with_last_byte(0xa0);
/// Delegates to [`BENEFICIARY`] (EIP-7702).
const TARGET: Address = Address::with_last_byte(0xb0);
const BENEFICIARY: Address = Address::with_last_byte(0xbe);
/// Satin's `MegaAccessControl` system contract.
const ACCESS_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04]);

/// `disableVolatileDataAccess()` through a CALL of 100,000 gas, then
/// `CALL(GAS, TARGET, 0, 0, 0, ret_offset, 1) STOP`.
fn caller_code(ret_offset: u64) -> Vec<u8> {
    let mut code = vec![0x63];
    code.extend_from_slice(&keccak256("disableVolatileDataAccess()")[..4]);
    // PUSH1 0xe0 SHL PUSH1 0 MSTORE; CALL(100_000, ACCESS_CONTROL, 0, 0, 4, 0, 0)
    code.extend_from_slice(&[0x60, 0xe0, 0x1b, 0x60, 0x00, 0x52]);
    code.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0x60, 0x04, 0x60, 0x00, 0x60, 0x00, 0x73]);
    code.extend_from_slice(ACCESS_CONTROL.as_slice());
    code.extend_from_slice(&[0x62, 0x01, 0x86, 0xa0, 0xf1, 0x50]);
    // CALL(GAS, TARGET, 0, 0, 0, ret_offset, 1) STOP
    code.extend_from_slice(&[0x60, 0x01, 0x67]);
    code.extend_from_slice(&ret_offset.to_be_bytes());
    code.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0x60, 0x00, 0x73]);
    code.extend_from_slice(TARGET.as_slice());
    code.extend_from_slice(&[0x5a, 0xf1, 0x00]);
    code
}

/// `disableVolatileDataAccess()`, then `CALL(1_000, CHILD, 0, 0, 0, 0, 0) STOP`.
fn caller_of_child() -> Vec<u8> {
    let mut code = caller_code(0);
    // Drop the tail CALL of TARGET: 3 + 8 + 7 + 20 + 3 bytes.
    code.truncate(code.len() - 41);
    code.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0x60, 0x00, 0x60, 0x00, 0x60, 0x00, 0x73]);
    code.extend_from_slice(CHILD.as_slice());
    code.extend_from_slice(&[0x61, 0x03, 0xe8, 0xf1, 0x00]);
    code
}

/// Runs `EXTCODECOPY(beneficiary, 0, 0, 1 MiB)` once its pushes (10 gas) are paid.
const CHILD: Address = Address::with_last_byte(0xc0);

fn satin_evm(db: InMemoryDB) -> Evm<'static, MegaTypes> {
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(10_000_000_000u64),
        beneficiary: BENEFICIARY,
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(db),
        precompiles(),
    );
    evm.state_mut().set_read_absent_storage(true);
    evm
}

/// Runs a transaction to [`CALLER`] whose CALL of [`TARGET`] returns into `ret_offset`.
fn run(ret_offset: u64) -> TxResult<MegaTypes> {
    let mut db = InMemoryDB::default();
    let code = Bytecode::new_legacy(caller_code(ret_offset).into());
    db.insert_account_info(&CALLER, AccountInfo::default().with_code(code));
    db.insert_account_info(
        &TARGET,
        AccountInfo::default().with_code(Bytecode::new_eip7702(BENEFICIARY)),
    );
    let tx = TxLegacy {
        chain_id: Some(1),
        nonce: 0,
        gas_price: 0,
        gas_limit: 1_000_000,
        to: TxKind::Call(CALLER),
        value: U256::ZERO,
        input: Bytes::new(),
    };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let tx = Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER);
    match satin_evm(db).transact(&tx) {
        Ok(executed) => executed.commit(),
        Err(error) => panic!("transaction refused: {error:?}"),
    }
}

#[test]
fn test_call_of_a_beneficiary_delegate_is_refused_when_access_is_off() {
    let result = run(0);
    assert_eq!(result.stop, InstrStop::Revert, "{result:?}");
    assert_eq!(result.output, refusal(10), "the beneficiary access is refused");
}

fn refusal(access: u64) -> Bytes {
    let mut refusal = keccak256("VolatileDataAccessDisabled(uint8)")[..4].to_vec();
    refusal.extend_from_slice(&U256::from(access).to_be_bytes::<32>());
    refusal.into()
}

/// The refusal comes before the CALL's memory expansion: a return range ending near 2^40 bytes is
/// refused rather than halting on the memory limit.
#[test]
fn test_call_with_a_2_40_return_range_is_refused_before_its_memory_when_access_is_off() {
    let result = run(1 << 40);
    assert_eq!(result.stop, InstrStop::Revert, "{result:?}");
    assert_eq!(result.output, refusal(10));
}

/// `SATIN-MAC-004`'s expected result: with access off, an `EXTCODECOPY(beneficiary, 0, 0, 1 MiB)`
/// on 1,000 of regular gas reverts with `VolatileDataAccessDisabled(10)` keeping the gas it held
/// at the opcode, the 990 its pushes left, though it could pay neither the opcode's static gas and
/// copy nor its memory.
#[test]
fn test_a_refused_read_costs_nothing() {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &CALLER,
        AccountInfo::default().with_code(Bytecode::new_legacy(caller_of_child().into())),
    );
    let mut child = vec![0x62, 0x10, 0x00, 0x00, 0x5f, 0x5f, 0x73];
    child.extend_from_slice(BENEFICIARY.as_slice());
    child.extend_from_slice(&[0x3c, 0x00]);
    db.insert_account_info(
        &CHILD,
        AccountInfo::default().with_code(Bytecode::new_legacy(child.into())),
    );
    let tx = TxLegacy {
        chain_id: Some(1),
        nonce: 0,
        gas_price: 0,
        gas_limit: 1_000_000,
        to: TxKind::Call(CALLER),
        value: U256::ZERO,
        input: Bytes::new(),
    };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let tx = Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER);
    let mut evm = satin_evm(db);
    evm.set_inspector(common::Frames::default());
    let result = evm.transact(&tx).expect("a valid call").commit();
    assert!(result.status, "{result:?}");
    let frames = evm.clear_inspector_as::<common::Frames>().expect("the inspector").frames;
    let child = frames.iter().find(|f| f.depth == 1 && f.gas_limit == 1_000).expect("the child");
    assert_eq!(child.stop, Some(InstrStop::Revert));
    assert_eq!(child.gas_left, 990);
}
