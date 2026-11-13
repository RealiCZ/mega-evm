//! A block's pre-block sequence, step by step (`SATIN-PBLK-003`): the EIP-2935 and EIP-4788
//! calls on a fixed budget (`SATIN-PBLK-001`, `SATIN-PBLK-002`), the EIP-7997 factory's check in
//! the block in which Satin activates and no other (`SATIN-ACT-004`, `SATIN-ACT-005`), the five
//! flat system contracts' deployment (`SATIN-SYSC-003`), the registry's due-change read and its
//! `applyPendingChanges()` call (`SATIN-PBLK-004`), the registry's deployment (`SATIN-REG-008`)
//! and the resolution of the live system address, once per block, which every transaction is
//! classified against with no read of the registry of its own (`SATIN-REG-002`, `SATIN-REG-004`);
//! block zero and a chain on which Satin is active from genesis (`SATIN-ACT-012`); and the
//! routing of each step's failures (`SATIN-PBLK-006`).

mod common;

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, KECCAK256_EMPTY, TxKind, U256, hex, keccak256};
use evm2::{
    DatabaseError,
    bytecode::Bytecode,
    evm::{AccountInfo, Database, Db, InMemoryDB},
    inspector::Inspector,
    interpreter::{Interpreter, Message, MessageResult, Word},
};
use mega_evm2::{
    MegaTypes, ORACLE, SEQUENCER_REGISTRY,
    block::{BEACON_ROOTS, BlockSession, HISTORY_STORAGE},
    chain::{BootstrapInput, ChainConfig, ChainConfigInput, Fork, ForkSchedule},
    error::{BlockError, BlockErrorKind, Disposition, Phase, SystemAddressFault},
    factory::{BlockHeader, BlockInput, BlockServices, ParentContext, SatinFactory},
    system_contracts::{
        CREATE2_FACTORY, FLAT_SYSTEM_CONTRACTS, HIGH_PRECISION_TIMESTAMP, MEGA_SYSTEM_ADDRESS,
        SEQUENCER_REGISTRY_CODE, SEQUENCER_REGISTRY_CODE_HASH, address_word,
    },
};
use op_evm2::OpTxEnvelope;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

const CHAIN_ID: u64 = 1;
const SENDER: Address = Address::with_last_byte(0x50);
/// The configuration's bootstrap values.
const SEQUENCER: Address = Address::repeat_byte(0x5e);
const ADMIN: Address = Address::repeat_byte(0xad);
const DELAY: u64 = 7_200;
/// A live system address a registry names, and the one a due change installs.
const LIVE: Address = Address::with_last_byte(0x5a);
const NEXT: Address = Address::with_last_byte(0x77);
/// The block number of the tests' blocks, at timestamp 100 on a parent at 99.
const NUMBER: u64 = 5;
/// The EIP-7997 factory's runtime code.
const CREATE2_FACTORY_CODE: &[u8] = &hex!(
    "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe03601600081602082378035828234f58015156039578182fd5b8082525050506014600cf3"
);
/// The index of a step's state in a block of the tests' chain with no change due: the two EIP
/// calls, the five deployments, the due-change read, the registry's deployment and the
/// resolution. No step reads the L1 block contract.
const DUE_READ: usize = 7;
const REGISTRY_DEPLOYMENT: usize = 8;
const RESOLUTION: usize = 9;

/// A chain on which Satin is scheduled at `satin`, genesis at `genesis`, every fork below it from
/// timestamp zero, and the tests' bootstrap values.
fn chain(satin: u64, genesis: u64) -> SatinFactory {
    let schedule = ForkSchedule::satin_from(0).with(Fork::Satin, Some(satin));
    let bootstrap = BootstrapInput {
        initial_sequencer: Some(SEQUENCER),
        initial_admin: Some(ADMIN),
        min_rotation_delay: Some(DELAY),
    };
    let input =
        ChainConfigInput { chain_id: CHAIN_ID, genesis_timestamp: genesis, schedule, bootstrap };
    SatinFactory::new(ChainConfig::new(input).expect("a valid configuration"))
}

/// Satin from genesis at timestamp zero: no block is the one in which Satin activates.
fn genesis_chain() -> SatinFactory {
    chain(0, 0)
}

/// Block `number` at `timestamp` on its parent at `timestamp - 1`, with a parent beacon root.
fn input(number: u64, timestamp: u64) -> BlockInput {
    common::block_input(number, timestamp, 30_000_000, Some(B256::repeat_byte(0x11)))
}

/// The tests' block, [`NUMBER`] at timestamp 100.
fn block_input() -> BlockInput {
    input(NUMBER, 100)
}

fn start(
    factory: &SatinFactory,
    input: &BlockInput,
    db: InMemoryDB,
) -> Result<BlockSession<'static>, BlockError> {
    factory.start_block(input, Db::new(db), common::services(input.header.parent_hash))
}

/// A world with a funded sender and nothing else.
fn world() -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
    );
    db
}

fn code(bytes: &[u8]) -> Bytecode {
    Bytecode::new_legacy(Bytes::copy_from_slice(bytes))
}

/// The registry's code of `version`, its account holding it.
fn registry_v1() -> Bytecode {
    let v1 = hex::decode(include_str!("fixtures/SequencerRegistry-1.0.0.hex").trim()).unwrap();
    Bytecode::new_legacy(v1.into())
}

fn registry_v2() -> Bytecode {
    Bytecode::new_legacy(SEQUENCER_REGISTRY_CODE)
}

/// `db` with a registry of `code` naming `LIVE` the system address and the bootstrap's other
/// roles, as a registry deployed at block 1 holds them.
fn with_registry(mut db: InMemoryDB, code: Bytecode) -> InMemoryDB {
    db.insert_account_info(&SEQUENCER_REGISTRY, AccountInfo::default().with_code(code));
    for (slot, value) in [
        (0, address_word(LIVE)),
        (1, address_word(SEQUENCER)),
        (2, address_word(ADMIN)),
        (4, address_word(LIVE)),
        (5, address_word(SEQUENCER)),
        (6, U256::from(1)),
    ] {
        db.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(slot), &value);
    }
    db
}

fn set_slot(db: &mut InMemoryDB, slot: u64, value: U256) {
    db.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(slot), &value);
}

/// The account the block's change set holds for `address`, when a step changed it.
fn changed_account(block: &BlockSession<'_>, address: Address) -> Option<Option<AccountInfo>> {
    block.changes().accounts().find(|(a, _)| *a == address).map(|(_, t)| t.current.clone())
}

/// The flat slots (below 2^64) of `address` the block's change set changed, sorted, with their
/// values; a history array's entries are left out.
fn changed_slots(block: &BlockSession<'_>, address: Address) -> Vec<(u64, U256)> {
    let mut slots: Vec<_> = block
        .changes()
        .storage()
        .filter(|(k, t)| k.address() == address && t.original != t.current)
        .filter_map(|(k, t)| u64::try_from(k.key()).ok().map(|key| (key, t.current)))
        .collect();
    slots.sort();
    slots
}

fn wiped(block: &BlockSession<'_>, address: Address) -> bool {
    block.changes().storage_wipes().any(|a| a == address)
}

fn legacy(from: Address, nonce: u64, to: Address, input: Bytes) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy {
        chain_id: Some(CHAIN_ID),
        nonce,
        gas_limit: 1_000_000,
        to: TxKind::Call(to),
        input,
        ..Default::default()
    };
    Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), from)
}

/// The calldata of the view `signature` with one word argument when given.
fn call_data(signature: &str, arg: Option<u64>) -> Bytes {
    let mut data = keccak256(signature)[..4].to_vec();
    if let Some(arg) = arg {
        data.extend_from_slice(&U256::from(arg).to_be_bytes::<32>());
    }
    data.into()
}

/// Runs a call from [`SENDER`] (its `nonce`th transaction) to `to` in `block`, commits it, and
/// returns its output as a word.
fn view(block: &mut BlockSession<'_>, nonce: u64, to: Address, data: Bytes) -> U256 {
    let out = block.execute_candidate(&legacy(SENDER, nonce, to, data), &[]).expect("runs");
    assert!(out.result().status, "{:?}", out.result());
    let word = U256::from_be_slice(&out.result().output);
    block.commit_candidate(out).expect("committed");
    word
}

/// The block error's kind, phase and disposition.
fn failure(result: Result<BlockSession<'static>, BlockError>) -> BlockError {
    result.err().expect("the block fails")
}

/// A database that counts the reads of one account, and can fail one read.
#[derive(Debug)]
struct Watched {
    inner: InMemoryDB,
    watch: Address,
    armed: Rc<Cell<bool>>,
    reads: Rc<Cell<usize>>,
    /// The account (`None`) or slot (`Some`) of `watch` to fail, and whether fatally.
    fail: Option<(Option<u64>, bool)>,
}

impl Watched {
    fn new(inner: InMemoryDB, watch: Address) -> Self {
        Self {
            inner,
            watch,
            armed: Rc::new(Cell::new(true)),
            reads: Rc::new(Cell::new(0)),
            fail: None,
        }
    }

    fn read(&self, slot: Option<u64>) -> Result<(), DatabaseError> {
        if !self.armed.get() {
            return Ok(());
        }
        self.reads.set(self.reads.get() + 1);
        match self.fail {
            Some((failing, fatal)) if failing == slot => {
                Err(DatabaseError::new(std::io::Error::other("unavailable"), fatal))
            }
            _ => Ok(()),
        }
    }
}

impl Database for Watched {
    type Error = DatabaseError;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        if *address == self.watch {
            self.read(None)?;
        }
        self.inner.get_account(address)
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        self.inner.get_code_by_hash(code_hash)
    }

    fn get_storage(&mut self, address: &Address, key: &Word) -> Result<Word, Self::Error> {
        if *address == self.watch {
            self.read(Some(key.to::<u64>()))?;
        }
        self.inner.get_storage(address, key)
    }

    fn get_block_hash(&mut self, number: &Word) -> Result<B256, Self::Error> {
        self.inner.get_block_hash(number)
    }
}

/// The block's start on `db` failing the read `fail` of `watch`, fatally and not: an internal
/// error in `phase` either way, the database error kept with its fatality.
fn assert_read_is_internal(
    factory: &SatinFactory,
    input: &BlockInput,
    db: &InMemoryDB,
    watch: Address,
    fail: Option<u64>,
    phase: Phase,
) {
    for fatal in [true, false] {
        let mut backend = Watched::new(db.clone(), watch);
        backend.fail = Some((fail, fatal));
        let error = factory
            .start_block(input, Db::new(backend), common::services(input.header.parent_hash))
            .err()
            .unwrap_or_else(|| panic!("{watch} {fail:?}: the block starts"));
        assert_eq!(error.phase(), phase, "{error:?}");
        assert_eq!(error.disposition(), Disposition::Internal, "{error:?}");
        assert_eq!(error.database_error().map(|e| e.is_fatal()), Some(fatal), "{error:?}");
        assert_eq!(error.context().address, Some(watch));
        assert_eq!(error.context().slot, fail.map(U256::from));
    }
}

/// Every system contract absent: each of the five is installed, holding its code with nonce and
/// balance zero, each in a state of its own and in order, after the two EIP calls; a call then
/// runs the installed code.
#[test]
fn lifecycle_absent_system_contracts_are_installed_each_as_a_state_of_its_own() {
    let mut block = start(&genesis_chain(), &block_input(), world()).expect("the block starts");
    let states = block.pre_block_states().to_vec();
    assert_eq!(states.len(), 10);
    for (i, contract) in FLAT_SYSTEM_CONTRACTS.iter().enumerate() {
        let state = &states[2 + i];
        assert_eq!(state.account_changes().count(), 1, "{}: a state of its own", contract.name);
        let deployed = state.account_changes_for(&contract.address).expect(contract.name);
        assert!(deployed.original.is_none(), "{}", contract.name);
        let installed = deployed.current.expect(contract.name);
        assert_eq!(installed.code_hash, contract.code_hash, "{}", contract.name);
        assert_eq!((installed.nonce, installed.balance), (0, U256::ZERO), "{}", contract.name);
        assert!(deployed.storage.slots.is_empty(), "{}: no storage written", contract.name);
        let changed = changed_account(&block, contract.address).flatten().expect(contract.name);
        assert_eq!(changed.code_hash, contract.code_hash, "{}", contract.name);
    }
    // The installed Oracle answers `getSlot`, which an absent account would not.
    let answer = block
        .execute_candidate(&legacy(SENDER, 0, ORACLE, call_data("getSlot(uint256)", Some(1))), &[])
        .expect("runs");
    assert_eq!(answer.result().output.len(), 32);
    // A deployment is charged nothing and counts toward none of the block's counters.
    assert_eq!(block.limiter().block_gas_used, 0);
    assert_eq!(block.limiter().usage, [0; 3]);
    assert_eq!(block.limiter().gas, [0; 4]);
}

/// An account holding other code at a system contract's address, or none: its code is replaced,
/// its nonce, balance and storage kept.
#[test]
fn lifecycle_wrong_code_is_replaced_keeping_nonce_balance_and_storage() {
    let mut db = world();
    for (i, contract) in FLAT_SYSTEM_CONTRACTS.iter().enumerate() {
        let info = AccountInfo::default().with_nonce(7).with_balance(U256::from(100 + i));
        // The High-Precision Timestamp's address holds an account without code.
        let info = if contract.address == HIGH_PRECISION_TIMESTAMP {
            info
        } else {
            info.with_code(code(&[0x60, i as u8, 0x00]))
        };
        db.insert_account_info(&contract.address, info);
        db.insert_account_storage(&contract.address, &U256::from(1), &U256::from(5));
    }
    let mut block = start(&genesis_chain(), &block_input(), db).expect("the block starts");
    for (i, contract) in FLAT_SYSTEM_CONTRACTS.iter().enumerate() {
        let state = &block.pre_block_states()[2 + i];
        let replaced = state.account_changes_for(&contract.address).expect(contract.name);
        let original = replaced.original.expect(contract.name);
        assert_ne!(original.code_hash, contract.code_hash, "{}", contract.name);
        let current = changed_account(&block, contract.address).flatten().expect(contract.name);
        assert_eq!(current.code_hash, contract.code_hash, "{}", contract.name);
        assert_eq!((current.nonce, current.balance), (7, U256::from(100 + i)), "{}", contract.name);
        assert!(!wiped(&block, contract.address), "{}: storage kept", contract.name);
        assert!(changed_slots(&block, contract.address).is_empty(), "{}", contract.name);
    }
    // The Oracle's slot 1 survives its new code, which reads it.
    let slot = view(&mut block, 0, ORACLE, call_data("getSlot(uint256)", Some(1)));
    assert_eq!(slot, U256::from(5));
}

/// Every system contract already holding its code: each step reads it and writes nothing, a
/// read-only entry of its own state; the registry's version 2.0.0 likewise.
#[test]
fn lifecycle_correct_system_contracts_and_registry_v2_are_left_as_they_are() {
    let mut db = with_registry(world(), registry_v2());
    set_slot(&mut db, 13, U256::from(99));
    for contract in &FLAT_SYSTEM_CONTRACTS {
        db.insert_account_info(
            &contract.address,
            AccountInfo::default()
                .with_nonce(1)
                .with_code(Bytecode::new_legacy(contract.code.clone())),
        );
        db.insert_account_storage(&contract.address, &U256::from(1), &U256::from(5));
    }
    let block = start(&genesis_chain(), &block_input(), db).expect("the block starts");
    let states = block.pre_block_states();
    for (i, contract) in FLAT_SYSTEM_CONTRACTS.iter().enumerate() {
        let read = states[2 + i].account_changes_for(&contract.address).expect(contract.name);
        assert!(!read.is_changed(), "{}: read-only", contract.name);
        assert_eq!(read.current.map(|i| i.code_hash), Some(contract.code_hash));
        assert!(changed_account(&block, contract.address).is_none(), "{}", contract.name);
    }
    let read = states[REGISTRY_DEPLOYMENT].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
    assert!(!read.is_changed() && read.storage.slots.is_empty(), "the registry: read-only");
    assert_eq!(read.current.map(|i| i.code_hash), Some(SEQUENCER_REGISTRY_CODE_HASH));
    assert!(changed_account(&block, SEQUENCER_REGISTRY).is_none());
    assert!(changed_slots(&block, SEQUENCER_REGISTRY).is_empty(), "its delay stays 99");
    assert!(block.changes().is_empty(), "nothing written: {:?}", block.changes());
    let resolved = states[RESOLUTION].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
    assert!(!resolved.is_changed());
    assert_eq!(resolved.storage.slots[&U256::ZERO].value.current, address_word(LIVE));
    assert_eq!(block.system_address(), Some(LIVE));
}

/// A registry of version 1.0.0 is migrated in place: its code replaced by version 2.0.0, slot 13
/// written with the configured delay and nothing else, every other slot, its nonce and its
/// balance kept.
#[test]
fn lifecycle_registry_v1_is_migrated_writing_slot_13_alone() {
    let mut db = with_registry(world(), registry_v1());
    db.insert_account_info(
        &SEQUENCER_REGISTRY,
        AccountInfo::default().with_nonce(3).with_balance(U256::from(4)).with_code(registry_v1()),
    );
    let mut block = start(&genesis_chain(), &block_input(), db).expect("the block starts");
    let state = &block.pre_block_states()[REGISTRY_DEPLOYMENT];
    let migrated = state.account_changes_for(&SEQUENCER_REGISTRY).unwrap();
    let slots: Vec<_> = migrated
        .storage
        .slots
        .iter()
        .map(|(k, s)| (*k, s.value.original, s.value.current))
        .collect();
    assert_eq!(slots, [(U256::from(13), U256::ZERO, U256::from(DELAY))]);
    let current = changed_account(&block, SEQUENCER_REGISTRY).flatten().unwrap();
    assert_eq!(current.code_hash, SEQUENCER_REGISTRY_CODE_HASH);
    assert_eq!((current.nonce, current.balance), (3, U256::from(4)));
    assert_eq!(changed_slots(&block, SEQUENCER_REGISTRY), [(13, U256::from(DELAY))]);
    assert!(!wiped(&block, SEQUENCER_REGISTRY), "the roles and histories are kept");
    assert_eq!(block.system_address(), Some(LIVE));
    // Version 2.0.0's code reads the kept roles and the new slot.
    assert_eq!(
        view(&mut block, 0, SEQUENCER_REGISTRY, call_data("minRotationDelay()", None)),
        U256::from(DELAY)
    );
    assert_eq!(
        view(&mut block, 1, SEQUENCER_REGISTRY, call_data("admin()", None)),
        address_word(ADMIN)
    );
}

/// An absent registry is deployed new and seeded from the configuration's bootstrap values: slots
/// 0 and 4 `MEGA_SYSTEM_ADDRESS`, 1 and 5 the initial sequencer, 2 the admin, 6 the block's
/// number, 13 the delay, and nothing else; the block's live system address is then
/// `MEGA_SYSTEM_ADDRESS`. The due-change read before it recorded the account absent.
#[test]
fn lifecycle_absent_registry_is_seeded_from_the_bootstrap_values() {
    let block = start(&genesis_chain(), &block_input(), world()).expect("the block starts");
    let due = block.pre_block_states()[DUE_READ].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
    assert!(due.original.is_none() && due.current.is_none() && due.storage.slots.is_empty());
    let current = changed_account(&block, SEQUENCER_REGISTRY).flatten().unwrap();
    assert_eq!(current.code_hash, SEQUENCER_REGISTRY_CODE_HASH);
    assert_eq!((current.nonce, current.balance), (0, U256::ZERO));
    let system = address_word(MEGA_SYSTEM_ADDRESS);
    assert_eq!(
        changed_slots(&block, SEQUENCER_REGISTRY),
        [
            (0, system),
            (1, address_word(SEQUENCER)),
            (2, address_word(ADMIN)),
            (4, system),
            (5, address_word(SEQUENCER)),
            (6, U256::from(NUMBER)),
            (13, U256::from(DELAY)),
        ]
    );
    assert!(!wiped(&block, SEQUENCER_REGISTRY), "an absent account holds no storage to clear");
    assert_eq!(block.system_address(), Some(MEGA_SYSTEM_ADDRESS));
}

/// An account without code at the registry's address receives version 2.0.0 as a new contract: its
/// nonce and balance kept, its earlier storage cleared, then seeded.
#[test]
fn lifecycle_registry_without_code_is_seeded_over_its_cleared_storage() {
    let mut db = world();
    db.insert_account_info(
        &SEQUENCER_REGISTRY,
        AccountInfo::default().with_nonce(2).with_balance(U256::from(3)),
    );
    // `_pendingAdmin` and a slot no seed writes.
    set_slot(&mut db, 3, address_word(Address::repeat_byte(0x99)));
    set_slot(&mut db, 20, U256::from(21));
    let mut block = start(&genesis_chain(), &block_input(), db).expect("the block starts");
    let current = changed_account(&block, SEQUENCER_REGISTRY).flatten().unwrap();
    assert_eq!(current.code_hash, SEQUENCER_REGISTRY_CODE_HASH);
    assert_eq!((current.nonce, current.balance), (2, U256::from(3)));
    assert!(wiped(&block, SEQUENCER_REGISTRY), "the earlier storage is cleared");
    let seeded: Vec<u64> =
        changed_slots(&block, SEQUENCER_REGISTRY).iter().map(|(k, _)| *k).collect();
    assert_eq!(seeded, [0, 1, 2, 4, 5, 6, 13]);
    assert_eq!(block.system_address(), Some(MEGA_SYSTEM_ADDRESS));
    // The cleared `_pendingAdmin` reads zero through the new code.
    assert_eq!(
        view(&mut block, 0, SEQUENCER_REGISTRY, call_data("pendingAdmin()", None)),
        U256::ZERO
    );
}

/// Any other code at the registry's address is never overwritten: the block is invalid.
#[test]
fn lifecycle_foreign_registry_code_invalidates_the_block() {
    let mut db = world();
    let foreign = [0x60, 0x01, 0x00];
    db.insert_account_info(&SEQUENCER_REGISTRY, AccountInfo::default().with_code(code(&foreign)));
    let error = failure(start(&genesis_chain(), &block_input(), db));
    assert_eq!(
        error.kind(),
        &BlockErrorKind::UnmigratableRegistry { code_hash: keccak256(foreign) }
    );
    assert_eq!(error.phase(), Phase::Deployment);
    assert_eq!(error.disposition(), Disposition::InvalidBlock);
    assert_eq!(error.context().address, Some(SEQUENCER_REGISTRY));
    assert!(error.database_error().is_none());
}

/// A registry whose slot 0 names no system address makes the block invalid when the block
/// resolves it; a zero sequencer does not, the live system address being slot 0's alone.
#[test]
fn lifecycle_a_zero_system_address_invalidates_the_block_and_a_zero_sequencer_does_not() {
    let mut db = with_registry(world(), registry_v2());
    set_slot(&mut db, 0, U256::ZERO);
    let error = failure(start(&genesis_chain(), &block_input(), db));
    assert_eq!(error.kind(), &BlockErrorKind::SystemAddress(SystemAddressFault::ZeroSystemAddress));
    assert_eq!(error.phase(), Phase::SystemAddressResolution);
    assert_eq!(error.disposition(), Disposition::InvalidBlock);
    assert_eq!(error.context().address, Some(SEQUENCER_REGISTRY));
    assert_eq!(error.context().slot, Some(U256::ZERO));
    let mut db = with_registry(world(), registry_v2());
    set_slot(&mut db, 1, U256::ZERO);
    let block = start(&genesis_chain(), &block_input(), db).expect("a zero sequencer");
    assert_eq!(block.system_address(), Some(LIVE));
}

/// The resolution itself refuses a registry that is absent or holds other code. The deployment
/// before it leaves neither (it installs an absent registry and refuses other code), so these are
/// reached with the deployments off, a comparison's rule.
#[cfg(feature = "compare")]
#[test]
fn lifecycle_resolution_refuses_an_absent_or_foreign_registry() {
    use mega_evm2::{ReferenceRule, factory::Simulation};
    let simulation = Simulation {
        reference_rules: vec![ReferenceRule::NoPreBlockDeployment],
        ..Simulation::default()
    };
    let start = |db: InMemoryDB| {
        genesis_chain().start_simulated_block(
            &block_input(),
            Db::new(db),
            common::services(B256::ZERO),
            &simulation,
        )
    };
    let error = failure(start(world()));
    assert_eq!(error.kind(), &BlockErrorKind::SystemAddress(SystemAddressFault::RegistryAbsent));
    let error = failure(start(with_registry(world(), registry_v1())));
    let v1 = keccak256(registry_v1().original_bytes());
    assert_eq!(error.kind(), &BlockErrorKind::SystemAddress(SystemAddressFault::RegistryCode(v1)));
    assert_eq!(error.phase(), Phase::SystemAddressResolution);
    assert_eq!(error.disposition(), Disposition::InvalidBlock);
    // Under the reference's per-transaction classification too, as the reference runs such a
    // block: the registry names no address and the block is not invalidated.
    let both = Simulation {
        reference_rules: vec![
            ReferenceRule::NoPreBlockDeployment,
            ReferenceRule::RegistryReadPerTransaction,
        ],
        ..Simulation::default()
    };
    for db in [world(), with_registry(world(), registry_v1())] {
        let block = genesis_chain()
            .start_simulated_block(&block_input(), Db::new(db), common::services(B256::ZERO), &both)
            .expect("the reference's rules invalidate nothing here");
        assert_eq!(block.system_address(), None);
        assert_eq!(block.evm().ext().system_address, None);
    }
}

/// The due-change read loads the registry's account and each role's pending address slot, and a
/// role's activation block slot only when its pending address is non-zero; it writes nothing.
#[test]
fn lifecycle_due_change_reads_an_activation_slot_only_for_a_pending_role() {
    for (pending, read) in [(7, [7, 8, 9]), (9, [7, 9, 10])] {
        let mut db = with_registry(world(), registry_v2());
        set_slot(&mut db, pending, address_word(NEXT));
        set_slot(&mut db, pending + 1, U256::from(NUMBER + 1));
        let mut block = start(&genesis_chain(), &block_input(), db).expect("the block starts");
        let due =
            block.pre_block_states()[DUE_READ].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
        assert!(!due.is_changed());
        let mut keys: Vec<u64> = due.storage.slots.keys().map(|k| k.to::<u64>()).collect();
        keys.sort();
        assert_eq!(keys, read, "pending in slot {pending}");
        assert_eq!(block.registry_due(), Ok(false), "activation at the next block");
    }
}

/// A change is due when its activation block, the slot's whole value, is at most the block's
/// number: an activation block above 2^64 is never due, whatever its low 64 bits, and at block
/// 2^64 − 1 an activation block of 2^64 is not due (Rex6's saturation found it due).
#[test]
fn lifecycle_due_change_compares_the_whole_activation_block() {
    let wide = (U256::from(1) << 128) | U256::from(NUMBER);
    let max = u64::MAX;
    for (number, activation, due) in [
        (NUMBER, U256::from(NUMBER), true),
        (NUMBER, U256::from(NUMBER + 1), false),
        (NUMBER, wide, false),
        (max, U256::from(1) << 64, false),
        (max, U256::from(max), true),
    ] {
        let mut db = with_registry(world(), registry_v2());
        set_slot(&mut db, 7, address_word(NEXT));
        set_slot(&mut db, 8, activation);
        let block = start(&genesis_chain(), &input(number, 100), db).expect("the block starts");
        let applied = block.pre_block_states().len() == 11;
        assert_eq!(applied, due, "block {number}, activation {activation}");
        let live = if due { NEXT } else { LIVE };
        assert_eq!(block.system_address(), Some(live), "block {number}, activation {activation}");
    }
}

/// A due change of both roles is applied by one `applyPendingChanges()` call, after the due-change
/// read and before the registry's deployment: on a registry of version 1.0.0, the call runs its
/// code, then the deployment migrates it, keeping what the call wrote.
#[test]
fn lifecycle_due_change_applies_both_roles_before_the_registry_is_deployed() {
    let mut db = with_registry(world(), registry_v1());
    let sequencer = Address::with_last_byte(0x88);
    set_slot(&mut db, 7, address_word(NEXT));
    set_slot(&mut db, 8, U256::from(NUMBER));
    set_slot(&mut db, 9, address_word(sequencer));
    set_slot(&mut db, 10, U256::from(NUMBER));
    let block = start(&genesis_chain(), &block_input(), db).expect("the block starts");
    let states = block.pre_block_states();
    assert_eq!(states.len(), 11);
    let call = states[DUE_READ + 1].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
    assert_eq!(call.current.map(|i| i.code_hash), call.original.map(|i| i.code_hash), "1.0.0 ran");
    let migrated = states[DUE_READ + 2].account_changes_for(&SEQUENCER_REGISTRY).unwrap();
    assert_eq!(migrated.current.map(|i| i.code_hash), Some(SEQUENCER_REGISTRY_CODE_HASH));
    let slots = changed_slots(&block, SEQUENCER_REGISTRY);
    for (slot, value) in
        [(0, address_word(NEXT)), (1, address_word(sequencer)), (13, U256::from(DELAY))]
    {
        assert!(slots.contains(&(slot, value)), "slot {slot}: {slots:?}");
    }
    for cleared in [7, 8, 9, 10] {
        assert!(slots.contains(&(cleared, U256::ZERO)), "slot {cleared} cleared: {slots:?}");
    }
    assert_eq!(block.system_address(), Some(NEXT));
}

/// Every transaction of a block is classified against the address resolved after the role change:
/// the retired address's legacy call to the Oracle is an ordinary transaction, the new one's a Mega
/// System Transaction.
#[test]
fn lifecycle_transactions_are_classified_against_the_address_resolved_last() {
    let mut db = with_registry(world(), registry_v2());
    set_slot(&mut db, 7, address_word(NEXT));
    set_slot(&mut db, 8, U256::from(NUMBER));
    for funded in [LIVE, NEXT] {
        db.insert_account_info(&funded, AccountInfo::default().with_balance(U256::from(1) << 64));
    }
    let mut block = start(&genesis_chain(), &block_input(), db).expect("the block starts");
    assert_eq!(block.system_address(), Some(NEXT));
    let data = call_data("getSlot(uint256)", Some(1));
    let retired = block.execute_candidate(&legacy(LIVE, 0, ORACLE, data.clone()), &[]).unwrap();
    assert!(!retired.is_system_transaction(), "the retired address sends a user's transaction");
    let live = block.execute_candidate(&legacy(NEXT, 0, ORACLE, data), &[]).unwrap();
    assert!(live.is_system_transaction(), "the resolved address sends a Mega System Transaction");
    assert!(live.result().status, "{:?}", live.result());
}

/// A probe that stores in its slot 0 what `EXTCODEHASH` of the registry costs (between two `GAS`).
fn probe() -> Bytecode {
    let mut c = vec![0x5a, 0x73];
    c.extend_from_slice(SEQUENCER_REGISTRY.as_slice());
    c.extend_from_slice(&[0x3f, 0x50, 0x5a, 0x90, 0x03, 0x5f, 0x55, 0x00]);
    code(&c)
}

/// No transaction of a block reads the registry, or finds its account or slot 0 warm, for its
/// classification (`SATIN-REG-002`): the block resolved the address once. A deposit's, a Mega
/// System Transaction's and a user transaction's read sets hold no registry state of their
/// classification, and a transaction's first access to the registry's account is cold, as in a
/// single transaction's EVM, which resolved its address once when it was made.
#[test]
fn lifecycle_classification_reads_no_registry_state_once_per_block() {
    const PROBE: Address = Address::with_last_byte(0xb0);
    let mut db = with_registry(world(), registry_v2());
    db.insert_account_info(&LIVE, AccountInfo::default().with_balance(U256::from(1) << 64));
    db.insert_account_info(&PROBE, AccountInfo::default().with_nonce(1).with_code(probe()));
    let mut block = start(&genesis_chain(), &block_input(), db.clone()).expect("the block starts");
    let deposit = Recovered::new_unchecked(
        OpTxEnvelope::deposit(op_evm2::TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(Address::with_last_byte(0xc0)),
            gas_limit: 100_000,
            ..Default::default()
        }),
        SENDER,
    );
    let mst = legacy(LIVE, 0, ORACLE, call_data("getSlot(uint256)", Some(1)));
    // The deposit took the sender's nonce 0.
    let user = legacy(SENDER, 1, Address::with_last_byte(0xc1), Bytes::new());
    let probing = legacy(SENDER, 2, PROBE, Bytes::new());
    for (name, tx) in [("deposit", deposit), ("system", mst), ("user", user), ("probe", probing)] {
        let out = block.execute_candidate(&tx, &[]).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(out.result().status, "{name}: {:?}", out.result());
        assert_eq!(out.is_system_transaction(), name == "system", "{name}");
        let registry = out.pending().account_changes_for(&SEQUENCER_REGISTRY);
        if name == "probe" {
            // Its own access: the account, not slot 0.
            let registry = registry.expect("the probe's access");
            assert!(registry.storage.slots.is_empty(), "{name}: {registry:?}");
        } else {
            assert!(registry.is_none(), "{name} read the registry: {registry:?}");
        }
        block.commit_candidate(out).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
    let in_block = common::committed_slot(&block, PROBE, U256::ZERO).expect("the probe stored");
    // EIP-8038's cold account access, 3,000, and the probe's own 3 + 2 (`PUSH20`, `POP`).
    assert_eq!(in_block, U256::from(3_000 + 5 + 2), "the registry's account is cold");
    let mut evm = common::evm(common::block(), Db::new(db));
    let tx = legacy(SENDER, 0, PROBE, Bytes::new());
    assert!(evm.transact(&tx).expect("runs").commit().status);
    let outside = evm.state_mut().storage_slot_untracked(&PROBE, &U256::ZERO).unwrap();
    assert_eq!(outside, in_block, "a single transaction's EVM prices it as a block does");
}

/// A single transaction's EVM resolves the live system address once, when it is made, off the
/// state it runs on, by a direct read with no warming and no charge (`SATIN-REG-002`): it
/// classifies against that address, reads nothing of the registry for it, and prices a
/// transaction that reads the registry as a block does. Outside a block a registry that is absent,
/// holds other code or names no address leaves no system address, and nothing is invalidated.
#[test]
fn lifecycle_a_single_transaction_evm_resolves_once_and_prices_as_a_block_does() {
    let mut db = with_registry(world(), registry_v2());
    db.insert_account_info(&LIVE, AccountInfo::default().with_balance(U256::from(1) << 64));
    db.insert_account_info(
        &ORACLE,
        AccountInfo::default()
            .with_code(Bytecode::new_legacy(FLAT_SYSTEM_CONTRACTS[0].code.clone())),
    );
    let mut set_slot_data = keccak256("setSlot(uint256,bytes32)")[..4].to_vec();
    set_slot_data.extend_from_slice(&U256::from(3).to_be_bytes::<32>());
    set_slot_data.extend_from_slice(&U256::from(4).to_be_bytes::<32>());
    // The Oracle's `setSlot` checks its caller against the registry's live system address.
    let mst = legacy(LIVE, 0, ORACLE, set_slot_data.into());
    let mut block = start(&genesis_chain(), &block_input(), db.clone()).expect("the block starts");
    let in_block = block.execute_candidate(&mst, &[]).expect("runs");
    assert!(
        in_block.is_system_transaction() && in_block.result().status,
        "{:?}",
        in_block.result()
    );
    let mut evm = common::evm(common::block(), Db::new(db.clone()));
    assert_eq!(evm.ext().system_address, Some(LIVE), "resolved when the EVM was made");
    let outside = evm.transact(&mst).expect("runs").detach();
    assert!(outside.result.status && evm.ext().is_system, "{:?}", outside.result);
    assert_eq!(outside.result.tx_gas_used(), in_block.result().tx_gas_used(), "the same gas");
    let registry = outside.pending_state.account_changes_for(&SEQUENCER_REGISTRY).expect("read");
    assert!(!registry.is_changed(), "the Oracle's own read of the registry, nothing more");
    // Outside a block, no address is the system address where a block would be invalid.
    let mut zero = db.clone();
    set_slot(&mut zero, 0, U256::ZERO);
    let mut foreign = db.clone();
    foreign
        .insert_account_info(&SEQUENCER_REGISTRY, AccountInfo::default().with_code(code(&[0x00])));
    for state in [world(), zero, foreign] {
        let evm = common::evm(common::block(), Db::new(state));
        assert_eq!(evm.ext().system_address, None);
    }
}

/// The block in which Satin activates, and no other, checks the EIP-7997 factory, third, after the
/// EIP calls and before any deployment: a missing account, an account without code or with other
/// code invalidates the block; the factory's nonce, balance and storage are not checked, and the
/// check writes nothing.
#[test]
fn lifecycle_the_factory_is_checked_in_the_crossing_block_only() {
    let crossing = chain(100, 0);
    let mut odd = AccountInfo::default().with_nonce(9).with_balance(U256::from(1));
    for (factory, found) in [
        (None, None),
        (Some(odd.clone()), Some(KECCAK256_EMPTY)),
        (Some(odd.clone().with_code(code(&[0x00]))), Some(keccak256([0x00]))),
    ] {
        let mut db = world();
        if let Some(info) = factory {
            db.insert_account_info(&CREATE2_FACTORY, info);
        }
        let error = failure(start(&crossing, &block_input(), db));
        assert_eq!(error.kind(), &BlockErrorKind::Create2Factory { code_hash: found });
        assert_eq!(error.phase(), Phase::FactoryCheck);
        assert_eq!(error.disposition(), Disposition::InvalidBlock);
        assert_eq!(error.context().address, Some(CREATE2_FACTORY));
    }
    odd = odd.with_code(code(CREATE2_FACTORY_CODE));
    let mut db = world();
    db.insert_account_info(&CREATE2_FACTORY, odd);
    db.insert_account_storage(&CREATE2_FACTORY, &U256::from(1), &U256::from(2));
    let block = start(&crossing, &block_input(), db).expect("the activation block starts");
    assert!(block.activation().satin_activates());
    let states = block.pre_block_states();
    assert_eq!(states.len(), 11);
    let check = &states[2];
    assert_eq!(check.account_changes().count(), 1);
    let read = check.account_changes_for(&CREATE2_FACTORY).unwrap();
    assert!(!read.is_changed() && read.storage.slots.is_empty(), "read-only, its storage unread");
    assert!(states[3].account_changes_for(&ORACLE).is_some(), "the deployments follow");
    assert!(changed_account(&block, CREATE2_FACTORY).is_none());
    // The next block checks nothing: it starts without the factory and reads it nowhere.
    let block = start(&crossing, &input(NUMBER + 1, 101), world()).expect("the next block");
    assert!(!block.activation().satin_activates());
    assert_eq!(block.pre_block_states().len(), 10);
    assert!(
        block.pre_block_states().iter().all(|s| s.account_changes_for(&CREATE2_FACTORY).is_none())
    );
}

/// Block zero of a chain on which Satin is active from genesis: no EIP-2935 or EIP-4788 call and
/// no factory check, and the rest of the sequence as in every block, installing every system
/// contract and seeding the registry at block zero.
#[test]
fn lifecycle_block_zero_makes_no_eip_call_and_installs_every_contract() {
    let zero = BlockInput {
        header: BlockHeader {
            parent_beacon_block_root: Some(B256::ZERO),
            ..BlockHeader::default()
        },
        parent: ParentContext::BlockZero,
    };
    let block = start(&genesis_chain(), &zero, InMemoryDB::default()).expect("block zero starts");
    assert!(block.activation().block_zero() && !block.activation().satin_activates());
    let states = block.pre_block_states();
    assert_eq!(states.len(), 8, "five deployments, the due read, the registry, the resolution");
    for address in [HISTORY_STORAGE, BEACON_ROOTS, CREATE2_FACTORY] {
        assert!(states.iter().all(|s| s.account_changes_for(&address).is_none()), "{address}");
    }
    for (state, contract) in states.iter().zip(&FLAT_SYSTEM_CONTRACTS) {
        let installed = state.account_changes_for(&contract.address).expect(contract.name);
        assert_eq!(installed.current.map(|i| i.code_hash), Some(contract.code_hash));
    }
    let slots = changed_slots(&block, SEQUENCER_REGISTRY);
    assert!(slots.contains(&(0, address_word(MEGA_SYSTEM_ADDRESS))), "{slots:?}");
    assert!(!slots.iter().any(|(k, _)| *k == 6), "the initial block, zero, writes no change");
    assert_eq!(block.system_address(), Some(MEGA_SYSTEM_ADDRESS));
}

/// Satin active from genesis at a genesis timestamp after `satinTime`: no block is the one in
/// which Satin activates, so the first block after genesis checks no factory and admits every
/// transaction; it installs every system contract the genesis state lacks.
#[test]
fn lifecycle_a_genesis_active_chain_has_no_crossing_block() {
    let factory = chain(500, 1_000);
    let first = BlockInput {
        header: BlockHeader {
            number: 1,
            timestamp: 1_001,
            gas_limit: 30_000_000,
            parent_beacon_block_root: Some(B256::repeat_byte(0x11)),
            ..BlockHeader::default()
        },
        parent: ParentContext::Parent { number: 0, hash: B256::ZERO, timestamp: 1_000 },
    };
    let block = start(&factory, &first, world()).expect("the first block starts");
    assert!(!block.activation().satin_activates() && !block.activation().deposits_only());
    let states = block.pre_block_states();
    assert_eq!(states.len(), 10);
    assert!(states.iter().all(|s| s.account_changes_for(&CREATE2_FACTORY).is_none()));
    assert_eq!(block.system_address(), Some(MEGA_SYSTEM_ADDRESS));
}

/// A pre-block call's first frame as the block's inspector saw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Call {
    caller: Address,
    target: Address,
    gas_limit: u64,
    reservoir: u64,
}

#[derive(Default)]
struct Calls(Rc<RefCell<Vec<Call>>>);

impl Inspector<MegaTypes> for Calls {
    fn call(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        m: &mut Message<MegaTypes>,
    ) -> Option<MessageResult<MegaTypes>> {
        if m.depth == 0 {
            self.0.borrow_mut().push(Call {
                caller: m.caller,
                target: m.call_target,
                gas_limit: m.gas_limit,
                reservoir: m.reservoir,
            });
        }
        None
    }
}

/// Each pre-block call runs from `SYSTEM_CALLER` on 30,000,000 of regular gas and a reservoir of
/// sixteen fresh slots' state gas at `m = 1` (1,566,720), whatever the block's gas limit (here
/// 1,000,000), and `GAS` inside reads at most 30,000,000.
#[test]
fn lifecycle_pre_block_calls_run_on_thirty_million_regular_gas_and_a_sixteen_slot_reservoir() {
    const SYSTEM_CALLER: Address = Address::new([
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, 0xfe,
    ]);
    let mut db = with_registry(world(), registry_v2());
    set_slot(&mut db, 7, address_word(NEXT));
    set_slot(&mut db, 8, U256::from(NUMBER));
    // The EIP-2935 address stores what `GAS` reads.
    db.insert_account_info(
        &HISTORY_STORAGE,
        AccountInfo::default().with_code(code(&[0x5a, 0x5f, 0x55, 0x00])),
    );
    let calls = Rc::new(RefCell::new(Vec::new()));
    let services = BlockServices {
        inspector: Some(Box::new(Calls(calls.clone()))),
        ..common::services(B256::ZERO)
    };
    let input = common::block_input(NUMBER, 100, 1_000_000, Some(B256::repeat_byte(0x11)));
    let block =
        genesis_chain().start_block(&input, Db::new(db), services).expect("the block starts");
    let budget = |target| Call {
        caller: SYSTEM_CALLER,
        target,
        gas_limit: 30_000_000,
        reservoir: 1_566_720,
    };
    assert_eq!(
        *calls.borrow(),
        [budget(HISTORY_STORAGE), budget(BEACON_ROOTS), budget(SEQUENCER_REGISTRY)]
    );
    let gas = common::committed_slot(&block, HISTORY_STORAGE, U256::ZERO).expect("stored");
    assert_eq!(gas, U256::from(30_000_000 - 2), "GAS reads the regular gas alone");
}

/// A read the database fails in the factory check, a deployment or the registry's migration is an
/// internal error, whatever the database declares (`SATIN-PBLK-006`).
#[test]
fn lifecycle_a_failed_read_of_a_deployment_or_the_factory_check_is_internal() {
    let mut db = world();
    db.insert_account_info(
        &CREATE2_FACTORY,
        AccountInfo::default().with_code(code(CREATE2_FACTORY_CODE)),
    );
    assert_read_is_internal(
        &chain(100, 0),
        &block_input(),
        &db,
        CREATE2_FACTORY,
        None,
        Phase::FactoryCheck,
    );
    for contract in &FLAT_SYSTEM_CONTRACTS {
        assert_read_is_internal(
            &genesis_chain(),
            &block_input(),
            &db,
            contract.address,
            None,
            Phase::Deployment,
        );
    }
    let v1 = with_registry(world(), registry_v1());
    assert_read_is_internal(
        &genesis_chain(),
        &block_input(),
        &v1,
        SEQUENCER_REGISTRY,
        Some(13),
        Phase::Deployment,
    );
}
