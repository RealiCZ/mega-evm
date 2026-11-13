//! The L1 block contract's reads (`SATIN-WIT-003`, `SATIN-WIT-009`, `SATIN-WIT-011`): no pre-block
//! step reads the contract; the fee computation of a non-deposit transaction that pays an L1 fee
//! reads its account and slots 1, 3, 7, 8 and, with empty Ecotone scalars, 5, and the block reads
//! slot 8 before every non-deposit transaction, a Mega System Transaction included. Each account
//! and slot read is reported once, as a read-only entry of the state of the first included
//! transaction that read it, whether a cache served it or not; a dropped candidate reports nothing,
//! and a read entry never replaces the transaction's own. A failed fee read is the transaction's, a
//! failed footprint read the block's (`SATIN-VAL-010`).

mod witness_fixture;

use alloy_primitives::{TxKind, U256};
use evm2::evm::{Db, InMemoryDB};
use mega_evm2::{
    block::BlockSession,
    error::{CandidateError, Disposition, Phase},
    factory::BlockServices,
    witness::{Evidence, Witness, WitnessKeys},
};
use op_evm2::constants::L1_BLOCK_CONTRACT;
use witness_fixture::*;

/// A session of the block over a [`Backend`] of `parent` failing `fail`.
fn session_over(
    parent: &InMemoryDB,
    fail: Option<(Option<u64>, bool)>,
) -> (BlockSession<'static>, Reached) {
    let (mut backend, reached) = Backend::new(parent.clone());
    backend.fail = fail.map(|(slot, fatal)| (L1_BLOCK_CONTRACT, slot.map(U256::from), fatal));
    let services = BlockServices::new(salt());
    let block = common::factory().start_block(&input(), Db::new(backend), services).unwrap();
    (block, reached)
}

/// Executes and commits `tx` in `block`.
fn include(block: &mut BlockSession<'_>, tx: &Tx) {
    let candidate = block.execute_candidate(&tx.tx, &tx.encoded).expect("it runs");
    block.commit_candidate(candidate).expect("it commits");
}

/// The slots `slots` of the fixture's L1 block contract, with their values.
fn l1_values(empty_scalars: bool, slots: &[u64]) -> Vec<(U256, U256)> {
    let all = l1_slots(empty_scalars);
    slots.iter().map(|s| (U256::from(*s), all.iter().find(|(k, _)| k == s).unwrap().1)).collect()
}

/// The first fee-paying transaction reports the account and the four slots its fee computation
/// and its footprint read, with the values read; the next one, priced from the cached fee data,
/// reports nothing, though its own reads name the same keys; a deposit reads none.
#[test]
fn witness_l1_fee_reads_are_reported_with_the_first_included_fee_payer() {
    let parent = chain(L1::Present);
    let txs = [deposit(NOOP), call(SENDER, 0, NOOP), call(SENDER, 1, NOOP)];
    let mut block = session(&parent);
    let mut reads = Vec::new();
    for tx in &txs {
        let candidate = block.execute_candidate(&tx.tx, &tx.encoded).unwrap();
        reads.push(candidate.l1_reads().clone());
        block.commit_candidate(candidate).unwrap();
    }
    assert!(reads[0].is_empty(), "a deposit reads no L1 block information");
    for r in &reads[1..] {
        assert_eq!(r.account().map(|a| a.map(|a| a.nonce)), Some(Some(1)));
        assert_eq!(r.slots(), l1_values(false, &[1, 3, 7, 8]));
    }
    let states = block.tx_states();
    assert_eq!(l1_entry(&states[0]), None);
    let (account, slots) = l1_entry(&states[1]).expect("the first fee payer reports them");
    assert_eq!(account.map(|a| a.nonce), Some(1));
    assert_eq!(slots, [1, 3, 7, 8]);
    let entry = states[1].account_changes_for(&L1_BLOCK_CONTRACT).unwrap();
    assert!(!entry.is_changed(), "read-only entries");
    for (key, value) in l1_values(false, &[1, 3, 7, 8]) {
        let slot = &entry.storage.slots[&key].value;
        assert_eq!((slot.original, slot.current), (value, value));
    }
    assert_eq!(l1_entry(&states[2]), None, "reported once");
    let built = produce(&parent, &included(&txs));
    assert_eq!(built.output.tx_states, block.finish().unwrap().tx_states);
    let (replayed, _) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
}

/// Empty Ecotone scalars make the fee computation read the overhead, slot 5, which the first
/// included fee payer reports with the others (Rex6 left it out of every state).
#[test]
fn witness_l1_fee_with_empty_scalars_reports_the_overhead_slot() {
    let parent = chain(L1::EmptyScalars);
    let built = produce(&parent, &included(&[call(SENDER, 0, NOOP), call(SENDER, 1, NOOP)]));
    let (_, slots) = l1_entry(&built.output.tx_states[0]).unwrap();
    assert_eq!(slots, [1, 3, 5, 7, 8]);
    assert_eq!(built.witness.slot(&L1_BLOCK_CONTRACT, &U256::from(5)), Some(U256::from(500)));
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    assert!(read.contains(&Evidence::Slot(L1_BLOCK_CONTRACT, U256::from(5))));
}

/// A block whose only non-deposit transactions are Mega System Transactions, which pay no fee,
/// reads the L1 block contract's account and slot 8 alone, for their footprint, and the first of
/// them reports both; no other slot is read, so none is in the witness.
#[test]
fn witness_footprint_only_block_reports_slot_8_with_the_first_system_transaction() {
    let parent = chain(L1::Present);
    let txs = [deposit(NOOP), system_tx(0, ORACLE_SLOT, 7), system_tx(1, ORACLE_SLOT, 8)];
    let (mut block, reached) = session_over(&parent, None);
    for tx in &txs {
        include(&mut block, tx);
    }
    assert_eq!(l1_reached(&reached), [None, Some(8)], "the account and slot 8, once each");
    let output = block.finish().unwrap();
    assert!(output.receipts.iter().all(|r| r.status));
    assert_eq!(l1_entry(&output.tx_states[0]), None);
    assert_eq!(l1_entry(&output.tx_states[1]).map(|e| e.1), Some(vec![8]));
    assert_eq!(l1_entry(&output.tx_states[2]), None);
    let built = produce(&parent, &included(&txs));
    for slot in [1, 3, 5, 7] {
        assert_eq!(built.witness.slot(&L1_BLOCK_CONTRACT, &U256::from(slot)), None, "{slot}");
    }
    assert!(built.witness.slot(&L1_BLOCK_CONTRACT, &U256::from(8)).is_some());
    let (replayed, _) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
}

/// A fee-free Mega System Transaction still reads, and reports, the footprint scalar; a fee payer
/// after it reports the slots of its fee computation the block had not reported, under its own
/// entry for the account (a slot needs its account's entry).
#[test]
fn witness_footprint_of_a_fee_free_system_transaction_is_its_own_report() {
    let parent = chain(L1::Present);
    let built = produce(&parent, &included(&[system_tx(0, ORACLE_SLOT, 7), call(SENDER, 0, NOOP)]));
    assert_eq!(l1_entry(&built.output.tx_states[0]).map(|e| e.1), Some(vec![8]));
    assert_eq!(l1_entry(&built.output.tx_states[1]).map(|e| e.1), Some(vec![1, 3, 7]));
    let (replayed, _) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
}

/// A candidate that loaded the L1 fee data and was dropped reports nothing: the first included
/// transaction priced from that cache reports every key it was loaded from, though its own
/// execution reached none of them in the database.
#[test]
fn witness_discarded_candidate_does_not_consume_the_first_read() {
    let parent = chain(L1::Present);
    let (mut block, reached) = session_over(&parent, None);
    let dropped = block.execute_candidate(&call(SENDER, 0, NOOP).tx, &[7; 100]).unwrap();
    let loaded = l1_reached(&reached);
    assert_eq!(loaded, [None, Some(8), Some(1), Some(7), Some(3)]);
    drop(dropped);
    let tx = call(OTHER, 0, NOOP);
    let candidate = block.execute_candidate(&tx.tx, &tx.encoded).unwrap();
    assert_eq!(l1_reached(&reached), loaded, "the cache served every read");
    assert_eq!(candidate.l1_reads().slots(), l1_values(false, &[1, 3, 7, 8]));
    block.commit_candidate(candidate).unwrap();
    let output = block.finish().unwrap();
    assert_eq!(l1_entry(&output.tx_states[0]).map(|e| e.1), Some(vec![1, 3, 7, 8]));
    let witness =
        Witness::resolve(&WitnessKeys::of(&output), &mut parent.clone(), salt().as_ref()).unwrap();
    let (replayed, _) = replayed(witness, &[tx]);
    assert_same_block(&replayed, &output);
}

/// Ownership is decided at the commit, not the execution: of two candidates executed before
/// either is committed, the one committed first reports the reads, though the other loaded the
/// fee data; the other, executed again on the committed state as a builder does and committed,
/// reports none.
#[test]
fn witness_first_included_is_decided_at_the_commit() {
    let parent = chain(L1::Present);
    let mut block = session(&parent);
    let (a, b) = (call(SENDER, 0, NOOP), call(OTHER, 0, NOOP));
    let first = block.execute_candidate(&a.tx, &a.encoded).unwrap();
    let second = block.execute_candidate(&b.tx, &b.encoded).unwrap();
    assert_eq!(first.l1_reads(), second.l1_reads());
    block.commit_candidate(second).unwrap();
    // Executed on the state before the other's commit, `first` is stale for the fee vaults it
    // credits: a builder executes it again.
    drop(first);
    let again = block.execute_candidate(&a.tx, &a.encoded).unwrap();
    assert_eq!(again.l1_reads().slots(), l1_values(false, &[1, 3, 7, 8]), "the cache's keys");
    block.commit_candidate(again).unwrap();
    let output = block.finish().unwrap();
    assert_eq!(l1_entry(&output.tx_states[0]).map(|e| e.1), Some(vec![1, 3, 7, 8]));
    assert_eq!(l1_entry(&output.tx_states[1]), None);
    let witness =
        Witness::resolve(&WitnessKeys::of(&output), &mut parent.clone(), salt().as_ref()).unwrap();
    let (replayed, _) = replayed(witness, &[b, a]);
    assert_same_block(&replayed, &output);
}

/// A transaction that writes the L1 block contract's account (a value transfer to it) keeps its
/// write: the read-only entries of its fee and footprint reads add the slots under its own entry
/// and replace nothing.
#[test]
fn witness_first_included_read_entry_never_replaces_the_transactions_write() {
    let parent = chain(L1::Present);
    let to_l1 = legacy(SENDER, 0, TxKind::Call(L1_BLOCK_CONTRACT), 5, Vec::new());
    let built = produce(&parent, &included(&[to_l1]));
    let state = &built.output.tx_states[0];
    let entry = state.account_changes_for(&L1_BLOCK_CONTRACT).unwrap();
    assert!(entry.is_changed());
    assert_eq!(entry.current.map(|a| a.balance), Some(U256::from(5)));
    assert_eq!(l1_entry(state).map(|e| e.1), Some(vec![1, 3, 7, 8]));
    let changed = built.output.changes.accounts().find(|(a, _)| *a == L1_BLOCK_CONTRACT);
    let balance = changed.and_then(|(_, t)| t.current.as_ref().map(|a| a.balance));
    assert_eq!(balance, Some(U256::from(5)), "the write is in the block's changes");
    let (replayed, _) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
}

/// An absent L1 block contract is recorded as absent, with no slot, by the first non-deposit
/// transaction, and the witness holds the absence alone: an absent account has no storage. The
/// footprint read stops at the account; the fee computation's slot reads still reach the node's
/// database, as revm's reads of an absent account's storage do (the factory's read set follows
/// revm's), and are recorded as no read of the state; a replay answers them zero.
#[test]
fn witness_absent_l1block_records_its_absence_and_no_slot() {
    let parent = chain(L1::Absent);
    let txs = [deposit(NOOP), call(SENDER, 0, NOOP), call(SENDER, 1, NOOP)];
    let (mut block, reached) = session_over(&parent, None);
    include(&mut block, &txs[0]);
    let tx = &txs[1];
    let candidate = block.execute_candidate(&tx.tx, &tx.encoded).unwrap();
    assert_eq!(candidate.l1_reads().account(), Some(None));
    assert!(candidate.l1_reads().slots().is_empty());
    // The footprint read stops at the account; the fee computation's first slot read follows.
    assert_eq!(l1_reached(&reached)[..2], [None, Some(1)]);
    block.commit_candidate(candidate).unwrap();
    include(&mut block, &txs[2]);
    let output = block.finish().unwrap();
    assert_eq!(l1_entry(&output.tx_states[1]), Some((None, vec![])));
    let entry = output.tx_states[1].account_changes_for(&L1_BLOCK_CONTRACT).unwrap();
    assert!(entry.original.is_none() && !entry.is_changed());
    assert_eq!(l1_entry(&output.tx_states[2]), None);
    let built = produce(&parent, &included(&txs));
    assert_eq!(built.witness.account(&L1_BLOCK_CONTRACT), Some(None));
    assert!(built.keys.accounts()[&L1_BLOCK_CONTRACT].is_empty());
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    assert!(read.contains(&Evidence::Account(L1_BLOCK_CONTRACT)));
    let l1_slot = |e: &Evidence| matches!(e, Evidence::Slot(a, _) if *a == L1_BLOCK_CONTRACT);
    assert!(!read.iter().any(l1_slot), "no slot of an absent account needs evidence");
}

/// A block of deposits reads the L1 block contract nowhere — neither before its transactions nor
/// for them — so no state names it and the witness holds none of it.
#[test]
fn witness_deposits_only_block_reads_no_l1block() {
    let parent = chain(L1::Present);
    let txs = [deposit(NOOP), deposit(HALTER), deposit(READER)];
    let (mut block, reached) = session_over(&parent, None);
    assert!(l1_reached(&reached).is_empty(), "the pre-block sequence reads none");
    for tx in &txs {
        include(&mut block, tx);
    }
    assert!(l1_reached(&reached).is_empty());
    let output = block.finish().unwrap();
    assert!(output.pre_block_states.iter().chain(&output.tx_states).all(|s| l1_entry(s).is_none()));
    let built = produce(&parent, &included(&txs));
    assert_eq!(built.witness.account(&L1_BLOCK_CONTRACT), None);
    let (replayed, _) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
}

/// A fee read the database fails is a failure of the transaction's own execution: internal when
/// the database declares it fatal, an invalid block otherwise; a Mega System Transaction, which
/// makes no fee read, is not affected (`SATIN-VAL-010`).
#[test]
fn witness_l1_fee_read_failure_is_the_transactions_by_fatality() {
    let parent = chain(L1::Present);
    for fatal in [true, false] {
        let (mut block, _) = session_over(&parent, Some((Some(1), fatal)));
        include(&mut block, &system_tx(0, ORACLE_SLOT, 7));
        let tx = call(SENDER, 0, NOOP);
        let error = block.execute_candidate(&tx.tx, &tx.encoded).expect_err("slot 1 fails");
        let CandidateError::Block(fault) = &error else { panic!("{error:?}") };
        assert_eq!(fault.phase(), Phase::Transaction, "fatal {fatal}");
        let expected = if fatal { Disposition::Internal } else { Disposition::InvalidBlock };
        assert_eq!(fault.disposition(), expected);
        assert!(fault.database_error().unwrap().downcast_ref::<Unavailable>().is_some());
    }
}

/// A footprint read the database fails, of the account or of slot 8, is the block's read for the
/// transaction: internal whatever the database declares, for a Mega System Transaction and a fee
/// payer alike; a deposit makes none.
#[test]
fn witness_footprint_read_failure_is_internal_whatever_the_fatality() {
    let parent = chain(L1::Present);
    for (slot, fatal) in [(Some(8), true), (Some(8), false), (None, true), (None, false)] {
        for tx in [system_tx(0, ORACLE_SLOT, 7), call(SENDER, 0, NOOP)] {
            let (mut block, _) = session_over(&parent, Some((slot, fatal)));
            include(&mut block, &deposit(NOOP));
            let error = block.execute_candidate(&tx.tx, &tx.encoded).expect_err("the read fails");
            let CandidateError::Block(fault) = &error else { panic!("{error:?}") };
            assert_eq!(fault.phase(), Phase::Bookkeeping, "{slot:?} {fatal}");
            assert_eq!(fault.disposition(), Disposition::Internal, "{slot:?} {fatal}");
            assert_eq!(fault.context().address, Some(L1_BLOCK_CONTRACT));
            assert_eq!(fault.context().slot, slot.map(U256::from));
        }
    }
}

/// A single transaction's EVM keeps the reads its last transaction made outside its journal — the
/// L1 fee reads, the block hashes served, the SALT lookups — and a transaction the engine refuses
/// before it begins (a typed transaction from the live system address) forgets them: no record of
/// the transaction before it is left behind.
#[test]
fn witness_refused_transaction_leaves_no_stale_record() {
    let parent = chain(L1::Present);
    let block = evm2::env::BlockEnv::<mega_evm2::MegaTypes> {
        number: U256::from(BLOCK),
        timestamp: U256::from(10_000 + BLOCK),
        gas_limit: U256::from(30_000_000u64),
        ..Default::default()
    };
    let mut evm = common::evm(block, Db::new(parent));
    let reader = call(SENDER, 0, READER);
    let _ = evm.transact(&reader.tx).expect("it runs").commit();
    let ext = evm.ext();
    assert_eq!(ext.l1_reads.slots(), l1_values(false, &[1, 3, 7, 8]));
    assert!(!ext.block_hashes.is_empty());
    assert!(!ext.salt.lookups().is_empty());
    let refused = call_1559(SYSTEM, 0, NOOP);
    evm.transact(&refused.tx).expect_err("a typed transaction from the system address");
    let ext = evm.ext();
    assert!(ext.l1_reads.is_empty());
    assert!(ext.block_hashes.is_empty());
    assert!(ext.salt.lookups().is_empty());
}
