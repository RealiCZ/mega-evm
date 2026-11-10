//! Each pre-block step returns its own whole state, including read-only witness entries.

mod common;

use alloy_primitives::{B256, Bytes, U256, hex};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{SEQUENCER_REGISTRY, block::BlockSession};
use op_evm2::constants::L1_BLOCK_CONTRACT;

fn block(present: bool, due: bool) -> BlockSession<'static> {
    let mut db = InMemoryDB::default();
    if present {
        // applyPendingChanges clears slot 7; the preceding read-only state must still hold 77.
        db.insert_account_info(
            &SEQUENCER_REGISTRY,
            AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from(hex!("5f60075500")))),
        );
        db.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(7), &U256::from(77));
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
        assert_eq!(states.len(), if due { 5 } else { 4 }, "due={due}");
        assert!(states[0].account_changes_for(&mega_evm2::block::HISTORY_STORAGE).is_some());
        assert!(states[1].account_changes_for(&mega_evm2::block::BEACON_ROOTS).is_some());
        let registry = states[2].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
        assert!(!registry.is_changed());
        let mut keys = registry.storage.slots.keys().copied().collect::<Vec<_>>();
        keys.sort();
        assert_eq!(keys, [7, 8, 9].map(U256::from));
        assert_eq!(registry.storage.slots[&U256::from(7)].value.current, U256::from(77));
        if due {
            let applied = states[3].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
            assert_eq!(applied.storage.slots[&U256::from(7)].value.current, U256::ZERO);
        }
        let l1 = states.last().unwrap().account_changes_for(&L1_BLOCK_CONTRACT).unwrap();
        assert!(!l1.is_changed());
        assert_eq!(l1.storage.slots.len(), 5);
        for slot in [1, 3, 5, 7, 8] {
            let value = &l1.storage.slots[&U256::from(slot)].value;
            assert_eq!(value.current, U256::from(slot * 10));
            assert_eq!(value.original, value.current);
        }
        assert_eq!(block.limiter().block_gas_used, 0);
        assert_eq!(block.limiter().usage, [0; 3]);
        // A due change was applied: none is due any more.
        assert_eq!(block.registry_due(), Ok(false));
    }
}

#[test]
fn test_absent_pre_block_accounts_are_recorded_without_slots() {
    let block = block(false, false);
    assert_eq!(block.pre_block_states().len(), 4);
    for (index, address) in [(2, SEQUENCER_REGISTRY), (3, L1_BLOCK_CONTRACT)] {
        let read = block.pre_block_states()[index]
            .account_changes_for(&address)
            .expect("absent account recorded");
        assert!(read.original.is_none());
        assert!(read.current.is_none());
        assert!(read.storage.slots.is_empty());
        assert!(!read.is_changed());
    }
}
