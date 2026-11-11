//! Protocol origin is determined from the caller and the deposit source hash in simulations too.
//! Such a transaction is held to no limit and not detained, and pays history gas.
mod common;

use alloy_consensus::{TxEip1559, TxEip2930, TxEip7702, TxLegacy, transaction::Recovered};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    Evm,
    bytecode::Bytecode,
    env::BlockEnv,
    ethereum::LazyTxEip7702,
    evm::{AccountInfo, Db, InMemoryDB, SYSTEM_ADDRESS},
};
use mega_evm2::{
    MegaTypes, TxLimits, factory::Simulation, tx::MEGA_SYSTEM_TRANSACTION_SOURCE_HASH,
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SENDER: Address = Address::with_last_byte(0x50);
const TARGET: Address = Address::with_last_byte(0xa0);

fn evm(sender: Address) -> Evm<'static, MegaTypes> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&sender, AccountInfo::default().with_balance(U256::from(1_000_000_000)));
    // TIMESTAMP POP; SSTORE(0, 1); STOP: detention, a state charge and a write record.
    db.insert_account_info(
        &TARGET,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from(hex!("425060015f5500")))),
    );
    let block = BlockEnv::<MegaTypes> { gas_limit: U256::from(30_000_000), ..Default::default() };
    // Deliberately no room: only a protocol-origin execution may write or continue after TIMESTAMP.
    let limits = TxLimits { tx: [0; 3], cap: 0, oracle_cap: 0 };
    let simulation = Simulation { limits: Some(limits), ..Simulation::default() };
    common::simulated_evm(block, Db::new(db), &simulation)
}

fn envelope(ty: u8, sender: Address, source_hash: B256) -> Recovered<OpTxEnvelope> {
    let gas_limit = 1_000_000;
    let to = TxKind::Call(TARGET);
    let env = match ty {
        0 => OpTxEnvelope::new(
            TxLegacy { chain_id: Some(1), gas_limit, to, ..Default::default() },
            Some(Bytes::new()),
        ),
        1 => OpTxEnvelope::new(
            TxEip2930 { chain_id: 1, gas_limit, to, ..Default::default() },
            Some(Bytes::new()),
        ),
        2 => OpTxEnvelope::new(
            TxEip1559 { chain_id: 1, gas_limit, to, ..Default::default() },
            Some(Bytes::new()),
        ),
        4 => {
            let tx = TxEip7702 {
                chain_id: 1,
                gas_limit,
                to: TARGET,
                authorization_list: vec![SignedAuthorization::new_unchecked(
                    Authorization { chain_id: U256::from(1), address: TARGET, nonce: 0 },
                    0,
                    U256::from(1),
                    U256::from(1),
                )],
                ..Default::default()
            };
            OpTxEnvelope::new(
                LazyTxEip7702::from_cached_recovered_authorizations(tx, vec![None]),
                Some(Bytes::new()),
            )
        }
        _ => OpTxEnvelope::deposit(TxDeposit {
            source_hash,
            from: sender,
            to,
            mint: 0,
            value: U256::ZERO,
            gas_limit,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
    };
    Recovered::new_unchecked(env, sender)
}

fn assert_protocol_run(ty: u8, sender: Address, hash: B256) {
    let mut evm = evm(sender);
    let out = evm.transact(&envelope(ty, sender, hash)).expect("protocol body exempt").detach();
    assert!(out.result.status, "type {ty}: {:?}", out.result);
    let target = out.pending_state.account_changes_for(&TARGET).unwrap();
    assert_eq!(target.storage.slots[&U256::ZERO].value.current, U256::from(1));
    // A system-originated transaction pays history gas as any transaction does
    // (`SATIN-MST-008`): its body's 310 bytes (and an authorization entry's 101) and the write
    // record's 40, at 88 each.
    let bytes = 310 + if ty == 4 { 101 } else { 0 } + 40;
    assert_eq!(evm.ext().history(true), (bytes * 88, bytes), "type {ty}");
    assert_eq!(evm.ext().usage(true)[1..], [1, 1]);
    assert!(evm.ext().salt.lookups().is_empty());
}

#[test]
fn test_protocol_caller_is_exempt_for_every_envelope() {
    for ty in [0, 1, 2, 4, 126] {
        assert_protocol_run(ty, SYSTEM_ADDRESS, B256::with_last_byte(0x42));
    }
}

#[test]
fn test_protocol_deposit_source_is_exempt_for_another_caller() {
    assert_protocol_run(126, SENDER, MEGA_SYSTEM_TRANSACTION_SOURCE_HASH);
}

#[test]
fn test_ordinary_deposit_still_obeys_body_limit() {
    let mut evm = evm(SENDER);
    let out = evm.transact(&envelope(126, SENDER, B256::with_last_byte(0x42))).unwrap().detach();
    assert!(out.result.ext.failed_deposit);
    assert!(out.pending_state.account_changes_for(&TARGET).is_none());
}
