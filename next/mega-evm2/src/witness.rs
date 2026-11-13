//! A block's stateless witness (`SATIN-WIT-001`, `SATIN-WIT-005`–`SATIN-WIT-007`,
//! `SATIN-WIT-016`, `SATIN-WIT-017`): the keys a block's execution names, their values as the
//! chain held them before the block, and a strict database a validator replays the block on.
//!
//! - **Keys** ([`WitnessKeys`]): every account and slot the block's committed states name — each
//!   pre-block step's ([`BlockOutput::pre_block_states`]) and each included transaction's
//!   ([`BlockOutput::tx_states`]), read-only entries included — with the block hashes and SALT
//!   buckets the block exported. They are a function of the states and the exports alone: a cache
//!   that served a value removes no key (`SATIN-WIT-005`).
//! - **Values** ([`Witness::resolve`], the building node's side): each key resolved against the
//!   block's parent state. An address the parent holds no account for is recorded as absent, never
//!   as an empty account (`SATIN-WIT-017`), and none of its slots is recorded: its storage is zero
//!   (`SATIN-WIT-006`). An existing account's slot is recorded with its value, an empty one as
//!   zero. Every existing account's code is the parent's, whether or not the block loaded it, and
//!   never the code a state carries (`SATIN-WIT-001`, `SATIN-WIT-007`): a state's original values
//!   are its transaction's start, after the block's earlier writes, so a redelegation earlier in
//!   the block would put its new code there. A validator fills a [`Witness`] from the proofs it
//!   verified instead ([`Witness::new`] and its `insert_` methods).
//! - **Replay** ([`WitnessDb`], and the [`Witness`] itself as the SALT snapshot): a database that
//!   answers exactly the witness. A read of anything it does not hold fails with
//!   [`MissingEvidence`], which is fatal: the block fails as an internal error of the node, never
//!   as an invalid block, and no default stands in for the missing value. A slot of an account the
//!   witness records as absent is zero, with no evidence needed (`SATIN-WIT-006`). A replay runs no
//!   Oracle service: every Oracle read pushes the slot's value (`SATIN-ORCL-012`), and a read
//!   before its charge that then runs out of gas still loads the slot (`SATIN-ORCL-004`), so the
//!   witness holds it.
//!
//! A replay's exports are a subset of the building node's, a proper one when the node executed
//! candidates it dropped, whose buckets and block hashes its exports also hold (`SATIN-WIT-016`).

use crate::{
    block::BlockOutput,
    error::BlockError,
    salt::{BucketId, SaltFailure, SaltReadError, SaltSnapshot, SnapshotId},
};
use alloy_primitives::{Address, B256, KECCAK256_EMPTY, U256};
use evm2::{
    DatabaseError,
    bytecode::Bytecode,
    evm::{AccountInfo, Database, DynDatabase, PendingState},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    vec::Vec,
};

/// One piece of a witness's evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Evidence {
    /// An account, present or absent.
    Account(Address),
    /// A slot of an account the parent state holds.
    Slot(Address, U256),
    /// Code, by its hash.
    Code(B256),
    /// The hash of a block, by its number.
    BlockHash(U256),
    /// A SALT bucket's capacity.
    Capacity(BucketId),
}

/// A read the witness holds no evidence for: the replay cannot go on. The error of
/// [`WitnessDb`], always fatal (`SATIN-VAL-010`: an internal error of the node, no verdict on the
/// block).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MissingEvidence(pub Evidence);

impl core::fmt::Display for MissingEvidence {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "the witness holds no evidence of {:?}", self.0)
    }
}

impl core::error::Error for MissingEvidence {}

impl MissingEvidence {
    /// The evidence whose lack failed a block, when a witness's lack did: a read [`WitnessDb`]
    /// could not answer, through every wrapper the engine carries it in, or a SALT capacity the
    /// snapshot does not hold ([`SaltReadError::Missing`], which a [`Witness`] answers for a
    /// bucket it does not hold).
    pub fn of(error: &BlockError) -> Option<Self> {
        if let Some(missing) = error.database_error().and_then(|e| e.downcast_ref::<Self>()) {
            return Some(*missing);
        }
        let lookup = error.salt_lookup_error()?;
        matches!(lookup.failure, SaltFailure::Read(SaltReadError::Missing))
            .then_some(Self(Evidence::Capacity(lookup.bucket)))
    }
}

/// The keys a block's execution names (`SATIN-WIT-005`): every account with the slots named of it,
/// every block hash it was served and every SALT bucket it was answered.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WitnessKeys {
    accounts: BTreeMap<Address, BTreeSet<U256>>,
    block_hashes: BTreeMap<u64, B256>,
    buckets: BTreeSet<BucketId>,
}

impl WitnessKeys {
    /// The keys of a finished block: its pre-block states, its included transactions' states and
    /// its exports.
    pub fn of(output: &BlockOutput) -> Self {
        let mut keys = Self::default();
        for state in output.pre_block_states.iter().chain(&output.tx_states) {
            keys.add_state(state);
        }
        keys.add_block_hashes(&output.block_hash_reads);
        keys.add_buckets(&output.bucket_reads);
        keys
    }

    /// Adds every account and slot `state` names, whether it loaded or wrote it, its loads in a
    /// failed frame included.
    pub fn add_state(&mut self, state: &PendingState) {
        for account in state.account_changes() {
            self.accounts.entry(account.address).or_default().extend(account.storage.slots.keys());
        }
    }

    /// Adds block hashes `BLOCKHASH` was served, by number.
    pub fn add_block_hashes(&mut self, hashes: &BTreeMap<u64, B256>) {
        self.block_hashes.extend(hashes.iter().map(|(n, h)| (*n, *h)));
    }

    /// Adds SALT buckets the block was answered a valid capacity of.
    pub fn add_buckets(&mut self, buckets: &BTreeSet<BucketId>) {
        self.buckets.extend(buckets.iter().copied());
    }

    /// Every account named, with the slots named of it.
    pub const fn accounts(&self) -> &BTreeMap<Address, BTreeSet<U256>> {
        &self.accounts
    }

    /// Every block hash served, by number.
    pub const fn block_hashes(&self) -> &BTreeMap<u64, B256> {
        &self.block_hashes
    }

    /// Every SALT bucket answered.
    pub const fn buckets(&self) -> &BTreeSet<BucketId> {
        &self.buckets
    }
}

/// A key the parent state or the SALT snapshot could not resolve ([`Witness::resolve`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolveError {
    /// The parent state failed to read the evidence.
    State { evidence: Evidence, error: DatabaseError },
    /// The SALT snapshot gave no capacity of the bucket.
    Capacity { bucket: BucketId, error: SaltReadError },
}

impl core::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::State { evidence, error } => write!(f, "parent state of {evidence:?}: {error}"),
            Self::Capacity { bucket, error } => write!(f, "capacity of bucket {bucket}: {error:?}"),
        }
    }
}

impl core::error::Error for ResolveError {}

/// A block's witness: the parent state's account, slot and code of every key the block names, the
/// block hashes it was served and the capacities of the SALT buckets it was answered, of the state
/// [`Self::snapshot`]. It is the SALT snapshot of a replay ([`SaltSnapshot`]); [`WitnessDb`] is
/// its database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Witness {
    snapshot: SnapshotId,
    /// `None`: the parent holds no account at the address.
    accounts: BTreeMap<Address, Option<AccountInfo>>,
    slots: BTreeMap<(Address, U256), U256>,
    codes: BTreeMap<B256, Bytecode>,
    block_hashes: BTreeMap<u64, B256>,
    capacities: BTreeMap<BucketId, u64>,
}

impl Witness {
    /// An empty witness of the state `snapshot`, the block's parent's, which a validator fills
    /// from the proofs it verified.
    pub const fn new(snapshot: SnapshotId) -> Self {
        Self {
            snapshot,
            accounts: BTreeMap::new(),
            slots: BTreeMap::new(),
            codes: BTreeMap::new(),
            block_hashes: BTreeMap::new(),
            capacities: BTreeMap::new(),
        }
    }

    /// The witness of `keys` in the block's parent state `parent` and SALT snapshot `salt` (of
    /// that state): the building node's side. An absent account is recorded as absent, with no
    /// slot; an existing one with every slot named of it and its code, the parent's, whether or
    /// not the block loaded it. The block hashes are the ones served, as `keys` holds them.
    pub fn resolve(
        keys: &WitnessKeys,
        parent: &mut dyn DynDatabase,
        salt: &dyn SaltSnapshot,
    ) -> Result<Self, ResolveError> {
        let state = |evidence| move |error| ResolveError::State { evidence, error };
        let mut witness = Self::new(salt.id());
        for (address, slots) in &keys.accounts {
            let address = *address;
            let info = parent.get_account(&address).map_err(state(Evidence::Account(address)))?;
            if let Some(info) = &info {
                if has_code(info) {
                    let code = match &info.code {
                        Some(code) if !code.is_empty() => code.clone(),
                        _ => parent
                            .get_code_by_hash(&info.code_hash)
                            .map_err(state(Evidence::Code(info.code_hash)))?,
                    };
                    witness.codes.insert(info.code_hash, code);
                }
                for key in slots {
                    let evidence = Evidence::Slot(address, *key);
                    let value = parent.get_storage(&address, key).map_err(state(evidence))?;
                    witness.slots.insert((address, *key), value);
                }
            }
            witness.insert_account(address, info);
        }
        witness.block_hashes.extend(keys.block_hashes.iter().map(|(n, h)| (*n, *h)));
        for bucket in &keys.buckets {
            let capacity = salt
                .capacity(*bucket)
                .map_err(|error| ResolveError::Capacity { bucket: *bucket, error })?;
            witness.capacities.insert(*bucket, capacity);
        }
        Ok(witness)
    }

    /// Records the account at `address`: `None` when the parent holds no account there. Its code,
    /// when the value carries it, is recorded by its hash as [`Self::insert_code`] does.
    pub fn insert_account(&mut self, address: Address, info: Option<AccountInfo>) {
        let info = info.map(|mut info| {
            if let Some(code) = info.code.take() &&
                has_code(&info) &&
                !code.is_empty()
            {
                self.codes.insert(info.code_hash, code);
            }
            info
        });
        self.accounts.insert(address, info);
    }

    /// Records slot `key` of the account at `address` as `value`.
    pub fn insert_slot(&mut self, address: Address, key: U256, value: U256) {
        self.slots.insert((address, key), value);
    }

    /// Records `code`, whose hash is `code_hash`.
    pub fn insert_code(&mut self, code_hash: B256, code: Bytecode) {
        self.codes.insert(code_hash, code);
    }

    /// Records the hash of block `number`.
    pub fn insert_block_hash(&mut self, number: u64, hash: B256) {
        self.block_hashes.insert(number, hash);
    }

    /// Records the capacity of `bucket`.
    pub fn insert_capacity(&mut self, bucket: BucketId, capacity: u64) {
        self.capacities.insert(bucket, capacity);
    }

    /// The state the witness is of.
    pub const fn snapshot(&self) -> SnapshotId {
        self.snapshot
    }

    /// The account at `address`: `None` when the witness holds no evidence of it, `Some(None)`
    /// when it holds its absence.
    pub fn account(&self, address: &Address) -> Option<Option<&AccountInfo>> {
        self.accounts.get(address).map(Option::as_ref)
    }

    /// Slot `key` of the account at `address`, when the witness holds it.
    pub fn slot(&self, address: &Address, key: &U256) -> Option<U256> {
        self.slots.get(&(*address, *key)).copied()
    }

    /// The code whose hash is `code_hash`, when the witness holds it.
    pub fn code(&self, code_hash: &B256) -> Option<&Bytecode> {
        self.codes.get(code_hash)
    }

    /// Every block hash the witness holds, by number.
    pub const fn block_hashes(&self) -> &BTreeMap<u64, B256> {
        &self.block_hashes
    }

    /// Every capacity the witness holds, by bucket.
    pub const fn capacities(&self) -> &BTreeMap<BucketId, u64> {
        &self.capacities
    }

    /// Every piece of evidence the witness holds, in [`Evidence`]'s order.
    pub fn evidence(&self) -> Vec<Evidence> {
        let mut all: Vec<Evidence> = self
            .accounts
            .keys()
            .map(|a| Evidence::Account(*a))
            .chain(self.slots.keys().map(|(a, k)| Evidence::Slot(*a, *k)))
            .chain(self.codes.keys().map(|h| Evidence::Code(*h)))
            .chain(self.block_hashes.keys().map(|n| Evidence::BlockHash(U256::from(*n))))
            .chain(self.capacities.keys().map(|b| Evidence::Capacity(*b)))
            .collect();
        all.sort_unstable();
        all
    }

    /// Removes `evidence`; whether the witness held it.
    pub fn remove(&mut self, evidence: &Evidence) -> bool {
        match evidence {
            Evidence::Account(a) => self.accounts.remove(a).is_some(),
            Evidence::Slot(a, k) => self.slots.remove(&(*a, *k)).is_some(),
            Evidence::Code(h) => self.codes.remove(h).is_some(),
            Evidence::BlockHash(n) => {
                u64::try_from(*n).is_ok_and(|n| self.block_hashes.remove(&n).is_some())
            }
            Evidence::Capacity(b) => self.capacities.remove(b).is_some(),
        }
    }
}

/// A replay's SALT snapshot: the capacities the witness holds; a bucket it does not hold is
/// [`SaltReadError::Missing`], a failed lookup (`SATIN-STATE-007`), never the minimum.
impl SaltSnapshot for Witness {
    fn id(&self) -> SnapshotId {
        self.snapshot
    }

    fn capacity(&self, bucket: BucketId) -> Result<u64, SaltReadError> {
        self.capacities.get(&bucket).copied().ok_or(SaltReadError::Missing)
    }
}

/// A stateless validator's database: exactly a [`Witness`], and nothing else. A read of anything
/// the witness does not hold fails with [`MissingEvidence`], which is fatal; a slot of an account
/// the witness records as absent is zero, with no evidence needed (`SATIN-WIT-006`).
///
/// It is shared (`Arc`) with the session's SALT snapshot, the same witness:
/// `factory.start_block(&input, Db::new(WitnessDb::new(witness.clone())),
/// BlockServices::new(witness))`, with no Oracle service.
#[derive(Clone, Debug)]
pub struct WitnessDb {
    witness: Arc<Witness>,
}

impl WitnessDb {
    pub const fn new(witness: Arc<Witness>) -> Self {
        Self { witness }
    }

    /// The witness the database answers.
    pub fn witness(&self) -> &Witness {
        &self.witness
    }
}

impl Database for WitnessDb {
    type Error = MissingEvidence;

    /// A missing piece of evidence is the node's lack, never the block's fault.
    fn is_fatal(_: &Self::Error) -> bool {
        true
    }

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        match self.witness.accounts.get(address) {
            Some(info) => Ok(info.clone()),
            None => Err(MissingEvidence(Evidence::Account(*address))),
        }
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        if *code_hash == KECCAK256_EMPTY || code_hash.is_zero() {
            return Ok(Bytecode::default());
        }
        self.witness
            .codes
            .get(code_hash)
            .cloned()
            .ok_or(MissingEvidence(Evidence::Code(*code_hash)))
    }

    fn get_storage(&mut self, address: &Address, key: &U256) -> Result<U256, Self::Error> {
        match self.witness.accounts.get(address) {
            // An account the chain did not hold has no storage: zero, with no lookup.
            Some(None) => Ok(U256::ZERO),
            Some(Some(_)) => self
                .witness
                .slot(address, key)
                .ok_or(MissingEvidence(Evidence::Slot(*address, *key))),
            None => Err(MissingEvidence(Evidence::Account(*address))),
        }
    }

    fn get_block_hash(&mut self, number: &U256) -> Result<B256, Self::Error> {
        let held = u64::try_from(*number).ok().and_then(|n| self.witness.block_hashes.get(&n));
        held.copied().ok_or(MissingEvidence(Evidence::BlockHash(*number)))
    }
}

/// Whether `info` holds code: a code hash that is neither the empty code's nor zero.
fn has_code(info: &AccountInfo) -> bool {
    !info.is_empty_code_hash() && !info.code_hash.is_zero()
}

#[cfg(test)]
mod tests {
    use super::*;
    use evm2::evm::InMemoryDB;

    const A: Address = Address::with_last_byte(0xa);
    const B: Address = Address::with_last_byte(0xb);
    const C: Address = Address::with_last_byte(0xc);

    fn parent() -> InMemoryDB {
        let mut db = InMemoryDB::default();
        let code = Bytecode::new_legacy(alloy_primitives::Bytes::from_static(&[0x00]));
        db.insert_account_info(&A, AccountInfo::default().with_nonce(1).with_code(code));
        db.insert_account_storage(&A, &U256::from(1), &U256::from(11));
        // An existing empty account: a leaf, not an absence.
        db.insert_account_info(&B, AccountInfo::default());
        db
    }

    fn keys() -> WitnessKeys {
        let mut keys = WitnessKeys::default();
        let mut state = PendingState::default();
        for address in [A, B, C] {
            state.insert_account(address, None, None);
            for key in [1u64, 2] {
                state.insert_storage(address, U256::from(key), U256::ZERO, U256::ZERO);
            }
        }
        keys.add_state(&state);
        keys
    }

    #[test]
    fn witness_resolve_records_absence_leaves_and_zero_slots() {
        let salt = crate::SaltCapacities::test_minimum(SnapshotId::default());
        let witness = Witness::resolve(&keys(), &mut parent(), &salt).unwrap();
        // The parent's values, not the states': an existing account and its slots, an empty
        // slot as zero.
        assert_eq!(witness.account(&A).unwrap().unwrap().nonce, 1);
        assert_eq!(witness.slot(&A, &U256::from(1)), Some(U256::from(11)));
        assert_eq!(witness.slot(&A, &U256::from(2)), Some(U256::ZERO));
        let code_hash = witness.account(&A).unwrap().unwrap().code_hash;
        assert!(witness.code(&code_hash).is_some());
        assert!(witness.account(&A).unwrap().unwrap().code.is_none());
        // An empty leaf is a leaf, with its slots proved zero.
        assert_eq!(witness.account(&B), Some(Some(&AccountInfo::default())));
        assert_eq!(witness.slot(&B, &U256::from(1)), Some(U256::ZERO));
        // An absent account: recorded as absent, no slot.
        assert_eq!(witness.account(&C), Some(None));
        assert_eq!(witness.slot(&C, &U256::from(1)), None);
        let mut db = WitnessDb::new(Arc::new(witness.clone()));
        assert_eq!(db.get_storage(&C, &U256::from(7)), Ok(U256::ZERO));
        assert_eq!(db.get_account(&C), Ok(None));
        assert_eq!(
            db.get_storage(&A, &U256::from(3)),
            Err(MissingEvidence(Evidence::Slot(A, U256::from(3))))
        );
        let d = Address::with_last_byte(0xd);
        assert_eq!(db.get_account(&d), Err(MissingEvidence(Evidence::Account(d))));
        assert_eq!(db.get_storage(&d, &U256::ZERO), Err(MissingEvidence(Evidence::Account(d))));
        assert_eq!(db.get_code_by_hash(&KECCAK256_EMPTY), Ok(Bytecode::default()));
        assert_eq!(
            db.get_block_hash(&U256::from(5)),
            Err(MissingEvidence(Evidence::BlockHash(U256::from(5))))
        );
        // Every piece listed is removable, once.
        let mut emptied = witness.clone();
        for evidence in witness.evidence() {
            assert!(emptied.remove(&evidence), "{evidence:?}");
            assert!(!emptied.remove(&evidence), "{evidence:?}");
        }
        assert!(emptied.evidence().is_empty());
    }
}
