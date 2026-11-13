//! Each pre-block step returns its own whole state, including read-only witness entries, in the
//! order of the sequence (`SATIN-PBLK-003`): the EIP-2935 and EIP-4788 calls, the five flat
//! system contracts' deployments, the registry's due-change read and its `applyPendingChanges()`
//! call when due, the registry's deployment and the resolution of the live system address, last.
//! No step reads the L1 block contract: its reads belong to the transactions that make them
//! (`SATIN-WIT-011`).

mod common;

use alloy_primitives::{Address, B256, U256};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    SEQUENCER_REGISTRY,
    block::BlockSession,
    system_contracts::{FLAT_SYSTEM_CONTRACTS, SEQUENCER_REGISTRY_CODE, address_word},
};
use op_evm2::constants::L1_BLOCK_CONTRACT;

/// The registry's live system address in the fixture, and the pending one a due change installs.
const LIVE: Address = Address::with_last_byte(0x5a);
const PENDING: Address = Address::with_last_byte(0x77);

fn block(present: bool, due: bool) -> BlockSession<'static> {
    let mut db = InMemoryDB::default();
    if present {
        // A pending system-address change whose activation block is the block (due) or the next.
        db.insert_account_info(
            &SEQUENCER_REGISTRY,
            AccountInfo::default().with_code(Bytecode::new_legacy(SEQUENCER_REGISTRY_CODE)),
        );
        db.insert_account_storage(&SEQUENCER_REGISTRY, &U256::ZERO, &address_word(LIVE));
        db.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(7), &address_word(PENDING));
        db.insert_account_storage(
            &SEQUENCER_REGISTRY,
            &U256::from(8),
            &U256::from(if due { 1 } else { 2 }),
        );
        db.insert_account_info(&L1_BLOCK_CONTRACT, AccountInfo::default().with_nonce(1));
        for slot in [1, 3, 5, 7, 8] {
            db.insert_account_storage(
                &L1_BLOCK_CONTRACT,
                &U256::from(slot),
                &U256::from(slot * 10),
            );
        }
    }
    let input = common::block_input(1, 1_000, 30_000_000, Some(B256::ZERO));
    common::start_block(&input, Db::new(db))
}

#[test]
fn test_read_only_pre_block_states_are_committed_in_order() {
    for due in [false, true] {
        // The session runs the pre-block sequence before it is returned.
        let mut block = block(true, due);
        let states = block.pre_block_states().to_vec();
        assert_eq!(states.len(), if due { 11 } else { 10 }, "due={due}");
        assert!(states[0].account_changes_for(&mega_evm2::block::HISTORY_STORAGE).is_some());
        assert!(states[1].account_changes_for(&mega_evm2::block::BEACON_ROOTS).is_some());
        for (state, contract) in states[2..7].iter().zip(&FLAT_SYSTEM_CONTRACTS) {
            let deployed = state.account_changes_for(&contract.address).expect(contract.name);
            assert_eq!(state.account_changes().count(), 1, "{}: its own state", contract.name);
            assert_eq!(deployed.current.map(|i| i.code_hash), Some(contract.code_hash));
        }
        let registry = states[7].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
        assert!(!registry.is_changed());
        let mut keys = registry.storage.slots.keys().copied().collect::<Vec<_>>();
        keys.sort();
        assert_eq!(keys, [7, 8, 9].map(U256::from));
        assert_eq!(registry.storage.slots[&U256::from(7)].value.current, address_word(PENDING));
        let next = if due {
            let applied = states[8].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
            assert_eq!(applied.storage.slots[&U256::from(7)].value.current, U256::ZERO);
            assert_eq!(applied.storage.slots[&U256::ZERO].value.current, address_word(PENDING));
            9
        } else {
            8
        };
        // Version 2.0.0: the deployment leaves it as it is, a read-only entry of its own.
        let deployed = states[next].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
        assert!(!deployed.is_changed() && deployed.storage.slots.is_empty());
        // The L1 block contract, though it exists, is in no pre-block state.
        assert!(states.iter().all(|s| s.account_changes_for(&L1_BLOCK_CONTRACT).is_none()));
        // The resolution, last, right after the deployment: the account and slot 0 as the steps
        // before left them.
        assert_eq!(states.len(), next + 2);
        let resolved = states.last().unwrap().account_changes_for(&SEQUENCER_REGISTRY).unwrap();
        assert!(!resolved.is_changed());
        let live = if due { PENDING } else { LIVE };
        assert_eq!(resolved.storage.slots[&U256::ZERO].value.current, address_word(live));
        assert_eq!(block.system_address(), Some(live));
        assert_eq!(block.limiter().block_gas_used, 0);
        assert_eq!(block.limiter().usage, [0; 3]);
        // A due change was applied: none is due any more.
        assert_eq!(block.registry_due(), Ok(false));
    }
}

#[test]
fn test_absent_pre_block_accounts_are_recorded_without_slots() {
    let block = block(false, false);
    assert_eq!(block.pre_block_states().len(), 10);
    let read = block.pre_block_states()[7]
        .account_changes_for(&SEQUENCER_REGISTRY)
        .expect("absent account recorded");
    assert!(read.original.is_none());
    assert!(read.current.is_none());
    assert!(read.storage.slots.is_empty());
    assert!(!read.is_changed());
    let states = block.pre_block_states();
    assert!(states.iter().all(|s| s.account_changes_for(&L1_BLOCK_CONTRACT).is_none()));
}
