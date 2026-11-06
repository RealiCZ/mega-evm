//! A block without a parent beacon block root is invalid (EIP-4788, Satin's pre-block sequence):
//! the executor refuses it at the EIP-4788 step instead of making the call with a zero root.

use alloy_primitives::{B256, U256};
use evm2::{
    Evm, ExecutionConfig,
    env::BlockEnv,
    evm::{Db, InMemoryDB},
};
use mega_evm2::{
    MegaSpec, MegaTypes, block::BlockExecutor, precompiles, satin_version, tx::registry,
};

fn block(parent_beacon_block_root: Option<B256>) -> BlockExecutor<'static> {
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let env = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000),
        ..BlockEnv::<MegaTypes>::default()
    };
    let evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        env,
        registry(),
        Db::new(InMemoryDB::default()),
        precompiles(),
    );
    BlockExecutor::new(evm, B256::ZERO, parent_beacon_block_root)
}

#[test]
fn test_a_block_without_a_parent_beacon_block_root_is_refused() {
    let refused = block(None).apply_pre_execution_changes();
    let error = refused.expect_err("a block without a parent beacon block root ran");
    assert!(error.contains("parent beacon block root"), "{error}");
}

/// The control: the same block with a root runs its pre-block calls.
#[test]
fn test_a_block_with_a_parent_beacon_block_root_runs() {
    block(Some(B256::repeat_byte(0x11))).apply_pre_execution_changes().expect("pre-block calls");
}
