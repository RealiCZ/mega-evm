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
/// The EIP-7997 factory's runtime code, which the block in which Satin activates requires.
const CREATE2_FACTORY_CODE: &[u8] = &alloy_primitives::hex!(
    "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe03601600081602082378035828234f58015156039578182fd5b8082525050506014600cf3"
);

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

/// A funded sender, a target that stops, and the EIP-7997 factory.
fn db() -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &mega_evm2::system_contracts::CREATE2_FACTORY,
        AccountInfo::default()
            .with_code(Bytecode::new_legacy(Bytes::from_static(CREATE2_FACTORY_CODE))),
    );
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
    factory.start_block(input, Db::new(db), services(input))
}

/// The SALT snapshot of the state after `parent_hash`: the minimum bucket everywhere (a test's
/// policy).
fn minimum_salt(parent_hash: B256) -> std::sync::Arc<dyn mega_evm2::SaltSnapshot> {
    let id = mega_evm2::SnapshotId { parent_hash, state_root: B256::ZERO };
    std::sync::Arc::new(mega_evm2::SaltCapacities::test_minimum(id))
}

/// A single-transaction EVM's SALT snapshot.
fn salt() -> std::sync::Arc<dyn mega_evm2::SaltSnapshot> {
    minimum_salt(B256::ZERO)
}

/// The services of `input`'s block: its parent's snapshot.
fn services(input: &BlockInput) -> BlockServices<'static> {
    BlockServices::new(minimum_salt(input.header.parent_hash))
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
fn input_error<T>(result: Result<T, BlockError>) -> InputError {
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
    assert!(out.result().status, "{:?}", out.result());
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
    // The five deployments, the due-change read, the registry's deployment and the resolution.
    assert_eq!(states.len(), 8, "every step but the EIP calls");
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
    let error = factory
        .evm(env, Db::new(db()), salt(), Default::default())
        .map(drop)
        .expect_err("no Satin EVM before satinTime");
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
    assert!(
        factory.evm(env, Db::new(db()), salt(), Default::default()).is_err(),
        "no single-transaction EVM either"
    );
    // A timestamp past the largest `u64` is refused, not saturated into a Satin timestamp; the
    // largest `u64` itself is one.
    let satin = self::factory(1, 0, 0, &[]);
    let largest = BlockEnv::<MegaTypes> {
        timestamp: U256::from(u64::MAX),
        ..BlockEnv::<MegaTypes>::default()
    };
    satin
        .evm(largest, Db::new(db()), salt(), Default::default())
        .map(drop)
        .expect("the largest timestamp");
    let past = BlockEnv::<MegaTypes> { timestamp: largest.timestamp + U256::from(1), ..largest };
    assert_eq!(
        input_error(satin.evm(past, Db::new(db()), salt(), Default::default())),
        InputError::TimestampTooLarge
    );
    let simulation = mega_evm2::factory::Simulation::default();
    assert_eq!(
        input_error(satin.simulated_evm(
            past,
            Db::new(db()),
            salt(),
            Default::default(),
            &simulation
        )),
        InputError::TimestampTooLarge
    );
}

/// A candidate is committed only by a session of its own block that runs under the same rules,
/// and the deposit-only rule holds at the commit as at the execution.
#[test]
fn chain_config_a_commit_holds_the_candidate_to_the_sessions_rules() {
    use mega_evm2::factory::Simulation;
    let factory = factory(1, 0, 0, &[]);
    let input = child(5, 100, 99);
    let simulation = Simulation { skip_nonce_check: true, ..Simulation::default() };
    let mut simulated = factory
        .start_simulated_block(&input, Db::new(db()), services(&input), &simulation)
        .expect("the simulated block starts");
    let out = simulated.execute_candidate(&legacy(1), &[]).expect("runs");
    assert!(out.is_simulated());
    let mut consensus = start(&factory, &input, db()).expect("the block starts");
    let error = consensus.commit_candidate(out).expect_err("a simulation's outcome");
    assert_eq!(error, CandidateError::OtherRules { simulated: true });
    assert_eq!(error.disposition(), Disposition::Retry);
    let out = consensus.execute_candidate(&legacy(1), &[]).expect("runs");
    let error = simulated.commit_candidate(out).expect_err("a consensus outcome");
    assert_eq!(error, CandidateError::OtherRules { simulated: false });
    assert!(consensus.receipts().is_empty() && simulated.receipts().is_empty());
    // The same block under a configuration in which Satin activates in it admits only deposits:
    // a user transaction another session ran is refused at the commit.
    let crossing = self::factory(1, 0, 100, &[]);
    let mut activation = start(&crossing, &input, db()).expect("the activation block starts");
    assert!(activation.activation().deposits_only());
    let out = consensus.execute_candidate(&legacy(1), &[]).expect("runs where no fork activates");
    let error = activation.commit_candidate(out).expect_err("a user transaction");
    assert_eq!(error, CandidateError::Refused(Refusal::OnlyDeposits));
    assert!(activation.fault().is_none());
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
    assert!(out.result().status, "{:?}", out.result());
    let env =
        BlockEnv::<MegaTypes> { timestamp: U256::from(10), ..BlockEnv::<MegaTypes>::default() };
    assert_eq!(
        factory.evm(env, Db::new(db()), salt(), Default::default()).unwrap().version().chain_id,
        4326
    );
    assert_eq!(factory.protocol().chain_id, 4326);
}
