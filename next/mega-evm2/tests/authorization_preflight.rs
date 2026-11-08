//! Every EIP-7702 authorization list is simulated before it is applied, whatever its length, so
//! every authority whose entry passes the chain-id, nonce and signature gates is loaded, also when
//! the application then runs out of gas before it reaches that authority: the accounts a
//! transaction loads, and so its witness, do not depend on the list's length or on where an
//! out-of-gas falls.

use alloy_consensus::{TxEip7702, transaction::Recovered};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, U256};
use evm2::{
    DatabaseError, Evm, ExecutionConfig,
    bytecode::Bytecode,
    env::BlockEnv,
    ethereum::LazyTxEip7702,
    evm::{AccountInfo, Database, Db, InMemoryDB},
    interpreter::{InstrStop, Word},
};
use mega_evm2::{MegaSpec, MegaTypes, precompiles, satin_version, tx::registry};
use op_evm2::OpTxEnvelope;
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

const SENDER: Address = Address::with_last_byte(0x50);
const TARGET: Address = Address::with_last_byte(0xa0);
const DELEGATE: Address = Address::with_last_byte(0xd0);
/// Two absent authorities: applying the first creates it, whose state gas the transaction cannot
/// pay.
const FIRST: Address = Address::repeat_byte(0x71);
const SECOND: Address = Address::repeat_byte(0x72);

/// The in-memory state, recording every account the engine reads from it.
struct Recording {
    inner: InMemoryDB,
    accounts: Rc<RefCell<BTreeSet<Address>>>,
}

impl Database for Recording {
    type Error = DatabaseError;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.accounts.borrow_mut().insert(*address);
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

/// Runs a type-4 transaction to [`TARGET`] whose list delegates [`FIRST`], then [`SECOND`], to
/// [`DELEGATE`], with 120,000 gas: enough for validation, not for the first authority's state gas.
/// Returns the transaction's stop and the accounts the engine read.
fn run() -> (InstrStop, BTreeSet<Address>) {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
    );
    db.insert_account_info(
        &DELEGATE,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from_static(&[0x00]))),
    );
    let accounts = Rc::new(RefCell::new(BTreeSet::new()));
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(Recording { inner: db, accounts: accounts.clone() }),
        precompiles(),
    );
    evm.state_mut().set_read_absent_storage(true);
    let entry = |nonce| {
        SignedAuthorization::new_unchecked(
            Authorization { chain_id: U256::from(1), address: DELEGATE, nonce },
            0,
            U256::ZERO,
            U256::ZERO,
        )
    };
    let tx = TxEip7702 {
        chain_id: 1,
        gas_limit: 120_000,
        to: TARGET,
        authorization_list: vec![entry(0), entry(0)],
        ..Default::default()
    };
    let envelope = OpTxEnvelope::new(
        LazyTxEip7702::from_cached_recovered_authorizations(tx, vec![Some(FIRST), Some(SECOND)]),
        Some(Bytes::new()),
    );
    let result = evm.transact(&Recovered::new_unchecked(envelope, SENDER)).expect("valid").commit();
    let read = accounts.borrow().clone();
    (result.stop, read)
}

#[test]
fn test_an_authority_the_application_never_reached_is_loaded() {
    let (stop, read) = run();
    // The first authority's state gas runs the transaction out of gas before its first frame.
    assert_eq!(stop, InstrStop::OutOfGas);
    assert!(read.contains(&FIRST), "{read:?}");
    assert!(read.contains(&SECOND), "the second authority was not loaded: {read:?}");
    assert!(!read.contains(&TARGET), "the recipient is read only by the first frame");
}
