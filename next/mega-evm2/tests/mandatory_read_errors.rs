//! A transaction outside a block session reads the live system address out of the
//! `SequencerRegistry` before anything else; a block reads the registry once, before its
//! transactions, to decide whether a role change is due and which address its transactions are
//! classified against; a block reads the L1 block info before its transactions, and a
//! transaction other than a deposit reads the L1Block account before it is priced. A read the
//! database cannot serve fails the transaction (or the block) with the database's error, whatever
//! the envelope; it is never taken for an absent account.
//!
//! The differential gates run on infallible databases, so this file covers the error path with a
//! database adapter that fails those reads. A block session runs its pre-block sequence before it
//! is returned, so a test of a transaction's read arms the failure once the session has started.

mod common;

use alloy_consensus::{TxEip1559, TxEip2930, TxEip7702, TxLegacy, transaction::Recovered};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex, keccak256};
use evm2::{
    DatabaseError, Evm,
    bytecode::Bytecode,
    env::BlockEnv,
    ethereum::LazyTxEip7702,
    evm::{AccountInfo, Database, Db, InMemoryDB},
    interpreter::Word,
    registry::HandlerError,
};
use mega_evm2::{
    MegaTypes, SEQUENCER_REGISTRY,
    block::BlockSession,
    error::{BlockError, CandidateError, Disposition, Phase},
    factory::BlockServices,
    tx::SEQUENCER_REGISTRY_CODE_HASH,
};
use op_evm2::{OpTxEnvelope, TxDeposit, constants::L1_BLOCK_CONTRACT};
use std::{cell::Cell, rc::Rc};

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
    SenderRead(usize),
}

/// A chain whose registry names [`SYSTEM`], over a backend that fails the reads `fail` names and
/// serves every other read.
#[derive(Debug)]
struct RegistryBackend {
    inner: InMemoryDB,
    fail: Fail,
    sender_reads: usize,
    /// Whether the reads `fail` names fail yet.
    armed: Rc<Cell<bool>>,
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
        Self { inner, fail, sender_reads: 0, armed: Rc::new(Cell::new(true)) }
    }

    fn unavailable() -> DatabaseError {
        DatabaseError::new(Unavailable, true)
    }
}

impl Database for RegistryBackend {
    type Error = DatabaseError;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        if !self.armed.get() {
            return self.inner.get_account(address);
        }
        if *address == SENDER {
            self.sender_reads += 1;
            if matches!(self.fail, Fail::SenderRead(n) if n == self.sender_reads) {
                return Err(Self::unavailable());
            }
        }
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
        if !self.armed.get() {
            return self.inner.get_storage(address, key);
        }
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
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000),
        ..BlockEnv::<MegaTypes>::default()
    };
    common::evm(block, Db::new(RegistryBackend::new(fail)))
}

/// Block 1 over a backend that fails the reads `fail` names: from the start when `armed`,
/// otherwise once the returned switch is set.
fn start_block(
    fail: Fail,
    armed: bool,
) -> (Result<BlockSession<'static>, BlockError>, Rc<Cell<bool>>) {
    start_block_over(RegistryBackend::new(fail), armed)
}

fn start_block_over(
    backend: RegistryBackend,
    armed: bool,
) -> (Result<BlockSession<'static>, BlockError>, Rc<Cell<bool>>) {
    let switch = backend.armed.clone();
    switch.set(armed);
    let input = common::block_input(1, 1_000, 30_000_000, Some(B256::repeat_byte(0x11)));
    let block = common::factory().start_block(&input, Db::new(backend), BlockServices::default());
    (block, switch)
}

/// The database error a block-level failure carries is the backend's own, its fatality kept.
fn assert_unavailable_block(name: &str, error: &BlockError) {
    let e =
        error.database_error().unwrap_or_else(|| panic!("{name}: no database error: {error:?}"));
    assert!(e.downcast_ref::<Unavailable>().is_some(), "{name}: {e:?}");
    assert!(e.is_fatal(), "{name}");
    assert!(error.to_string().contains("Unavailable"), "{name}: {error}");
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

/// A failed read of the live system address a transaction is classified against: the database's
/// own error, carried so that no path takes it for a refusal and the block routes it as an internal
/// error (`SATIN-REG-007`).
fn assert_registry_unavailable(name: &str, error: &HandlerError) {
    let failed = mega_evm2::error::system_address_read_failure(error)
        .unwrap_or_else(|| panic!("{name}: not the classification read: {error:?}"));
    assert!(failed.0.downcast_ref::<Unavailable>().is_some(), "{name}: {failed:?}");
    let db = mega_evm2::error::database_error(error).expect("the database error");
    assert!(db.downcast_ref::<Unavailable>().is_some() && db.is_fatal(), "{name}");
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
        assert_registry_unavailable(name, &error);
    }
}

#[test]
fn test_unreadable_registry_account_fails_every_envelope() {
    for (name, tx) in every_envelope() {
        let mut evm = satin_evm(Fail::Account);
        let error = evm.transact(&tx).err().unwrap_or_else(|| panic!("{name}: executed"));
        assert_registry_unavailable(name, &error);
    }
}

/// A block classifies its transactions against the live system address its pre-block sequence
/// resolved (`SATIN-REG-002`): once the session has started, an unreadable registry fails none of
/// them, whatever its envelope, since none reads it for its classification.
#[test]
fn test_block_classifies_every_envelope_without_reading_the_registry() {
    for fail in [Fail::Account, Fail::Storage] {
        for (name, tx) in every_envelope() {
            let mut backend = RegistryBackend::new(fail);
            backend.inner.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(7), &U256::ZERO);
            backend.inner.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(8), &U256::ZERO);
            let (block, armed) = start_block_over(backend, false);
            let mut block = block.expect("the block starts");
            assert_eq!(block.system_address(), SYSTEM);
            armed.set(true);
            let out = block.execute_candidate(&tx, &[]);
            let out = out.unwrap_or_else(|e| panic!("{name} {fail:?}: {e:?}"));
            assert!(out.result().status, "{name} {fail:?}: {:?}", out.result());
            block.commit_candidate(out).unwrap_or_else(|e| panic!("{name} {fail:?}: {e:?}"));
        }
    }
}

/// The pre-block step reads the registry's pending role changes: an unreadable registry fails the
/// block rather than reading as no change due.
#[test]
fn test_pre_block_step_fails_on_an_unreadable_registry() {
    for fail in [Fail::Account, Fail::Storage] {
        let error = start_block(fail, true).0.err().expect("an unreadable registry is no answer");
        assert_eq!(error.phase(), Phase::DueChangeRead, "{fail:?}");
        assert_eq!(error.disposition(), Disposition::Internal, "{fail:?}");
        assert_eq!(error.context().address, Some(SEQUENCER_REGISTRY), "{fail:?}");
        assert_unavailable_block("registry", &error);
    }
    // The control: a change is due, so the sequence calls `applyPendingChanges()` after the
    // registry's read-only state: the two EIP calls, five deployments, the read, the call, the
    // registry's deployment, the L1 block info read and the resolution.
    let block = start_block(Fail::Nothing, true).0.expect("the block starts");
    assert_eq!(block.pre_block_states().len(), 12, "the control: a change is due");
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
        let error = start_block(fail, true).0.err().expect("an unreadable L1 block info");
        assert_eq!(error.phase(), Phase::L1BlockInfoRead, "{fail:?}");
        assert_eq!(error.disposition(), Disposition::Internal, "{fail:?}");
        assert_eq!(error.context().address, Some(L1_BLOCK_CONTRACT), "{fail:?}");
        assert_unavailable_block("L1 block info", &error);
    }
}

/// A transient failure of the deposit sender read cannot become an absent caller and a
/// fabricated failed deposit, even when a subsequent database read would succeed.
#[test]
fn test_deposit_sender_read_error_is_internal() {
    let (_, tx) = every_envelope().pop().unwrap();
    let mut evm = satin_evm(Fail::SenderRead(1));
    let error =
        evm.transact(&tx).err().unwrap_or_else(|| panic!("the sender read must fail execution"));
    assert_unavailable("deposit sender", &error);
}

#[test]
fn test_block_deposit_sender_read_error_has_no_outcome() {
    let (_, tx) = every_envelope().pop().unwrap();
    // The accepted overlay caches a successful first read, so fail the block's sender read.
    let mut block = start_block(Fail::SenderRead(1), true).0.expect("the block starts");
    let error = block.execute_candidate(&tx, &[]).expect_err("no deposit outcome");
    let CandidateError::Block(fault) = &error else { panic!("{error:?}") };
    assert_eq!(fault.phase(), Phase::Bookkeeping);
    assert_eq!(fault.disposition(), Disposition::Internal);
    assert_eq!(fault.context().address, Some(SENDER));
    assert_unavailable_block("deposit sender", fault);
    assert!(block.receipts().is_empty());
    assert_eq!(block.limiter().block_gas_used, 0);
}
