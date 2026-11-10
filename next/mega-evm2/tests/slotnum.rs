//! `SLOTNUM` pushes zero in every block, whatever slot number the block environment carries:
//! MegaETH has no slot.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, Bytes, Signature, TxKind, U256};
use evm2::{
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::MegaTypes;
use op_evm2::OpTxEnvelope;

const SENDER: Address = Address::with_last_byte(0x50);
const CONTRACT: Address = Address::with_last_byte(0xc0);

#[test]
fn test_slotnum_pushes_zero() {
    // SLOTNUM PUSH0 MSTORE PUSH1 32 PUSH0 RETURN: returns the word SLOTNUM pushed.
    let code = [0x4b, 0x5f, 0x52, 0x60, 0x20, 0x5f, 0xf3];
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
    );
    db.insert_account_info(
        &CONTRACT,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::copy_from_slice(&code))),
    );
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        slot_num: U256::from(42),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = common::evm(block, Db::new(db));
    let tx = TxLegacy {
        chain_id: Some(1),
        gas_limit: 1_000_000,
        to: TxKind::Call(CONTRACT),
        ..Default::default()
    };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let tx = Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER);
    let result = evm.transact(&tx).expect("valid").commit();
    assert!(result.status, "{result:?}");
    assert_eq!(result.output, Bytes::from(vec![0u8; 32]));
}
