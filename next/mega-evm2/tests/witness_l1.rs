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
use evm2::evm::{AccountInfo, Db, InMemoryDB};
use mega_evm2::{
    block::BlockSession,
    error::{CandidateError, Disposition, Phase},
    factory::BlockServices,
    witness::Evidence,
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

/// Executes and commits `tx` in `block`, and returns its ledger.
fn include(block: &mut BlockSession<'_>, tx: &Tx) -> TxLedger {
    let candidate = block.execute_candidate(&tx.tx, &tx.encoded).expect("it runs");
    let ledger = TxLedger::of(&candidate);
    block.commit_candidate(candidate).expect("it commits");
    ledger
}

/// The fixture's chain with `l1` at the L1 block contract, whose code — which a transaction
/// calling the contract runs, as the L1 attributes deposit runs the contract's — is `code`.
fn chain_with_l1_code(l1: L1, code: Vec<u8>) -> InMemoryDB {
    let mut parent = chain(l1);
    parent.insert_account_info(&L1_BLOCK_CONTRACT, contract(code));
    parent
}

/// The L1 data fee `ledger`'s transaction paid.
fn l1_fee(ledger: &TxLedger) -> U256 {
    ledger.result.ext.l1_fee
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
    assert_same_block(&replayed, &built);
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
    // The replay reads it, and without it fails naming it, as it does each piece it reads.
    let read = assert_exact_replay(&built);
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
    assert_same_block(&replayed, &built);
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
    assert_same_block(&replayed, &built);
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
    let ledger = TxLedger::of(&candidate);
    block.commit_candidate(candidate).unwrap();
    let built = Produced::of(block.finish().unwrap(), &parent, vec![tx], vec![ledger]);
    assert_eq!(l1_entry(&built.output.tx_states[0]).map(|e| e.1), Some(vec![1, 3, 7, 8]));
    assert_exact_replay(&built);
}

/// Ownership is decided at the commit, not the execution, and a commit takes only a candidate run
/// on the block's committed state: of two candidates executed before either is committed, the one
/// committed first reports the reads, though the other loaded the fee data. The other, run on the
/// state before that commit — both credit the L1 fee vault, and committing both would keep one
/// credit — is a retry, refused before anything changes: no receipt, counter, state or L1 read
/// ownership. Executed again and committed, it reports none, and the block is the serial one.
#[test]
fn witness_first_included_is_decided_at_the_commit() {
    let parent = chain(L1::Present);
    let mut block = session(&parent);
    let (a, b) = (call(SENDER, 0, NOOP), call(OTHER, 0, NOOP));
    let first = block.execute_candidate(&a.tx, &a.encoded).unwrap();
    let second = block.execute_candidate(&b.tx, &b.encoded).unwrap();
    assert_eq!(first.l1_reads(), second.l1_reads());
    assert_eq!(first.prefix(), second.prefix());
    assert!(!first.result().ext.l1_fee.is_zero(), "both credit the L1 fee vault");
    let mut ledgers = vec![TxLedger::of(&second)];
    block.commit_candidate(second).unwrap();
    let before = (
        block.receipts().len(),
        format!("{:?}", block.limiter()),
        format!("{:?}", block.changes()),
        block.tx_states().to_vec(),
        block.prefix(),
    );
    let found = first.prefix();
    let refused = block.commit_candidate(first).expect_err("it ran before the commit");
    assert_eq!(refused, CandidateError::OtherPrefix { expected: block.prefix(), found });
    assert_eq!(refused.disposition(), Disposition::Retry);
    let after = (
        block.receipts().len(),
        format!("{:?}", block.limiter()),
        format!("{:?}", block.changes()),
        block.tx_states().to_vec(),
        block.prefix(),
    );
    assert_eq!(after, before, "the refused commit changed nothing");
    assert!(block.fault().is_none());
    let again = block.execute_candidate(&a.tx, &a.encoded).unwrap();
    assert_eq!(again.l1_reads().slots(), l1_values(false, &[1, 3, 7, 8]), "the cache's keys");
    ledgers.push(TxLedger::of(&again));
    block.commit_candidate(again).unwrap();
    let built = Produced::of(block.finish().unwrap(), &parent, vec![b.clone(), a.clone()], ledgers);
    assert_eq!(l1_entry(&built.output.tx_states[0]).map(|e| e.1), Some(vec![1, 3, 7, 8]));
    assert_eq!(l1_entry(&built.output.tx_states[1]), None);
    let serial = produce(&parent, &included(&[b, a]));
    assert_eq!(built.ledgers, serial.ledgers);
    assert_same_output(&built.output, &serial.output);
    assert_exact_replay(&built);
}

/// A candidate another session ran commits only at the committed prefix it ran on (a node's
/// worker executing for its builder): run before either committed anything, it commits; run while
/// the builder is a transaction ahead, it is a retry; run after the worker committed another
/// transaction, as many as the builder, it is a retry, its prefix's digest another; run after the
/// worker committed the builder's transaction, it commits, and the block is the serial one.
#[test]
fn witness_replay_of_a_candidate_from_another_session_needs_its_prefix() {
    let parent = chain(L1::Present);
    let (y, z, x) = (call(SENDER, 0, NOOP), deposit(NOOP), call(OTHER, 0, NOOP));
    let run = |block: &mut BlockSession<'_>, tx: &Tx| {
        block.execute_candidate(&tx.tx, &tx.encoded).expect("it runs")
    };
    let mut builder = session(&parent);
    let mut worker = session(&parent);
    let candidate = run(&mut worker, &x);
    assert_eq!(candidate.prefix(), builder.prefix());
    builder.commit_candidate(candidate).expect("both at the empty prefix");
    let mut builder = session(&parent);
    include(&mut builder, &y);
    // The worker committed nothing; a deposit; another transaction of the builder's sender, with
    // the same nonce; the builder's transaction's bytes from another sender (the fixture's
    // signatures give no sender); against a builder that committed a deposit, another deposit of
    // the same depositor, nonce and source hash.
    let same_nonce = call(SENDER, 0, PROBE);
    let same_bytes = call(DEPOSITOR, 0, NOOP);
    assert_eq!(same_bytes.encoded, y.encoded);
    let other_deposit = deposit(HALTER);
    let mut after_deposit = session(&parent);
    include(&mut after_deposit, &z);
    let stale = |builder: &mut BlockSession<'_>, worker_commits: Option<&Tx>| {
        let mut worker = session(&parent);
        if let Some(tx) = worker_commits {
            include(&mut worker, tx);
        }
        let stale = run(&mut worker, &x);
        let found = stale.prefix();
        assert_eq!(found.committed() == builder.prefix().committed(), worker_commits.is_some());
        assert_ne!(found, builder.prefix());
        let refused = builder.commit_candidate(stale).expect_err("another prefix");
        assert_eq!(refused, CandidateError::OtherPrefix { expected: builder.prefix(), found });
        assert_eq!(refused.disposition(), Disposition::Retry);
    };
    for worker_commits in [None, Some(&z), Some(&same_nonce), Some(&same_bytes)] {
        stale(&mut builder, worker_commits);
    }
    stale(&mut after_deposit, Some(&other_deposit));
    let mut worker = session(&parent);
    include(&mut worker, &y);
    assert_eq!(worker.prefix(), builder.prefix());
    let candidate = run(&mut worker, &x);
    let ledger = TxLedger::of(&candidate);
    builder.commit_candidate(candidate).expect("the same prefix");
    let serial = produce(&parent, &included(&[y, x]));
    assert_eq!(ledger, serial.ledgers[1]);
    assert_same_output(&builder.finish().unwrap(), &serial.output);
}

/// The block's L1 info cache is its first included fee payer's also when another session ran it:
/// a builder that commits a worker's first fee payer, which changes the L1 base fee, prices its
/// own next transaction from the cache that candidate was priced from, not from the state it left,
/// as the serial block and its replay do.
#[test]
fn witness_l1_fee_cache_of_another_sessions_first_fee_payer_is_the_blocks() {
    let parent = chain_with_l1_code(L1::Present, [asm::sstore(1, 5_000), vec![0x00]].concat());
    let (x, y) = (call(OTHER, 0, L1_BLOCK_CONTRACT), call(SENDER, 0, NOOP));
    let mut builder = session(&parent);
    let mut worker = session(&parent);
    let candidate = worker.execute_candidate(&x.tx, &x.encoded).unwrap();
    let mut ledgers = vec![TxLedger::of(&candidate)];
    builder.commit_candidate(candidate).unwrap();
    ledgers.push(include(&mut builder, &y));
    assert_eq!(l1_fee(&ledgers[1]), l1_fee(&ledgers[0]), "the first fee payer's cache");
    let built =
        Produced::of(builder.finish().unwrap(), &parent, vec![x.clone(), y.clone()], ledgers);
    let serial = produce(&parent, &included(&[x, y]));
    assert_eq!(built.ledgers, serial.ledgers);
    assert_same_output(&built.output, &serial.output);
    assert_exact_replay(&built);
}

/// A dropped candidate's L1 info cache, loaded before a deposit that empties the Ecotone scalars,
/// prices nothing after it: the next fee computation loads the info from the state the deposit
/// left, takes the Bedrock fallback and reads the overhead, slot 5, which its transaction reports.
/// The block is the one built with no candidate before the deposit, and replays exactly, slot 5
/// required (a cache kept across the commit would price from the old scalars, report no slot 5,
/// and fail the replay for want of it).
#[test]
fn witness_l1_fee_cache_of_a_discarded_candidate_dies_with_a_deposit_that_empties_the_scalars() {
    let parent = chain_with_l1_code(L1::Present, [asm::sstore(3, 0), vec![0x00]].concat());
    let (update, user) = (deposit(L1_BLOCK_CONTRACT), call(SENDER, 0, NOOP));
    let steps = [
        Step::Drop(call(OTHER, 0, NOOP)),
        Step::Include(update.clone()),
        Step::Include(user.clone()),
    ];
    let built = produce(&parent, &steps);
    assert!(built.output.receipts.iter().all(|r| r.status));
    let (_, slots) = l1_entry(&built.output.tx_states[1]).expect("the first fee payer reports");
    assert_eq!(slots, [1, 3, 5, 7, 8]);
    let serial = produce(&parent, &included(&[update, user]));
    assert_eq!(built.ledgers, serial.ledgers);
    assert_same_output(&built.output, &serial.output);
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Slot(L1_BLOCK_CONTRACT, U256::from(5))));
}

/// A dropped candidate's L1 info cache, loaded before a deposit that changes the L1 base fee,
/// prices nothing after it: the next fee payer pays the fee of the new base fee, as in the block
/// built with no candidate before the deposit, and the replay charges the same.
#[test]
fn witness_l1_fee_cache_of_a_discarded_candidate_dies_with_a_deposit_that_changes_the_base_fee() {
    let parent = chain_with_l1_code(L1::Present, [asm::sstore(1, 5_000), vec![0x00]].concat());
    let (update, user) = (deposit(L1_BLOCK_CONTRACT), call(SENDER, 0, NOOP));
    let steps = [
        Step::Drop(call(OTHER, 0, NOOP)),
        Step::Include(update.clone()),
        Step::Include(user.clone()),
    ];
    let built = produce(&parent, &steps);
    let old = produce(&parent, &included(&[user.clone()]));
    assert!(l1_fee(&built.ledgers[1]) > l1_fee(&old.ledgers[0]), "priced at the new base fee");
    let serial = produce(&parent, &included(&[update, user]));
    assert_eq!(built.ledgers, serial.ledgers);
    assert_same_output(&built.output, &serial.output);
    assert_exact_replay(&built);
}

/// The block's L1 info cache is the one its first included fee payer was priced from, kept to the
/// block's end as op-revm keeps it per block: a later transaction that changes the L1 block
/// contract changes the price of no transaction after it, which reports nothing, here and on
/// replay.
#[test]
fn witness_l1_fee_cache_of_the_first_included_fee_payer_is_kept_to_the_block_end() {
    let parent = chain_with_l1_code(L1::Present, [asm::sstore(1, 5_000), vec![0x00]].concat());
    let txs = [call(SENDER, 0, NOOP), call(OTHER, 0, L1_BLOCK_CONTRACT), call(SENDER, 1, NOOP)];
    let built = produce(&parent, &included(&txs));
    assert!(built.output.receipts.iter().all(|r| r.status));
    let written = built.output.changes.storage().find(|(k, _)| k.address() == L1_BLOCK_CONTRACT);
    assert!(written.is_some(), "the second transaction changed the base fee");
    assert_eq!(l1_fee(&built.ledgers[2]), l1_fee(&built.ledgers[0]));
    assert_eq!(l1_entry(&built.output.tx_states[2]), None);
    assert_exact_replay(&built);
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
    assert_same_block(&replayed, &built);
}

/// An absent L1 block contract is recorded as absent, with no slot, by the first non-deposit
/// transaction, and the witness holds the absence alone: an absent account has no storage, so
/// neither read looks a slot up (`SATIN-WIT-011`). Over a node database that fails every storage
/// lookup once the block has started, the block's transactions run, and only the account is read;
/// the fee computation prices from the info all-zero storage gives, as it prices on an existing
/// contract with no storage.
#[test]
fn witness_absent_l1block_records_its_absence_and_no_slot() {
    let parent = chain(L1::Absent);
    let txs = [deposit(NOOP), call(SENDER, 0, NOOP), call(SENDER, 1, NOOP)];
    let (backend, reached) = Backend::new(parent.clone());
    let deny = backend.deny_storage.clone();
    let services = BlockServices::new(salt());
    let mut block = common::factory().start_block(&input(), Db::new(backend), services).unwrap();
    deny.set(true);
    let mut ledgers = Vec::new();
    for (i, tx) in txs.iter().enumerate() {
        let candidate = block.execute_candidate(&tx.tx, &tx.encoded).expect("no slot is looked up");
        if i == 1 {
            assert_eq!(candidate.l1_reads().account(), Some(None));
            assert!(candidate.l1_reads().slots().is_empty());
        }
        ledgers.push(TxLedger::of(&candidate));
        block.commit_candidate(candidate).unwrap();
    }
    let l1 = l1_reached(&reached);
    assert!(!l1.is_empty() && l1.iter().all(Option::is_none), "the account alone: {l1:?}");
    let built = Produced::of(block.finish().unwrap(), &parent, txs.to_vec(), ledgers);
    assert_eq!(l1_entry(&built.output.tx_states[1]), Some((None, vec![])));
    let entry = built.output.tx_states[1].account_changes_for(&L1_BLOCK_CONTRACT).unwrap();
    assert!(entry.original.is_none() && !entry.is_changed());
    assert_eq!(l1_entry(&built.output.tx_states[2]), None);
    assert_eq!(built.witness.account(&L1_BLOCK_CONTRACT), Some(None));
    assert!(built.keys.accounts()[&L1_BLOCK_CONTRACT].is_empty());
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Account(L1_BLOCK_CONTRACT)));
    let l1_slot = |e: &Evidence| matches!(e, Evidence::Slot(a, _) if *a == L1_BLOCK_CONTRACT);
    assert!(!read.iter().any(l1_slot), "no slot of an absent account needs evidence");
    // The same prices as on an existing contract whose storage is empty.
    let mut empty = chain(L1::Absent);
    empty.insert_account_info(&L1_BLOCK_CONTRACT, AccountInfo::default().with_nonce(1));
    assert_eq!(produce(&empty, &included(&txs)).ledgers, built.ledgers);
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
    assert_same_block(&replayed, &built);
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
