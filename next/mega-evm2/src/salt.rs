//! SALT, as the engine prices state by it.
//!
//! - **The mapping** ([`bucket_for`]): the bucket an account or a storage slot lands in is SALT's
//!   own definition (`SATIN-STATE-010`): the production hasher (`salt_hash`), the default; feature
//!   `salt-test-hasher` swaps in the differential scenarios' test hasher. Ids are [`BucketId`]s,
//!   SALT's 32-bit bucket ids, and every conversion into one is checked.
//! - **The snapshot** ([`SaltSnapshot`]): the node's fallible reader of the bucket capacities of
//!   one immutable state, the parent state of the block it serves, named by [`SnapshotId`]. The
//!   database the block executes on reads the same state. A capacity the snapshot cannot give is an
//!   error, never a default. [`SaltCapacities`] is an in-memory snapshot whose policy for a bucket
//!   it does not list says what it does: [`Unlisted::Refuse`], production's, or
//!   [`Unlisted::TestMinimum`], a test's or a tool's.
//! - **The per-attempt cache** ([`SaltCache`], `Lite::salt`): an attempt (one execution of a
//!   transaction) validates the first capacity it reads of a bucket — one below [`MIN_BUCKET_SIZE`]
//!   fails as an unreadable one does (`SATIN-STATE-007`) — and keeps its multiplier `floor(capacity
//!   / MIN_BUCKET_SIZE)` for every later charge and give-back in that bucket, across frame reverts,
//!   until the next attempt begins (`SATIN-STATE-006`). A system-originated transaction and a
//!   system call price at `m = 1` and read nothing (`SATIN-STATE-009`, `crate::reprice`).
//! - **The exports** (`SATIN-WIT-013`): an attempt's valid lookups ([`SaltCache::lookups`], and a
//!   candidate's own) — a failed lookup is not one, the lookups before it are — and the block's
//!   union over every attempt of a block session, dropped candidates included, which a session that
//!   executed for another one hands over as a [`SaltExport`].

use crate::{MIN_BUCKET_SIZE, error::BlockIdentity};
use alloy_primitives::{B256, map::HashMap};
use evm2::{AnyError, version::StateGasSite};
use std::{collections::BTreeSet, sync::Arc, vec::Vec};

/// The number of SALT buckets.
pub const NUM_BUCKETS: u32 = 1 << 24;
/// The meta buckets at the bottom of SALT's id space, which hold the other buckets' metadata: no
/// account or slot lands in one.
pub const NUM_META_BUCKETS: u32 = NUM_BUCKETS / 256;

/// A SALT bucket id: SALT's own 32-bit id, below [`NUM_BUCKETS`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BucketId(u32);

impl BucketId {
    /// `id`, when it is a SALT bucket id.
    pub const fn new(id: u32) -> Option<Self> {
        if id < NUM_BUCKETS { Some(Self(id)) } else { None }
    }

    /// The id as SALT numbers its buckets.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<u64> for BucketId {
    type Error = BucketIdOutOfRange;

    /// A wider id, checked: never truncated into another bucket.
    fn try_from(id: u64) -> Result<Self, Self::Error> {
        u32::try_from(id).ok().and_then(Self::new).ok_or(BucketIdOutOfRange(id))
    }
}

impl From<BucketId> for u32 {
    fn from(id: BucketId) -> Self {
        id.0
    }
}

impl From<BucketId> for u64 {
    fn from(id: BucketId) -> Self {
        u64::from(id.0)
    }
}

impl core::fmt::Display for BucketId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A number that is not a SALT bucket id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BucketIdOutOfRange(pub u64);

impl core::fmt::Display for BucketIdOutOfRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{} is not a SALT bucket id (below {NUM_BUCKETS})", self.0)
    }
}

impl core::error::Error for BucketIdOutOfRange {}

/// The KV bucket a key's 64-bit hash lands in: above the meta buckets, below [`NUM_BUCKETS`].
fn kv_bucket(hash: u64) -> BucketId {
    let id = hash % u64::from(NUM_BUCKETS - NUM_META_BUCKETS) + u64::from(NUM_META_BUCKETS);
    BucketId::try_from(id).expect("a KV bucket id is below NUM_BUCKETS")
}

/// The bucket of SALT key `key`, by SALT's own production mapping (`salt_hash`), which the
/// specification takes as an input.
#[cfg(not(feature = "salt-test-hasher"))]
#[inline(always)]
pub fn bucket_id(key: &[u8]) -> BucketId {
    kv_bucket(crate::salt_hash::hash(key))
}

/// The FNV-1a bucket of key `key` of Satin's `TestExternalEnvs` (`SimpleBucketHasher`), which the
/// differential scenarios name buckets with (feature `salt-test-hasher` only: a test hasher).
#[cfg(feature = "salt-test-hasher")]
pub fn bucket_id(key: &[u8]) -> BucketId {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in key {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    kv_bucket(hash)
}

/// The bucket the state a charge at `site` creates lands in: an account's SALT key is its 20-byte
/// address, a slot's the address followed by the 32-byte big-endian slot.
pub fn bucket_for(site: StateGasSite) -> BucketId {
    match site {
        StateGasSite::Account(a) => bucket_id(a.as_slice()),
        StateGasSite::Storage(a, k) => {
            // The 52-byte key on the stack (not a heap `Vec` per lookup).
            let mut key = [0u8; 52];
            key[..20].copy_from_slice(a.as_slice());
            key[20..].copy_from_slice(&k.to_be_bytes::<32>());
            bucket_id(&key)
        }
    }
}

/// The state a [`SaltSnapshot`] reads its capacities from: the post-state of the block named by
/// `parent_hash`, whose state root is `state_root`. A block executes on the snapshot of its
/// parent: the factory refuses one of another parent, so the same height under another parent
/// (a reorganization) is another snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct SnapshotId {
    /// The hash of the block whose post-state the snapshot reads: the parent hash of the block it
    /// serves (block zero's header carries its own, zero on an OP chain).
    pub parent_hash: B256,
    /// The state root of that state, which the node binds its database to as well.
    pub state_root: B256,
}

/// The node's reader of SALT's bucket capacities in one immutable state ([`SnapshotId`]).
///
/// Object-safe and free of any thread or runtime requirement: a node adapts its bucket metadata
/// reader, a validator or a prover its verified capacity metadata. A capacity the snapshot cannot
/// give is an error ([`SaltReadError`]), which fails the transaction that asked as an internal
/// error of the node (`SATIN-STATE-007`, `SATIN-VAL-010`); a snapshot never answers a bucket it
/// cannot read with the minimum.
pub trait SaltSnapshot {
    /// The state the capacities are of.
    fn id(&self) -> SnapshotId;

    /// The capacity SALT holds for `bucket` in that state.
    fn capacity(&self, bucket: BucketId) -> Result<u64, SaltReadError>;
}

/// Why a [`SaltSnapshot`] gave no capacity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaltReadError {
    /// The snapshot holds no capacity for the bucket: a witness without the bucket's proof, a map
    /// that does not list it.
    Missing,
    /// The backend failed to read it.
    Backend(AnyError),
}

/// What a [`SaltCapacities`] answers for a bucket it does not list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unlisted {
    /// [`SaltReadError::Missing`]: a capacity the node does not hold is a failed lookup
    /// (`SATIN-STATE-007`). Production's policy.
    Refuse,
    /// The minimum capacity, [`MIN_BUCKET_SIZE`]: a test's or a tool's stand-in for the rest of
    /// SALT's state, which no node may use for a chain's blocks.
    TestMinimum,
}

/// An in-memory [`SaltSnapshot`]: capacities by bucket, of the state `id`, with a policy for the
/// buckets it does not list.
#[derive(Clone, Debug)]
pub struct SaltCapacities {
    id: SnapshotId,
    capacities: HashMap<BucketId, u64>,
    unlisted: Unlisted,
}

impl SaltCapacities {
    /// Capacities of the state `id` under production's policy: a bucket they do not list is a
    /// failed lookup ([`Unlisted::Refuse`]).
    pub fn new(id: SnapshotId) -> Self {
        Self { id, capacities: HashMap::default(), unlisted: Unlisted::Refuse }
    }

    /// Capacities of the state `id` under a test's policy: a bucket they do not list has the
    /// minimum capacity ([`Unlisted::TestMinimum`]).
    pub fn test_minimum(id: SnapshotId) -> Self {
        Self { unlisted: Unlisted::TestMinimum, ..Self::new(id) }
    }

    /// Lists `bucket` at `capacity`, whatever the capacity: also one below the minimum.
    pub fn insert(&mut self, bucket: BucketId, capacity: u64) {
        self.capacities.insert(bucket, capacity);
    }

    /// [`Self::insert`], by value.
    pub fn with(mut self, bucket: BucketId, capacity: u64) -> Self {
        self.insert(bucket, capacity);
        self
    }

    /// The policy for a bucket the capacities do not list.
    pub const fn unlisted(&self) -> Unlisted {
        self.unlisted
    }
}

impl SaltSnapshot for SaltCapacities {
    fn id(&self) -> SnapshotId {
        self.id
    }

    fn capacity(&self, bucket: BucketId) -> Result<u64, SaltReadError> {
        match (self.capacities.get(&bucket), self.unlisted) {
            (Some(capacity), _) => Ok(*capacity),
            (None, Unlisted::TestMinimum) => Ok(MIN_BUCKET_SIZE),
            (None, Unlisted::Refuse) => Err(SaltReadError::Missing),
        }
    }
}

/// A SALT lookup that failed: no multiplier prices the transaction, which has no outcome
/// (`SATIN-STATE-007`). Its `Display` names the bucket and the cause after "SALT bucket lookup
/// failed", and its `Debug` is its `Display`, as Satin reports the failure (a block error shows
/// the failure through its cause's `Debug`).
#[derive(Clone, PartialEq, Eq)]
pub struct SaltLookupError {
    pub bucket: BucketId,
    pub failure: SaltFailure,
}

/// Why a SALT lookup failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaltFailure {
    /// The snapshot answered a capacity below [`MIN_BUCKET_SIZE`]: a broken backend.
    BelowMinimum { capacity: u64 },
    /// The snapshot gave no capacity.
    Read(SaltReadError),
    /// The EVM has no snapshot to read: it was not made by the factory.
    NoSnapshot,
}

impl core::fmt::Display for SaltLookupError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let bucket = self.bucket;
        f.write_str("SALT bucket lookup failed: ")?;
        match &self.failure {
            SaltFailure::BelowMinimum { capacity } => write!(
                f,
                "bucket {bucket} reports capacity {capacity}, below the minimum bucket of \
                 {MIN_BUCKET_SIZE}"
            ),
            SaltFailure::Read(SaltReadError::Missing) => {
                write!(f, "bucket {bucket} has no capacity in the snapshot")
            }
            SaltFailure::Read(SaltReadError::Backend(e)) => write!(f, "bucket {bucket}: {e}"),
            SaltFailure::NoSnapshot => write!(f, "bucket {bucket}: no SALT snapshot installed"),
        }
    }
}

impl core::fmt::Debug for SaltLookupError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(self, f)
    }
}

impl core::error::Error for SaltLookupError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match &self.failure {
            SaltFailure::Read(SaltReadError::Backend(e)) => Some(e),
            _ => None,
        }
    }
}

/// An EVM's SALT state (`Lite::salt`): the snapshot it reads, and the multiplier of every bucket
/// the running attempt has read a valid capacity of.
#[derive(Clone, Default)]
pub struct SaltCache {
    snapshot: Option<Arc<dyn SaltSnapshot>>,
    multipliers: HashMap<BucketId, u64>,
}

impl core::fmt::Debug for SaltCache {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SaltCache")
            .field("snapshot", &self.snapshot_id())
            .field("multipliers", &self.multipliers)
            .finish()
    }
}

impl SaltCache {
    /// A cache over `snapshot`, no attempt begun.
    pub fn new(snapshot: Arc<dyn SaltSnapshot>) -> Self {
        Self { snapshot: Some(snapshot), multipliers: HashMap::default() }
    }

    /// The state the snapshot reads; none without a snapshot.
    pub fn snapshot_id(&self) -> Option<SnapshotId> {
        self.snapshot.as_ref().map(|s| s.id())
    }

    /// Begins an attempt: the previous attempt's multipliers are forgotten, so its buckets are
    /// read again.
    pub fn begin_attempt(&mut self) {
        self.multipliers.clear();
    }

    /// The multiplier of `bucket` in the running attempt: the one it read, or, at the attempt's
    /// first charge in the bucket, the capacity read now, validated and kept. A failed read or a
    /// capacity below the minimum is an error and is not kept, so it is no lookup of the attempt.
    pub fn multiplier(&mut self, bucket: BucketId) -> Result<u64, SaltLookupError> {
        if let Some(m) = self.multipliers.get(&bucket) {
            return Ok(*m);
        }
        let fail = |failure| SaltLookupError { bucket, failure };
        let snapshot = self.snapshot.as_ref().ok_or_else(|| fail(SaltFailure::NoSnapshot))?;
        let capacity = snapshot.capacity(bucket).map_err(|e| fail(SaltFailure::Read(e)))?;
        if capacity < MIN_BUCKET_SIZE && !crate::off("salt_min_bucket") {
            return Err(fail(SaltFailure::BelowMinimum { capacity }));
        }
        let m = capacity / MIN_BUCKET_SIZE;
        self.multipliers.insert(bucket, m);
        Ok(m)
    }

    /// The buckets the running attempt read a valid capacity of, ascending: its export.
    pub fn lookups(&self) -> Vec<BucketId> {
        let mut lookups: Vec<BucketId> = self.multipliers.keys().copied().collect();
        lookups.sort_unstable();
        lookups
    }
}

/// The valid SALT lookups a block session made, drained to be merged into the session that builds
/// the block (`BlockSession::drain_bucket_reads`, `BlockSession::merge_bucket_reads`): bound to the
/// block and to the snapshot they were read from.
#[must_use = "the lookups are evidence the block's witness needs: merge them"]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaltExport {
    pub(crate) block: BlockIdentity,
    pub(crate) snapshot: SnapshotId,
    pub(crate) buckets: BTreeSet<BucketId>,
}

impl SaltExport {
    pub const fn block(&self) -> BlockIdentity {
        self.block
    }

    pub const fn snapshot(&self) -> SnapshotId {
        self.snapshot
    }

    pub const fn buckets(&self) -> &BTreeSet<BucketId> {
        &self.buckets
    }
}

/// A [`SaltExport`] a merge refused: it is of another block or another snapshot than the
/// session's. The export comes back with the refusal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignSaltExport {
    /// The merging session's block and snapshot.
    pub block: BlockIdentity,
    pub snapshot: SnapshotId,
    pub export: SaltExport,
}

impl core::fmt::Display for ForeignSaltExport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (e, s) = (&self.export.block, &self.export.snapshot);
        write!(
            f,
            "SALT lookups of block {} on {} (state {}) merged into block {} on {} (state {})",
            e.number,
            e.parent_hash,
            s.state_root,
            self.block.number,
            self.block.parent_hash,
            self.snapshot.state_root
        )
    }
}

impl core::error::Error for ForeignSaltExport {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bucket an account or a slot lands in is SALT's own mapping: each id below is
    /// `salt::state::hasher::bucket_id`'s for its key (the account, slot and zero-address keys
    /// cross-verified against salt v1.0.1, the two boundary keys computed at salt v1.0.6 and
    /// v1.0.5, by Satin's hasher tests).
    #[cfg(not(feature = "salt-test-hasher"))]
    #[test]
    fn salt_the_default_mapping_is_salts_on_its_vectors() {
        use alloy_primitives::{Address, U256};
        let account = Address::new([
            0xde, 0xad, 0xbe, 0xef, 0xca, 0xfe, 0xba, 0xbe, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05,
            0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b,
        ]);
        let id = |n| BucketId::new(n).unwrap();
        assert_eq!(bucket_for(StateGasSite::Account(account)), id(12_196_828));
        assert_eq!(bucket_for(StateGasSite::Storage(account, U256::from(1))), id(9_450_146));
        assert_eq!(bucket_for(StateGasSite::Account(Address::ZERO)), id(12_666_336));
        // A slot's key is the address and the big-endian slot: 52 bytes.
        let mut key = [0u8; 52];
        key[..20].copy_from_slice(account.as_slice());
        key[51] = 1;
        assert_eq!(bucket_id(&key), id(9_450_146));
        // The longest input of the hasher's small branch, and the large branch's block-loop
        // boundary.
        assert_eq!(bucket_id(&[1, 2, 3, 4, 5, 6, 7, 8]), id(1_978_187));
        assert_eq!(bucket_id(&[0x22; 32]), id(10_093_090));
    }

    /// Every key, whatever its length, lands in a KV bucket: above the meta buckets, below
    /// `NUM_BUCKETS`.
    #[test]
    fn salt_every_key_lands_in_a_kv_bucket() {
        let keys: [&[u8]; 9] = [
            b"",
            b"a",
            b"test",
            &[0; 20],
            &[0xff; 20],
            &[0; 32],
            &[0; 52],
            &[0xff; 32],
            &[0xff; 1024],
        ];
        for key in keys {
            let id = bucket_id(key).get();
            assert!((NUM_META_BUCKETS..NUM_BUCKETS).contains(&id), "{key:?}: {id}");
        }
        assert_eq!(kv_bucket(0).get(), NUM_META_BUCKETS);
        assert_eq!(kv_bucket(u64::from(NUM_BUCKETS - NUM_META_BUCKETS) - 1).get(), NUM_BUCKETS - 1);
        assert_eq!(
            kv_bucket(u64::MAX),
            kv_bucket(u64::MAX % u64::from(NUM_BUCKETS - NUM_META_BUCKETS))
        );
    }

    /// A wider id converts only when it is a bucket id: never truncated into another bucket.
    #[test]
    fn salt_bucket_id_conversions_are_checked() {
        let last = u64::from(NUM_BUCKETS) - 1;
        assert_eq!(BucketId::try_from(last).map(u64::from), Ok(last));
        assert_eq!(BucketId::try_from(0).map(u32::from), Ok(0));
        for wide in [u64::from(NUM_BUCKETS), u64::from(u32::MAX), (1 << 32) | 5, u64::MAX] {
            assert_eq!(BucketId::try_from(wide), Err(BucketIdOutOfRange(wide)), "{wide}");
        }
        assert_eq!(BucketId::new(NUM_BUCKETS), None);
        assert_eq!(BucketId::new(NUM_BUCKETS - 1).map(BucketId::get), Some(NUM_BUCKETS - 1));
    }

    /// The cache validates the first capacity of a bucket and keeps `floor(capacity / 256)`
    /// until the next attempt; a failed lookup is neither kept nor exported.
    #[test]
    fn salt_the_cache_keeps_the_first_valid_multiplier_until_the_next_attempt() {
        let (b, c) = (BucketId::new(1 << 16).unwrap(), BucketId::new(1 << 17).unwrap());
        let salt = SaltCapacities::new(SnapshotId::default()).with(b, 767).with(c, 255);
        let mut cache = SaltCache::new(Arc::new(salt));
        cache.begin_attempt();
        assert_eq!(cache.multiplier(b), Ok(2));
        let below =
            SaltLookupError { bucket: c, failure: SaltFailure::BelowMinimum { capacity: 255 } };
        assert_eq!(cache.multiplier(c), Err(below));
        assert_eq!(cache.lookups(), [b]);
        cache.begin_attempt();
        assert!(cache.lookups().is_empty());
        assert_eq!(cache.snapshot_id(), Some(SnapshotId::default()));
    }
}
