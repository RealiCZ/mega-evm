//! In a frame whose volatile-data access is off, a call opcode reads its target's account and
//! code only at the target's load, after its earlier steps, and is refused at its delegate's load
//! when that delegate is the block beneficiary (`SATIN-MAC-007`); a load it makes is required: a
//! failure of it gives the transaction no outcome (`SATIN-VAL-010`), never a target taken as
//! delegating nowhere. The specification's example: an access-off `CALLCODE` with all seven
//! operands, no value, a 1 MiB input range and 1,000 of regular gas, whose code-source address
//! delegates to the beneficiary, halts out of gas on its memory on every node and reads nothing,
//! whether or not the node could read that address.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256, keccak256};
use evm2::{
    DatabaseError, Evm,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Database, Db, InMemoryDB},
    interpreter::Word,
};
use mega_evm2::MegaTypes;
use op_evm2::OpTxEnvelope;

const SENDER: Address = Address::with_last_byte(0x50);
const CALLER: Address = Address::with_last_byte(0xa0);
/// Runs the `CALLCODE` on 1,000 of gas, its access off.
const CHILD: Address = Address::with_last_byte(0xc0);
/// The `CALLCODE`'s code-source address, which delegates to [`BENEFICIARY`].
const SOURCE: Address = Address::with_last_byte(0xb0);
const BENEFICIARY: Address = Address::with_last_byte(0xbe);
const ACCESS_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04]);

#[derive(Debug)]
struct Unreadable;

impl core::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("account unreadable")
    }
}

impl std::error::Error for Unreadable {}

/// An in-memory database whose read of [`SOURCE`]'s account fails when `fail` is set.
#[derive(Debug)]
struct Backend {
    inner: InMemoryDB,
    fail: bool,
}

impl Database for Backend {
    type Error = DatabaseError;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        if self.fail && *address == SOURCE {
            return Err(DatabaseError::new(Unreadable, false));
        }
        self.inner.get_account(address)
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        self.inner.get_code_by_hash(code_hash)
    }

    fn get_storage(&mut self, address: &Address, key: &Word) -> Result<Word, Self::Error> {
        self.inner.get_storage(address, key)
    }

    fn get_block_hash(&mut self, number: &Word) -> Result<B256, Self::Error> {
        self.inner.get_block_hash(number)
    }
}

fn code(c: Vec<u8>) -> AccountInfo {
    AccountInfo::default().with_code(Bytecode::new_legacy(c.into()))
}

/// `disableVolatileDataAccess()` through a CALL of 100,000 gas, then
/// `CALL(child_gas, CHILD, 0, 0, 0, 0, 0)`, returning that call's success word.
fn caller(child_gas: u16) -> Vec<u8> {
    let mut c = vec![0x63];
    c.extend_from_slice(&keccak256("disableVolatileDataAccess()")[..4]);
    c.extend_from_slice(&[0x60, 0xe0, 0x1b, 0x60, 0x00, 0x52]);
    c.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0x60, 0x04, 0x60, 0x00, 0x60, 0x00, 0x73]);
    c.extend_from_slice(ACCESS_CONTROL.as_slice());
    c.extend_from_slice(&[0x62, 0x01, 0x86, 0xa0, 0xf1, 0x50]);
    c.extend_from_slice(&[0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x73]);
    c.extend_from_slice(CHILD.as_slice());
    c.push(0x61);
    c.extend_from_slice(&child_gas.to_be_bytes());
    c.extend_from_slice(&[0xf1, 0x5f, 0x52, 0x60, 0x20, 0x5f, 0xf3]);
    c
}

/// `CALLCODE(GAS, SOURCE, 0, 0, input, 0, 0)` with an input range of `input` bytes (at most 1
/// MiB); the caller reads the child's failure either way and the transaction's result tells
/// them apart.
fn child(input: u32) -> Vec<u8> {
    let mut c = vec![0x5f, 0x5f, 0x62];
    c.extend_from_slice(&input.to_be_bytes()[1..]);
    c.extend_from_slice(&[0x5f, 0x5f, 0x73]);
    c.extend_from_slice(SOURCE.as_slice());
    c.extend_from_slice(&[0x5a, 0xf2, 0x00]);
    c
}

/// The specification's example: a 1 MiB input range on 1,000 of gas.
const MIB: u32 = 0x10_0000;

/// A Satin EVM over `backend`, the block's beneficiary [`BENEFICIARY`].
fn evm_over(backend: Backend) -> Evm<'static, MegaTypes> {
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        beneficiary: BENEFICIARY,
        ..BlockEnv::<MegaTypes>::default()
    };
    common::evm(block, Db::new(backend))
}

/// The transaction: a call of [`CALLER`] from the sender.
fn tx() -> Recovered<OpTxEnvelope> {
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
    Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER)
}

/// The transaction's outcome and the child's frame (its stop and the gas it left), or the error;
/// the child given `child_gas` and an input range of `input` bytes.
fn run(
    fail: bool,
    delegates: bool,
    child_gas: u16,
    input: u32,
) -> Result<(bool, common::Frame), String> {
    let mut inner = InMemoryDB::default();
    inner.insert_account_info(&CALLER, code(caller(child_gas)));
    inner.insert_account_info(&CHILD, code(child(input)));
    let source = if delegates { Bytecode::new_eip7702(BENEFICIARY) } else { Bytecode::default() };
    inner.insert_account_info(&SOURCE, AccountInfo::default().with_nonce(1).with_code(source));
    let mut evm = evm_over(Backend { inner, fail });
    let tx = tx();
    evm.set_inspector(common::Frames::default());
    let result =
        evm.transact(&tx).map(|executed| executed.commit()).map_err(|e| format!("{e:?}"))?;
    let frames = evm.clear_inspector_as::<common::Frames>().expect("the inspector").frames;
    let child =
        *frames.iter().find(|f| f.gas_limit == u64::from(child_gas)).expect("the child's frame");
    Ok((result.status, child))
}

/// The specification's example: the `CALLCODE` makes its memory expansion before its target's
/// load, and halts on it, whether its source delegates to the beneficiary or delegates nowhere.
#[test]
fn test_a_call_whose_source_delegates_to_the_beneficiary_halts_on_its_memory_first() {
    for delegates in [true, false] {
        let (status, child) = run(false, delegates, 1_000, MIB).expect("an outcome");
        assert!(status);
        assert!(child.stop.expect("the child ended").is_halt(), "{child:?}");
        assert_eq!(child.gas_left, 0);
    }
}

/// The call halted on its memory reads nothing: the source's account is in no state the
/// transaction returns.
#[test]
fn test_a_call_halted_on_its_memory_reads_nothing() {
    let mut inner = InMemoryDB::default();
    inner.insert_account_info(&CALLER, code(caller(1_000)));
    inner.insert_account_info(&CHILD, code(child(MIB)));
    inner.insert_account_info(
        &SOURCE,
        AccountInfo::default().with_nonce(1).with_code(Bytecode::new_eip7702(BENEFICIARY)),
    );
    let mut evm = evm_over(Backend { inner, fail: false });
    let out = evm.transact(&tx()).expect("an outcome").detach();
    assert!(out.result.status);
    assert!(out.pending_state.account_changes_for(&SOURCE).is_none(), "the source was not read");
}

/// The source's account cannot be read: the call of the example never reads it, so the
/// transaction has the same outcome; a call that reaches the source's load (no input range,
/// 10,000 of gas) has none.
#[test]
fn test_an_unreadable_source_fails_the_transaction_only_when_its_load_is_reached() {
    let (status, child) = run(true, true, 1_000, MIB).expect("an outcome");
    assert!(status);
    assert!(child.stop.expect("the child ended").is_halt());
    let error = run(true, true, 10_000, 0).expect_err("no outcome");
    assert!(error.contains("Unreadable"), "{error}");
}
