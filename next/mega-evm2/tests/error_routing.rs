//! A read the database fails is reported by where it failed and what the database declares
//! (`SATIN-VAL-010`, `SATIN-PBLK-006`, `SATIN-REG-007`), at every phase of a block:
//!
//! - a pre-block call: a fatal error is internal, any other an invalid block;
//! - the pre-block due-change read and L1 block info read, the live system address read, and the
//!   block's own reads for a transaction outside its execution (the sender, the footprint scalar):
//!   always internal;
//! - a transaction's validation or execution (its frames, a deposit's, an authorization's
//!   preflight): a fatal error is internal, any other an invalid block.
//!
//! The database error keeps its type and fatality, the failure its phase and context; a failure
//! that decides the block leaves the candidate without an outcome, and the session runs nothing
//! after it. A refusal and a stale candidate decide nothing about the block; a revert, a halt and a
//! failed deposit are outcomes, not errors.

mod common;

use alloy_consensus::{TxEip7702, TxLegacy, transaction::Recovered};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex};
use evm2::{
    DatabaseError,
    bytecode::Bytecode,
    ethereum::LazyTxEip7702,
    evm::{AccountInfo, Database, Db, InMemoryDB},
    interpreter::Word,
    registry::HandlerError,
};
use mega_evm2::{
    SEQUENCER_REGISTRY,
    block::{BlockSession, HISTORY_STORAGE},
    error::{BlockError, BlockErrorKind, CandidateError, Disposition, Phase, Refusal},
    factory::BlockServices,
};
use op_evm2::{OpTxEnvelope, TxDeposit, constants::L1_BLOCK_CONTRACT};
use std::{cell::RefCell, rc::Rc};

const SENDER: Address = Address::with_last_byte(0x50);
/// Reads its slot 0: `PUSH0 SLOAD POP STOP`.
const TARGET: Address = Address::with_last_byte(0xa0);
/// Reverts: `PUSH0 PUSH0 REVERT`.
const REVERTER: Address = Address::with_last_byte(0xa1);
const AUTHORITY: Address = Address::with_last_byte(0xc0);
const READ_SLOT_ZERO: [u8; 4] = [0x5f, 0x54, 0x50, 0x00];
/// An account no test reads: a plan failing it fails nothing.
const NOBODY: Address = Address::repeat_byte(0xee);

#[derive(Debug)]
struct Unavailable;

impl core::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("unavailable")
    }
}

impl std::error::Error for Unavailable {}

/// A read the plan fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Read {
    Account(Address),
    Storage(Address, u64),
}

/// Which read fails, how the database declares it, and from when.
#[derive(Debug, Default)]
struct Plan {
    armed: bool,
    fail: Option<Read>,
    fatal: bool,
    /// How many times the failure was served.
    served: usize,
}

#[derive(Debug)]
struct Backend {
    inner: InMemoryDB,
    plan: Rc<RefCell<Plan>>,
}

impl Backend {
    fn fails(&self, read: Read) -> Option<DatabaseError> {
        let mut plan = self.plan.borrow_mut();
        if !plan.armed || plan.fail != Some(read) {
            return None;
        }
        plan.served += 1;
        Some(DatabaseError::new(Unavailable, plan.fatal))
    }
}

impl Database for Backend {
    type Error = DatabaseError;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        match self.fails(Read::Account(*address)) {
            Some(e) => Err(e),
            None => self.inner.get_account(address),
        }
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        self.inner.get_code_by_hash(code_hash)
    }

    fn get_storage(&mut self, address: &Address, key: &Word) -> Result<Word, Self::Error> {
        let read = u64::try_from(*key).map(|k| Read::Storage(*address, k));
        match read.ok().and_then(|read| self.fails(read)) {
            Some(e) => Err(e),
            None => self.inner.get_storage(address, key),
        }
    }

    fn get_block_hash(&mut self, number: &Word) -> Result<B256, Self::Error> {
        self.inner.get_block_hash(number)
    }
}

fn code(bytes: &[u8]) -> AccountInfo {
    AccountInfo::default()
        .with_nonce(1)
        .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(bytes)))
}

/// The world: a funded sender, the two targets, an EIP-2935 contract that reads its slot 0, the
/// trusted registry naming no system address change, and the L1 block contract when `l1`.
fn world(l1: bool) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
    );
    db.insert_account_info(&TARGET, code(&READ_SLOT_ZERO));
    db.insert_account_info(&REVERTER, code(&[0x5f, 0x5f, 0xfd]));
    db.insert_account_info(&HISTORY_STORAGE, code(&READ_SLOT_ZERO));
    let registry =
        hex::decode(include_str!("fixtures/SequencerRegistry-2.0.0.hex").trim()).unwrap();
    db.insert_account_info(
        &SEQUENCER_REGISTRY,
        AccountInfo::default().with_code(Bytecode::new_legacy(registry.into())),
    );
    db.insert_account_storage(&SEQUENCER_REGISTRY, &U256::ZERO, &U256::from(0x5a));
    if l1 {
        db.insert_account_info(&L1_BLOCK_CONTRACT, AccountInfo::default().with_nonce(1));
    }
    db
}

/// Block 1 over [`world`] failing `fail` as `fatal` declares it: armed from the start when
/// `armed`, otherwise once the returned plan is armed.
fn start(
    l1: bool,
    fail: Read,
    fatal: bool,
    armed: bool,
) -> (Result<BlockSession<'static>, BlockError>, Rc<RefCell<Plan>>) {
    let plan = Rc::new(RefCell::new(Plan { armed, fail: Some(fail), fatal, served: 0 }));
    let backend = Backend { inner: world(l1), plan: plan.clone() };
    let input = common::block_input(1, 1_000, 30_000_000, Some(B256::repeat_byte(0x11)));
    let block = common::factory().start_block(&input, Db::new(backend), BlockServices::default());
    (block, plan)
}

/// A session started with nothing failing yet; `fail` fails from now on.
fn started(l1: bool, fail: Read, fatal: bool) -> (BlockSession<'static>, Rc<RefCell<Plan>>) {
    let (block, plan) = start(l1, fail, fatal, false);
    let block = block.expect("the block starts");
    plan.borrow_mut().armed = true;
    (block, plan)
}

fn legacy(to: Address) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy {
        chain_id: Some(common::CHAIN_ID),
        gas_limit: 1_000_000,
        to: TxKind::Call(to),
        ..Default::default()
    };
    Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), SENDER)
}

fn deposit(to: Address) -> Recovered<OpTxEnvelope> {
    deposit_with_gas(to, 1_000_000)
}

fn deposit_with_gas(to: Address, gas_limit: u64) -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(to),
            gas_limit,
            ..Default::default()
        }),
        SENDER,
    )
}

fn delegation() -> Recovered<OpTxEnvelope> {
    let tx = TxEip7702 {
        chain_id: common::CHAIN_ID,
        gas_limit: 1_000_000,
        to: TARGET,
        authorization_list: vec![SignedAuthorization::new_unchecked(
            Authorization { chain_id: U256::from(common::CHAIN_ID), address: TARGET, nonce: 0 },
            0,
            U256::from(1),
            U256::from(1),
        )],
        ..Default::default()
    };
    let lazy = LazyTxEip7702::from_cached_recovered_authorizations(tx, vec![Some(AUTHORITY)]);
    Recovered::new_unchecked(OpTxEnvelope::new(lazy, Some(Bytes::new())), SENDER)
}

/// The failure carries the backend's own error with its fatality, and the phase and disposition
/// expected.
fn assert_routed(error: &BlockError, phase: Phase, disposition: Disposition, fatal: bool) {
    assert_eq!(error.phase(), phase, "{error:?}");
    assert_eq!(error.disposition(), disposition, "{error:?}");
    let db = error.database_error().unwrap_or_else(|| panic!("no database error: {error:?}"));
    assert!(db.downcast_ref::<Unavailable>().is_some(), "{db:?}");
    assert_eq!(db.is_fatal(), fatal, "{error:?}");
}

/// A candidate's failure that decides the block: the session keeps it, executes nothing after it,
/// and cannot finish.
fn assert_decides(mut block: BlockSession<'static>, error: CandidateError) -> BlockError {
    let CandidateError::Block(fault) = error else { panic!("not the block's: {error:?}") };
    assert_eq!(block.fault(), Some(&fault));
    assert!(block.receipts().is_empty(), "no outcome");
    let after = block.execute_candidate(&deposit(SENDER), &[]).expect_err("nothing after it");
    assert_eq!(after, CandidateError::Block(fault.clone()));
    assert_eq!(block.finish().expect_err("no result"), fault);
    fault
}

fn by_fatality(fatal: bool) -> Disposition {
    if fatal { Disposition::Internal } else { Disposition::InvalidBlock }
}

#[test]
fn test_routing_a_pre_block_call_by_the_databases_fatality() {
    for fatal in [true, false] {
        let (block, plan) = start(true, Read::Storage(HISTORY_STORAGE, 0), fatal, true);
        let error = block.err().expect("the EIP-2935 call's read fails");
        assert_eq!(plan.borrow().served, 1);
        assert_routed(&error, Phase::PreBlockCall, by_fatality(fatal), fatal);
        assert!(
            matches!(error.kind(), BlockErrorKind::SystemCall { target, .. } if *target == HISTORY_STORAGE),
            "{error:?}"
        );
        assert_eq!(error.context().address, Some(HISTORY_STORAGE));
    }
}

#[test]
fn test_routing_the_pre_block_reads_as_internal_whatever_the_fatality() {
    for (fail, phase, address, slot) in [
        (Read::Account(SEQUENCER_REGISTRY), Phase::DueChangeRead, SEQUENCER_REGISTRY, None),
        (Read::Storage(SEQUENCER_REGISTRY, 7), Phase::DueChangeRead, SEQUENCER_REGISTRY, Some(7)),
        (Read::Storage(SEQUENCER_REGISTRY, 9), Phase::DueChangeRead, SEQUENCER_REGISTRY, Some(9)),
        (Read::Account(L1_BLOCK_CONTRACT), Phase::L1BlockInfoRead, L1_BLOCK_CONTRACT, None),
        (Read::Storage(L1_BLOCK_CONTRACT, 1), Phase::L1BlockInfoRead, L1_BLOCK_CONTRACT, Some(1)),
        (Read::Storage(L1_BLOCK_CONTRACT, 8), Phase::L1BlockInfoRead, L1_BLOCK_CONTRACT, Some(8)),
    ] {
        for fatal in [true, false] {
            let error = start(true, fail, fatal, true).0.err().expect("the read fails");
            assert_routed(&error, phase, Disposition::Internal, fatal);
            assert_eq!(error.context().address, Some(address), "{fail:?}");
            assert_eq!(error.context().slot, slot.map(U256::from), "{fail:?}");
        }
    }
}

#[test]
fn test_routing_the_live_system_address_read_as_internal_whatever_the_fatality() {
    for fatal in [true, false] {
        let (mut block, plan) = started(true, Read::Storage(SEQUENCER_REGISTRY, 0), fatal);
        let error = block.execute_candidate(&legacy(TARGET), &[]).expect_err("the read fails");
        assert_eq!(plan.borrow().served, 1);
        let fault = assert_decides(block, error);
        assert_routed(&fault, Phase::SystemAddressResolution, Disposition::Internal, fatal);
        assert_eq!(fault.context().address, Some(SEQUENCER_REGISTRY));
        let tx = fault.context().transaction.expect("the transaction");
        assert_eq!((tx.index, tx.sender, tx.ty), (0, SENDER, 0));
    }
}

#[test]
fn test_routing_the_blocks_own_reads_for_a_transaction_as_internal_whatever_the_fatality() {
    for fatal in [true, false] {
        // The sender's account before the transaction runs.
        let (mut block, _) = started(true, Read::Account(SENDER), fatal);
        let error = block.execute_candidate(&deposit(TARGET), &[]).expect_err("the read fails");
        let fault = assert_decides(block, error);
        assert_routed(&fault, Phase::Bookkeeping, Disposition::Internal, fatal);
        assert_eq!(fault.context().address, Some(SENDER));
        // The footprint gas scalar before a non-deposit (no L1 block contract read before it).
        let (mut block, plan) = started(false, Read::Storage(L1_BLOCK_CONTRACT, 8), fatal);
        let error = block.execute_candidate(&legacy(TARGET), &[]).expect_err("the read fails");
        assert_eq!(plan.borrow().served, 1);
        let fault = assert_decides(block, error);
        assert_routed(&fault, Phase::Bookkeeping, Disposition::Internal, fatal);
        assert_eq!(fault.context().address, Some(L1_BLOCK_CONTRACT));
    }
}

#[test]
fn test_routing_a_transactions_reads_by_the_databases_fatality() {
    for fatal in [true, false] {
        // A frame's read, of a user transaction and of a deposit: no outcome, no failed deposit.
        for tx in [legacy(TARGET), deposit(TARGET)] {
            let (mut block, plan) = started(true, Read::Storage(TARGET, 0), fatal);
            let error = block.execute_candidate(&tx, &[]).expect_err("the frame's read fails");
            assert_eq!(plan.borrow().served, 1);
            let fault = assert_decides(block, error);
            assert_routed(&fault, Phase::Transaction, by_fatality(fatal), fatal);
            assert!(matches!(fault.kind(), BlockErrorKind::Execution(_)), "{fault:?}");
            assert_eq!(fault.context().transaction.map(|t| t.sender), Some(SENDER));
        }
        // An authorization's preflight read of its authority.
        let (mut block, plan) = started(true, Read::Account(AUTHORITY), fatal);
        let error = block.execute_candidate(&delegation(), &[]).expect_err("the read fails");
        assert!(plan.borrow().served >= 1);
        let fault = assert_decides(block, error);
        assert_routed(&fault, Phase::Transaction, by_fatality(fatal), fatal);
    }
}

#[test]
fn test_routing_refusals_and_stale_candidates_decide_nothing() {
    let (mut block, _) = started(true, Read::Account(NOBODY), true);
    // A refused transaction: its nonce is wrong.
    let mut tx = TxLegacy {
        chain_id: Some(common::CHAIN_ID),
        nonce: 5,
        gas_limit: 1_000_000,
        to: TxKind::Call(TARGET),
        ..Default::default()
    };
    let wrong_nonce =
        Recovered::new_unchecked(OpTxEnvelope::new(tx.clone(), Some(Bytes::new())), SENDER);
    let refused = block.execute_candidate(&wrong_nonce, &[]).expect_err("a nonce too high");
    assert!(
        matches!(
            &refused,
            CandidateError::Refused(Refusal::Transaction(HandlerError::InvalidNonce { .. }))
        ),
        "{refused:?}"
    );
    assert_eq!(refused.disposition(), Disposition::Refused);
    // A stale candidate: its sender changed after it ran.
    let first = block.execute_candidate(&legacy(TARGET), &[]).expect("runs");
    let second = block.execute_candidate(&deposit(TARGET), &[]).expect("runs");
    block.commit_candidate(second).expect("committed");
    let stale = block.commit_candidate(first).expect_err("the sender's nonce moved");
    assert_eq!(stale, CandidateError::Stale { sender: SENDER });
    assert_eq!(stale.disposition(), Disposition::Retry);
    // A candidate that no longer fits the block when it is committed: all the gas the block has
    // left when it runs, with a deposit committed before it.
    let left = 30_000_000 - block.limiter().block_gas_used;
    tx.nonce = 1;
    tx.gas_limit = left;
    let big = Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), SENDER);
    let a = block.execute_candidate(&big, &[]).expect("fits alone");
    let b = block.execute_candidate(&deposit(TARGET), &[]).expect("runs");
    block.commit_candidate(b).expect("committed");
    let available = 30_000_000 - block.limiter().block_gas_used;
    let room = block.commit_candidate(a).expect_err("no room left");
    assert_eq!(
        room,
        CandidateError::Refused(Refusal::BlockGasLimit { gas_limit: left, available })
    );
    assert_eq!(room.disposition(), Disposition::Refused);
    assert!(block.fault().is_none());
    block.finish().expect("the block finishes");
}

#[test]
fn test_routing_reverts_halts_and_failed_deposits_are_outcomes() {
    let (mut block, _) = started(true, Read::Account(NOBODY), true);
    let reverted = block.execute_candidate(&legacy(REVERTER), &[]).expect("an outcome");
    assert!(!reverted.result.status);
    block.commit_candidate(reverted).expect("committed");
    // A deposit whose gas limit cannot pay its intrinsic gas fails, and is included as failed.
    let tx = deposit_with_gas(TARGET, 1);
    let failed = block.execute_candidate(&tx, &[]).expect("a failed deposit is an outcome");
    assert!(failed.result.ext.failed_deposit, "{:?}", failed.result);
    block.commit_candidate(failed).expect("committed");
    assert_eq!(block.receipts().len(), 2);
    assert!(block.receipts().iter().all(|r| !r.status));
    assert!(block.fault().is_none());
    block.finish().expect("the block finishes");
}
