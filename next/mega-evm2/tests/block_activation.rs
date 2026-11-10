//! What a block session takes from the chain configuration and the block's context: the block in
//! which Satin activates admits only deposits (`SATIN-ACT-004`, `SATIN-ACT-010`, `SATIN-BASE-006`),
//! block zero makes no EIP-2935 or EIP-4788 call and has a zero parent beacon block root
//! (`SATIN-ACT-012`, `SATIN-PBLK-003`), a block before `satinTime` is the legacy engine's
//! (`SATIN-ACT-001`), a block whose input contradicts its context or the configuration is not
//! executed, a fork the engine has no rules for fails explicitly, and every EVM runs on the
//! configuration's chain id. None of these is an input a caller can omit or set.

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256};
use evm2::{
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    MegaTypes,
    block::{BEACON_ROOTS, BlockSession, HISTORY_STORAGE},
    chain::{BootstrapInput, ChainConfig, ChainConfigInput, Fork, ForkSchedule},
    error::{BlockError, BlockErrorKind, CandidateError, Disposition, InputError, Phase, Refusal},
    factory::{BlockHeader, BlockInput, BlockServices, ParentContext, SatinFactory},
};
use op_evm2::{OpTxEnvelope, TxDeposit};

const SENDER: Address = Address::with_last_byte(0x50);
const TARGET: Address = Address::with_last_byte(0xa0);

fn bootstrap() -> BootstrapInput {
    BootstrapInput {
        initial_sequencer: Some(Address::repeat_byte(0x5e)),
        initial_admin: Some(Address::repeat_byte(0xad)),
        min_rotation_delay: Some(1),
    }
}

/// Chain `chain_id`, genesis at `genesis`, every base fork and every Mega spec below Satin from
/// genesis, Satin at `satin`; `extra` scheduled on top.
fn factory(chain_id: u64, genesis: u64, satin: u64, extra: &[(Fork, u64)]) -> SatinFactory {
    let mut schedule = ForkSchedule::satin_from(0).with(Fork::Satin, Some(satin));
    for (fork, time) in extra {
        schedule.set(*fork, Some(*time));
    }
    let input =
        ChainConfigInput { chain_id, genesis_timestamp: genesis, schedule, bootstrap: bootstrap() };
    SatinFactory::new(ChainConfig::new(input).expect("a valid configuration"))
}

fn db() -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
    );
    db.insert_account_info(
        &TARGET,
        AccountInfo::default()
            .with_nonce(1)
            .with_code(Bytecode::new_legacy(Bytes::from_static(&[0x00]))),
    );
    db
}

fn header(number: u64, timestamp: u64) -> BlockHeader {
    BlockHeader {
        number,
        timestamp,
        gas_limit: 30_000_000,
        parent_hash: B256::repeat_byte(0x01),
        parent_beacon_block_root: Some(B256::repeat_byte(0x11)),
        ..BlockHeader::default()
    }
}

/// Block `number` at `timestamp` on its parent at `parent`.
fn child(number: u64, timestamp: u64, parent: u64) -> BlockInput {
    BlockInput {
        header: header(number, timestamp),
        parent: ParentContext::Parent {
            number: number - 1,
            hash: B256::repeat_byte(0x01),
            timestamp: parent,
        },
    }
}

fn start(
    factory: &SatinFactory,
    input: &BlockInput,
    db: InMemoryDB,
) -> Result<BlockSession<'static>, BlockError> {
    factory.start_block(input, Db::new(db), BlockServices::default())
}

fn legacy(chain_id: u64) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy {
        chain_id: Some(chain_id),
        gas_limit: 100_000,
        to: TxKind::Call(TARGET),
        ..Default::default()
    };
    Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), SENDER)
}

fn deposit() -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(TARGET),
            gas_limit: 100_000,
            ..Default::default()
        }),
        SENDER,
    )
}

/// The input error of a block that does not start.
fn input_error(result: Result<BlockSession<'static>, BlockError>) -> InputError {
    let error = result.err().expect("the block does not start");
    assert_eq!(error.phase(), Phase::BlockInput, "{error:?}");
    assert_eq!(error.disposition(), Disposition::Internal, "no verdict on the block: {error:?}");
    match error.kind() {
        BlockErrorKind::Input(e) => *e,
        other => panic!("not an input error: {other:?}"),
    }
}

#[test]
fn chain_config_activation_block_admits_deposits_only() {
    let factory = factory(1, 0, 100, &[]);
    // Satin activates in the block at 100 whose parent is at 99: deposits only.
    let mut block = start(&factory, &child(5, 100, 99), db()).expect("the block starts");
    assert!(block.activation().satin_activates());
    assert!(block.activation().deposits_only());
    let error = block.execute_candidate(&legacy(1), &[]).expect_err("a user transaction");
    assert_eq!(error, CandidateError::Refused(Refusal::OnlyDeposits));
    assert_eq!(error.disposition(), Disposition::Refused);
    assert_eq!(error.to_string(), "unexpected non-deposit transaction in an activation block");
    let out = block.execute_candidate(&deposit(), &[]).expect("a deposit");
    block.commit_candidate(out).expect("committed");
    assert!(block.fault().is_none(), "a refusal decides nothing about the block by itself");
    // The next block admits every transaction.
    let mut next = start(&factory, &child(6, 101, 100), db()).expect("the block starts");
    assert!(!next.activation().satin_activates() && !next.activation().deposits_only());
    let out = next.execute_candidate(&legacy(1), &[]).expect("a user transaction");
    assert!(out.result.status, "{:?}", out.result);
    // An OP fork at or after Jovian activating in a Satin block: deposits only, no Satin
    // activation (here a fork the engine has no rules for, which then fails the block itself).
    let factory = self::factory(1, 0, 0, &[(Fork::Lagoon, 200)]);
    let activation = factory.config().activation(200, Some(199));
    assert!(activation.deposits_only() && !activation.satin_activates());
}

#[test]
fn chain_config_block_zero_skips_the_eip_calls() {
    let factory = factory(1, 0, 0, &[]);
    // Code at both system-call addresses that would fail the block if it were called.
    let mut failing = db();
    for target in [HISTORY_STORAGE, BEACON_ROOTS] {
        failing.insert_account_info(
            &target,
            AccountInfo::default()
                .with_code(Bytecode::new_legacy(Bytes::from_static(&[0x5f, 0x5f, 0xfd]))),
        );
    }
    let zero = |root: Option<B256>| BlockInput {
        header: BlockHeader {
            parent_hash: B256::ZERO,
            parent_beacon_block_root: root,
            ..header(0, 0)
        },
        parent: ParentContext::BlockZero,
    };
    let block =
        start(&factory, &zero(Some(B256::ZERO)), failing.clone()).expect("block zero starts");
    assert!(block.activation().block_zero());
    assert!(!block.activation().satin_activates() && !block.activation().deposits_only());
    let states = block.pre_block_states();
    assert_eq!(states.len(), 2, "the registry's and the L1 block info's read-only states");
    for state in states {
        assert!(state.account_changes_for(&HISTORY_STORAGE).is_none());
        assert!(state.account_changes_for(&BEACON_ROOTS).is_none());
    }
    // Block zero's parent beacon block root must be zero, and present.
    let error = start(&factory, &zero(Some(B256::repeat_byte(0x11))), db()).err().unwrap();
    assert_eq!(error.kind(), &BlockErrorKind::GenesisParentBeaconRoot(B256::repeat_byte(0x11)));
    assert_eq!(error.disposition(), Disposition::InvalidBlock);
    let error = start(&factory, &zero(None), db()).err().unwrap();
    assert_eq!(error.kind(), &BlockErrorKind::MissingParentBeaconRoot);
    assert_eq!(error.disposition(), Disposition::InvalidBlock);
    // The control: block one makes the EIP-2935 call, which fails here and invalidates the block.
    let error = start(&factory, &child(1, 1, 0), failing).err().expect("the call fails");
    assert!(
        matches!(error.kind(), BlockErrorKind::SystemCallFailed { target, .. } if *target == HISTORY_STORAGE),
        "{error:?}"
    );
    assert_eq!(error.phase(), Phase::PreBlockCall);
    assert_eq!(error.disposition(), Disposition::InvalidBlock);
}

#[test]
fn chain_config_refuses_a_block_the_legacy_engine_executes() {
    let factory = factory(1, 0, 100, &[]);
    let error = input_error(start(&factory, &child(5, 99, 98), db()));
    assert_eq!(error, InputError::NotSatin { timestamp: 99, satin_time: Some(100) });
    let env =
        BlockEnv::<MegaTypes> { timestamp: U256::from(99), ..BlockEnv::<MegaTypes>::default() };
    let error =
        factory.evm(env, Db::new(db())).map(drop).expect_err("no Satin EVM before satinTime");
    assert_eq!(
        error.kind(),
        &BlockErrorKind::Input(InputError::NotSatin { timestamp: 99, satin_time: Some(100) })
    );
    let unscheduled = SatinFactory::new(
        ChainConfig::from_config_json(r#"{"chainId": 1}"#, 0).expect("a chain without Satin"),
    );
    let error = input_error(start(&unscheduled, &child(5, 99, 98), db()));
    assert_eq!(error, InputError::NotSatin { timestamp: 99, satin_time: None });
}

#[test]
fn chain_config_refuses_a_block_inconsistent_with_its_context() {
    let factory = factory(1, 50, 0, &[]);
    let parent = |number, hash, timestamp| ParentContext::Parent { number, hash, timestamp };
    let cases = [
        (
            BlockInput { header: header(1, 50), parent: ParentContext::BlockZero },
            InputError::Number { number: 1, parent: None },
        ),
        (
            BlockInput { header: header(0, 60), parent: ParentContext::BlockZero },
            InputError::GenesisTimestamp { timestamp: 60, genesis_timestamp: 50 },
        ),
        (
            BlockInput { header: header(7, 60), parent: parent(5, B256::repeat_byte(0x01), 55) },
            InputError::Number { number: 7, parent: Some(5) },
        ),
        (
            BlockInput {
                header: header(0, 60),
                parent: parent(u64::MAX, B256::repeat_byte(0x01), 55),
            },
            InputError::Number { number: 0, parent: Some(u64::MAX) },
        ),
        (
            BlockInput { header: header(6, 60), parent: parent(5, B256::repeat_byte(0x02), 55) },
            InputError::ParentHash {
                parent_hash: B256::repeat_byte(0x01),
                parent: B256::repeat_byte(0x02),
            },
        ),
        (
            BlockInput { header: header(6, 60), parent: parent(5, B256::repeat_byte(0x01), 61) },
            InputError::ParentAfterBlock { parent_timestamp: 61, timestamp: 60 },
        ),
        (
            BlockInput { header: header(6, 60), parent: parent(5, B256::repeat_byte(0x01), 49) },
            InputError::GenesisTimestamp { timestamp: 49, genesis_timestamp: 50 },
        ),
        (
            BlockInput { header: header(1, 60), parent: parent(0, B256::repeat_byte(0x01), 51) },
            InputError::GenesisTimestamp { timestamp: 51, genesis_timestamp: 50 },
        ),
        (
            BlockInput {
                header: BlockHeader { excess_blob_gas: 1, ..header(6, 60) },
                parent: parent(5, B256::repeat_byte(0x01), 55),
            },
            InputError::ExcessBlobGas { excess_blob_gas: 1 },
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(input_error(start(&factory, &input, db())), expected, "{input:?}");
    }
    // The controls: the same block on a consistent parent, and block one on the genesis block.
    start(&factory, &child(6, 60, 55), db()).expect("a consistent block");
    start(&factory, &child(1, 60, 50), db()).expect("block one on genesis");
    // Equal timestamps of parent and block are not refused.
    start(&factory, &child(6, 60, 60), db()).expect("a parent at the block's timestamp");
}

#[test]
fn chain_config_fails_explicitly_under_a_fork_without_rules() {
    let factory = factory(1, 0, 0, &[(Fork::Lagoon, 200)]);
    start(&factory, &child(5, 199, 198), db()).expect("before the fork");
    let error = input_error(start(&factory, &child(6, 200, 199), db()));
    assert_eq!(error, InputError::UnsupportedFork { fork: Fork::Lagoon, timestamp: 200 });
    let env =
        BlockEnv::<MegaTypes> { timestamp: U256::from(250), ..BlockEnv::<MegaTypes>::default() };
    assert!(factory.evm(env, Db::new(db())).is_err(), "no single-transaction EVM either");
}

#[test]
fn chain_config_factory_installs_the_configured_chain_id() {
    let factory = factory(4326, 0, 0, &[]);
    let mut block = start(&factory, &child(5, 10, 9), db()).expect("the block starts");
    assert_eq!(block.evm().version().chain_id, 4326);
    let refused = block.execute_candidate(&legacy(1), &[]).expect_err("signed for chain 1");
    let CandidateError::Refused(Refusal::Transaction(cause)) = &refused else {
        panic!("{refused:?}")
    };
    assert!(
        matches!(cause, evm2::registry::HandlerError::InvalidChainId { expected: 4326, got: 1 }),
        "{cause:?}"
    );
    let out = block.execute_candidate(&legacy(4326), &[]).expect("signed for the chain");
    assert!(out.result.status, "{:?}", out.result);
    let env =
        BlockEnv::<MegaTypes> { timestamp: U256::from(10), ..BlockEnv::<MegaTypes>::default() };
    assert_eq!(factory.evm(env, Db::new(db())).unwrap().version().chain_id, 4326);
    assert_eq!(factory.protocol().chain_id, 4326);
}
