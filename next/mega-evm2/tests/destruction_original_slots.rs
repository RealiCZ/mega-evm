//! The destruction table's rows for a slot whose original value is not zero: a creation can land
//! on an address with nonce zero, no code and storage, and a slot of it that the transaction loaded
//! before the creation keeps the value it held then as its original. Its write record stands when
//! the account is destroyed, and the deletion makes that record when the slot still holds its
//! original; a slot the transaction never loaded makes none.
//!
//! No instruction loads such a slot (the account has no code to run in before the creation, and
//! an access list only pre-warms), so the test loads it as a tool does: an inspector reads it from
//! its `log` hook, at a LOG0 the factory emits right before its CREATE2.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, Log, Signature, TxKind, U256, keccak256};
use evm2::{
    Evm,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    inspector::Inspector,
};
use mega_evm2::{MegaTypes, WRITE_RECORD_SIZE};
use op_evm2::OpTxEnvelope;

const SENDER: Address = Address::with_last_byte(0x50);
const FACTORY: Address = Address::with_last_byte(0xf0);
const BENEFICIARY: Address = Address::with_last_byte(0xb0);
const SLOT: u64 = 5;
const SALT: u8 = 0x2a;

/// The constructor: `SSTORE(SLOT, 9)` when `writes`, then `SELFDESTRUCT(BENEFICIARY)`.
fn init_code(writes: bool) -> Vec<u8> {
    let mut code = Vec::new();
    if writes {
        code.extend_from_slice(&[0x60, 0x09, 0x60, SLOT as u8, 0x55]);
    }
    code.push(0x73);
    code.extend_from_slice(BENEFICIARY.as_slice());
    code.push(0xff);
    code
}

/// `LOG0(0, 0)`, then `CREATE2(0, 0, len(init), SALT)` of `init` stored at memory 0, then STOP.
fn factory_code(init: &[u8]) -> Vec<u8> {
    let mut code = vec![0x5f, 0x5f, 0xa0];
    for (i, byte) in init.iter().enumerate() {
        // PUSH1 byte PUSH2 i MSTORE8
        code.extend_from_slice(&[0x60, *byte, 0x61, (i >> 8) as u8, i as u8, 0x53]);
    }
    code.extend_from_slice(&[0x60, SALT, 0x60, init.len() as u8, 0x5f, 0x5f, 0xf5, 0x50, 0x00]);
    code
}

fn created(init: &[u8]) -> Address {
    FACTORY.create2(B256::with_last_byte(SALT), keccak256(init))
}

/// Loads `SLOT` of the created address into the transaction's state at the factory's LOG0.
struct ToolLoad(Address);

impl Inspector<MegaTypes> for ToolLoad {
    fn log(&mut self, _log: &Log, host: &mut Evm<'_, MegaTypes>) {
        host.state_mut().storage_slot(&self.0, U256::from(SLOT)).expect("the slot loads");
    }
}

/// (KV updates, data size, history gas, history bytes) of the transaction.
fn run(writes: bool, tool_load: bool) -> [u64; 4] {
    let init = init_code(writes);
    let address = created(&init);
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(24))),
    );
    db.insert_account_info(
        &FACTORY,
        AccountInfo::default()
            .with_code(Bytecode::new_legacy(Bytes::from(factory_code(&init))))
            .with_nonce(1),
    );
    // The address the factory creates onto: nonce zero, no code, slot `SLOT` = 7.
    db.insert_account_info(&address, AccountInfo::default());
    db.insert_account_storage(&address, &U256::from(SLOT), &U256::from(7));
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(10_000_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = common::evm(block, Db::new(db));
    if tool_load {
        evm.set_inspector(ToolLoad(address));
    }
    let tx = TxLegacy {
        chain_id: Some(1),
        gas_limit: 5_000_000,
        to: TxKind::Call(FACTORY),
        ..Default::default()
    };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let tx = Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER);
    let result = evm.transact(&tx).expect("the transaction is valid").detach().result;
    assert!(result.status, "{result:?}");
    let [data, kv, _] = evm.ext().usage(true);
    let (history, history_bytes) = evm.ext().history(true);
    [kv, data, history, history_bytes]
}

/// One write record more than the transaction makes without the slot: one KV update, 40 bytes of
/// data size and of history, 3,520 of history gas.
fn plus_one_record(base: [u64; 4]) -> [u64; 4] {
    [base[0] + 1, base[1] + WRITE_RECORD_SIZE, base[2] + 3_520, base[3] + WRITE_RECORD_SIZE]
}

#[test]
fn test_a_written_slot_whose_original_is_not_zero_keeps_its_record() {
    // Without the load the slot first loads after the creation, reads zero and is on a zero row:
    // its 0 -> 9 write's record is taken back.
    let unloaded = run(true, false);
    assert_eq!(run(true, true), plus_one_record(unloaded));
}

#[test]
fn test_the_deletion_makes_the_record_of_an_unchanged_slot_whose_original_is_not_zero() {
    // Without the load the slot is never loaded and has no record.
    let unloaded = run(false, false);
    assert_eq!(run(false, true), plus_one_record(unloaded));
    // The same transactions without the write keep the same records with the load.
    assert_eq!(run(false, true), run(true, true));
}
