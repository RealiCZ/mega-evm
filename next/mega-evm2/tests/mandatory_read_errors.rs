//! Every transaction reads the live system address out of the `SequencerRegistry` before anything
//! else, and a block reads it to decide whether a transaction is a Mega System Transaction and
//! whether a role change is due; a block reads the L1 block info before its transactions, and a
//! transaction other than a deposit reads the L1Block account before it is priced. A read the
//! database cannot serve fails the transaction (or the block) with the database's error, whatever
//! the envelope; it is never taken for an absent account.
//!
//! The differential gates run on infallible databases, so this file covers the error path with a
//! database adapter that fails those reads.

use alloy_consensus::{TxEip1559, TxEip2930, TxEip7702, TxLegacy, transaction::Recovered};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex, keccak256};
use evm2::{
    DatabaseError, Evm, ExecutionConfig,
    bytecode::Bytecode,
    env::BlockEnv,
    ethereum::LazyTxEip7702,
    evm::{AccountInfo, Database, Db, InMemoryDB},
    interpreter::Word,
    registry::HandlerError,
};
use mega_evm2::{
    MegaSpec, MegaTypes, SEQUENCER_REGISTRY,
    block::BlockExecutor,
    precompiles, satin_version,
    tx::{SEQUENCER_REGISTRY_CODE_HASH, registry},
};
use op_evm2::{OpTxEnvelope, TxDeposit, constants::L1_BLOCK_CONTRACT};

const SENDER: Address = Address::with_last_byte(0x50);
/// Stores 1 in slot 0.
const TARGET: Address = Address::with_last_byte(0xa0);
const AUTHORITY: Address = Address::with_last_byte(0xc0);
const SYSTEM: Address = Address::with_last_byte(0x5a);
const SEQUENCER_REGISTRY_CODE: &str = include_str!("fixtures/SequencerRegistry-2.0.0.hex");

#[derive(Debug)]
struct Unavailable;

impl core::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("registry unavailable")
    }
}

impl std::error::Error for Unavailable {}

/// Which reads fail: the registry's account or storage, or the L1Block account's or storage.
#[derive(Clone, Copy, Debug)]
enum Fail {
    Nothing,
    Account,
    Storage,
    L1Account,
    L1Storage,
}

/// A chain whose registry names [`SYSTEM`], over a backend that fails the reads `fail` names and
/// serves every other read.
#[derive(Debug)]
struct RegistryBackend {
    inner: InMemoryDB,
    fail: Fail,
}

impl RegistryBackend {
    fn new(fail: Fail) -> Self {
        let mut inner = InMemoryDB::default();
        inner.insert_account_info(
            &SENDER,
            AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
        );
        inner.insert_account_info(
            &TARGET,
            AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from_static(&[
                0x60, 0x01, 0x60, 0x00, 0x55, 0x00,
            ]))),
        );
        let code = hex::decode(SEQUENCER_REGISTRY_CODE.trim()).unwrap();
        assert_eq!(keccak256(&code), SEQUENCER_REGISTRY_CODE_HASH);
        inner.insert_account_info(
            &SEQUENCER_REGISTRY,
            AccountInfo::default().with_code(Bytecode::new_legacy(code.into())),
        );
        inner.insert_account_storage(
            &SEQUENCER_REGISTRY,
            &U256::ZERO,
            &U256::from_be_slice(SYSTEM.as_slice()),
        );
        // A role change pending since block 1, so the block's due check reads both its slots.
        inner.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(7), &U256::from(0x77));
        inner.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(8), &U256::from(1));
        inner.insert_account_info(&L1_BLOCK_CONTRACT, AccountInfo::default().with_nonce(1));
        Self { inner, fail }
    }

    fn unavailable() -> DatabaseError {
        DatabaseError::new(Unavailable, true)
    }
}

impl Database for RegistryBackend {
    type Error = DatabaseError;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        if matches!(self.fail, Fail::Account) && *address == SEQUENCER_REGISTRY ||
            matches!(self.fail, Fail::L1Account) && *address == L1_BLOCK_CONTRACT
        {
            return Err(Self::unavailable());
        }
        self.inner.get_account(address)
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        self.inner.get_code_by_hash(code_hash)
    }

    fn get_storage(&mut self, address: &Address, key: &Word) -> Result<Word, Self::Error> {
        if matches!(self.fail, Fail::Storage) && *address == SEQUENCER_REGISTRY ||
            matches!(self.fail, Fail::L1Storage) && *address == L1_BLOCK_CONTRACT
        {
            return Err(Self::unavailable());
        }
        self.inner.get_storage(address, key)
    }

    fn get_block_hash(&mut self, number: &Word) -> Result<B256, Self::Error> {
        self.inner.get_block_hash(number)
    }
}

fn satin_evm(fail: Fail) -> Evm<'static, MegaTypes> {
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(RegistryBackend::new(fail)),
        precompiles(),
    );
    evm.state_mut().set_read_absent_storage(true);
    evm
}

/// One transaction of every envelope the engine handles, from [`SENDER`] to [`TARGET`].
fn every_envelope() -> Vec<(&'static str, Recovered<OpTxEnvelope>)> {
    let gas_limit = 1_000_000;
    let input = Bytes::new();
    let to = TxKind::Call(TARGET);
    let legacy = TxLegacy {
        chain_id: Some(1),
        nonce: 0,
        gas_price: 0,
        gas_limit,
        to,
        value: U256::ZERO,
        input: input.clone(),
    };
    let eip2930 = TxEip2930 {
        chain_id: 1,
        nonce: 0,
        gas_price: 0,
        gas_limit,
        to,
        value: U256::ZERO,
        access_list: Default::default(),
        input: input.clone(),
    };
    let eip1559 = TxEip1559 {
        chain_id: 1,
        nonce: 0,
        gas_limit,
        max_fee_per_gas: 0,
        max_priority_fee_per_gas: 0,
        to,
        value: U256::ZERO,
        access_list: Default::default(),
        input: input.clone(),
    };
    let eip7702 = TxEip7702 {
        chain_id: 1,
        nonce: 0,
        gas_limit,
        max_fee_per_gas: 0,
        max_priority_fee_per_gas: 0,
        to: TARGET,
        value: U256::ZERO,
        access_list: Default::default(),
        authorization_list: vec![SignedAuthorization::new_unchecked(
            Authorization { chain_id: U256::from(1), address: TARGET, nonce: 0 },
            0,
            U256::from(1),
            U256::from(1),
        )],
        input: input.clone(),
    };
    let deposit = TxDeposit {
        source_hash: B256::with_last_byte(0x42),
        from: SENDER,
        to,
        mint: 0,
        value: U256::ZERO,
        gas_limit,
        is_system_transaction: false,
        input,
    };
    let lazy = LazyTxEip7702::from_cached_recovered_authorizations(eip7702, vec![Some(AUTHORITY)]);
    [
        ("legacy", OpTxEnvelope::new(legacy, Some(Bytes::new()))),
        ("eip2930", OpTxEnvelope::new(eip2930, Some(Bytes::new()))),
        ("eip1559", OpTxEnvelope::new(eip1559, Some(Bytes::new()))),
        ("eip7702", OpTxEnvelope::new(lazy, Some(Bytes::new()))),
        ("deposit", OpTxEnvelope::deposit(deposit)),
    ]
    .into_iter()
    .map(|(name, env)| (name, Recovered::new_unchecked(env, SENDER)))
    .collect()
}

fn assert_unavailable(name: &str, error: &HandlerError) {
    match error {
        HandlerError::Database(e) => {
            assert!(e.downcast_ref::<Unavailable>().is_some(), "{name}: {e:?}");
        }
        other => panic!("{name}: expected the database error, got {other:?}"),
    }
}

/// The control: with the registry readable, every envelope executes.
#[test]
fn test_every_envelope_executes_when_the_registry_is_readable() {
    for (name, tx) in every_envelope() {
        let mut evm = satin_evm(Fail::Nothing);
        match evm.transact(&tx) {
            Ok(executed) => assert!(executed.commit().status, "{name}"),
            Err(e) => panic!("{name}: refused {e:?}"),
        }
    }
}

#[test]
fn test_unreadable_registry_slot_fails_every_envelope() {
    for (name, tx) in every_envelope() {
        let mut evm = satin_evm(Fail::Storage);
        let error = evm.transact(&tx).err().unwrap_or_else(|| panic!("{name}: executed"));
        assert_unavailable(name, &error);
    }
}

#[test]
fn test_unreadable_registry_account_fails_every_envelope() {
    for (name, tx) in every_envelope() {
        let mut evm = satin_evm(Fail::Account);
        let error = evm.transact(&tx).err().unwrap_or_else(|| panic!("{name}: executed"));
        assert_unavailable(name, &error);
    }
}

/// A block decides whether a transaction is a Mega System Transaction off the registry before it
/// runs it: an unreadable registry fails the transaction, its envelope whatever it is.
#[test]
fn test_block_fails_a_transaction_on_an_unreadable_registry() {
    for fail in [Fail::Account, Fail::Storage] {
        let (name, tx) = every_envelope().remove(0);
        let mut block = BlockExecutor::new(satin_evm(fail), B256::ZERO, None);
        let error = block.run_transaction(&tx, &[]).err();
        let error = error.unwrap_or_else(|| panic!("{name} {fail:?}: executed"));
        assert!(error.contains("Unavailable"), "{fail:?}: {error}");
    }
}

/// The pre-block step reads the registry's pending role changes: an unreadable registry fails the
/// block rather than reading as no change due.
#[test]
fn test_pre_block_step_fails_on_an_unreadable_registry() {
    for fail in [Fail::Account, Fail::Storage] {
        let mut block = BlockExecutor::new(satin_evm(fail), B256::ZERO, None);
        let error = block.registry_due().expect_err("an unreadable registry is no answer");
        assert!(error.contains("Unavailable"), "{fail:?}: {error}");
    }
    let mut block = BlockExecutor::new(satin_evm(Fail::Nothing), B256::ZERO, None);
    assert_eq!(block.registry_due(), Ok(true), "the control: a change is due");
}

/// A transaction other than a deposit reads the L1Block account, then its slots, before it is
/// priced: an unreadable L1Block fails it. A deposit pays no L1 fee and reads neither.
#[test]
fn test_unreadable_l1_block_fails_every_envelope_but_a_deposit() {
    for fail in [Fail::L1Account, Fail::L1Storage] {
        for (name, tx) in every_envelope() {
            let mut evm = satin_evm(fail);
            let outcome = evm.transact(&tx);
            if name == "deposit" {
                assert!(outcome.is_ok(), "{fail:?} {name}: {:?}", outcome.err());
                continue;
            }
            let error = outcome.err().unwrap_or_else(|| panic!("{fail:?} {name}: executed"));
            assert_unavailable(name, &error);
        }
    }
}

/// The pre-block step reads the L1 block info: an unreadable L1Block account or slot fails the
/// block.
#[test]
fn test_pre_block_step_fails_on_an_unreadable_l1_block() {
    for fail in [Fail::L1Account, Fail::L1Storage] {
        // A parent beacon block root: a block without one is refused before the L1 read.
        let root = Some(B256::repeat_byte(0x11));
        let mut block = BlockExecutor::new(satin_evm(fail), B256::ZERO, root);
        let error = block.apply_pre_execution_changes().expect_err("an unreadable L1 block info");
        assert!(error.contains("Unavailable"), "{fail:?}: {error}");
    }
}
