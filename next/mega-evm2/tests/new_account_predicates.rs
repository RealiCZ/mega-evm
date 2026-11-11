//! The two new-account predicates (`SATIN-STATE-011`, `SATIN-ACCT-015`): an applied EIP-7702
//! authorization's authority is new only when the state holds no account at its address; every
//! other new-account charge, and the state-growth rise with it, is decided by EIP-161 emptiness,
//! so an existing empty leaf is charged and counted as an absent account.

mod common;

use alloy_consensus::{SignableTransaction, TxEip7702, TxLegacy, transaction::Recovered};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256};
use evm2::{
    ethereum::LazyTxEip7702,
    evm::{AccountInfo, Db, InMemoryDB},
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SENDER: Address = Address::with_last_byte(0x50);
const TARGET: Address = Address::with_last_byte(0xa0);
const DELEGATE: Address = Address::with_last_byte(0xd0);
/// The account each case decides on.
const SUBJECT: Address = Address::repeat_byte(0x71);
const ACCOUNT_STATE: u64 = common::ACCOUNT_STATE;

/// What the state holds at [`SUBJECT`].
#[derive(Clone, Copy, Debug)]
enum Subject {
    /// No account.
    Absent,
    /// A leaf empty under EIP-161: nonce zero, zero balance, no code.
    EmptyLeaf,
    /// A leaf with nonce one.
    Existing,
}

fn db(subject: Subject) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_nonce(1).with_balance(U256::from(10).pow(U256::from(18))),
    );
    db.insert_account_info(&TARGET, common::contract(&[common::op::STOP]));
    db.insert_account_info(&DELEGATE, common::contract(&[common::op::STOP]));
    match subject {
        Subject::Absent => {}
        Subject::EmptyLeaf => db.insert_account_info(&SUBJECT, AccountInfo::default()),
        Subject::Existing => db.insert_account_info(&SUBJECT, AccountInfo::default().with_nonce(1)),
    }
    db
}

/// Runs `tx` over [`db`] of `subject` and returns its state gas and its state growth.
fn charged(subject: Subject, tx: &Recovered<OpTxEnvelope>) -> (u64, u64) {
    let mut evm = common::evm(common::block(), Db::new(db(subject)));
    let result = evm.transact(tx).expect("the transaction runs").commit();
    assert!(result.status, "{subject:?}: {result:?}");
    (result.state_gas_spent, evm.ext().usage(true)[2])
}

/// The charge and growth `subject` adds over an existing account, for each subject.
fn over_existing(tx: &Recovered<OpTxEnvelope>) -> [(u64, u64); 2] {
    let (base_gas, base_growth) = charged(Subject::Existing, tx);
    [Subject::Absent, Subject::EmptyLeaf].map(|s| {
        let (gas, growth) = charged(s, tx);
        (gas - base_gas, growth - base_growth)
    })
}

#[test]
fn test_an_authority_is_new_only_when_the_state_holds_no_account() {
    // [`SUBJECT`] delegates to [`DELEGATE`]; its nonce is zero when absent or an empty leaf.
    let tx = |nonce| {
        let entry = SignedAuthorization::new_unchecked(
            Authorization { chain_id: U256::from(1), address: DELEGATE, nonce },
            0,
            U256::ZERO,
            U256::ZERO,
        );
        let tx = TxEip7702 {
            chain_id: 1,
            nonce: 1,
            gas_limit: 1_000_000,
            to: TARGET,
            authorization_list: vec![entry],
            ..Default::default()
        };
        let lazy = LazyTxEip7702::from_cached_recovered_authorizations(tx, vec![Some(SUBJECT)]);
        Recovered::new_unchecked(OpTxEnvelope::new(lazy, Some(Bytes::new())), SENDER)
    };
    let (absent, absent_growth) = charged(Subject::Absent, &tx(0));
    let (empty, empty_growth) = charged(Subject::EmptyLeaf, &tx(0));
    // The absent authority is charged its leaf and counted; the empty leaf is neither, though it
    // is empty under EIP-161 (both carry the delegation indicator's charge alike).
    assert_eq!(absent - empty, ACCOUNT_STATE);
    assert_eq!((absent_growth, empty_growth), (1, 0));
}

#[test]
fn test_a_deposit_caller_is_new_when_it_is_empty_under_eip161() {
    let tx = Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SUBJECT,
            to: TxKind::Call(TARGET),
            mint: 0,
            value: U256::ZERO,
            gas_limit: 1_000_000,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        SUBJECT,
    );
    // An empty leaf is charged and counted as an absent caller.
    assert_eq!(over_existing(&tx), [(ACCOUNT_STATE, 1), (ACCOUNT_STATE, 1)]);
}

#[test]
fn test_a_value_recipient_is_new_when_it_is_empty_under_eip161() {
    let tx = TxLegacy {
        chain_id: Some(1),
        nonce: 1,
        gas_price: 0,
        gas_limit: 1_000_000,
        to: TxKind::Call(SUBJECT),
        value: U256::from(1),
        input: Bytes::new(),
    };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let tx = Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER);
    assert_eq!(over_existing(&tx), [(ACCOUNT_STATE, 1), (ACCOUNT_STATE, 1)]);
}
