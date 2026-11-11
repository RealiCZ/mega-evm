//! SALT pricing through the engine's public seams: the node's capacity snapshot, bound to the
//! block's parent; the per-attempt multiplier cache; the system exemptions; and the two export
//! lifetimes, an attempt's lookups and the block session's union, with its explicit drain.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256};
use common::op;
use evm2::{
    AnyError, DatabaseError, Evm,
    evm::{AccountInfo, Database, Db, InMemoryDB, SYSTEM_ADDRESS, SystemTx},
    registry::HandlerError,
    version::StateGasSite,
};
use mega_evm2::{
    BucketId, EXECUTION_CAP, MegaTypes, SaltCapacities, SaltExport, SaltFailure, SaltLookupError,
    SaltReadError, SaltSnapshot, SnapshotId, Unlisted,
    block::{BlockSession, Candidate, HISTORY_STORAGE},
    bucket_for,
    error::{BlockErrorKind, CandidateError, Disposition, InputError, Phase},
    factory::{BlockInput, BlockServices, ParentContext},
    tx::MEGA_SYSTEM_TRANSACTION_SOURCE_HASH,
};
use op_evm2::{OpTxEnvelope, TxDeposit};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

const SENDER: Address = common::SENDER;
/// Writes slot 0 to 1.
const WRITER: Address = Address::with_last_byte(0xa1);
/// Writes slots 0 and 1 to 1.
const TWO_WRITER: Address = Address::with_last_byte(0xa2);
/// Writes and clears slot 0, writes it again in a child that reverts, then writes it a last time.
const REWRITER: Address = Address::with_last_byte(0xa3);
/// Does nothing.
const NOOP: Address = Address::with_last_byte(0xb0);

/// A fresh slot's state gas at `m = 1` (`SLOT_STATE_BYTES` × `COST_PER_STATE_BYTE`).
const SLOT_STATE: u64 = common::SLOT_STATE;
/// A new account's at `m = 1`.
const ACCOUNT_STATE: u64 = common::ACCOUNT_STATE;
/// The history of a legacy call's 310-byte body and one write record, at 88 a byte.
const BODY_AND_RECORD_HISTORY: u64 = (310 + 40) * 88;

fn slot(address: Address, key: u64) -> BucketId {
    bucket_for(StateGasSite::Storage(address, U256::from(key)))
}

fn account(address: Address) -> BucketId {
    bucket_for(StateGasSite::Account(address))
}

const fn id(parent_hash: B256) -> SnapshotId {
    SnapshotId { parent_hash, state_root: B256::repeat_byte(0x5a) }
}

/// The world the tests run in: a funded sender and the contracts above.
fn db() -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&SENDER, AccountInfo::default().with_nonce(1));
    let sstore = |key: u8| [op::PUSH1, 1, op::PUSH1, key, op::SSTORE];
    db.insert_account_info(
        &WRITER,
        common::contract(&[sstore(0).as_slice(), &[op::STOP]].concat()),
    );
    db.insert_account_info(
        &TWO_WRITER,
        common::contract(&[sstore(0).as_slice(), &sstore(1), &[op::STOP]].concat()),
    );
    db.insert_account_info(&REWRITER, common::contract(&rewriter_code()));
    db.insert_account_info(&NOOP, common::contract(&[op::STOP]));
    db
}

/// [`REWRITER`]'s code. Without calldata (the transaction's own frame): slot 0 := 1 (a charge),
/// slot 0 := 0 (its give-back), a call of itself with one byte of calldata, slot 0 := 1 (the charge
/// the transaction keeps). With calldata (the child): slot 0 := 1, then a revert, which takes the
/// charge back.
fn rewriter_code() -> Vec<u8> {
    const CHILD: u8 = 34;
    let code = vec![
        0x36, // CALLDATASIZE
        op::PUSH1,
        CHILD,
        0x57, // JUMPI
        op::PUSH1,
        1,
        op::PUSH1,
        0,
        op::SSTORE,
        op::PUSH1,
        0,
        op::PUSH1,
        0,
        op::SSTORE,
        op::PUSH1,
        0, // retSize
        op::PUSH1,
        0, // retOffset
        op::PUSH1,
        1, // argsSize
        op::PUSH1,
        0, // argsOffset
        op::PUSH1,
        0,    // value
        0x30, // ADDRESS
        op::GAS,
        op::CALL,
        op::POP,
        op::PUSH1,
        1,
        op::PUSH1,
        0,
        op::SSTORE,
        op::STOP,
        0x5b, // JUMPDEST, at CHILD
        op::PUSH1,
        1,
        op::PUSH1,
        0,
        op::SSTORE,
        op::PUSH1,
        0,
        op::PUSH1,
        0,
        op::REVERT,
    ];
    assert_eq!(code[usize::from(CHILD)], 0x5b);
    code
}

/// A snapshot that counts the queries it answers, per bucket, over `inner`.
struct Counting {
    inner: SaltCapacities,
    queries: Queries,
}

impl SaltSnapshot for Counting {
    fn id(&self) -> SnapshotId {
        self.inner.id()
    }

    fn capacity(&self, bucket: BucketId) -> Result<u64, SaltReadError> {
        *self.queries.lock().unwrap().entry(bucket).or_default() += 1;
        self.inner.capacity(bucket)
    }
}

type Queries = Arc<Mutex<BTreeMap<BucketId, u32>>>;

/// `inner`, counted: the snapshot, and its counts.
fn counting(inner: SaltCapacities) -> (Arc<dyn SaltSnapshot>, Queries) {
    let queries = Queries::default();
    (Arc::new(Counting { inner, queries: queries.clone() }), queries)
}

/// A snapshot whose backend answers `bucket` with `first` once and fails every later query of it;
/// every other bucket is at the minimum.
struct AnswersOnce {
    bucket: BucketId,
    first: u64,
    asked: Mutex<u32>,
}

impl SaltSnapshot for AnswersOnce {
    fn id(&self) -> SnapshotId {
        id(B256::ZERO)
    }

    fn capacity(&self, bucket: BucketId) -> Result<u64, SaltReadError> {
        if bucket != self.bucket {
            return Ok(256);
        }
        let mut asked = self.asked.lock().unwrap();
        *asked += 1;
        match *asked {
            1 => Ok(self.first),
            _ => Err(SaltReadError::Backend(AnyError::from("asked twice".to_string()))),
        }
    }
}

#[derive(Debug)]
struct Unavailable;

impl core::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("bucket metadata unavailable")
    }
}

impl core::error::Error for Unavailable {}

/// A snapshot whose backend fails every read with a database error the node does not declare
/// fatal: a SALT read is still the node's fault, never an invalid block (`SATIN-VAL-010`).
struct Failing;

impl SaltSnapshot for Failing {
    fn id(&self) -> SnapshotId {
        id(B256::ZERO)
    }

    fn capacity(&self, _: BucketId) -> Result<u64, SaltReadError> {
        Err(SaltReadError::Backend(AnyError::new(DatabaseError::new(Unavailable, false))))
    }
}

/// A single transaction's EVM over [`db`] reading `salt`.
fn evm(salt: Arc<dyn SaltSnapshot>) -> Evm<'static, MegaTypes> {
    common::factory().evm(common::block(), Db::new(db()), salt).expect("a Satin timestamp")
}

/// A legacy call from `from` to `to`, `from`'s first transaction in [`db`].
fn legacy(from: Address, to: Address, gas_limit: u64, input: Bytes) -> Recovered<OpTxEnvelope> {
    legacy_at(if from == SENDER { 1 } else { 0 }, from, to, gas_limit, input)
}

fn legacy_at(
    nonce: u64,
    from: Address,
    to: Address,
    gas_limit: u64,
    input: Bytes,
) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy {
        chain_id: Some(1),
        nonce,
        gas_price: 0,
        gas_limit,
        to: TxKind::Call(to),
        value: U256::ZERO,
        input,
    };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), from)
}

fn deposit(from: Address, to: Address, source_hash: B256) -> Recovered<OpTxEnvelope> {
    deposit_with_gas(from, to, source_hash, 1_000_000)
}

fn deposit_with_gas(
    from: Address,
    to: Address,
    source_hash: B256,
    gas_limit: u64,
) -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash,
            from,
            to: TxKind::Call(to),
            mint: 0,
            value: U256::ZERO,
            gas_limit,
            is_system_transaction: false,
            input: Bytes::new(),
        }),
        from,
    )
}

/// An ordinary deposit from [`SENDER`] to `to`.
fn user_deposit(to: Address) -> Recovered<OpTxEnvelope> {
    deposit(SENDER, to, B256::with_last_byte(0x42))
}

/// Runs `tx` on `evm` and commits it: (state gas, regular gas, reservoir returned).
fn spend(evm: &mut Evm<'static, MegaTypes>, tx: &Recovered<OpTxEnvelope>) -> (u64, u64, u64) {
    let result = evm.transact(tx).expect("the transaction runs").commit();
    assert!(result.status, "{result:?}");
    let regular = result.total_gas_spent - result.state_gas_spent;
    (result.state_gas_spent, regular, evm.ext().reservoir_left)
}

/// The failed SALT lookup behind a transaction's error.
fn lookup_error(error: &HandlerError) -> &SaltLookupError {
    let HandlerError::Fatal(any) = error else { panic!("not fatal: {error:?}") };
    any.downcast_ref::<SaltLookupError>().expect("a SALT lookup error")
}

/// Block 5 on the parent `parent` (hash), with a zero parent beacon block root.
fn input(parent: B256) -> BlockInput {
    let mut input = common::block_input(5, 1_005, 30_000_000, Some(B256::ZERO));
    input.header.parent_hash = parent;
    input.parent = ParentContext::Parent { number: 4, hash: parent, timestamp: 1_004 };
    input
}

fn session(input: &BlockInput, salt: Arc<dyn SaltSnapshot>) -> BlockSession<'static> {
    session_over(input, db(), salt)
}

fn session_over(
    input: &BlockInput,
    db: InMemoryDB,
    salt: Arc<dyn SaltSnapshot>,
) -> BlockSession<'static> {
    common::factory()
        .start_block(input, Db::new(db), BlockServices::new(salt))
        .expect("the block starts")
}

fn run(block: &mut BlockSession<'_>, tx: &Recovered<OpTxEnvelope>) -> Candidate {
    let out = block.execute_candidate(tx, &[]).expect("the candidate runs");
    assert!(out.result().status, "{:?}", out.result());
    out
}

#[test]
fn salt_capacities_255_256_511_and_512_price_by_whole_minimum_buckets() {
    let bucket = slot(WRITER, 0);
    let priced = |capacity| {
        // Production's policy: only the listed bucket has a capacity.
        let salt = SaltCapacities::new(id(B256::ZERO)).with(bucket, capacity);
        let mut evm = evm(Arc::new(salt));
        let tx = legacy(SENDER, WRITER, 1_000_000, Bytes::new());
        match evm.transact(&tx) {
            Ok(executed) => Ok(executed.commit().state_gas_spent),
            Err(error) => Err(lookup_error(&error).clone()),
        }
    };
    assert_eq!(
        priced(255),
        Err(SaltLookupError { bucket, failure: SaltFailure::BelowMinimum { capacity: 255 } })
    );
    // m = floor(capacity / 256): 1, 1, 2.
    assert_eq!(priced(256), Ok(SLOT_STATE + BODY_AND_RECORD_HISTORY));
    assert_eq!(priced(511), Ok(SLOT_STATE + BODY_AND_RECORD_HISTORY));
    assert_eq!(priced(512), Ok(2 * SLOT_STATE + BODY_AND_RECORD_HISTORY));
}

#[test]
fn salt_a_missing_capacity_is_an_internal_error_with_no_outcome() {
    // Production's policy, and the bucket is not listed: the lookup fails, it is never priced at
    // the minimum (`SATIN-STATE-007`), for a deposit as for any transaction.
    let salt = SaltCapacities::new(id(B256::ZERO));
    assert_eq!(salt.unlisted(), Unlisted::Refuse);
    let mut block = session(&input(B256::ZERO), Arc::new(salt));
    let error = block.execute_candidate(&user_deposit(WRITER), &[]).expect_err("no capacity");
    let CandidateError::Block(fault) = &error else { panic!("not the block's: {error:?}") };
    assert_eq!(fault.disposition(), Disposition::Internal);
    assert_eq!(fault.phase(), Phase::Transaction);
    assert_eq!(fault.context().bucket, Some(slot(WRITER, 0)));
    let BlockErrorKind::Execution(cause) = fault.kind() else { panic!("{fault:?}") };
    assert_eq!(lookup_error(cause).failure, SaltFailure::Read(SaltReadError::Missing));
    assert!(error.to_string().contains("SALT bucket lookup failed"), "{error}");
    // No outcome, no export of the failed lookup, and nothing runs after it.
    assert!(block.receipts().is_empty());
    assert!(block.bucket_reads().is_empty());
    assert!(block.last_attempt_bucket_reads().is_empty());
    assert_eq!(block.execute_candidate(&user_deposit(NOOP), &[]).expect_err("decided"), error);
    assert_eq!(block.finish().expect_err("the block cannot finish"), *fault);
}

#[test]
fn salt_only_a_policy_named_for_tests_answers_an_unlisted_bucket() {
    let bucket = slot(WRITER, 0);
    let production = SaltCapacities::new(id(B256::ZERO));
    assert_eq!(production.capacity(bucket), Err(SaltReadError::Missing));
    let test = SaltCapacities::test_minimum(id(B256::ZERO));
    assert_eq!(test.unlisted(), Unlisted::TestMinimum);
    assert_eq!(test.capacity(bucket), Ok(256));
    // A listed capacity is answered as listed under either policy, also one below the minimum.
    assert_eq!(production.with(bucket, 7).capacity(bucket), Ok(7));
    assert_eq!(test.with(bucket, 1_024).capacity(bucket), Ok(1_024));
}

#[test]
fn salt_an_evm_without_a_snapshot_fails_its_lookup() {
    // A cache that no factory gave a snapshot (the ledger's default) refuses, as a missing
    // capacity does: there is no silent minimum anywhere.
    let mut cache = mega_evm2::SaltCache::default();
    cache.begin_attempt();
    let bucket = slot(WRITER, 0);
    assert_eq!(
        cache.multiplier(bucket),
        Err(SaltLookupError { bucket, failure: SaltFailure::NoSnapshot })
    );
    assert!(cache.lookups().is_empty());
    assert_eq!(cache.snapshot_id(), None);
}

#[test]
fn salt_an_error_after_a_successful_lookup_keeps_the_earlier_one_in_both_exports() {
    // Slot 0 is answered, slot 1 is not: the attempt fails at its second lookup.
    let first = slot(TWO_WRITER, 0);
    let salt = SaltCapacities::new(id(B256::ZERO)).with(first, 512);
    let mut block = session(&input(B256::ZERO), Arc::new(salt));
    let error = block.execute_candidate(&user_deposit(TWO_WRITER), &[]).expect_err("fails");
    assert_eq!(error.disposition(), Disposition::Internal);
    let CandidateError::Block(fault) = &error else { panic!("{error:?}") };
    assert_eq!(fault.context().bucket, Some(slot(TWO_WRITER, 1)));
    // The lookup that succeeded is the attempt's and the block's; the one that failed is in
    // neither.
    assert_eq!(block.last_attempt_bucket_reads(), [first]);
    assert_eq!(*block.bucket_reads(), [first].into_iter().collect());
    // A drain after the failure still hands it over.
    let export = block.drain_bucket_reads();
    assert_eq!(*export.buckets(), [first].into_iter().collect());
    assert!(block.bucket_reads().is_empty());
}

#[test]
fn salt_a_backend_failure_is_internal_even_when_it_wraps_a_nonfatal_database_error() {
    let mut block = session(&input(B256::ZERO), Arc::new(Failing));
    let error = block.execute_candidate(&user_deposit(WRITER), &[]).expect_err("fails");
    let CandidateError::Block(fault) = &error else { panic!("{error:?}") };
    assert_eq!(fault.disposition(), Disposition::Internal);
    assert_eq!(fault.phase(), Phase::Transaction);
    assert_eq!(fault.context().bucket, Some(slot(WRITER, 0)));
    // The cause is still reachable, with its fatality.
    assert_eq!(fault.database_error().map(DatabaseError::is_fatal), Some(false));
    assert!(error.to_string().contains("bucket metadata unavailable"), "{error}");
    assert!(block.bucket_reads().is_empty());
}

#[test]
fn salt_repeated_charges_and_give_backs_across_a_revert_use_the_first_multiplier() {
    // The slot's bucket answers 768 (m = 3) once, and fails if it is asked again: the charge, its
    // give-back, the child's charge its revert takes back, and the last charge all use m = 3.
    let bucket = slot(REWRITER, 0);
    let once = AnswersOnce { bucket, first: 768, asked: Mutex::new(0) };
    let tx = legacy(SENDER, REWRITER, 5_000_000, Bytes::new());
    let (state, ..) = spend(&mut evm(Arc::new(once)), &tx);
    // The same transaction at m = 1.
    let (base, ..) = spend(&mut evm(common::minimum_salt(B256::ZERO)), &tx);
    // Exactly one slot's worth more per multiplier step: every give-back cancelled its charge.
    assert_eq!(state - base, 2 * SLOT_STATE);
    // And at m = 1 the transaction keeps one fresh slot.
    let (single, ..) = spend(
        &mut evm(common::minimum_salt(B256::ZERO)),
        &legacy(SENDER, WRITER, 5_000_000, Bytes::new()),
    );
    assert_eq!(base, single);
}

#[test]
fn salt_a_transaction_reads_each_bucket_once() {
    let (salt, queries) = counting(
        SaltCapacities::new(id(B256::ZERO))
            .with(slot(REWRITER, 0), 512)
            .with(slot(TWO_WRITER, 0), 256)
            .with(slot(TWO_WRITER, 1), 1_024),
    );
    let mut evm = evm(salt);
    spend(&mut evm, &legacy(SENDER, REWRITER, 5_000_000, Bytes::new()));
    // Three charges, a give-back and a reverted child's charge in one bucket: one read.
    assert_eq!(*queries.lock().unwrap(), [(slot(REWRITER, 0), 1)].into_iter().collect());
    assert_eq!(evm.ext().salt.lookups(), [slot(REWRITER, 0)]);
    // The next transaction is the next attempt: it reads its own buckets, each once.
    spend(&mut evm, &legacy_at(2, SENDER, TWO_WRITER, 5_000_000, Bytes::new()));
    let mut expected = vec![slot(TWO_WRITER, 0), slot(TWO_WRITER, 1)];
    expected.sort();
    assert_eq!(evm.ext().salt.lookups(), expected);
    assert_eq!(
        *queries.lock().unwrap(),
        [(slot(REWRITER, 0), 1), (slot(TWO_WRITER, 0), 1), (slot(TWO_WRITER, 1), 1)]
            .into_iter()
            .collect()
    );
}

#[test]
fn salt_the_next_attempt_of_a_block_reads_its_buckets_again() {
    let bucket = slot(WRITER, 0);
    let (salt, queries) = counting(SaltCapacities::new(id(B256::ZERO)).with(bucket, 512));
    let mut block = session(&input(B256::ZERO), salt);
    // The same deposit twice, the first candidate dropped: each attempt reads the bucket.
    let first = run(&mut block, &user_deposit(WRITER));
    assert_eq!(first.bucket_reads(), [bucket]);
    drop(first);
    let second = run(&mut block, &user_deposit(WRITER));
    assert_eq!(second.bucket_reads(), [bucket]);
    assert_eq!(queries.lock().unwrap()[&bucket], 2);
    assert_eq!(second.gas()[1], first_state_gas(512));
    // An attempt that reads nothing exports nothing, though the last one read a bucket.
    let none = run(&mut block, &user_deposit(NOOP));
    assert!(none.bucket_reads().is_empty());
    assert!(block.last_attempt_bucket_reads().is_empty());
    // Nor does one the block refuses before it runs.
    drop(run(&mut block, &user_deposit(WRITER)));
    let too_big = deposit_with_gas(SENDER, WRITER, B256::with_last_byte(0x42), 40_000_000);
    let refused = block.execute_candidate(&too_big, &[]).expect_err("over the block's gas limit");
    assert_eq!(refused.disposition(), Disposition::Refused);
    assert!(block.last_attempt_bucket_reads().is_empty());
    assert_eq!(*block.bucket_reads(), [bucket].into_iter().collect());
}

/// The state gas (less history) of a deposit to [`WRITER`] whose slot's bucket has `capacity`.
fn first_state_gas(capacity: u64) -> u64 {
    capacity / 256 * SLOT_STATE
}

#[test]
fn salt_a_dropped_candidate_and_a_workers_lookups_reach_the_block_only_through_its_drain() {
    let snapshot = SaltCapacities::new(id(B256::ZERO))
        .with(slot(WRITER, 0), 512)
        .with(slot(TWO_WRITER, 0), 512)
        .with(slot(TWO_WRITER, 1), 512)
        .with(slot(REWRITER, 0), 512);
    let shared: Arc<dyn SaltSnapshot> = Arc::new(snapshot);
    let mut builder = session(&input(B256::ZERO), shared.clone());
    let mut worker = session(&input(B256::ZERO), shared);
    // The builder's own dropped candidate stays in its union.
    drop(run(&mut builder, &user_deposit(REWRITER)));
    // The worker drops one candidate and runs another the builder commits.
    drop(run(&mut worker, &user_deposit(WRITER)));
    let executed = run(&mut worker, &user_deposit(TWO_WRITER));
    builder.commit_candidate(executed).expect("the builder commits the worker's outcome");
    // The commit adds none of the worker's lookups.
    assert_eq!(*builder.bucket_reads(), [slot(REWRITER, 0)].into_iter().collect());
    // The drain hands them over, and empties the worker's set.
    let export: SaltExport = worker.drain_bucket_reads();
    assert_eq!(export.block(), builder.identity());
    assert_eq!(export.snapshot(), builder.salt_snapshot());
    assert!(worker.bucket_reads().is_empty());
    builder.merge_bucket_reads(export).expect("an export of the same block and snapshot");
    let union = [slot(WRITER, 0), slot(TWO_WRITER, 0), slot(TWO_WRITER, 1), slot(REWRITER, 0)];
    assert_eq!(*builder.bucket_reads(), union.into_iter().collect());
    let output = builder.finish().expect("the block finishes");
    assert_eq!(output.bucket_reads, union.into_iter().collect());
    assert_eq!(output.salt_snapshot, id(B256::ZERO));
}

#[test]
fn salt_system_calls_and_system_originated_transactions_read_no_bucket() {
    // A snapshot that answers nothing: any read would fail the work that made it.
    let (salt, queries) = counting(SaltCapacities::new(id(B256::ZERO)));
    let mut evm = evm(salt.clone());
    // A system call writing a fresh slot.
    evm.ext_mut().begin_system_call();
    let executed = evm.system_call(SystemTx::new(WRITER, Bytes::new())).expect("runs at m = 1");
    let result = executed.commit();
    assert!(result.status, "{result:?}");
    assert!(evm.ext().salt.lookups().is_empty());
    // A transaction from the system address, of the minimum bucket's price.
    let mut evm = self::evm(salt.clone());
    let (state, ..) = spend(&mut evm, &legacy(SYSTEM_ADDRESS, TWO_WRITER, 5_000_000, Bytes::new()));
    assert!((2 * SLOT_STATE..3 * SLOT_STATE).contains(&state), "{state}");
    assert!(evm.ext().salt.lookups().is_empty());
    // A deposit carrying the Mega System Transaction source hash, from any caller.
    let mut evm = self::evm(salt.clone());
    spend(&mut evm, &deposit(SENDER, WRITER, MEGA_SYSTEM_TRANSACTION_SOURCE_HASH));
    assert!(evm.ext().salt.lookups().is_empty());
    // A block's pre-block calls: its EIP-2935 call runs a history contract that writes a fresh
    // slot here.
    let mut world = db();
    world.insert_account_info(&HISTORY_STORAGE, db().get_account(&WRITER).unwrap().unwrap());
    let block = session_over(&input(B256::ZERO), world, salt);
    let history = block.pre_block_states()[0].account_changes_for(&HISTORY_STORAGE);
    let written = history.expect("the call ran").storage.slots[&U256::ZERO].value.current;
    assert_eq!(written, U256::from(1));
    assert!(block.bucket_reads().is_empty());
    assert!(queries.lock().unwrap().is_empty(), "{:?}", queries.lock().unwrap());
}

#[test]
fn salt_an_ordinary_deposit_is_priced_by_its_buckets() {
    // An absent caller is created by its deposit, at the caller's bucket; the slot at its own.
    let caller = Address::with_last_byte(0xcc);
    let (salt, queries) = counting(
        SaltCapacities::new(id(B256::ZERO)).with(slot(WRITER, 0), 512).with(account(caller), 1_024),
    );
    let mut block = session(&input(B256::ZERO), salt);
    let out = run(&mut block, &deposit(caller, WRITER, B256::with_last_byte(0x42)));
    assert!(!out.result().ext.failed_deposit);
    // 4 × the caller's leaf and 2 × the slot.
    assert_eq!(out.gas()[1], 4 * ACCOUNT_STATE + 2 * SLOT_STATE);
    let mut expected = vec![slot(WRITER, 0), account(caller)];
    expected.sort();
    assert_eq!(out.bucket_reads(), expected);
    assert_eq!(queries.lock().unwrap().values().copied().collect::<Vec<_>>(), [1, 1]);
}

#[test]
fn salt_the_legacy_share_is_state_gas_and_flat_at_every_multiplier() {
    let tx = |gas_limit| legacy(SENDER, WRITER, gas_limit, Bytes::new());
    let at = |capacity| SaltCapacities::new(id(B256::ZERO)).with(slot(WRITER, 0), capacity);
    // With a reservoir (a gas limit above the execution cap): the share comes from regular gas at
    // every multiplier, and the rest of the slot's price from the reservoir.
    let reservoir = EXECUTION_CAP + 1_000_000;
    let (state1, regular1, left1) = spend(&mut evm(Arc::new(at(256))), &tx(reservoir));
    let (state2, regular2, left2) = spend(&mut evm(Arc::new(at(512))), &tx(reservoir));
    assert_eq!(regular1, regular2, "the share is not multiplied");
    assert_eq!(state2 - state1, SLOT_STATE, "the share is counted as state gas");
    assert_eq!(left1 - left2, SLOT_STATE);
    assert_eq!(state1, SLOT_STATE + BODY_AND_RECORD_HISTORY);
    // Without a reservoir regular gas pays the whole price.
    let (state1, regular1, _) = spend(&mut evm(Arc::new(at(256))), &tx(1_000_000));
    let (state2, regular2, _) = spend(&mut evm(Arc::new(at(512))), &tx(1_000_000));
    assert_eq!(state2 - state1, SLOT_STATE);
    assert_eq!(regular1, regular2);
}

/// A node's SALT store across a reorganization: one snapshot per parent block.
struct Store(BTreeMap<B256, SaltCapacities>);

impl Store {
    fn snapshot(&self, parent: B256) -> Arc<dyn SaltSnapshot> {
        Arc::new(self.0[&parent].clone())
    }
}

#[test]
fn salt_reorg_the_same_height_under_another_parent_is_another_snapshot() {
    let (a, b) = (B256::repeat_byte(0xa1), B256::repeat_byte(0xb2));
    let bucket = slot(WRITER, 0);
    let store = Store(
        [
            (a, SaltCapacities::new(id(a)).with(bucket, 512)),
            (b, SaltCapacities::new(id(b)).with(bucket, 1_024)),
        ]
        .into_iter()
        .collect(),
    );
    // Block 5 on either parent prices the same write by its own parent's state.
    let mut on_a = session(&input(a), store.snapshot(a));
    let mut on_b = session(&input(b), store.snapshot(b));
    let out_a = run(&mut on_a, &user_deposit(WRITER));
    let out_b = run(&mut on_b, &user_deposit(WRITER));
    assert_eq!((out_a.gas()[1], out_b.gas()[1]), (2 * SLOT_STATE, 4 * SLOT_STATE));
    assert_ne!(on_a.salt_snapshot(), on_b.salt_snapshot());
    // A snapshot of the other parent is refused before anything runs.
    let error = common::factory()
        .start_block(&input(b), Db::new(db()), BlockServices::new(store.snapshot(a)))
        .err()
        .expect("a snapshot of another parent");
    assert_eq!(error.disposition(), Disposition::Internal);
    assert_eq!(error.phase(), Phase::BlockInput);
    assert_eq!(
        error.kind(),
        &BlockErrorKind::Input(InputError::SaltSnapshotParent { parent_hash: b, snapshot: id(a) })
    );
    // Neither the outcome nor the lookups of one fork are taken by the other.
    let foreign = on_b.commit_candidate(out_a).expect_err("another block's outcome");
    assert_eq!(foreign.disposition(), Disposition::Retry);
    let export = on_a.drain_bucket_reads();
    let refused = on_b.merge_bucket_reads(export.clone()).expect_err("another block's lookups");
    assert_eq!(refused.export, export);
    assert_eq!(refused.snapshot, id(b));
    assert_eq!(*on_b.bucket_reads(), [bucket].into_iter().collect());
}

#[test]
fn salt_the_next_block_starts_with_an_empty_union_and_reads_again() {
    let (parent, next) = (B256::repeat_byte(0xa1), B256::repeat_byte(0xc3));
    let bucket = slot(WRITER, 0);
    let (first_salt, first_queries) = counting(SaltCapacities::new(id(parent)).with(bucket, 512));
    let (next_salt, next_queries) = counting(SaltCapacities::new(id(next)).with(bucket, 768));
    let mut block = session(&input(parent), first_salt);
    let out = run(&mut block, &user_deposit(WRITER));
    block.commit_candidate(out).expect("commits");
    assert_eq!(block.finish().expect("finishes").bucket_reads, [bucket].into_iter().collect());
    // Block 6 on block 5: its own snapshot, an empty union, its own read.
    let mut input = common::block_input(6, 1_006, 30_000_000, Some(B256::ZERO));
    input.header.parent_hash = next;
    input.parent = ParentContext::Parent { number: 5, hash: next, timestamp: 1_005 };
    let mut child = session(&input, next_salt);
    assert!(child.bucket_reads().is_empty());
    let out = run(&mut child, &user_deposit(NOOP));
    child.commit_candidate(out).expect("commits");
    assert!(child.bucket_reads().is_empty());
    let out = run(&mut child, &user_deposit(WRITER));
    assert_eq!(out.gas()[1], 3 * SLOT_STATE);
    assert_eq!(*child.bucket_reads(), [bucket].into_iter().collect());
    assert_eq!(
        (first_queries.lock().unwrap()[&bucket], next_queries.lock().unwrap()[&bucket]),
        (1, 1)
    );
}

/// Every kind of state charge carries a failed lookup out as the node's internal error, at the
/// bucket that failed: the value call's new account, a creation, a `SELFDESTRUCT`'s new
/// beneficiary, a deposit's created caller, a transaction's new value recipient and an absent
/// EIP-7702 authority, besides the fresh slot above.
#[test]
fn salt_every_charge_site_routes_a_failed_lookup_as_internal() {
    let absent = |n: u8| Address::with_last_byte(0xe0 + n);
    let caller = |code: &[u8]| common::contract(code).with_balance(U256::from(10));
    let call_value = [
        [op::PUSH1, 0, op::PUSH1, 0, op::PUSH1, 0, op::PUSH1, 0, op::PUSH1, 1, op::PUSH20]
            .as_slice(),
        absent(1).as_slice(),
        &[op::GAS, op::CALL, op::STOP],
    ]
    .concat();
    let create = [op::PUSH1, 0, op::PUSH1, 0, op::PUSH1, 0, op::CREATE, op::STOP];
    let destroy = [[op::PUSH20].as_slice(), absent(2).as_slice(), &[op::SELFDESTRUCT]].concat();
    let (value_caller, creator, destroyer) = (
        Address::with_last_byte(0xc1),
        Address::with_last_byte(0xc2),
        Address::with_last_byte(0xc3),
    );
    let mut world = db();
    world.insert_account_info(&value_caller, caller(&call_value));
    world.insert_account_info(&creator, common::contract(&create));
    world.insert_account_info(&destroyer, caller(&destroy));
    world.insert_account_info(
        &SENDER,
        AccountInfo::default().with_nonce(1).with_balance(U256::from(10).pow(U256::from(18))),
    );
    let value_tx = TxLegacy {
        chain_id: Some(1),
        nonce: 1,
        gas_price: 0,
        gas_limit: 1_000_000,
        to: TxKind::Call(absent(4)),
        value: U256::from(1),
        input: Bytes::new(),
    };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let value_tx = Recovered::new_unchecked(
        OpTxEnvelope::from_signed(value_tx.into_signed(signature)),
        SENDER,
    );
    let cases = [
        ("value call", user_deposit(value_caller), account(absent(1))),
        ("creation", user_deposit(creator), account(creator.create(1))),
        ("selfdestruct", user_deposit(destroyer), account(absent(2))),
        (
            "deposit caller",
            deposit(absent(3), NOOP, B256::with_last_byte(0x42)),
            account(absent(3)),
        ),
        ("value recipient", value_tx, account(absent(4))),
        ("authority", authorizing(absent(5)), account(absent(5))),
    ];
    for (site, tx, bucket) in cases {
        let mut block = session_over(&input(B256::ZERO), world.clone(), Arc::new(Failing));
        let error = block.execute_candidate(&tx, &[]).expect_err(site);
        let CandidateError::Block(fault) = &error else { panic!("{site}: {error:?}") };
        assert_eq!(fault.disposition(), Disposition::Internal, "{site}");
        assert_eq!(fault.phase(), Phase::Transaction, "{site}");
        assert_eq!(fault.context().bucket, Some(bucket), "{site}");
        assert!(block.receipts().is_empty() && block.bucket_reads().is_empty(), "{site}");
    }
}

/// A type-4 transaction from [`SENDER`] to [`NOOP`] whose one authorization delegates `authority`
/// (nonce zero) to [`NOOP`].
fn authorizing(authority: Address) -> Recovered<OpTxEnvelope> {
    use alloy_eips::eip7702::{Authorization, SignedAuthorization};
    let entry = SignedAuthorization::new_unchecked(
        Authorization { chain_id: U256::from(1), address: NOOP, nonce: 0 },
        0,
        U256::ZERO,
        U256::ZERO,
    );
    let tx = alloy_consensus::TxEip7702 {
        chain_id: 1,
        nonce: 1,
        gas_limit: 1_000_000,
        to: NOOP,
        authorization_list: vec![entry],
        ..Default::default()
    };
    let lazy = evm2::ethereum::LazyTxEip7702::from_cached_recovered_authorizations(
        tx,
        vec![Some(authority)],
    );
    Recovered::new_unchecked(OpTxEnvelope::new(lazy, Some(Bytes::new())), SENDER)
}

/// An absent authority's two charges — its new leaf and its delegation indicator — land in its
/// account's bucket, which the transaction reads once and exports.
#[test]
fn salt_an_authority_is_priced_by_its_bucket_once_for_both_of_its_charges() {
    let authority = Address::with_last_byte(0xe5);
    let state = |capacity| {
        let (salt, queries) =
            counting(SaltCapacities::new(id(B256::ZERO)).with(account(authority), capacity));
        let mut evm = evm(salt);
        let (state, ..) = spend(&mut evm, &authorizing(authority));
        assert_eq!(evm.ext().salt.lookups(), [account(authority)]);
        assert_eq!(*queries.lock().unwrap(), [(account(authority), 1)].into_iter().collect());
        state
    };
    // The leaf (120 bytes) and the indicator (23 bytes), each at the bucket's multiplier.
    assert_eq!(state(512) - state(256), ACCOUNT_STATE + 23 * 1_530);
}

/// A simulation's lookups are not a consensus block's, as a simulation's candidate is not.
#[test]
fn salt_a_simulations_lookups_are_not_merged_into_a_consensus_session() {
    let salt: Arc<dyn SaltSnapshot> =
        Arc::new(SaltCapacities::new(id(B256::ZERO)).with(slot(WRITER, 0), 512));
    let simulation =
        mega_evm2::factory::Simulation { skip_nonce_check: true, ..Default::default() };
    let mut simulated = common::factory()
        .start_simulated_block(
            &input(B256::ZERO),
            Db::new(db()),
            BlockServices::new(salt.clone()),
            &simulation,
        )
        .expect("the simulated block starts");
    drop(run(&mut simulated, &user_deposit(WRITER)));
    let export = simulated.drain_bucket_reads();
    assert!(export.is_simulated());
    let mut consensus = session(&input(B256::ZERO), salt);
    let refused = consensus.merge_bucket_reads(export.clone()).expect_err("other rules");
    assert!(!refused.simulated);
    assert_eq!(refused.export, export);
    assert!(consensus.bucket_reads().is_empty());
}
