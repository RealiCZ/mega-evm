//! A block replayed on exactly its witness (`SATIN-WIT-016`): the building node derives the
//! witness from the block's committed states and exports alone (`mega_evm2::witness`), and a
//! validator replays the block on a strict database that answers exactly that witness, with no
//! live state and no Oracle service. The replay produces the same receipts, state changes, states,
//! ledgers, logs and counters, and reads nothing the witness does not hold; each piece of evidence
//! it reads, removed, fails it with a typed missing-evidence error.

mod witness_fixture;

use alloy_primitives::{Address, TxKind, U256};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, CacheDB, Db},
};
use mega_evm2::{
    oracle::{OracleAnswers, OracleServices},
    tx::ORACLE,
    witness::{Evidence, WitnessKeys},
};
use op_evm2::constants::L1_BLOCK_CONTRACT;
use std::{collections::BTreeSet, sync::Arc};
use witness_fixture::*;

/// The account the creation in [`mixed`] makes.
fn created() -> Address {
    SENDER.create(1)
}

/// A block of every kind of read: a deposit, a failed deposit, a Mega System Transaction, a
/// dropped user transaction, user transactions whose frames revert and halt, that read a block
/// hash and the balance of a contract without its code, a creation and a later read of the new
/// account, and an Oracle read that runs out of gas.
fn mixed() -> Vec<Step> {
    vec![
        Step::Include(deposit(NOOP)),
        Step::Include(deposit(HALTER)),
        Step::Include(system_tx(0, ORACLE_SLOT, 7)),
        Step::Drop(call(SENDER, 0, PROBE)),
        Step::Include(call(SENDER, 0, READER)),
        Step::Include(legacy(SENDER, 1, TxKind::Create, 0, creation_code())),
        Step::Include(call(OTHER, 0, created())),
        Step::Include(call_1559(OTHER, 1, PROBE)),
        Step::Include(call(OTHER, 2, ORACLE_OOG)),
    ]
}

#[test]
fn witness_replay_of_a_mixed_block_gives_identical_results() {
    let parent = chain(L1::Present);
    let built = produce(&parent, &mixed());
    let statuses: Vec<bool> = built.output.receipts.iter().map(|r| r.status).collect();
    // The halted deposit failed; the others succeeded (the reverted and halted frames are
    // inner ones; the Oracle read's call ran out of gas inside a frame that went on).
    assert_eq!(statuses, [true, false, true, true, true, true, true, true]);
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    // Every piece of evidence the replay read is in the witness, and what it did not read is
    // code no load needed.
    let held: BTreeSet<Evidence> = built.witness.evidence().into_iter().collect();
    assert!(read.is_subset(&held));
    let unread: Vec<&Evidence> = held.difference(&read).collect();
    assert!(unread.iter().all(|e| matches!(e, Evidence::Code(_))), "{unread:?}");
}

/// Each piece of evidence the replay of [`mixed`] reads — an account, present or absent, a slot,
/// an empty one included, code, a block hash, a SALT capacity — removed from the witness alone,
/// fails the replay with a typed missing-evidence error naming that piece: an internal error,
/// never a panic, a default value or another block.
#[test]
fn witness_omit_each_piece_the_replay_reads_and_it_fails_with_that_evidence() {
    let parent = chain(L1::Present);
    let built = produce(&parent, &mixed());
    let (_, read) = replayed(built.witness.clone(), &built.included);
    let kinds = |f: fn(&Evidence) -> bool| read.iter().filter(|e| f(e)).count();
    assert!(kinds(|e| matches!(e, Evidence::Account(_))) > 10);
    assert!(kinds(|e| matches!(e, Evidence::Slot(..))) > 5);
    assert!(kinds(|e| matches!(e, Evidence::Code(_))) > 3);
    assert_eq!(kinds(|e| matches!(e, Evidence::BlockHash(_))), 1);
    assert!(kinds(|e| matches!(e, Evidence::Capacity(_))) > 0);
    // The L1 block contract's account and the slots its two reads use, and the absent account the
    // creation makes, are among them.
    for slot in [1, 3, 7, 8] {
        assert!(read.contains(&Evidence::Slot(L1_BLOCK_CONTRACT, U256::from(slot))), "{slot}");
    }
    assert!(read.contains(&Evidence::Account(created())));
    assert_eq!(built.witness.account(&created()), Some(None));
    for evidence in &read {
        let mut witness = built.witness.clone();
        assert!(witness.remove(evidence));
        let (result, _) = replay(witness, &built.included);
        let error = result.expect_err("the replay cannot go on without it");
        assert_eq!(missing(&error), *evidence, "{error:?}");
    }
}

/// A transaction that loads a contract's account and not its code (`BALANCE`) names the account,
/// and the witness holds the contract's code as the chain held it before the block all the same
/// (`SATIN-WIT-001`): the account is recorded without it, and the replay, which needs no code,
/// reads none.
#[test]
fn witness_code_before_the_block_is_held_for_an_account_loaded_without_it() {
    let parent = chain(L1::Present);
    let built = produce(&parent, &included(&[call(SENDER, 0, PROBE)]));
    let state = &built.output.tx_states[0];
    assert!(state.account_changes_for(&HOLDER).is_some());
    let info = built.witness.account(&HOLDER).unwrap().expect("the holder exists");
    assert!(info.code.is_none(), "the account is recorded without its code");
    let code = built.witness.code(&info.code_hash).expect("its code is held");
    assert_eq!(code.original_bytes()[..], [0x60, 0x2a, 0x00]);
    let (output, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&output, &built.output);
    assert!(!read.contains(&Evidence::Code(info.code_hash)), "no load needed it");
    // Removing it fails nothing: a piece no read needs is a superset item.
    let mut witness = built.witness.clone();
    assert!(witness.remove(&Evidence::Code(info.code_hash)));
    let (again, _) = replayed(witness, &built.included);
    assert_same_block(&again, &built.output);
}

/// An authority delegated anew is admitted against the delegation the chain held before the
/// block, which the witness holds; the later transaction's state carries the new delegation as
/// its original, which a witness built from the states' values would hold in its place
/// (`SATIN-WIT-007`). Without the old delegation the replay fails at the authorization's read.
#[test]
fn witness_redelegation_keeps_the_delegation_the_chain_held() {
    let authority = Address::with_last_byte(0x7a);
    let (old, new) = (Address::with_last_byte(0x7b), Address::with_last_byte(0x7c));
    let mut parent = chain(L1::Present);
    parent.insert_account_info(
        &authority,
        AccountInfo::default().with_nonce(4).with_code(Bytecode::new_eip7702(old)),
    );
    parent.insert_account_info(&old, contract([asm::sstore(1, 1), vec![0x00]].concat()));
    parent.insert_account_info(&new, contract([asm::sstore(2, 2), vec![0x00]].concat()));
    let txs = [delegation(SENDER, 0, authority, 4, new), call(OTHER, 0, authority)];
    let built = produce(&parent, &included(&txs));
    assert!(built.output.receipts.iter().all(|r| r.status));
    let old_hash = Bytecode::new_eip7702(old).hash_slow();
    let new_hash = Bytecode::new_eip7702(new).hash_slow();
    // The call ran the new delegation's code in the authority's storage.
    let written: Vec<U256> = built
        .output
        .changes
        .storage()
        .filter(|(k, _)| k.address() == authority)
        .map(|(k, _)| k.key())
        .collect();
    assert_eq!(written, [U256::from(2)]);
    let state = &built.output.tx_states[1];
    let seen = state.account_changes_for(&authority).unwrap().original.unwrap().code_hash;
    assert_eq!(seen, new_hash, "the later state's original is the new delegation");
    let held = built.witness.account(&authority).unwrap().unwrap().code_hash;
    assert_eq!(held, old_hash, "the witness holds the chain's");
    assert!(built.witness.code(&old_hash).is_some());
    assert!(built.witness.code(&new_hash).is_none());
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    assert!(read.contains(&Evidence::Code(old_hash)));
    let mut witness = built.witness.clone();
    witness.remove(&Evidence::Code(old_hash));
    let (result, _) = replay(witness, &built.included);
    assert_eq!(
        missing(&result.expect_err("the old delegation is needed")),
        Evidence::Code(old_hash)
    );
}

/// A failed deposit takes back its frame's writes, not its reads: the slot its halted frame read is
/// in its state and in the witness, and the replay, which fails it the same way, needs it.
#[test]
fn witness_failed_deposit_keeps_its_reads_in_the_witness() {
    let parent = chain(L1::Present);
    let built = produce(&parent, &included(&[deposit(HALTER)]));
    assert!(!built.output.receipts[0].status, "a failed deposit");
    let state = &built.output.tx_states[0];
    let halter = state.account_changes_for(&HALTER).unwrap();
    assert!(halter.storage.slots.contains_key(&U256::from(1)));
    assert_eq!(built.witness.slot(&HALTER, &U256::from(1)), Some(U256::from(31)));
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    let slot = Evidence::Slot(HALTER, U256::from(1));
    assert!(read.contains(&slot));
    let mut witness = built.witness.clone();
    witness.remove(&slot);
    let (result, _) = replay(witness, &built.included);
    assert_eq!(missing(&result.expect_err("the failed deposit's read is needed")), slot);
}

/// The reads of a frame that reverted and of one that halted — their rollback takes back writes,
/// not reads — are in the transaction's state and the witness, each needed by the replay, an empty
/// slot of an existing account included, proved zero.
#[test]
fn witness_rollback_keeps_the_reads_of_reverted_and_halted_frames() {
    let parent = chain(L1::Present);
    let built = produce(&parent, &included(&[call(SENDER, 0, READER)]));
    assert!(built.output.receipts[0].status);
    let state = &built.output.tx_states[0];
    for contract in [REVERTER, HALTER] {
        let entry = state.account_changes_for(&contract).unwrap();
        assert!(entry.storage.slots.contains_key(&U256::from(1)), "{contract}");
        assert!(!entry.is_changed(), "{contract}");
    }
    assert_eq!(built.witness.slot(&READER, &U256::from(2)), Some(U256::ZERO), "proved zero");
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    for slot in [
        Evidence::Slot(REVERTER, U256::from(1)),
        Evidence::Slot(HALTER, U256::from(1)),
        Evidence::Slot(READER, U256::from(2)),
    ] {
        assert!(read.contains(&slot), "{slot:?}");
        let mut witness = built.witness.clone();
        witness.remove(&slot);
        let (result, _) = replay(witness, &built.included);
        assert_eq!(missing(&result.expect_err("needed")), slot);
    }
}

/// An account the chain did not hold before the block, created by one transaction and read by a
/// later one, is recorded as absent with no slot (`SATIN-WIT-006`): its slots are the block's own
/// writes or zero, which the replay answers without evidence.
#[test]
fn witness_new_account_read_later_in_the_block_has_no_storage_proof() {
    let parent = chain(L1::Present);
    let txs =
        [legacy(SENDER, 0, TxKind::Create, 0, creation_code()), call(OTHER, 0, SENDER.create(0))];
    let built = produce(&parent, &included(&txs));
    assert!(built.output.receipts.iter().all(|r| r.status));
    let new = SENDER.create(0);
    let later = built.output.tx_states[1].account_changes_for(&new).unwrap();
    let mut keys: Vec<U256> = later.storage.slots.keys().copied().collect();
    keys.sort();
    assert_eq!(keys, [U256::ZERO, U256::from(1)], "the later transaction read both");
    assert_eq!(later.storage.slots[&U256::ZERO].value.original, U256::from(1), "the block's write");
    assert_eq!(built.witness.account(&new), Some(None));
    assert!(
        built.witness.evidence().iter().all(|e| !matches!(e, Evidence::Slot(a, _) if *a == new))
    );
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    assert!(read.contains(&Evidence::Account(new)));
}

/// A node whose state cache an earlier execution filled — warm, so the block's reads reach no
/// database — names the same keys as one that starts cold (`SATIN-WIT-005`): the witness is a
/// function of the states and the exports, and replays either way.
#[test]
fn witness_warm_and_cold_caches_name_the_same_keys() {
    let parent = chain(L1::Present);
    let (backend, reached) = Backend::new(parent.clone());
    let mut cache = CacheDB::new(Db::new(backend));
    let cold = produce_over(&parent, &mut cache, OracleServices::default(), &mixed());
    let after_cold = reached.borrow().len();
    assert!(after_cold > 20);
    let warm = produce_over(&parent, &mut cache, OracleServices::default(), &mixed());
    assert_eq!(reached.borrow().len(), after_cold, "the warm cache served every read");
    assert_eq!(warm.keys, cold.keys);
    assert_eq!(warm.output.tx_states, cold.output.tx_states);
    assert_eq!(warm.output.pre_block_states, cold.output.pre_block_states);
    assert_eq!(warm.witness, cold.witness);
    let (replayed, _) = replayed(warm.witness.clone(), &warm.included);
    assert_same_block(&replayed, &warm.output);
}

/// An Oracle read made before its charge loads the slot even when its frame then runs out of gas
/// at the `SLOAD` (`SATIN-ORCL-004`): the building node's service answered it, and it pushed
/// nothing; the slot is in the transaction's state and in the witness, and the replay, which runs
/// no service, makes the same read and needs it (`SATIN-WIT-012`).
#[test]
fn witness_oracle_oog_read_is_required_evidence() {
    let parent = chain(L1::Present);
    let service = Arc::new(OracleAnswers::from_iter([(U256::from(ORACLE_SLOT), U256::from(42))]));
    let built = produce_over(
        &parent,
        Db::new(parent.clone()),
        OracleServices::service(service),
        &included(&[call(OTHER, 0, ORACLE_OOG)]),
    );
    assert!(built.output.receipts[0].status, "the caller goes on after its call ran out of gas");
    let oracle = built.output.tx_states[0].account_changes_for(&ORACLE).unwrap();
    assert!(oracle.storage.slots.contains_key(&U256::from(ORACLE_SLOT)));
    let slot = Evidence::Slot(ORACLE, U256::from(ORACLE_SLOT));
    assert_eq!(
        built.witness.slot(&ORACLE, &U256::from(ORACLE_SLOT)),
        Some(U256::from(ORACLE_HELD))
    );
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    assert!(read.contains(&slot));
    let mut witness = built.witness.clone();
    witness.remove(&slot);
    let (result, _) = replay(witness, &built.included);
    assert_eq!(missing(&result.expect_err("the unpaid read is needed")), slot);
}

/// A dropped candidate's block hash and SALT buckets stay in the building node's exports, and so
/// in the witness, while its loads are in no committed state and in no witness; the replay, which
/// never runs it, exports a proper subset and reads neither (`SATIN-WIT-016`).
#[test]
fn witness_discarded_candidate_exports_are_a_superset_the_replay_does_not_read() {
    let parent = chain(L1::Present);
    let steps = [Step::Drop(call(OTHER, 0, HASHER)), Step::Include(call(SENDER, 0, NOOP))];
    let built = produce(&parent, &steps);
    assert_eq!(built.output.block_hash_reads.keys().copied().collect::<Vec<_>>(), [900]);
    assert!(!built.output.bucket_reads.is_empty(), "the dropped candidate's new slot");
    assert_eq!(built.witness.account(&HASHER), None, "no committed state loaded it");
    assert!(built.witness.block_hashes().contains_key(&900));
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    assert!(replayed.block_hash_reads.is_empty());
    assert!(replayed.bucket_reads.is_empty());
    assert!(!read.iter().any(|e| matches!(e, Evidence::BlockHash(_) | Evidence::Capacity(_))));
}

/// The witness keys are the committed states' keys and the exports, nothing else: every account and
/// slot a pre-block or transaction state names, each export, and no account of a dropped
/// candidate alone.
#[test]
fn witness_keys_are_the_committed_states_and_the_exports() {
    let parent = chain(L1::Present);
    let built = produce(&parent, &mixed());
    let mut expected = WitnessKeys::default();
    for state in built.output.pre_block_states.iter().chain(&built.output.tx_states) {
        expected.add_state(state);
    }
    expected.add_block_hashes(&built.output.block_hash_reads);
    expected.add_buckets(&built.output.bucket_reads);
    assert_eq!(built.keys, expected);
    assert_eq!(built.output.tx_states.len(), built.included.len());
    // Every pre-block step is in the keys: the registry's slot 0 read by the resolution, the
    // system contracts the deployments installed.
    assert!(built.keys.accounts()[&mega_evm2::SEQUENCER_REGISTRY].contains(&U256::ZERO));
    for contract in &mega_evm2::system_contracts::FLAT_SYSTEM_CONTRACTS {
        assert!(built.keys.accounts().contains_key(&contract.address), "{}", contract.name);
    }
}

/// A frame stopped at gas detention's crossing keeps what it read before the stop: the slot is in
/// the transaction's state and in the witness, and the replay, stopped the same way, needs it.
#[test]
fn witness_rollback_of_a_stopped_frame_keeps_its_read() {
    let parent = chain(L1::Present);
    let tx = legacy_with_gas(SENDER, 0, TxKind::Call(STOPPED), 0, Vec::new(), 25_000_000);
    let built = produce(&parent, &included(&[tx]));
    assert!(!built.output.receipts[0].status, "stopped");
    let gas = built.output.receipts[0].cumulative_gas_used;
    assert!(gas < 21_000_000, "stopped at the cap, not out of the transaction's gas: {gas}");
    let slot = Evidence::Slot(STOPPED, U256::from(1));
    let state = &built.output.tx_states[0];
    assert!(
        state.account_changes_for(&STOPPED).unwrap().storage.slots.contains_key(&U256::from(1))
    );
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    assert!(read.contains(&slot));
    let mut witness = built.witness.clone();
    witness.remove(&slot);
    let (result, _) = replay(witness, &built.included);
    assert_eq!(missing(&result.expect_err("the stopped frame's read is needed")), slot);
}

/// Every authority the authorization preflight consulted is loaded, also one the application
/// never reached because the transaction ran out of gas first (`SATIN-WIT-004`): both absent
/// authorities are in the transaction's state and in the witness, and the replay, which makes the
/// same preflight, needs the second as well.
#[test]
fn witness_unapplied_authority_is_required_evidence() {
    let parent = chain(L1::Present);
    let (first, second) = (Address::repeat_byte(0x71), Address::repeat_byte(0x72));
    let tx = delegations(SENDER, 0, 120_000, &[(first, 0, NOOP), (second, 0, NOOP)]);
    let built = produce(&parent, &included(&[tx]));
    assert!(!built.output.receipts[0].status, "out of gas before its first frame");
    let state = &built.output.tx_states[0];
    for authority in [first, second] {
        assert!(state.account_changes_for(&authority).is_some(), "{authority}");
        assert_eq!(built.witness.account(&authority), Some(None));
    }
    let (replayed, read) = replayed(built.witness.clone(), &built.included);
    assert_same_block(&replayed, &built.output);
    let evidence = Evidence::Account(second);
    assert!(read.contains(&evidence));
    let mut witness = built.witness.clone();
    witness.remove(&evidence);
    let (result, _) = replay(witness, &built.included);
    assert_eq!(missing(&result.expect_err("the second authority is needed")), evidence);
}
