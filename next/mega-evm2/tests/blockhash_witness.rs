//! The block hashes of a block's witness (`SATIN-WIT-014`, `SATIN-WIT-016`): block execution
//! exports every hash `BLOCKHASH` was served, recorded where it is served and not where a database
//! is reached, emptied when the block starts; and a block replayed on exactly its witness produces
//! the same block and reads no hash the witness does not hold.

mod common;

use alloy_consensus::transaction::Recovered;
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, keccak256};
use evm2::{
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, CacheDB, Database, Db, DynDatabase, InMemoryDB, PendingState},
};
use mega_evm2::{
    BlockHashMergeRefusal, BucketId, MIN_BUCKET_SIZE, MegaTypes, SaltCapacities,
    block::{BlockOutput, BlockSession, HISTORY_STORAGE, encode_receipt},
    error::{CandidateError, Disposition, Phase},
    factory::BlockServices,
};
use op_evm2::{OpTxEnvelope, TxDeposit};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, HashMap},
    rc::Rc,
    sync::Arc,
};

const SENDER: Address = Address::with_last_byte(0x50);
/// Reads the hash of every block its calldata names, a word each ([`reader`]).
const READER: Address = Address::with_last_byte(0xa0);
/// Reads the hash of the block its calldata names, then reverts.
const REVERTER: Address = Address::with_last_byte(0xb0);
/// Calls [`REVERTER`] with its own calldata and stops, whatever the call returned.
const OUTER: Address = Address::with_last_byte(0xc0);
/// Switches its volatile-data access off, then reads the hash of the block its calldata names.
const REFUSED: Address = Address::with_last_byte(0xd0);
const NOOP: Address = Address::with_last_byte(0xe0);
const ACCESS_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04]);

/// The block the tests execute, and the oldest and the newest block whose hash it can read.
const BLOCK: u64 = 1_000;
const OLDEST: u64 = BLOCK - 256;
const NEWEST: u64 = BLOCK - 1;

/// The chain's hash of block `number` in the fixture.
fn hash_of(number: u64) -> B256 {
    keccak256(format!("block {number}"))
}

/// For each of the first `reads` words `n` of the calldata: `SSTORE(n, BLOCKHASH(n))` and a
/// `LOG0` of the word `BLOCKHASH` pushed, so the state and the receipt both carry it.
fn reader(reads: u8) -> Vec<u8> {
    let mut code = Vec::new();
    for i in 0..reads {
        // PUSH1 32·i CALLDATALOAD DUP1 BLOCKHASH DUP1 PUSH0 MSTORE SWAP1 SSTORE
        code.extend_from_slice(&[0x60, 32 * i, 0x35, 0x80, 0x40, 0x80, 0x5f, 0x52, 0x90, 0x55]);
        // PUSH1 32 PUSH0 LOG0
        code.extend_from_slice(&[0x60, 0x20, 0x5f, 0xa0]);
    }
    code.push(0x00);
    code
}

/// `BLOCKHASH(calldata[0])`, then `REVERT(0, 0)`.
fn reverter() -> Vec<u8> {
    vec![0x5f, 0x35, 0x40, 0x5f, 0x5f, 0xfd]
}

/// `CALL(GAS, REVERTER, 0, 0, CALLDATASIZE, 0, 0)` of its own calldata, then `STOP`.
fn outer() -> Vec<u8> {
    let mut code = vec![0x36, 0x5f, 0x5f, 0x37, 0x5f, 0x5f, 0x36, 0x5f, 0x5f, 0x73];
    code.extend_from_slice(REVERTER.as_slice());
    code.extend_from_slice(&[0x5a, 0xf1, 0x00]);
    code
}

/// `disableVolatileDataAccess()` through a `CALL` of 100,000 gas, then `BLOCKHASH(calldata[0])`
/// in the same frame, whose access is then off.
fn refused() -> Vec<u8> {
    let mut code = vec![0x63];
    code.extend_from_slice(&keccak256("disableVolatileDataAccess()")[..4]);
    code.extend_from_slice(&[0x60, 0xe0, 0x1b, 0x5f, 0x52]);
    code.extend_from_slice(&[0x5f, 0x5f, 0x60, 0x04, 0x5f, 0x5f, 0x73]);
    code.extend_from_slice(ACCESS_CONTROL.as_slice());
    code.extend_from_slice(&[0x62, 0x01, 0x86, 0xa0, 0xf1, 0x50]);
    code.extend_from_slice(&[0x5f, 0x35, 0x40, 0x00]);
    code
}

fn code(code: Vec<u8>) -> AccountInfo {
    AccountInfo::default().with_nonce(1).with_code(Bytecode::new_legacy(code.into()))
}

/// The chain before block [`BLOCK`]: the fixture's accounts, and the hash of every block the
/// block can read and of a few it cannot (older than the window).
fn chain() -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&SENDER, AccountInfo::default().with_nonce(1));
    db.insert_account_info(&NOOP, AccountInfo::default().with_nonce(1));
    db.insert_account_info(&READER, code(reader(3)));
    db.insert_account_info(&REVERTER, code(reverter()));
    db.insert_account_info(&OUTER, code(outer()));
    db.insert_account_info(&REFUSED, code(refused()));
    for number in OLDEST - 4..BLOCK {
        db.insert_block_hash(&U256::from(number), &hash_of(number));
    }
    db
}

/// A database that counts the block-hash reads that reach it: what a recorder under a cache sees.
#[derive(Debug)]
struct Counting {
    inner: InMemoryDB,
    reached: Rc<RefCell<Vec<u64>>>,
}

impl Counting {
    fn new(inner: InMemoryDB) -> (Self, Rc<RefCell<Vec<u64>>>) {
        let reached = Rc::new(RefCell::new(Vec::new()));
        (Self { inner, reached: reached.clone() }, reached)
    }
}

impl Database for Counting {
    type Error = <InMemoryDB as Database>::Error;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        Database::get_account(&mut self.inner, address)
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        Database::get_code_by_hash(&mut self.inner, code_hash)
    }

    fn get_storage(&mut self, address: &Address, key: &U256) -> Result<U256, Self::Error> {
        Database::get_storage(&mut self.inner, address, key)
    }

    fn get_block_hash(&mut self, number: &U256) -> Result<B256, Self::Error> {
        self.reached.borrow_mut().push(number.to::<u64>());
        Database::get_block_hash(&mut self.inner, number)
    }
}

/// What a witness does not hold.
#[derive(Clone, Debug, PartialEq, Eq)]
enum NotInWitness {
    Account(Address),
    Slot(Address, U256),
    Code(B256),
    BlockHash(U256),
}

impl core::fmt::Display for NotInWitness {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "not in the witness: {self:?}")
    }
}

impl std::error::Error for NotInWitness {}

/// A stateless validator's database: exactly a block's witness, and nothing else. A read of
/// anything the witness does not hold fails; none is answered with a default.
#[derive(Clone, Debug, Default)]
struct Witness {
    accounts: HashMap<Address, Option<AccountInfo>>,
    slots: HashMap<(Address, U256), U256>,
    codes: HashMap<B256, Bytecode>,
    block_hashes: BTreeMap<u64, B256>,
}

impl Witness {
    /// The witness of a block executed on `chain`: every account and slot `states` (the
    /// pre-block states and the included transactions') name, as the chain held it before the
    /// block — an absent account as absent, with no slot — its code, and the block hashes the
    /// block's execution exported.
    fn of(chain: &InMemoryDB, states: &[PendingState], hashes: &BTreeMap<u64, B256>) -> Self {
        let mut chain = chain.clone();
        let mut witness = Self { block_hashes: hashes.clone(), ..Self::default() };
        for state in states {
            for account in state.account_changes() {
                let address = account.address;
                let info = Database::get_account(&mut chain, &address).expect("the chain");
                if let Some(info) = &info {
                    // Its code, whether or not the load that named the account loaded it.
                    if !info.is_empty_code_hash() {
                        let code = Database::get_code_by_hash(&mut chain, &info.code_hash);
                        witness.codes.insert(info.code_hash, code.expect("the chain"));
                    }
                    for key in account.storage.slots.keys() {
                        let value = Database::get_storage(&mut chain, &address, key).unwrap();
                        witness.slots.insert((address, *key), value);
                    }
                }
                witness.accounts.insert(address, info);
            }
        }
        witness
    }
}

impl Database for Witness {
    type Error = NotInWitness;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.accounts.get(address).cloned().ok_or(NotInWitness::Account(*address))
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        self.codes.get(code_hash).cloned().ok_or(NotInWitness::Code(*code_hash))
    }

    fn get_storage(&mut self, address: &Address, key: &U256) -> Result<U256, Self::Error> {
        match self.accounts.get(address) {
            // An account the chain does not hold has no storage: zero, without a lookup.
            Some(None) => Ok(U256::ZERO),
            Some(Some(_)) => {
                self.slots.get(&(*address, *key)).copied().ok_or(NotInWitness::Slot(*address, *key))
            }
            None => Err(NotInWitness::Account(*address)),
        }
    }

    fn get_block_hash(&mut self, number: &U256) -> Result<B256, Self::Error> {
        let held = u64::try_from(*number).ok().and_then(|n| self.block_hashes.get(&n));
        held.copied().ok_or(NotInWitness::BlockHash(*number))
    }
}

/// Block `number` over `database`, every SALT bucket at the minimum capacity.
fn block_over<'a>(number: u64, database: impl DynDatabase + 'a) -> BlockSession<'a> {
    let input = common::block_input(number, 10_000 + number, 30_000_000, Some(B256::ZERO));
    common::start_block(&input, database)
}

/// Block [`BLOCK`] over the fixture's chain.
fn block() -> BlockSession<'static> {
    block_over(BLOCK, Db::new(chain()))
}

/// A deposit from [`SENDER`] calling `to` with the block numbers `numbers` as calldata words.
fn deposit_to(to: Address, numbers: &[U256]) -> TxDeposit {
    let input: Vec<u8> = numbers.iter().flat_map(|n| n.to_be_bytes::<32>()).collect();
    TxDeposit {
        source_hash: B256::with_last_byte(0x42),
        from: SENDER,
        to: TxKind::Call(to),
        mint: 0,
        value: U256::ZERO,
        gas_limit: 2_000_000,
        is_system_transaction: false,
        input: Bytes::from(input),
    }
}

fn recovered(tx: TxDeposit) -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(OpTxEnvelope::deposit(tx), SENDER)
}

/// [`deposit_to`], as a block takes it.
fn deposit(to: Address, numbers: &[U256]) -> Recovered<OpTxEnvelope> {
    recovered(deposit_to(to, numbers))
}

/// A deposit that has [`READER`] read the hashes of the blocks `numbers`.
fn read(numbers: &[u64]) -> Recovered<OpTxEnvelope> {
    deposit(READER, &numbers.iter().map(|n| U256::from(*n)).collect::<Vec<_>>())
}

/// `numbers` with the fixture's hashes: what an export of exactly those reads holds.
fn served(numbers: &[u64]) -> BTreeMap<u64, B256> {
    numbers.iter().map(|n| (*n, hash_of(*n))).collect()
}

/// Executes and commits `txs` in order, and returns each one's state.
fn include(block: &mut BlockSession<'_>, txs: &[Recovered<OpTxEnvelope>]) -> Vec<PendingState> {
    txs.iter()
        .map(|tx| {
            let candidate = block.execute_candidate(tx, &[]).expect("the candidate runs");
            let state = candidate.pending().clone();
            block.commit_candidate(candidate).expect("the candidate commits");
            state
        })
        .collect()
}

/// The states a finished block's witness is built from: its pre-block states, then its included
/// transactions'.
fn witness_states(output: &BlockOutput, included: &[PendingState]) -> Vec<PendingState> {
    output.pre_block_states.iter().chain(included).cloned().collect()
}

/// Replays block [`BLOCK`] with `txs` on `witness` alone, the SALT snapshot holding exactly the
/// buckets `buckets` (a bucket it does not list is a failed lookup).
fn replay(
    witness: Witness,
    buckets: &BTreeSet<BucketId>,
    txs: &[Recovered<OpTxEnvelope>],
) -> Result<BlockOutput, CandidateError> {
    let mut salt = SaltCapacities::new(common::snapshot_id(B256::ZERO));
    for bucket in buckets {
        salt.insert(*bucket, MIN_BUCKET_SIZE);
    }
    let input = common::block_input(BLOCK, 10_000 + BLOCK, 30_000_000, Some(B256::ZERO));
    let services = BlockServices::new(Arc::new(salt));
    let mut block = common::factory()
        .start_block(&input, Db::new(witness), services)
        .expect("the pre-block sequence reads only the witness");
    for tx in txs {
        let candidate = block.execute_candidate(tx, &[])?;
        block.commit_candidate(candidate)?;
    }
    Ok(block.finish().expect("the block finishes"))
}

/// The block's export holds the number and the hash of every block `BLOCKHASH` was served, the
/// two ends of the window included, as the candidate that read them does.
#[test]
fn blockhash_witness_export_has_the_number_and_the_hash() {
    let mut block = block();
    // The pre-block sequence read no block hash.
    assert!(block.block_hash_reads().is_empty());
    let candidate = block.execute_candidate(&read(&[NEWEST, OLDEST, 850]), &[]).unwrap();
    assert!(candidate.result().status);
    let expected = served(&[OLDEST, 850, NEWEST]);
    let own: Vec<(u64, B256)> = expected.iter().map(|(n, h)| (*n, *h)).collect();
    assert_eq!(candidate.block_hash_reads(), own);
    assert_eq!(block.last_attempt_block_hash_reads().unwrap(), own);
    // The word `BLOCKHASH` pushed is the hash recorded: the receipt's logs carry it.
    let pushed: Vec<B256> =
        candidate.result().logs.iter().map(|log| B256::from_slice(&log.data.data)).collect();
    assert_eq!(pushed, [hash_of(NEWEST), hash_of(OLDEST), hash_of(850)]);
    block.commit_candidate(candidate).unwrap();
    assert_eq!(*block.block_hash_reads(), expected);
    // A candidate refused before it runs (its gas limit over the block's) is an attempt of its
    // own, served nothing.
    let mut over = TxDeposit { gas_limit: 30_000_001, ..deposit_to(READER, &[U256::from(851)]) };
    let refused = block.execute_candidate(&recovered(over.clone()), &[]).expect_err("no room");
    assert_eq!(refused.disposition(), Disposition::Refused);
    assert!(block.last_attempt_block_hash_reads().unwrap().is_empty());
    assert_eq!(*block.block_hash_reads(), expected);
    // The same candidate with room is served its hash.
    over.gas_limit = 2_000_000;
    let candidate = block.execute_candidate(&recovered(over), &[]).unwrap();
    assert_eq!(candidate.block_hash_reads(), [(851, hash_of(851))]);
    drop(candidate);
    let expected = served(&[OLDEST, 850, 851, NEWEST]);
    // Committing added nothing, and a transaction that reads none adds none.
    include(&mut block, &[deposit(NOOP, &[])]);
    assert!(block.last_attempt_block_hash_reads().unwrap().is_empty());
    assert_eq!(*block.block_hash_reads(), expected);
    assert_eq!(block.finish().expect("the block finishes").block_hash_reads, expected);
}

/// A hash a cache answers is exported as one the database answers: the second transaction's read
/// of a hash the first one read never reaches the database, and is in that transaction's own
/// record and in the block's export all the same.
#[test]
fn blockhash_witness_cache_served_second_read_is_exported() {
    let (database, reached) = Counting::new(chain());
    let mut block = block_over(BLOCK, Db::new(database));
    // One transaction reading the same hash twice: the database is reached once.
    let first = block.execute_candidate(&read(&[900, 900]), &[]).unwrap();
    assert_eq!(first.block_hash_reads(), [(900, hash_of(900))]);
    block.commit_candidate(first).unwrap();
    assert_eq!(*reached.borrow(), [900]);
    // Another transaction reading it again, and one more hash: only the new one reaches the
    // database.
    let second = block.execute_candidate(&read(&[900, 901]), &[]).unwrap();
    assert_eq!(*reached.borrow(), [900, 901]);
    assert_eq!(second.block_hash_reads(), [(900, hash_of(900)), (901, hash_of(901))]);
    assert_eq!(second.result().logs[0].data.data[..], hash_of(900)[..]);
    block.commit_candidate(second).unwrap();
    // A third whose only read the cache answers: its record still holds the read.
    let third = block.execute_candidate(&read(&[901]), &[]).unwrap();
    assert_eq!(*reached.borrow(), [900, 901]);
    assert_eq!(third.block_hash_reads(), [(901, hash_of(901))]);
    assert_eq!(*block.block_hash_reads(), served(&[900, 901]));
}

/// The export is emptied when a block starts, whatever a cache under the block holds: a block
/// over a state cache an earlier block filled exports only what its own execution was served —
/// nothing when it reads no hash, and a hash the cache answers when it reads that one.
#[test]
fn blockhash_witness_cleared_when_the_next_block_starts() {
    let (database, reached) = Counting::new(chain());
    // The node's state cache, kept across blocks.
    let mut cache = CacheDB::new(Db::new(database));
    let mut first = block_over(BLOCK, &mut cache);
    include(&mut first, &[read(&[900, 901])]);
    assert_eq!(first.finish().unwrap().block_hash_reads, served(&[900, 901]));
    assert_eq!(*reached.borrow(), [900, 901]);
    for number in [900, 901] {
        assert_eq!(cache.cache.block_hashes.get(&U256::from(number)), Some(&hash_of(number)));
    }
    // The next block reads no hash: its export is empty, though the cache holds two.
    let mut second = block_over(BLOCK + 1, &mut cache);
    assert!(second.block_hash_reads().is_empty());
    include(&mut second, &[deposit(NOOP, &[])]);
    assert!(second.block_hash_reads().is_empty());
    assert!(second.finish().unwrap().block_hash_reads.is_empty());
    // The one after reads one of them: the cache answers, the database is not reached, and the
    // export holds that hash and not the other one the cache holds.
    let mut third = block_over(BLOCK + 2, &mut cache);
    assert!(third.block_hash_reads().is_empty());
    include(&mut third, &[read(&[901])]);
    assert_eq!(*reached.borrow(), [900, 901]);
    assert_eq!(third.finish().unwrap().block_hash_reads, served(&[901]));
}

/// A dropped candidate's reads stay in the building node's export (`SATIN-WIT-016`: the building
/// node's exports also hold the block hashes that candidates it executed and dropped asked for),
/// and a replay of the block, which never runs the candidate, exports a proper subset of them.
#[test]
fn blockhash_witness_dropped_candidate_reads_stay_in_the_export() {
    let chain = chain();
    let mut block = block_over(BLOCK, Db::new(chain.clone()));
    let dropped = block.execute_candidate(&read(&[800]), &[]).unwrap();
    assert_eq!(dropped.block_hash_reads(), [(800, hash_of(800))]);
    drop(dropped);
    let txs = [read(&[900])];
    let included = include(&mut block, &txs);
    assert_eq!(*block.block_hash_reads(), served(&[800, 900]));
    let output = block.finish().unwrap();
    assert_eq!(output.block_hash_reads, served(&[800, 900]));
    // The validator's replay reads only what the included transaction read.
    let states = witness_states(&output, &included);
    let witness = Witness::of(&chain, &states, &output.block_hash_reads);
    let replayed = replay(witness, &output.bucket_reads, &txs).expect("the replay runs");
    assert_eq!(replayed.block_hash_reads, served(&[900]));
    assert!(
        replayed.block_hash_reads.iter().all(|(n, h)| output.block_hash_reads.get(n) == Some(h))
    );
    assert_ne!(replayed.block_hash_reads, output.block_hash_reads);
}

/// A block replayed on exactly its witness — the accounts, slots and code its states name, the
/// SALT buckets and the block hashes it exported — produces the same receipts, state changes, gas
/// ledgers, logs and block counters, and the same exports.
#[test]
fn blockhash_witness_replay_on_the_witness_gives_identical_results() {
    let chain = chain();
    let txs = [
        read(&[NEWEST, OLDEST, 850]),
        deposit(NOOP, &[]),
        // A hash the block has read already, a new one, and one outside the window.
        read(&[850, 851, OLDEST - 1]),
        // A read in a frame that reverts.
        deposit(OUTER, &[U256::from(852)]),
    ];
    let mut block = block_over(BLOCK, Db::new(chain.clone()));
    let included = include(&mut block, &txs);
    let output = block.finish().unwrap();
    assert_eq!(output.block_hash_reads, served(&[OLDEST, 850, 851, 852, NEWEST]));
    assert_eq!(output.receipts.len(), 4);
    assert!(output.receipts.iter().all(|receipt| receipt.status));

    let states = witness_states(&output, &included);
    let witness = Witness::of(&chain, &states, &output.block_hash_reads);
    assert_eq!(witness.block_hashes.len(), 5);
    let replayed = replay(witness, &output.bucket_reads, &txs).expect("the replay runs");

    let encoded = |o: &BlockOutput| o.receipts.iter().map(encode_receipt).collect::<Vec<_>>();
    assert_eq!(encoded(&replayed), encoded(&output));
    assert_eq!(replayed.changes, output.changes);
    assert_eq!(replayed.pre_block_states, output.pre_block_states);
    assert_eq!(format!("{:?}", replayed.limiter), format!("{:?}", output.limiter));
    assert_eq!(replayed.system_address, output.system_address);
    assert_eq!(replayed.bucket_reads, output.bucket_reads);
    assert_eq!(replayed.block_hash_reads, output.block_hash_reads);
    // The state carries the hashes served: the reader's slots, by block number.
    let slot = |number: u64| {
        let key = U256::from(number);
        replayed
            .changes
            .storage()
            .find(|(k, _)| k.address() == READER && k.key() == key)
            .map(|(_, tracked)| B256::from(tracked.current))
    };
    for number in [OLDEST, 850, 851, NEWEST] {
        assert_eq!(slot(number), Some(hash_of(number)), "block {number}");
    }
    assert_eq!(slot(OLDEST - 1), None);
}

/// A replay on a witness short of one served hash fails at the read: the transaction has no
/// outcome, as a read the node cannot make has none — it is not served zero, nor a hash from
/// anywhere else — and the block cannot finish.
#[test]
fn blockhash_witness_missing_hash_fails_the_replay_at_the_read() {
    let chain = chain();
    let txs = [read(&[850, 851]), read(&[852])];
    let mut block = block_over(BLOCK, Db::new(chain.clone()));
    let included = include(&mut block, &txs);
    let output = block.finish().unwrap();
    assert_eq!(output.block_hash_reads, served(&[850, 851, 852]));
    let states = witness_states(&output, &included);
    for missing in [850, 851, 852] {
        let mut hashes = output.block_hash_reads.clone();
        hashes.remove(&missing);
        let witness = Witness::of(&chain, &states, &hashes);
        let error = replay(witness, &output.bucket_reads, &txs).expect_err("the read fails");
        let CandidateError::Block(fault) = &error else { panic!("not the block's: {error:?}") };
        assert_eq!(fault.disposition(), Disposition::Internal, "block {missing}");
        assert_eq!(fault.phase(), Phase::Transaction);
        let database = fault.database_error().expect("a database error");
        assert!(database.is_fatal());
        assert_eq!(
            database.downcast_ref::<NotInWitness>(),
            Some(&NotInWitness::BlockHash(U256::from(missing))),
        );
    }
    // The whole witness replays.
    let witness = Witness::of(&chain, &states, &output.block_hash_reads);
    replay(witness, &output.bucket_reads, &txs).expect("the replay runs");
}

/// An attempt that fails keeps what it was served before it failed: the block's export holds the
/// hash the database answered, not the one it could not, and the session runs nothing after it.
#[test]
fn blockhash_witness_failed_attempt_keeps_what_it_was_served() {
    let chain = chain();
    let states = {
        let mut block = block_over(BLOCK, Db::new(chain.clone()));
        let included = include(&mut block, &[read(&[850, 851])]);
        witness_states(&block.finish().unwrap(), &included)
    };
    let witness = Witness::of(&chain, &states, &served(&[850]));
    let mut block = block_over(BLOCK, Db::new(witness));
    let error = block.execute_candidate(&read(&[850, 851]), &[]).expect_err("the read fails");
    assert_eq!(error.disposition(), Disposition::Internal);
    assert_eq!(block.last_attempt_block_hash_reads().unwrap(), [(850, hash_of(850))]);
    assert_eq!(*block.block_hash_reads(), served(&[850]));
    assert!(block.execute_candidate(&deposit(NOOP, &[]), &[]).is_err());
    // The drained export is available after the failure, and empties the session's.
    let export = block.drain_block_hash_reads();
    assert_eq!(*export.hashes(), served(&[850]));
    assert!(block.block_hash_reads().is_empty());
    assert!(block.finish().is_err());
}

/// A pre-block system call executes the code its target holds, and a `BLOCKHASH` in it is served
/// and exported as a transaction's is: the block's export holds it before any transaction runs,
/// and a replay on the witness makes the same call on the same hash.
#[test]
fn blockhash_witness_system_call_read_is_exported() {
    // The EIP-2935 contract's address holding code that stores the parent's hash as `BLOCKHASH`
    // serves it: PUSH2 999 BLOCKHASH PUSH0 SSTORE STOP.
    let mut chain = chain();
    let [hi, lo] = (NEWEST as u16).to_be_bytes();
    chain.insert_account_info(&HISTORY_STORAGE, code(vec![0x61, hi, lo, 0x40, 0x5f, 0x55, 0x00]));
    let (database, reached) = Counting::new(chain.clone());
    let mut block = block_over(BLOCK, Db::new(database));
    assert_eq!(*block.block_hash_reads(), served(&[NEWEST]));
    assert_eq!(*reached.borrow(), [NEWEST]);
    assert_eq!(
        common::committed_slot(&block, HISTORY_STORAGE, U256::ZERO),
        Some(U256::from_be_bytes(hash_of(NEWEST).0)),
    );
    // A transaction's attempt begins with an empty record, and adds its own reads to the call's.
    let txs = [read(&[NEWEST, 850])];
    let candidate = block.execute_candidate(&deposit(NOOP, &[]), &[]).unwrap();
    assert!(candidate.block_hash_reads().is_empty());
    drop(candidate);
    let included = include(&mut block, &txs);
    assert_eq!(*reached.borrow(), [NEWEST, 850]);
    let output = block.finish().unwrap();
    assert_eq!(output.block_hash_reads, served(&[850, NEWEST]));
    // The replay: its pre-block call reads the hash off the witness, and fails without it.
    let states = witness_states(&output, &included);
    let witness = Witness::of(&chain, &states, &output.block_hash_reads);
    let replayed = replay(witness, &output.bucket_reads, &txs).expect("the replay runs");
    assert_eq!(replayed.changes, output.changes);
    assert_eq!(replayed.block_hash_reads, output.block_hash_reads);
    let witness = Witness::of(&chain, &states, &served(&[850]));
    let input = common::block_input(BLOCK, 10_000 + BLOCK, 30_000_000, Some(B256::ZERO));
    let services = common::services(B256::ZERO);
    let Err(fault) = common::factory().start_block(&input, Db::new(witness), services) else {
        panic!("the pre-block call read a hash the witness does not hold")
    };
    assert_eq!(fault.disposition(), Disposition::Internal);
    assert_eq!(
        fault.database_error().and_then(|e| e.downcast_ref::<NotInWitness>()),
        Some(&NotInWitness::BlockHash(U256::from(NEWEST))),
    );
}

/// A `BLOCKHASH` of the current block, of a future block or of a block older than the window
/// pushes zero, reads nothing and is not recorded (`SATIN-WIT-012`); the two ends of the window
/// are read.
#[test]
fn blockhash_witness_out_of_range_reads_nothing_and_pushes_zero() {
    let pushed = |tx: &Recovered<OpTxEnvelope>| {
        let (database, reached) = Counting::new(chain());
        let mut block = block_over(BLOCK, Db::new(database));
        let candidate = block.execute_candidate(tx, &[]).unwrap();
        assert!(candidate.result().status);
        let words: Vec<B256> =
            candidate.result().logs.iter().map(|log| B256::from_slice(&log.data.data)).collect();
        let reads = candidate.block_hash_reads().to_vec();
        assert_eq!(*block.block_hash_reads(), reads.iter().copied().collect());
        (words, reads, reached.borrow().clone())
    };
    // The current block, the next one and the one just older than the window: the fixture's
    // chain holds a hash for the last, which is not served.
    let out_of_range = [BLOCK, BLOCK + 1, OLDEST - 1].map(U256::from);
    assert_eq!(pushed(&deposit(READER, &out_of_range)), (vec![B256::ZERO; 3], vec![], vec![]));
    // A number no block has, and block zero from a block more than 256 after it.
    let far = [U256::MAX, U256::from(1) << 64, U256::ZERO];
    assert_eq!(pushed(&deposit(READER, &far)), (vec![B256::ZERO; 3], vec![], vec![]));
    // Between two out-of-range reads, the ends of the window are served.
    let (words, reads, reached) = pushed(&read(&[OLDEST, BLOCK, NEWEST]));
    assert_eq!(words, [hash_of(OLDEST), B256::ZERO, hash_of(NEWEST)]);
    assert_eq!(reads, [(OLDEST, hash_of(OLDEST)), (NEWEST, hash_of(NEWEST))]);
    assert_eq!(reached, [OLDEST, NEWEST]);
}

/// A read in a frame that reverts was served all the same, and is exported: a failed frame's
/// writes are taken back, not its reads.
#[test]
fn blockhash_witness_reverted_frame_read_is_exported() {
    let mut block = block();
    let candidate = block.execute_candidate(&deposit(OUTER, &[U256::from(750)]), &[]).unwrap();
    assert!(candidate.result().status);
    assert_eq!(candidate.block_hash_reads(), [(750, hash_of(750))]);
    // A transaction whose own frame reverts after the read.
    let candidate = block.execute_candidate(&deposit(REVERTER, &[U256::from(751)]), &[]).unwrap();
    assert!(!candidate.result().status);
    assert_eq!(candidate.block_hash_reads(), [(751, hash_of(751))]);
    assert_eq!(*block.block_hash_reads(), served(&[750, 751]));
}

/// A `BLOCKHASH` refused in a frame whose volatile-data access is off is not served: nothing is
/// read and nothing recorded (`SATIN-WIT-012`).
#[test]
fn blockhash_witness_refused_read_is_not_recorded() {
    let (database, reached) = Counting::new(chain());
    let mut block = block_over(BLOCK, Db::new(database));
    let candidate = block.execute_candidate(&deposit(REFUSED, &[U256::from(760)]), &[]).unwrap();
    // The frame reverted with `VolatileDataAccessDisabled(7)`.
    assert!(!candidate.result().status);
    let mut refusal = keccak256("VolatileDataAccessDisabled(uint8)")[..4].to_vec();
    refusal.extend_from_slice(&U256::from(7).to_be_bytes::<32>());
    assert_eq!(candidate.result().output[..], refusal[..]);
    assert!(candidate.block_hash_reads().is_empty());
    assert!(block.block_hash_reads().is_empty());
    assert!(reached.borrow().is_empty());
}

/// A single transaction's EVM, outside a block session, exposes the hashes its last transaction
/// was served (`Lite::block_hashes`), emptied when the next transaction begins.
#[test]
fn blockhash_witness_single_transaction_evm_exposes_its_own_set() {
    let env = BlockEnv::<MegaTypes> {
        number: U256::from(BLOCK),
        timestamp: U256::from(10_000 + BLOCK),
        gas_limit: U256::from(30_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = common::evm(env, Db::new(chain()));
    assert!(evm.ext().block_hashes.is_empty());
    let result = evm.transact(&read(&[900, 901, 900])).expect("valid").commit();
    assert!(result.status);
    let word = U256::from;
    assert_eq!(
        evm.ext().block_hashes.served(),
        [(word(900), hash_of(900)), (word(901), hash_of(901))]
    );
    // The next transaction reads another hash; the overlay still holds the first two.
    let result = evm.transact(&read(&[902])).expect("valid").commit();
    assert!(result.status);
    assert_eq!(evm.ext().block_hashes.served(), [(word(902), hash_of(902))]);
    assert_eq!(evm.overlay_db().cache.block_hashes.len(), 3);
    let result = evm.transact(&deposit(NOOP, &[])).expect("valid").commit();
    assert!(result.status);
    assert!(evm.ext().block_hashes.is_empty());
}

/// A session that executed candidates for another one hands its served hashes over, and the
/// builder's takes them into its union; an export of another block or of other rules, and one
/// that holds another hash for a block the session was served, is refused whole and handed back.
#[test]
fn blockhash_witness_merge_takes_another_sessions_reads() {
    let mut worker = block();
    let candidate = worker.execute_candidate(&read(&[800, 801]), &[]).unwrap();
    let mut builder = block();
    // Committing an outcome another session executed adds no read: they come with the export.
    builder.commit_candidate(candidate).unwrap();
    assert!(builder.block_hash_reads().is_empty());
    include(&mut builder, &[read(&[801, 900])]);
    assert_eq!(*builder.block_hash_reads(), served(&[801, 900]));
    let export = worker.drain_block_hash_reads();
    assert!(worker.block_hash_reads().is_empty());
    assert_eq!(export.block(), builder.identity());
    assert!(!export.is_simulated());
    assert_eq!(*export.hashes(), served(&[800, 801]));
    builder.merge_block_hash_reads(export).expect("the same block");
    assert_eq!(*builder.block_hash_reads(), served(&[800, 801, 900]));

    // Another block's.
    let mut other = block_over(BLOCK + 1, Db::new(chain()));
    include(&mut other, &[read(&[802])]);
    let refused = builder.merge_block_hash_reads(other.drain_block_hash_reads()).unwrap_err();
    assert_eq!(
        refused.reason,
        BlockHashMergeRefusal::Foreign { block: builder.identity(), simulated: false }
    );
    assert_eq!(*refused.export.hashes(), served(&[802]));
    assert!(refused.to_string().contains("merged into block 1000"), "{refused}");

    // A simulation's.
    let input = common::block_input(BLOCK, 10_000 + BLOCK, 30_000_000, Some(B256::ZERO));
    let mut simulated = common::factory()
        .start_simulated_block(
            &input,
            Db::new(chain()),
            common::services(B256::ZERO),
            &Default::default(),
        )
        .unwrap();
    drop(simulated.execute_candidate(&read(&[803]), &[]).unwrap());
    let refused = builder.merge_block_hash_reads(simulated.drain_block_hash_reads()).unwrap_err();
    assert!(matches!(refused.reason, BlockHashMergeRefusal::Foreign { .. }), "{refused:?}");
    assert!(refused.export.is_simulated());

    // The same block on a chain that holds another hash for block 900: neither is taken.
    let mut fork = chain();
    let conflicting = B256::repeat_byte(0xee);
    fork.insert_block_hash(&U256::from(900), &conflicting);
    let mut forked = block_over(BLOCK, Db::new(fork));
    drop(forked.execute_candidate(&read(&[804, 900]), &[]).unwrap());
    let refused = builder.merge_block_hash_reads(forked.drain_block_hash_reads()).unwrap_err();
    assert_eq!(
        refused.reason,
        BlockHashMergeRefusal::Conflict { number: 900, held: hash_of(900), merged: conflicting }
    );
    assert!(refused.to_string().contains("different hashes of block 900"), "{refused}");
    // Nothing of a refused export was taken.
    assert_eq!(*builder.block_hash_reads(), served(&[800, 801, 900]));
}
