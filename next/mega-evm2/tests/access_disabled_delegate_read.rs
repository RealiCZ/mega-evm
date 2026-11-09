//! In a frame whose volatile-data access is off, a call opcode reads its target's account and
//! code to find whether the target delegates to the block beneficiary, before it charges anything
//! (`SATIN-MAC-007`); that read is required: a failure of it gives the transaction no outcome
//! (`SATIN-VAL-010`), never a target taken as delegating nowhere. The specification's example: an
//! access-off `CALLCODE` with all seven operands, no value, a 1 MiB input range and 1,000 of
//! regular gas, whose code-source address delegates to the beneficiary, is refused for free when
//! the address can be read, and has no outcome — not a memory out-of-gas halt — when it cannot.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256, keccak256};
use evm2::{
    DatabaseError, Evm, ExecutionConfig,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Database, Db, InMemoryDB},
    interpreter::{InstrStop, Word},
};
use mega_evm2::{MegaSpec, MegaTypes, precompiles, satin_version, tx::registry};
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
/// `CALL(1_000, CHILD, 0, 0, 0, 0, 0)`, returning that call's success word.
fn caller() -> Vec<u8> {
    let mut c = vec![0x63];
    c.extend_from_slice(&keccak256("disableVolatileDataAccess()")[..4]);
    c.extend_from_slice(&[0x60, 0xe0, 0x1b, 0x60, 0x00, 0x52]);
    c.extend_from_slice(&[0x60, 0x00, 0x60, 0x00, 0x60, 0x04, 0x60, 0x00, 0x60, 0x00, 0x73]);
    c.extend_from_slice(ACCESS_CONTROL.as_slice());
    c.extend_from_slice(&[0x62, 0x01, 0x86, 0xa0, 0xf1, 0x50]);
    c.extend_from_slice(&[0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x73]);
    c.extend_from_slice(CHILD.as_slice());
    c.extend_from_slice(&[0x61, 0x03, 0xe8, 0xf1, 0x5f, 0x52, 0x60, 0x20, 0x5f, 0xf3]);
    c
}

/// `CALLCODE(GAS, SOURCE, 0, 0, 1 MiB, 0, 0)`; the refusal reverts with its own data, so the
/// caller reads the child's failure either way and the transaction's result tells them apart.
fn child() -> Vec<u8> {
    let mut c = vec![0x5f, 0x5f, 0x62, 0x10, 0x00, 0x00, 0x5f, 0x5f, 0x73];
    c.extend_from_slice(SOURCE.as_slice());
    c.extend_from_slice(&[0x5a, 0xf2, 0x00]);
    c
}

/// The transaction's outcome and the child's frame (its stop and the gas it left), or the error.
fn run(fail: bool, delegates: bool) -> Result<(bool, common::Frame), String> {
    let mut inner = InMemoryDB::default();
    inner.insert_account_info(&CALLER, code(caller()));
    inner.insert_account_info(&CHILD, code(child()));
    let source = if delegates { Bytecode::new_eip7702(BENEFICIARY) } else { Bytecode::default() };
    inner.insert_account_info(&SOURCE, AccountInfo::default().with_nonce(1).with_code(source));
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        beneficiary: BENEFICIARY,
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(Backend { inner, fail }),
        precompiles(),
    );
    evm.state_mut().set_read_absent_storage(true);
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
    evm.set_inspector(common::Frames::default());
    let result =
        evm.transact(&tx).map(|executed| executed.commit()).map_err(|e| format!("{e:?}"))?;
    let frames = evm.clear_inspector_as::<common::Frames>().expect("the inspector").frames;
    let child = *frames.iter().find(|f| f.gas_limit == 1_000).expect("the child's frame");
    Ok((result.status, child))
}

/// The delegate found, the `CALLCODE` is refused before its memory: the child reverts on the 984
/// of gas its eight pushes and `GAS` left. A source that delegates nowhere (control) is not
/// refused: the child halts on the 1 MiB memory expansion.
#[test]
fn test_a_call_whose_source_delegates_to_the_beneficiary_is_refused_before_its_memory() {
    let (status, child) = run(false, true).expect("an outcome");
    assert!(status);
    assert_eq!((child.stop, child.gas_left), (Some(InstrStop::Revert), 984));
    let (status, child) = run(false, false).expect("an outcome");
    assert!(status);
    assert_eq!(child.stop, Some(InstrStop::MemoryOOG));
}

/// The source's account cannot be read: the transaction has no outcome.
#[test]
fn test_an_unreadable_delegate_lookup_gives_the_transaction_no_outcome() {
    let error = run(true, true).expect_err("no outcome");
    assert!(error.contains("Unreadable"), "{error}");
}
