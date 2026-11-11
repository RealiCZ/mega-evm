//! A block without a parent beacon block root is invalid (EIP-4788, Satin's pre-block sequence):
//! the block session refuses it at the EIP-4788 step instead of making the call with a zero root.

mod common;

use alloy_primitives::B256;
use evm2::evm::{Db, InMemoryDB};
use mega_evm2::{
    block::BEACON_ROOTS,
    error::{BlockError, BlockErrorKind, Disposition, Phase},
};

fn start(parent_beacon_block_root: Option<B256>) -> Result<(), BlockError> {
    let input = common::block_input(1, 1_000, 30_000_000, parent_beacon_block_root);
    common::factory()
        .start_block(&input, Db::new(InMemoryDB::default()), common::services(B256::ZERO))
        .map(drop)
}

#[test]
fn test_a_block_without_a_parent_beacon_block_root_is_refused() {
    let error = start(None).expect_err("a block without a parent beacon block root ran");
    assert_eq!(error.kind(), &BlockErrorKind::MissingParentBeaconRoot);
    assert_eq!(error.phase(), Phase::PreBlockCall);
    assert_eq!(error.disposition(), Disposition::InvalidBlock);
    assert_eq!(error.context().address, Some(BEACON_ROOTS));
    assert!(error.to_string().contains("parent beacon block root"), "{error}");
}

/// The control: the same block with a root runs its pre-block calls.
#[test]
fn test_a_block_with_a_parent_beacon_block_root_runs() {
    start(Some(B256::repeat_byte(0x11))).expect("pre-block calls");
}
