//! The L1 block contract's reads a transaction makes outside its journal (`SATIN-WIT-003`,
//! `SATIN-WIT-009`, `SATIN-WIT-011`), as the engine records them and reports them.
//!
//! - **Two read paths.** A non-deposit transaction that pays an L1 fee is priced from the L1 block
//!   info, which the fee computation loads from the contract's account and its slots 1, 3, 7, 8
//!   and, when the Ecotone scalars are empty, 5 (`tx::MegaHooks::before_execution`); every
//!   non-deposit transaction, a Mega System Transaction included, has its footprint priced from
//!   slot 8, which the block reads before it runs (`BlockSession::execute_candidate`). Neither read
//!   goes through the transaction's journal, so neither lands in its returned state.
//! - **What is recorded.** The account, as present or as absent, and each slot read with the value
//!   it was served. A slot of an account read as absent is not a read of the state: an absent
//!   account has no storage (`SATIN-WIT-006`), and none is recorded.
//! - **Cached fee data.** The fee computation loads the info once per block and prices every later
//!   transaction from that cache; the keys the cache was loaded from (`Lite::l1_sources`) are every
//!   user's reads, so the first included user reports them however many candidates used, and
//!   dropped, the cache before it.
//! - **Ownership** ([`L1Reads::attach`]): a block session adds each key, once, as a read-only entry
//!   to the state of the first transaction it commits that read the key, never replacing an entry
//!   that state holds; a candidate dropped before its commit owns nothing.

use alloy_primitives::U256;
use evm2::evm::{AccountInfo, PendingState};
use op_evm2::constants::L1_BLOCK_CONTRACT;
use std::vec::Vec;

/// The L1 block contract's account and slots read outside a journal, each with what the read was
/// served.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct L1Reads {
    /// The account: `None` when it was not read, `Some(None)` when it was read as absent.
    account: Option<Option<AccountInfo>>,
    /// The slots read, each once, ascending by key, with the value read; none when the account
    /// was read as absent.
    slots: Vec<(U256, U256)>,
}

impl L1Reads {
    /// Forgets every read: a new attempt begins.
    pub fn clear(&mut self) {
        self.account = None;
        self.slots.clear();
    }

    /// Whether nothing was read.
    pub fn is_empty(&self) -> bool {
        self.account.is_none() && self.slots.is_empty()
    }

    /// The account as read: `None` when it was not read, `Some(None)` when it was read as absent.
    pub fn account(&self) -> Option<Option<&AccountInfo>> {
        self.account.as_ref().map(Option::as_ref)
    }

    /// The slots read, ascending by key, each with the value it was served.
    pub fn slots(&self) -> &[(U256, U256)] {
        &self.slots
    }

    /// Whether `key` was read.
    pub fn has_slot(&self, key: &U256) -> bool {
        self.slots.binary_search_by(|(k, _)| k.cmp(key)).is_ok()
    }

    /// Records the account as read: `None` when it is absent. A read recorded before stands.
    pub fn record_account(&mut self, info: Option<AccountInfo>) {
        if self.account.is_none() {
            self.account = Some(info);
        }
    }

    /// Records slot `key` read as `value`, unless the account was read as absent, which has no
    /// storage (`SATIN-WIT-006`). A read recorded before stands.
    pub fn record_slot(&mut self, key: U256, value: U256) {
        if matches!(self.account, Some(None)) {
            return;
        }
        if let Err(at) = self.slots.binary_search_by(|(k, _)| k.cmp(&key)) {
            self.slots.insert(at, (key, value));
        }
    }

    /// Adds what `other` read; a read recorded here before stands.
    pub fn merge(&mut self, other: &Self) {
        if let Some(account) = &other.account {
            self.record_account(account.clone());
        }
        for (key, value) in &other.slots {
            self.record_slot(*key, *value);
        }
    }

    /// Reports in `state`, the state of a transaction being committed, every read not in
    /// `reported` (the block's reads earlier commits reported), and adds them to `reported`
    /// (`SATIN-WIT-011`). Each is a read-only entry with the value read, and none replaces an entry
    /// the state holds for the same account or slot: the transaction's own load or write stands,
    /// and reports the key. A slot's entry needs its account's in the same state, so a transaction
    /// that reports only a slot names the account as well.
    pub fn attach(&self, state: &mut PendingState, reported: &mut Self) {
        let Some(account) = &self.account else { return };
        let fresh: Vec<(U256, U256)> =
            self.slots.iter().filter(|(key, _)| !reported.has_slot(key)).copied().collect();
        if reported.account.is_some() && fresh.is_empty() {
            return;
        }
        let holds_slots = match state.account_changes_for(&L1_BLOCK_CONTRACT) {
            None => {
                state.insert_account(L1_BLOCK_CONTRACT, account.clone(), account.clone());
                Vec::new()
            }
            // The transaction's own entry. A storage it wiped or created is its own as a whole:
            // a read-only slot under it would read as a write, so none is added.
            Some(own) if own.storage.wiped || own.created => {
                fresh.iter().map(|(key, _)| *key).collect()
            }
            Some(own) => fresh
                .iter()
                .map(|(key, _)| *key)
                .filter(|k| own.storage.slots.contains_key(k))
                .collect(),
        };
        for (key, value) in &fresh {
            if !holds_slots.contains(key) {
                state.insert_storage(L1_BLOCK_CONTRACT, *key, *value, *value);
            }
        }
        reported.merge(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::Address;

    fn info(nonce: u64) -> AccountInfo {
        AccountInfo::default().with_nonce(nonce)
    }

    fn reads(account: Option<AccountInfo>, slots: &[u64]) -> L1Reads {
        let mut reads = L1Reads::default();
        reads.record_account(account);
        for slot in slots {
            reads.record_slot(U256::from(*slot), U256::from(slot * 10));
        }
        reads
    }

    #[test]
    fn witness_l1_reads_keep_the_first_value_and_no_slot_of_an_absent_account() {
        let mut r = reads(Some(info(1)), &[8, 1, 3]);
        r.record_slot(U256::from(1), U256::from(99));
        r.record_account(None);
        assert_eq!(r.account(), Some(Some(&info(1))));
        assert_eq!(r.slots().iter().map(|(k, _)| k.to::<u64>()).collect::<Vec<_>>(), [1, 3, 8]);
        assert_eq!(r.slots()[0].1, U256::from(10));
        let absent = reads(None, &[8]);
        assert_eq!(absent.account(), Some(None));
        assert!(absent.slots().is_empty());
        assert!(!absent.is_empty());
        let mut merged = L1Reads::default();
        assert!(merged.is_empty());
        merged.merge(&r);
        assert_eq!(merged, r);
        merged.clear();
        assert!(merged.is_empty());
    }

    #[test]
    fn witness_first_included_attach_reports_each_key_once_and_replaces_nothing() {
        let mut reported = L1Reads::default();
        // The first: the account and slot 8, as read-only entries.
        let mut first = PendingState::default();
        reads(Some(info(1)), &[8]).attach(&mut first, &mut reported);
        let l1 = first.account_changes_for(&L1_BLOCK_CONTRACT).unwrap();
        assert!(!l1.is_changed());
        assert_eq!(l1.current, Some(&info(1)));
        assert_eq!(l1.storage.slots.len(), 1);
        // The second reads 8 again and 1: only slot 1 is its own, under the account's entry.
        let mut second = PendingState::default();
        reads(Some(info(1)), &[1, 8]).attach(&mut second, &mut reported);
        let l1 = second.account_changes_for(&L1_BLOCK_CONTRACT).unwrap();
        assert_eq!(l1.storage.slots.keys().map(|k| k.to::<u64>()).collect::<Vec<_>>(), [1]);
        // A third that reads nothing new has no entry.
        let mut third = PendingState::default();
        reads(Some(info(1)), &[1, 8]).attach(&mut third, &mut reported);
        assert!(third.is_empty());
        // A transaction's own entry stands: its write to slot 3 and its account are not replaced.
        let mut own = PendingState::default();
        own.insert_account(L1_BLOCK_CONTRACT, Some(info(1)), Some(info(2)));
        own.insert_storage(L1_BLOCK_CONTRACT, U256::from(3), U256::from(30), U256::from(31));
        reads(Some(info(1)), &[3, 7]).attach(&mut own, &mut reported);
        let l1 = own.account_changes_for(&L1_BLOCK_CONTRACT).unwrap();
        assert_eq!(l1.current, Some(&info(2)));
        assert_eq!(l1.storage.slots[&U256::from(3)].value.current, U256::from(31));
        assert_eq!(l1.storage.slots[&U256::from(7)].value.current, U256::from(70));
        assert!(reported.has_slot(&U256::from(3)) && reported.has_slot(&U256::from(7)));
        // Another account's entries are untouched.
        assert!(own.account_changes_for(&Address::ZERO).is_none());
    }

    #[test]
    fn witness_absent_l1block_attach_records_the_absence_alone() {
        let mut reported = L1Reads::default();
        let mut state = PendingState::default();
        reads(None, &[8]).attach(&mut state, &mut reported);
        let l1 = state.account_changes_for(&L1_BLOCK_CONTRACT).unwrap();
        assert!(l1.original.is_none() && l1.current.is_none());
        assert!(l1.storage.slots.is_empty());
        assert!(!l1.is_changed());
        // Nothing read: nothing attached.
        let mut none = PendingState::default();
        L1Reads::default().attach(&mut none, &mut L1Reads::default());
        assert!(none.is_empty());
    }
}
