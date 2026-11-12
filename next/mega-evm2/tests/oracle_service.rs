//! The engine's Oracle seam (`mega_evm2::oracle`): the oracle service an Oracle read asks and a
//! hint is handed to, and the observer of each execution's Oracle reads and standing writes, with
//! small stub services and an observer that record what they were asked and shown.
//!
//! The Oracle runs its own code (version 2.0.0) unless a test says otherwise; its slot 5 holds 3
//! on the chain.

mod common;

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, hex, keccak256};
use common::{SENDER, TOP, contract};
use evm2::{
    DatabaseError, Evm, TxResult,
    bytecode::Bytecode,
    evm::{AccountInfo, Database, Db, InMemoryDB},
    interpreter::Word,
};
use mega_evm2::{
    MegaTypes, SEQUENCER_REGISTRY,
    block::{BEACON_ROOTS, HISTORY_STORAGE},
    factory::BlockServices,
    keyless::KEYLESS,
    oracle::{
        OracleAccess, OracleAnswers, OracleBoundary, OracleCall, OracleExecution, OracleObserver,
        OracleService, OracleServices, RecordingOracle,
    },
    system_contracts::ORACLE_CODE,
    tx::ORACLE,
};
use op_evm2::{OpTxEnvelope, TxDeposit};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const SLOT: u64 = 5;
const ON_CHAIN: u64 = 3;
/// A contract the registry names as the live system address, and the contract that calls it.
const SYS: Address = Address::with_last_byte(0x51);
const OUTER: Address = Address::with_last_byte(0xa1);
/// A hint's topic.
const TOPIC: B256 = B256::repeat_byte(0x11);

// ---------------------------------------------------------------------------------------------
// Stub services and the observer

/// Answers every slot with `base`, `base + 1`, … at each ask: a value of the moment of the read.
struct Ticking {
    base: u64,
    asks: AtomicU64,
}

impl Ticking {
    fn new(base: u64) -> Self {
        Self { base, asks: AtomicU64::new(0) }
    }
}

impl OracleService for Ticking {
    fn answer(&self, _: U256) -> Option<U256> {
        Some(U256::from(self.base + self.asks.fetch_add(1, Ordering::Relaxed)))
    }
}

/// Answers every slot with what the last hint fetched (its data read as a number), `first`
/// before any hint.
struct Fetching(Mutex<U256>);

impl OracleService for Fetching {
    fn answer(&self, _: U256) -> Option<U256> {
        Some(*self.0.lock().unwrap())
    }

    fn hint(&self, _: Address, _: B256, data: &[u8]) {
        *self.0.lock().unwrap() = U256::from_be_slice(data);
    }
}

/// Answers each ask with the next of its values, nothing once they run out.
struct InTurn(Mutex<VecDeque<u64>>);

impl InTurn {
    fn new(values: &[u64]) -> Self {
        Self(Mutex::new(values.iter().copied().collect()))
    }
}

impl OracleService for InTurn {
    fn answer(&self, _: U256) -> Option<U256> {
        self.0.lock().unwrap().pop_front().map(U256::from)
    }
}

/// Records every execution it is shown.
#[derive(Default)]
struct Seen(Mutex<Vec<(OracleBoundary, Vec<OracleAccess>)>>);

impl OracleObserver for Seen {
    fn observe(&self, execution: OracleExecution<'_>) {
        self.0.lock().unwrap().push((execution.boundary, execution.accesses.to_vec()));
    }
}

impl Seen {
    fn all(&self) -> Vec<(OracleBoundary, Vec<OracleAccess>)> {
        self.0.lock().unwrap().clone()
    }

    /// The accesses of the one execution shown, a single transaction's EVM's first.
    fn only(&self) -> Vec<OracleAccess> {
        let all = self.all();
        assert_eq!(all.len(), 1, "one execution: {all:?}");
        assert_eq!(all[0].0, OracleBoundary::Transaction { index: 0, executed: true });
        all[0].1.clone()
    }
}

/// A read of `SLOT` that pushed (or would have pushed) `value` over the slot's `held`, which is
/// not a write of the execution's own.
fn read(value: u64, held: u64, served: bool, pushed: bool) -> OracleAccess {
    let (slot, value, held) = (U256::from(SLOT), U256::from(value), U256::from(held));
    OracleAccess::Read { slot, value, held, own: false, served, pushed }
}

/// `read` over a `held` that is a write of the execution's own in effect at the read.
fn own(read: OracleAccess) -> OracleAccess {
    match read {
        OracleAccess::Read { slot, value, held, served, pushed, .. } => {
            OracleAccess::Read { slot, value, held, own: true, served, pushed }
        }
        write => write,
    }
}

fn write(value: u64) -> OracleAccess {
    OracleAccess::Write { slot: U256::from(SLOT), value: U256::from(value) }
}

fn asked(answer: Option<u64>) -> OracleCall {
    OracleCall::Answer { slot: U256::from(SLOT), answer: answer.map(U256::from) }
}

fn hinted(from: Address, data: &[u8]) -> OracleCall {
    OracleCall::Hint { from, topic: TOPIC, data: Bytes::copy_from_slice(data) }
}

fn recording<S: OracleService>(inner: S) -> Arc<RecordingOracle<S>> {
    Arc::new(RecordingOracle::new(inner))
}

/// A service answering `SLOT` with `value`, recording its calls.
fn answering(value: u64) -> Arc<RecordingOracle> {
    recording(OracleAnswers::from_iter([(U256::from(SLOT), U256::from(value))]))
}

// ---------------------------------------------------------------------------------------------
// Inputs, code and transactions

fn selector(signature: &str) -> Vec<u8> {
    keccak256(signature)[..4].to_vec()
}

fn word(n: u64) -> [u8; 32] {
    U256::from(n).to_be_bytes()
}

fn get_slot() -> Vec<u8> {
    [selector("getSlot(uint256)"), word(SLOT).to_vec()].concat()
}

fn set_slot(value: u64) -> Vec<u8> {
    [selector("setSlot(uint256,bytes32)"), word(SLOT).to_vec(), word(value).to_vec()].concat()
}

/// `setSlots([SLOT], [])`: arrays of different lengths, refused with `InvalidLength`.
fn set_slots_mismatched() -> Vec<u8> {
    let mut input = selector("setSlots(uint256[],bytes32[])");
    for w in [0x40, 0x80, 1, SLOT, 0] {
        input.extend_from_slice(&word(w));
    }
    input
}

fn padded(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    out.resize(data.len().div_ceil(32) * 32, 0);
    out
}

fn send_hint(data: &[u8]) -> Vec<u8> {
    let mut input = selector("sendHint(bytes32,bytes)");
    input.extend_from_slice(TOPIC.as_slice());
    input.extend_from_slice(&word(0x40));
    input.extend_from_slice(&word(data.len() as u64));
    input.extend(padded(data));
    input
}

fn multi_call(payloads: &[Vec<u8>]) -> Vec<u8> {
    let mut input = selector("multiCall(bytes[])");
    input.extend_from_slice(&word(0x20));
    input.extend_from_slice(&word(payloads.len() as u64));
    let mut offset = 32 * payloads.len() as u64;
    for p in payloads {
        input.extend_from_slice(&word(offset));
        offset += 32 + padded(p).len() as u64;
    }
    for p in payloads {
        input.extend_from_slice(&word(p.len() as u64));
        input.extend(padded(p));
    }
    input
}

/// One `CALL` a [`caller`] makes: its target, value, gas (all it may forward when `None`) and
/// input.
struct Call {
    to: Address,
    value: u8,
    gas: Option<u16>,
    input: Vec<u8>,
}

fn oracle(input: Vec<u8>) -> Call {
    Call { to: ORACLE, value: 0, gas: None, input }
}

/// A call to `to` with no input.
fn call_to(to: Address) -> Call {
    Call { to, value: 0, gas: None, input: Vec::new() }
}

/// Code that makes one call of `op` (`STATICCALL` or `DELEGATECALL`, which carry no value) to `to`
/// with `input`, copied from the code's tail to memory 0x400, and returns its success in word 0 and
/// the first word of its return data in word 1.
fn call_without_value(op: u8, to: Address, input: &[u8]) -> Vec<u8> {
    const BODY: u8 = 49;
    let len = (input.len() as u16).to_be_bytes();
    // CODECOPY(0x400, BODY, len)
    let mut code = vec![0x61, len[0], len[1], 0x61, 0x00, BODY, 0x61, 0x04, 0x00, 0x39];
    // op(GAS, to, 0x400, len, 32, 32)
    code.extend([0x60, 0x20, 0x60, 0x20, 0x61, len[0], len[1], 0x61, 0x04, 0x00, 0x73]);
    code.extend_from_slice(to.as_slice());
    code.extend([0x5a, op]);
    // MSTORE(0, success); RETURN(0, 64)
    code.extend([0x5f, 0x52, 0x60, 0x40, 0x5f, 0xf3]);
    assert_eq!(code.len(), BODY as usize);
    code.extend_from_slice(input);
    code
}

#[derive(Clone, Copy)]
enum End {
    Return,
    Revert,
    /// Loops until the frame runs out of gas.
    Loop,
}

/// Code that makes `calls` in order, each input copied from the code's tail to memory 0x400, the
/// first word of its return data written at 32·i and its success at 32·(n + i), and then returns
/// or reverts with those 64·n bytes, or loops until it runs out of gas.
fn caller(calls: &[Call], end: End) -> Vec<u8> {
    let n = calls.len() as u16;
    let body: usize =
        calls.iter().map(|c| 49 + if c.gas.is_some() { 3 } else { 1 }).sum::<usize>() + 5;
    let mut code = Vec::new();
    let mut tail = Vec::new();
    for (i, c) in calls.iter().enumerate() {
        let i = i as u16;
        let len = (c.input.len() as u16).to_be_bytes();
        let at = ((body + tail.len()) as u16).to_be_bytes();
        // CODECOPY(0x400, at, len)
        code.extend([0x61, len[0], len[1], 0x61, at[0], at[1], 0x61, 0x04, 0x00, 0x39]);
        // CALL(gas, to, value, 0x400, len, 32·i, 32)
        let out = (32 * i).to_be_bytes();
        code.extend([0x60, 32, 0x61, out[0], out[1], 0x61, len[0], len[1], 0x61, 0x04, 0x00]);
        code.extend([0x60, c.value, 0x73]);
        code.extend_from_slice(c.to.as_slice());
        match c.gas {
            Some(g) => code.extend([0x61, (g >> 8) as u8, g as u8]),
            None => code.push(0x5a),
        }
        // MSTORE(32·(n + i), success)
        let flag = (32 * (n + i)).to_be_bytes();
        code.extend([0xf1, 0x61, flag[0], flag[1], 0x52]);
        tail.extend_from_slice(&c.input);
    }
    let size = (64 * n).to_be_bytes();
    match end {
        End::Return => code.extend([0x61, size[0], size[1], 0x5f, 0xf3]),
        End::Revert => code.extend([0x61, size[0], size[1], 0x5f, 0xfd]),
        End::Loop => {
            let here = (code.len() as u16).to_be_bytes();
            code.extend([0x5b, 0x61, here[0], here[1], 0x56]);
        }
    }
    assert_eq!(code.len(), body);
    code.extend(tail);
    code
}

fn oracle_code() -> AccountInfo {
    contract(&hex::decode(include_str!("fixtures/Oracle-2.0.0.hex").trim()).unwrap())
}

/// [`SENDER`] with 10 wei, the Oracle with its own code and slot 5 at 3, and `TOP` running
/// `top` when given.
fn db(top: Option<Vec<u8>>) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&SENDER, AccountInfo::default().with_balance(U256::from(10)));
    db.insert_account_info(&ORACLE, oracle_code());
    db.insert_account_storage(&ORACLE, &U256::from(SLOT), &U256::from(ON_CHAIN));
    if let Some(top) = top {
        db.insert_account_info(&TOP, contract(&top));
    }
    db
}

/// The registry (version 2.0.0) naming `system` as the live system address.
fn with_registry(db: &mut InMemoryDB, system: Address) {
    let code = hex::decode(include_str!("fixtures/SequencerRegistry-2.0.0.hex").trim()).unwrap();
    db.insert_account_info(
        &SEQUENCER_REGISTRY,
        AccountInfo::default().with_code(Bytecode::new_legacy(code.into())),
    );
    db.insert_account_storage(
        &SEQUENCER_REGISTRY,
        &U256::ZERO,
        &U256::from_be_slice(system.as_slice()),
    );
}

fn evm_with(
    db: InMemoryDB,
    service: Option<Arc<dyn OracleService>>,
    observer: Option<Arc<Seen>>,
) -> Evm<'static, MegaTypes> {
    let observer = observer.map(|o| o as Arc<dyn OracleObserver>);
    common::oracle_evm(common::block(), Db::new(db), OracleServices { service, observer })
}

fn legacy(
    from: Address,
    nonce: u64,
    to: Address,
    value: u64,
    input: Vec<u8>,
) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy {
        chain_id: Some(common::CHAIN_ID),
        nonce,
        gas_limit: 1_000_000,
        to: TxKind::Call(to),
        value: U256::from(value),
        input: input.into(),
        ..TxLegacy::default()
    };
    Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), from)
}

fn deposit(to: Address, input: Vec<u8>) -> Recovered<OpTxEnvelope> {
    Recovered::new_unchecked(
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: B256::with_last_byte(0x42),
            from: SENDER,
            to: TxKind::Call(to),
            mint: 0,
            value: U256::ZERO,
            gas_limit: 1_000_000,
            is_system_transaction: false,
            input: input.into(),
        }),
        SENDER,
    )
}

fn transact(
    evm: &mut Evm<'static, MegaTypes>,
    tx: &Recovered<OpTxEnvelope>,
) -> TxResult<MegaTypes> {
    evm.transact(tx).expect("a valid transaction").commit()
}

/// The `i`-th word of `output`.
fn out(output: &[u8], i: usize) -> u64 {
    U256::from_be_slice(&output[32 * i..32 * (i + 1)]).to::<u64>()
}

// ---------------------------------------------------------------------------------------------
// Reads

/// Two reads of one slot in one transaction each ask the service, and each pushes the service's
/// answer of its moment (`SATIN-ORCL-002`): a ticking service answers 100 then 101.
#[test]
fn oracle_service_changing_answer_pushes_each_current_answer() {
    let service = recording(Ticking::new(100));
    let seen = Arc::new(Seen::default());
    let top = caller(&[oracle(get_slot()), oracle(get_slot())], End::Return);
    let mut evm = evm_with(db(Some(top)), Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!((out(&r.output, 0), out(&r.output, 1)), (100, 101), "the values pushed");
    assert_eq!(service.calls(), [asked(Some(100)), asked(Some(101))]);
    assert_eq!(seen.only(), [read(100, ON_CHAIN, true, true), read(101, ON_CHAIN, true, true)]);
}

/// The Oracle's code for the warmth probe: reads slot 5 (when `read`) into word 0, then writes slot
/// 5's own value (3) back and returns in word 1 what `PUSH1; PUSH1; SSTORE; GAS` cost: 108 when
/// the slot is warm (the `SSTORE`'s warm 100), 2,108 when it is cold (the cold access's 2,100, its
/// warm base included).
fn warmth_probe(read: bool) -> Vec<u8> {
    let mut code = if read { vec![0x60, SLOT as u8, 0x54, 0x5f, 0x52] } else { Vec::new() };
    code.extend([0x5a, 0x60, ON_CHAIN as u8, 0x60, SLOT as u8, 0x55, 0x5a, 0x90, 0x03]);
    code.extend([0x60, 32, 0x52, 0x60, 64, 0x5f, 0xf3]);
    code
}

/// A service with no value for the slot answers `None`: the read pushes the slot's value on the
/// chain, and the slot is loaded all the same, so a later access finds it warm (`SATIN-ORCL-002`,
/// `SATIN-ORCL-003`); without the read the same write pays the cold access.
#[test]
fn oracle_service_none_falls_back_to_the_slot_and_leaves_it_warm() {
    for (probe_reads, cost) in [(true, 108), (false, 2_108)] {
        let mut state = db(None);
        state.insert_account_info(&ORACLE, contract(&warmth_probe(probe_reads)));
        let service = recording(OracleAnswers::default());
        let seen = Arc::new(Seen::default());
        let mut evm = evm_with(state, Some(service.clone()), Some(seen.clone()));
        let r = transact(&mut evm, &legacy(SENDER, 0, ORACLE, 0, Vec::new()));
        assert!(r.status, "{:?}", r.stop);
        assert_eq!(out(&r.output, 1), cost, "the write's cost, read {probe_reads}");
        if probe_reads {
            assert_eq!(out(&r.output, 0), ON_CHAIN, "the slot's value pushed");
            assert_eq!(service.calls(), [asked(None)]);
            assert_eq!(seen.only(), [read(ON_CHAIN, ON_CHAIN, false, true), write(ON_CHAIN)]);
        } else {
            assert_eq!(service.calls(), []);
        }
    }
}

/// With no service every read pushes the chain's value — a validator's, a replaying node's, an
/// RPC call's EVM — and an observer without a service sees the read, unserved; the EVM numbers
/// the transactions it executes from zero. A service answering the slot is the control.
#[test]
fn oracle_service_no_service_pushes_the_chains_values() {
    let top = || caller(&[oracle(get_slot())], End::Return);
    let mut evm = evm_with(db(Some(top())), None, None);
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert_eq!(out(&r.output, 0), ON_CHAIN);
    assert_eq!(evm.ext().oracle_reads, 1, "the read is one that asks");

    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(db(Some(top())), None, Some(seen.clone()));
    for nonce in 0..2 {
        let r = transact(&mut evm, &legacy(SENDER, nonce, TOP, 0, Vec::new()));
        assert_eq!(out(&r.output, 0), ON_CHAIN, "{nonce}");
    }
    let shown = (0..2)
        .map(|index| {
            (
                OracleBoundary::Transaction { index, executed: true },
                vec![read(ON_CHAIN, ON_CHAIN, false, true)],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(seen.all(), shown);

    let mut evm = evm_with(db(Some(top())), Some(answering(42)), None);
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert_eq!(out(&r.output, 0), 42, "the control: the service's value");

    // A hint with no service is admitted and counted all the same (`SATIN-ORCL-009`); nothing
    // receives it.
    let hint = send_hint(&[0xaa]);
    let len = hint.len() as u64;
    let top = caller(&[oracle(hint), oracle(get_slot())], End::Return);
    let mut evm = evm_with(db(Some(top)), None, None);
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(evm.ext().mechanisms.hint_bytes, len, "the hint counted");
    assert_eq!(out(&r.output, 1), ON_CHAIN);
}

/// A deposit's Oracle read asks the service nothing and pushes the chain's value
/// (`SATIN-ORCL-005`); the observer sees it, unserved.
#[test]
fn oracle_service_deposit_is_never_asked() {
    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(db(None), Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &deposit(ORACLE, get_slot()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 0), ON_CHAIN);
    assert_eq!(service.calls(), []);
    assert_eq!(seen.only(), [read(ON_CHAIN, ON_CHAIN, false, true)]);
}

/// A Mega System Transaction's Oracle read asks the service nothing (`SATIN-ORCL-005`); the
/// observer sees it, unserved.
#[test]
fn oracle_service_mst_is_never_asked() {
    let system = Address::with_last_byte(0x52);
    let mut state = db(None);
    state.insert_account_info(&system, AccountInfo::default().with_balance(U256::from(1)));
    with_registry(&mut state, system);
    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(state, Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(system, 0, ORACLE, 0, get_slot()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 0), ON_CHAIN);
    assert_eq!(service.calls(), []);
    assert_eq!(seen.only(), [read(ON_CHAIN, ON_CHAIN, false, true)]);
}

/// A block whose beacon-roots pre-block call reads the Oracle's slot 5 (`getSlot(5)`) and stores
/// the word in its own slot 0, connected to `oracle`.
fn block_reading_the_oracle(oracle: OracleServices) -> mega_evm2::block::BlockSession<'static> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &ORACLE,
        AccountInfo::default().with_code(Bytecode::new_legacy(ORACLE_CODE)),
    );
    db.insert_account_storage(&ORACLE, &U256::from(SLOT), &U256::from(ON_CHAIN));
    db.insert_account_info(&SENDER, AccountInfo::default().with_balance(U256::from(10)));
    // MSTORE(0, getSlot's selector << 224); MSTORE(4, 5);
    // CALL(GAS, ORACLE, 0, 0, 36, 0, 32) POP; SSTORE(0, MLOAD(0)); STOP
    let mut code = vec![0x63, 0x7e, 0xba, 0x7b, 0xa6, 0x60, 0xe0, 0x1b, 0x5f, 0x52];
    code.extend_from_slice(&[0x60, SLOT as u8, 0x60, 0x04, 0x52]);
    code.extend_from_slice(&[0x60, 0x20, 0x5f, 0x60, 0x24, 0x5f, 0x5f, 0x73]);
    code.extend_from_slice(ORACLE.as_slice());
    code.extend_from_slice(&[0x5a, 0xf1, 0x50, 0x5f, 0x51, 0x5f, 0x55, 0x00]);
    db.insert_account_info(
        &BEACON_ROOTS,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from(code))),
    );
    let input = common::block_input(1, 1_000, 30_000_000, Some(B256::repeat_byte(0x11)));
    let services = BlockServices { oracle, ..common::services(B256::ZERO) };
    common::factory().start_block(&input, Db::new(db), services).expect("pre-block calls")
}

/// A pre-block system call's Oracle read asks the service nothing (`SATIN-ORCL-005`); the
/// observer sees each pre-block call in order, the beacon-roots call's read unserved. A
/// transaction of the block making the same call is served (the control).
#[test]
fn oracle_service_system_call_is_never_asked() {
    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let oracle = OracleServices { service: Some(service.clone()), observer: Some(seen.clone()) };
    let mut block = block_reading_the_oracle(oracle);
    assert_eq!(
        common::committed_slot(&block, BEACON_ROOTS, U256::ZERO),
        Some(U256::from(ON_CHAIN))
    );
    assert_eq!(service.calls(), [], "a system call asked the service");
    assert_eq!(
        seen.all(),
        [
            (OracleBoundary::SystemCall { target: HISTORY_STORAGE }, vec![]),
            (
                OracleBoundary::SystemCall { target: BEACON_ROOTS },
                vec![read(ON_CHAIN, ON_CHAIN, false, true)]
            ),
        ]
    );
    let candidate = block
        .execute_candidate(&legacy(SENDER, 0, BEACON_ROOTS, 0, Vec::new()), &[])
        .expect("the call runs");
    assert!(candidate.result().status);
    assert_eq!(service.calls(), [asked(Some(42))]);
    assert_eq!(
        seen.all()[2],
        (
            OracleBoundary::Transaction { index: 0, executed: true },
            vec![read(42, ON_CHAIN, true, true)]
        )
    );
}

/// The Oracle's code run by `DELEGATECALL` from another account reads that account's storage, in
/// that account's frame: no Oracle read, so the service is not asked and the observer sees nothing.
#[test]
fn oracle_service_delegatecall_into_the_oracles_code_is_neither_asked_nor_observed() {
    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let mut state = db(Some(call_without_value(0xf4, ORACLE, &get_slot())));
    state.insert_account_storage(&TOP, &U256::from(SLOT), &U256::from(77));
    let mut evm = evm_with(state, Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!((out(&r.output, 0), out(&r.output, 1)), (1, 77), "TOP's own slot");
    assert_eq!(service.calls(), []);
    assert_eq!(seen.only(), []);
}

// ---------------------------------------------------------------------------------------------
// Hints

/// A hint made before a read reaches the service before that read asks, with its topic, its data
/// and the address of the frame that made the call (`SATIN-ORCL-006`, `SATIN-ORCL-007`); a
/// `sendHint` that is the transaction's own call is the transaction's sender's.
#[test]
fn oracle_service_hint_order_hands_the_hint_before_the_next_read_asks() {
    let service = answering(42);
    let top = caller(&[oracle(send_hint(&[0xaa, 0xbb])), oracle(get_slot())], End::Return);
    let mut evm = evm_with(db(Some(top)), Some(service.clone()), None);
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 1), 42);
    assert_eq!(service.calls(), [hinted(TOP, &[0xaa, 0xbb]), asked(Some(42))]);

    let service = answering(42);
    let mut evm = evm_with(db(None), Some(service.clone()), None);
    let r = transact(&mut evm, &legacy(SENDER, 0, ORACLE, 0, send_hint(&[0xcc])));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(service.calls(), [hinted(SENDER, &[0xcc])], "the transaction's own call");
}

/// A frame that hints, reads and then reverts: the hint was handed over and the read is observed
/// all the same, as the transaction's own frame and as a frame below it.
#[test]
fn oracle_service_revert_keeps_the_hint_and_the_read() {
    for nested in [false, true] {
        let service = answering(42);
        let seen = Arc::new(Seen::default());
        let top = caller(&[oracle(send_hint(&[0xaa])), oracle(get_slot())], End::Revert);
        let mut state = db(Some(top));
        let to = if nested {
            let call = Call { to: TOP, value: 0, gas: None, input: Vec::new() };
            state.insert_account_info(&OUTER, contract(&caller(&[call], End::Return)));
            OUTER
        } else {
            TOP
        };
        let mut evm = evm_with(state, Some(service.clone()), Some(seen.clone()));
        let r = transact(&mut evm, &legacy(SENDER, 0, to, 0, Vec::new()));
        assert_eq!(r.status, nested, "nested {nested}");
        assert_eq!(service.calls(), [hinted(TOP, &[0xaa]), asked(Some(42))], "nested {nested}");
        assert_eq!(seen.only(), [read(42, ON_CHAIN, true, true)], "nested {nested}");
    }
}

/// A frame that hints, reads and then runs out of gas keeps both; a read whose frame cannot pay
/// its cold access asks and is observed, pushing nothing (`SATIN-ORCL-004`): `getSlot(5)` called
/// with 250 of gas reaches its `SLOAD` with 29.
#[test]
fn oracle_service_out_of_gas_keeps_the_hint_and_the_read() {
    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let top = caller(&[oracle(send_hint(&[0xaa])), oracle(get_slot())], End::Loop);
    let mut evm = evm_with(db(Some(top)), Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(!r.status);
    assert!(r.stop.is_halt(), "{:?}", r.stop);
    assert_eq!(service.calls(), [hinted(TOP, &[0xaa]), asked(Some(42))]);
    assert_eq!(seen.only(), [read(42, ON_CHAIN, true, true)]);

    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let short = Call { to: ORACLE, value: 0, gas: Some(250), input: get_slot() };
    let top = caller(&[oracle(send_hint(&[0xaa])), short], End::Return);
    let mut evm = evm_with(db(Some(top)), Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 3), 0, "the read's call ran out of gas");
    assert_eq!(service.calls(), [hinted(TOP, &[0xaa]), asked(Some(42))]);
    assert_eq!(seen.only(), [read(42, ON_CHAIN, true, false)], "made, and pushed nothing");
}

/// A read that pushes nothing is made, asked and observed all the same (`SATIN-ORCL-004`): a
/// `getSlot` whose frame reaches its `SLOAD` with about 1,000 of gas, over the warm base and short
/// of the 2,100 cold access; and one stopped at gas detention's crossing, the Oracle's detention
/// cap lowered to 1,000 so that the cold access crosses it in a frame with gas to spare.
#[test]
fn oracle_service_unpushed_read_short_of_the_cold_access_or_at_detention_is_observed() {
    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let short = Call { to: ORACLE, value: 0, gas: Some(1_250), input: get_slot() };
    let top = caller(&[short], End::Return);
    let mut evm = evm_with(db(Some(top)), Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 1), 0, "the read's call ran out of gas");
    assert_eq!(service.calls(), [asked(Some(42))]);
    assert_eq!(seen.only(), [read(42, ON_CHAIN, true, false)]);

    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(db(None), Some(service.clone()), Some(seen.clone()));
    evm.ext_mut().limits.oracle_cap = 1_000;
    let r = transact(&mut evm, &legacy(SENDER, 0, ORACLE, 0, get_slot()));
    assert!(!r.status, "stopped at the crossing");
    assert!(r.tx_gas_used() < 100_000, "not out of the transaction's gas: {}", r.tx_gas_used());
    assert_eq!(service.calls(), [asked(Some(42))]);
    assert_eq!(seen.only(), [read(42, ON_CHAIN, true, false)]);
}

/// `sendHint(7, 64 × 0xab)`: 164 bytes, 474 with the body of the transaction whose own call it is.
fn hint_64() -> Vec<u8> {
    send_hint(&[0xab; 64])
}

/// A hint whose input would take the transaction over its data-size limit is answered with the
/// stop and never handed to the service (`SATIN-ORCL-010`): 474 + 164 over a limit of 600; under
/// one of 1,000 it is handed over.
#[test]
fn oracle_service_over_limit_hint_is_never_delivered() {
    for (limit, delivered) in [(600, false), (1_000, true)] {
        let service = answering(42);
        let mut evm = evm_with(db(None), Some(service.clone()), None);
        evm.ext_mut().limits.tx[0] = limit;
        let r = transact(&mut evm, &legacy(SENDER, 0, ORACLE, 0, hint_64()));
        let expected = if delivered { vec![hinted(SENDER, &[0xab; 64])] } else { vec![] };
        assert_eq!(service.calls(), expected, "limit {limit}");
        assert_eq!(r.status, delivered, "limit {limit}");
        if !delivered {
            assert_eq!(r.output.as_ref(), mega_evm2::limit_exceeded_output(0, limit).as_slice());
        }
    }
}

/// A value-bearing `sendHint` its caller cannot fund is admitted before the frame start checks the
/// funds (`SATIN-ORCL-006`, `SATIN-ICPT-007`): handed over and counted, and then the call fails.
#[test]
fn oracle_service_unfunded_value_hint_is_delivered_then_fails_for_funds() {
    let service = answering(42);
    let input = send_hint(&[0xaa]);
    let len = input.len() as u64;
    let call = Call { to: ORACLE, value: 1, gas: None, input };
    let mut evm = evm_with(db(Some(caller(&[call], End::Return))), Some(service.clone()), None);
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 1), 0, "the call failed for its value");
    assert_eq!(service.calls(), [hinted(TOP, &[0xaa])]);
    assert_eq!(evm.ext().mechanisms.hint_bytes, len, "the hint counted");
}

/// An admitted input that does not decode as `sendHint(bytes32,bytes)` is counted and reaches no
/// service (`SATIN-ORCL-009`: "that includes an input that does not decode as
/// `sendHint(bytes32,bytes)`, which reaches no service"); a `sendHint` the Oracle runs by
/// `DELEGATECALL`, a `multiCall` payload, is no `sendHint` call and counts nothing
/// (`SATIN-ORCL-008`).
#[test]
fn oracle_service_malformed_hint_is_counted_and_not_delivered() {
    let service = answering(42);
    let malformed = [selector("sendHint(bytes32,bytes)"), vec![0x01; 10]].concat();
    let top = caller(&[oracle(malformed)], End::Return);
    let mut evm = evm_with(db(Some(top)), Some(service.clone()), None);
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(evm.ext().mechanisms.hint_bytes, 14, "the input counted");
    assert_eq!(service.calls(), [], "nothing handed over");

    let service = answering(42);
    let mut evm = evm_with(db(None), Some(service.clone()), None);
    let r = transact(&mut evm, &legacy(SENDER, 0, ORACLE, 0, multi_call(&[send_hint(&[0xaa])])));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(evm.ext().mechanisms.hint_bytes, 0);
    assert_eq!(service.calls(), []);
}

/// A `sendHint` made by `STATICCALL` is admitted, counted and handed over as one made by `CALL`
/// (`SATIN-ORCL-006`).
#[test]
fn oracle_service_staticcall_hint_is_delivered() {
    let service = answering(42);
    let input = send_hint(&[0xaa]);
    let len = input.len() as u64;
    let top = call_without_value(0xfa, ORACLE, &input);
    let mut evm = evm_with(db(Some(top)), Some(service.clone()), None);
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 0), 1, "the call succeeded");
    assert_eq!(service.calls(), [hinted(TOP, &[0xaa])]);
    assert_eq!(evm.ext().mechanisms.hint_bytes, len, "the hint counted");
}

// ---------------------------------------------------------------------------------------------
// Writes

/// A restricted write the system address makes stands as the Oracle's code authorizes it
/// (`SATIN-ORCL-011`), and the observer sees it: a Mega System Transaction's `setSlot(5, 8)`.
#[test]
fn oracle_service_standing_write_by_the_system_address_is_observed() {
    let system = Address::with_last_byte(0x52);
    let mut state = db(None);
    state.insert_account_info(&system, AccountInfo::default().with_balance(U256::from(1)));
    with_registry(&mut state, system);
    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(state, Some(answering(42)), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(system, 0, ORACLE, 0, set_slot(8)));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(seen.only(), [write(8)]);
}

/// A write that does not stand is not observed: the same `setSlot` inside a `multiCall` whose next
/// payload fails (`InvalidLength`), which reverts the system address's transaction; a `setSlot`
/// from an account that is not the system address, which the Oracle's code reverts with
/// `NotSystemAddress()`; and a write the system address's code makes in a frame that then reverts,
/// between reads that stay: the one inside that frame pushed the write and says so (`own`), and
/// the one after it sees the chain's value again, no write of the transaction's own in effect.
#[test]
fn oracle_service_standing_write_in_a_reverting_frame_is_not_observed() {
    let system = Address::with_last_byte(0x52);
    let mut state = db(None);
    state.insert_account_info(&system, AccountInfo::default().with_balance(U256::from(1)));
    with_registry(&mut state, system);
    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(state, None, Some(seen.clone()));
    let input = multi_call(&[set_slot(8), set_slots_mismatched()]);
    let r = transact(&mut evm, &legacy(system, 0, ORACLE, 0, input));
    assert!(!r.status, "the multiCall reverts");
    assert_eq!(seen.only(), [], "the reverted write");

    let seen = Arc::new(Seen::default());
    let mut state = db(None);
    with_registry(&mut state, system);
    let mut evm = evm_with(state, None, Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, ORACLE, 0, set_slot(8)));
    assert!(!r.status, "NotSystemAddress()");
    assert_eq!(seen.only(), [], "the unauthorized write");

    // SYS, the live system address, writes 8, reads it back and reverts; TOP reads before and
    // after calling it.
    let seen = Arc::new(Seen::default());
    let sys = caller(&[oracle(set_slot(8)), oracle(get_slot())], End::Revert);
    let call_sys = Call { to: SYS, value: 0, gas: None, input: Vec::new() };
    let top = caller(&[oracle(get_slot()), call_sys, oracle(get_slot())], End::Return);
    let mut state = db(Some(top));
    state.insert_account_info(&SYS, contract(&sys));
    with_registry(&mut state, SYS);
    let mut evm = evm_with(state, None, Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 4), 0, "SYS reverted");
    assert_eq!(
        seen.only(),
        [
            read(ON_CHAIN, ON_CHAIN, false, true),
            own(read(8, 8, false, true)),
            read(ON_CHAIN, ON_CHAIN, false, true)
        ]
    );
}

/// A write that stands is observed in its place, before the read it precedes: a Mega System
/// Transaction's `multiCall([setSlot(5, 8), getSlot(5)])`, whose read asks nothing and pushes the
/// write; and a user's transaction that reads 7 from the service, has the system address's code
/// write 8, and reads 8 (`SATIN-ORCL-013`'s example), which the observer shows as read, write,
/// read.
#[test]
fn oracle_service_standing_write_before_a_read_is_observed_before_it() {
    let system = Address::with_last_byte(0x52);
    let mut state = db(None);
    state.insert_account_info(&system, AccountInfo::default().with_balance(U256::from(1)));
    with_registry(&mut state, system);
    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(state, Some(answering(42)), Some(seen.clone()));
    let r =
        transact(&mut evm, &legacy(system, 0, ORACLE, 0, multi_call(&[set_slot(8), get_slot()])));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(seen.only(), [write(8), own(read(8, 8, false, true))]);

    let seen = Arc::new(Seen::default());
    let sys = caller(&[oracle(set_slot(8))], End::Return);
    let call_sys = Call { to: SYS, value: 0, gas: None, input: Vec::new() };
    let top = caller(&[oracle(get_slot()), call_sys, oracle(get_slot())], End::Return);
    let mut state = db(Some(top));
    state.insert_account_info(&SYS, contract(&sys));
    with_registry(&mut state, SYS);
    let mut evm = evm_with(state, Some(recording(Ticking::new(7))), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!((out(&r.output, 0), out(&r.output, 2)), (7, 8));
    assert_eq!(seen.only(), [read(7, ON_CHAIN, true, true), write(8), own(read(8, 8, true, true))]);
}

/// A transaction that reads a slot, sends a hint that has the service fetch another value, and
/// reads the slot again: the observer sees two reads of one slot with two values, which no single
/// value written before the transaction reproduces (`SATIN-ORCL-013`).
#[test]
fn oracle_service_two_values_for_one_slot_are_both_observed() {
    let service = recording(Fetching(Mutex::new(U256::from(7))));
    let seen = Arc::new(Seen::default());
    let top =
        caller(&[oracle(get_slot()), oracle(send_hint(&word(9))), oracle(get_slot())], End::Return);
    let mut evm = evm_with(db(Some(top)), Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!((out(&r.output, 0), out(&r.output, 2)), (7, 9));
    assert_eq!(service.calls(), [asked(Some(7)), hinted(TOP, &word(9)), asked(Some(9))]);
    assert_eq!(seen.only(), [read(7, ON_CHAIN, true, true), read(9, ON_CHAIN, true, true)]);
}

/// A deposit that halts (a failed deposit) is handed over as executed, its reads kept and its
/// writes taken back: the deposit calls `SYS`, the live system address, whose code writes 8, reads
/// it and runs out of gas. The deposit's read asks nothing (`SATIN-ORCL-005`).
#[test]
fn oracle_service_halting_deposit_keeps_its_reads_and_no_write() {
    let sys = caller(&[oracle(set_slot(8)), oracle(get_slot())], End::Loop);
    let mut state = db(None);
    state.insert_account_info(&SYS, contract(&sys));
    with_registry(&mut state, SYS);
    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(state, Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &deposit(SYS, Vec::new()));
    assert!(!r.status);
    assert!(r.stop.is_halt(), "{:?}", r.stop);
    assert_eq!(service.calls(), []);
    assert_eq!(seen.only(), [own(read(8, 8, false, true))]);
}

/// A pre-block system call that fails is handed over with its reads and none of its writes, and
/// fails the block: the beacon-roots call's code, which the registry names the live system address
/// here, writes 8, reads it and reverts.
#[test]
fn oracle_service_failing_system_call_reports_no_write() {
    let code = caller(&[oracle(set_slot(8)), oracle(get_slot())], End::Revert);
    let mut state = db(None);
    state.insert_account_info(&BEACON_ROOTS, contract(&code));
    with_registry(&mut state, BEACON_ROOTS);
    let seen = Arc::new(Seen::default());
    let services = OracleServices { service: Some(answering(42)), observer: Some(seen.clone()) };
    let input = common::block_input(1, 1_000, 30_000_000, Some(B256::repeat_byte(0x11)));
    let services = BlockServices { oracle: services, ..common::services(B256::ZERO) };
    let started = common::factory().start_block(&input, Db::new(state), services);
    assert!(started.is_err(), "the failed call fails the block");
    assert_eq!(
        seen.all(),
        [
            (OracleBoundary::SystemCall { target: HISTORY_STORAGE }, vec![]),
            (
                OracleBoundary::SystemCall { target: BEACON_ROOTS },
                vec![own(read(8, 8, false, true))]
            ),
        ]
    );
}

/// A failed frame that mixed reads and writes keeps its reads, in their order, and none of its
/// writes, and a write that stands after it is reported in its place: `SYS`, the live system
/// address, runs a `multiCall` of `setSlot(8)`, `getSlot`, `setSlot(9)`, `getSlot` and a failing
/// payload, then reads (the chain's value again), writes 10 and reads it.
#[test]
fn oracle_service_standing_write_after_a_failed_frame_keeps_the_order() {
    let failing =
        multi_call(&[set_slot(8), get_slot(), set_slot(9), get_slot(), set_slots_mismatched()]);
    let sys = caller(
        &[oracle(failing), oracle(get_slot()), oracle(set_slot(10)), oracle(get_slot())],
        End::Return,
    );
    let mut state = db(None);
    state.insert_account_info(&SYS, contract(&sys));
    with_registry(&mut state, SYS);
    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(state, None, Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, SYS, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 4), 0, "the multiCall failed");
    assert_eq!(
        seen.only(),
        [
            own(read(8, 8, false, true)),
            own(read(9, 9, false, true)),
            read(ON_CHAIN, ON_CHAIN, false, true),
            write(10),
            own(read(10, 10, false, true)),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Reads over the execution's own writes
//
// A read reports the value the slot held (`held`) and whether that value was a write of the
// execution's own still in effect (`own`), so that a builder decides each read on its own, also
// when a later failure takes the write back and the record no longer shows it.

/// `TOP`: calls `SYS` and returns the second word `SYS` returned or reverted with, the value its
/// `getSlot` pushed.
fn top_returning_the_sys_read() -> Vec<u8> {
    // CALL(GAS, SYS, 0, 0, 0, 0, 0) POP; RETURNDATACOPY(0, 32, 32); RETURN(0, 32)
    let mut code = vec![0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x73];
    code.extend_from_slice(SYS.as_slice());
    code.extend([0x5a, 0xf1, 0x50, 0x60, 0x20, 0x60, 0x20, 0x5f, 0x3e, 0x60, 0x20, 0x5f, 0xf3]);
    code
}

/// Slot 5 at `on_chain`; `SYS`, the live system address, runs `sys`; `TOP` runs `top` when given.
fn system_state(sys: &[u8], top: Option<Vec<u8>>, on_chain: u64) -> InMemoryDB {
    let mut state = db(top);
    state.insert_account_storage(&ORACLE, &U256::from(SLOT), &U256::from(on_chain));
    state.insert_account_info(&SYS, contract(sys));
    with_registry(&mut state, SYS);
    state
}

/// [`system_state`] with `SYS` writing 8 to the slot, reading it and reverting with what it read.
fn own_write_state(top: Option<Vec<u8>>, on_chain: u64) -> InMemoryDB {
    let sys = caller(&[oracle(set_slot(8)), oracle(get_slot())], End::Revert);
    system_state(&sys, top, on_chain)
}

/// A read over the transaction's own write keeps what it saw when a failed frame later takes the
/// write back: `SYS` writes 8, reads (the service answers 42) and reverts, and `TOP` returns the
/// 42. The record shows a served 42 over the transaction's own 8, which the builder's rule leaves
/// out (`SATIN-ORCL-013`): a node replaying the block pushes the write there whatever was
/// persisted before the transaction, as a replay over a slot already at 42 shows (it returns 8).
#[test]
fn oracle_service_own_write_a_failed_frame_takes_back_stays_in_the_read() {
    let seen = Arc::new(Seen::default());
    let state = own_write_state(Some(top_returning_the_sys_read()), ON_CHAIN);
    let mut evm = evm_with(state, Some(answering(42)), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 0), 42);
    assert_eq!(seen.only(), [own(read(42, 8, true, true))], "no write: it did not stand");

    let seen = Arc::new(Seen::default());
    let state = own_write_state(Some(top_returning_the_sys_read()), 42);
    let mut evm = evm_with(state, None, Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 0), 8, "the replay pushes the write, not 42");
    assert_eq!(seen.only(), [own(read(8, 8, false, true))]);
}

/// The same with the transaction's own frame failing, a transaction the block includes all the
/// same: the sender calls `SYS` directly, and its revert data carries the 42 the service answered
/// over the transaction's own 8; a replay's carries the 8.
#[test]
fn oracle_service_own_write_the_failed_transaction_takes_back_stays_in_the_read() {
    let seen = Arc::new(Seen::default());
    let state = own_write_state(None, ON_CHAIN);
    let mut evm = evm_with(state, Some(answering(42)), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, SYS, 0, Vec::new()));
    assert!(!r.status);
    assert_eq!(out(&r.output, 1), 42);
    assert_eq!(seen.only(), [own(read(42, 8, true, true))], "no write: it did not stand");

    let mut evm = evm_with(own_write_state(None, 42), None, None);
    let r = transact(&mut evm, &legacy(SENDER, 0, SYS, 0, Vec::new()));
    assert!(!r.status);
    assert_eq!(out(&r.output, 1), 8, "the replay's revert data carries the write, not 42");
}

/// Two values for one slot, the second over the transaction's own write in effect at its read and
/// taken back afterwards: `TOP` reads 7 (served), then `SYS` writes 8, reads 8 (served) and
/// reverts. Only the read without `own` asks the slot for a value before the transaction, so the
/// builder persists 7 and keeps the transaction (`SATIN-ORCL-013` leaves it out only when no
/// single value reproduces its reads); a replay over 7 with no service reproduces both reads.
#[test]
fn oracle_service_own_write_explains_the_second_of_two_values() {
    let top = || caller(&[oracle(get_slot()), call_to(SYS)], End::Return);
    let seen = Arc::new(Seen::default());
    let state = own_write_state(Some(top()), ON_CHAIN);
    let mut evm = evm_with(state, Some(recording(Ticking::new(7))), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(seen.only(), [read(7, ON_CHAIN, true, true), own(read(8, 8, true, true))]);

    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(own_write_state(Some(top()), 7), None, Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 0), 7);
    assert_eq!(
        seen.only(),
        [read(7, 7, false, true), own(read(8, 8, false, true))],
        "both reproduced"
    );
}

/// `SYS`'s code: writes 8, runs a `multiCall` that writes 9, reads it and fails, reads again, and
/// returns or reverts as `end` says, with the read in word 2 and the `multiCall`'s success in
/// word 4.
fn sys_over_a_later_write_taken_back(end: End) -> Vec<u8> {
    let failing = multi_call(&[set_slot(9), get_slot(), set_slots_mismatched()]);
    caller(&[oracle(set_slot(8)), oracle(failing), oracle(get_slot())], end)
}

/// A later write to a slot that a failed frame takes back leaves the execution's earlier write in
/// effect: `SYS` writes 8, its `multiCall` writes 9, reads it (the service answers 9) and fails,
/// and `SYS` reads again. That read is over the execution's own 8 whatever the service answers, so
/// a served 42 is left out by the builder's rule (`SATIN-ORCL-013`): a replay pushes the write,
/// over 42 persisted before the transaction as well. A served 8, the matching control, needs
/// nothing persisted, and a replay over the chain's value reproduces every read. The write of 8
/// stands and is reported, the one of 9 is not.
#[test]
fn oracle_service_own_write_stays_in_effect_when_a_later_one_is_taken_back() {
    let sys = sys_over_a_later_write_taken_back(End::Return);
    for (answer, persisted) in [(42, 42), (8, ON_CHAIN)] {
        let service = recording(InTurn::new(&[9, answer]));
        let seen = Arc::new(Seen::default());
        let state = system_state(&sys, None, ON_CHAIN);
        let mut evm = evm_with(state, Some(service.clone()), Some(seen.clone()));
        let r = transact(&mut evm, &legacy(SENDER, 0, SYS, 0, Vec::new()));
        assert!(r.status, "{:?}", r.stop);
        assert_eq!(out(&r.output, 4), 0, "the multiCall failed");
        assert_eq!(out(&r.output, 2), answer);
        assert_eq!(service.calls(), [asked(Some(9)), asked(Some(answer))]);
        assert_eq!(
            seen.only(),
            [write(8), own(read(9, 9, true, true)), own(read(answer, 8, true, true))],
            "answered {answer}"
        );

        let seen = Arc::new(Seen::default());
        let mut evm = evm_with(system_state(&sys, None, persisted), None, Some(seen.clone()));
        let r = transact(&mut evm, &legacy(SENDER, 0, SYS, 0, Vec::new()));
        assert!(r.status, "{:?}", r.stop);
        assert_eq!(out(&r.output, 2), 8, "the replay over {persisted} pushes the write");
        assert_eq!(
            seen.only(),
            [write(8), own(read(9, 9, false, true)), own(read(8, 8, false, true))]
        );
    }
}

/// The same `SYS` reverting at its end, called by `TOP`, which reads afterwards: the frame that
/// fails now takes back the write of 8, the last one of the slot in effect, and `TOP`'s read is
/// over the chain's value, no write of the execution's own (the service answers 9, 8, then 7).
/// The builder persists 7 and keeps the transaction; a replay over 7 reproduces every read.
#[test]
fn oracle_service_own_write_the_enclosing_frame_takes_back_leaves_none_in_effect() {
    let sys = sys_over_a_later_write_taken_back(End::Revert);
    let top = || caller(&[call_to(SYS), oracle(get_slot())], End::Return);
    let service = recording(InTurn::new(&[9, 8, 7]));
    let seen = Arc::new(Seen::default());
    let state = system_state(&sys, Some(top()), ON_CHAIN);
    let mut evm = evm_with(state, Some(service.clone()), Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!((out(&r.output, 2), out(&r.output, 1)), (0, 7), "SYS reverted; TOP read 7");
    assert_eq!(
        seen.only(),
        [own(read(9, 9, true, true)), own(read(8, 8, true, true)), read(7, ON_CHAIN, true, true)],
        "no write: none stood"
    );

    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(system_state(&sys, Some(top()), 7), None, Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!(out(&r.output, 1), 7);
    assert_eq!(
        seen.only(),
        [own(read(9, 9, false, true)), own(read(8, 8, false, true)), read(7, 7, false, true)],
        "every read reproduced"
    );
}

/// A contract that creates with the init code its code carries.
const FACTORY: Address = Address::with_last_byte(0xc1);

/// How the init code [`init_calling_sys`] makes ends, after its call.
#[derive(Clone, Copy, Debug)]
enum Constructor {
    /// `REVERT`.
    Reverts,
    /// Returns code that begins with `0xEF`, which the code deposit refuses (EIP-3541).
    DepositsEf,
    /// Returns the code `0x00`.
    Deploys,
    /// Returns no code, which a keyless deployment refuses.
    DepositsNothing,
}

/// Init code that calls `SYS` with no input and then ends as `end` says.
fn init_calling_sys(end: Constructor) -> Vec<u8> {
    // CALL(GAS, SYS, 0, 0, 0, 0, 0) POP
    let mut code = vec![0x5f, 0x5f, 0x5f, 0x5f, 0x5f, 0x73];
    code.extend_from_slice(SYS.as_slice());
    code.extend([0x5a, 0xf1, 0x50]);
    code.extend(match end {
        // REVERT(0, 0)
        Constructor::Reverts => vec![0x5f, 0x5f, 0xfd],
        // MSTORE8(0, 0xef) RETURN(0, 1)
        Constructor::DepositsEf => vec![0x60, 0xef, 0x5f, 0x53, 0x60, 0x01, 0x5f, 0xf3],
        // RETURN(0, 1)
        Constructor::Deploys => vec![0x60, 0x01, 0x5f, 0xf3],
        // RETURN(0, 0)
        Constructor::DepositsNothing => vec![0x5f, 0x5f, 0xf3],
    });
    code
}

/// [`FACTORY`]'s code: `CREATE` with `init`, then return or revert with the created address (zero
/// when the creation failed) in word 0.
fn creating(init: &[u8], end: End) -> Vec<u8> {
    const BODY: u8 = 19;
    let len = (init.len() as u16).to_be_bytes();
    // CODECOPY(0, BODY, len) CREATE(0, 0, len) MSTORE(0, created)
    let mut code = vec![0x61, len[0], len[1], 0x60, BODY, 0x5f, 0x39];
    code.extend([0x61, len[0], len[1], 0x5f, 0x5f, 0xf0, 0x5f, 0x52]);
    // RETURN(0, 32) or REVERT(0, 32)
    code.extend([0x60, 0x20, 0x5f, if matches!(end, End::Revert) { 0xfd } else { 0xf3 }]);
    assert_eq!(code.len(), BODY as usize);
    code.extend_from_slice(init);
    code
}

/// `TOP` reads the slot, calls [`FACTORY`] running `factory`, whose creation runs a constructor
/// that calls `SYS`, which writes 8 and reads it; then `TOP` reads again. The creation or
/// `FACTORY` fails, taking the write back. The service answers 7, then `middle` (the read over
/// the write), then 7: the record shows the reads of 7 over the chain's value and the read over
/// the execution's own 8, without the write. A served 8 there needs nothing persisted, so the
/// builder persists 7 and keeps the transaction; a served 42 leaves it out (`SATIN-ORCL-013`). A
/// replay over 7 with no service pushes 7, 8 and 7. Returns `FACTORY`'s word and success.
fn assert_creation_takes_back_the_own_write(factory: &[u8]) -> (Address, u64) {
    let sys = caller(&[oracle(set_slot(8)), oracle(get_slot())], End::Return);
    let top = || caller(&[oracle(get_slot()), call_to(FACTORY), oracle(get_slot())], End::Return);
    let state = |on_chain| {
        let mut state = system_state(&sys, Some(top()), on_chain);
        state.insert_account_info(&FACTORY, contract(factory));
        state
    };
    let mut factory_end = None;
    for middle in [8, 42] {
        let service = recording(InTurn::new(&[7, middle, 7]));
        let seen = Arc::new(Seen::default());
        let mut evm = evm_with(state(ON_CHAIN), Some(service.clone()), Some(seen.clone()));
        let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
        assert!(r.status, "{:?}", r.stop);
        assert_eq!((out(&r.output, 0), out(&r.output, 2)), (7, 7));
        assert_eq!(service.calls(), [asked(Some(7)), asked(Some(middle)), asked(Some(7))]);
        assert_eq!(
            seen.only(),
            [
                read(7, ON_CHAIN, true, true),
                own(read(middle, 8, true, true)),
                read(7, ON_CHAIN, true, true)
            ],
            "answered {middle}; no write: it did not stand"
        );
        let created = Address::from_word(B256::from_slice(&r.output[32..64]));
        factory_end = Some((created, out(&r.output, 4)));
    }

    let seen = Arc::new(Seen::default());
    let mut evm = evm_with(state(7), None, Some(seen.clone()));
    let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
    assert!(r.status, "{:?}", r.stop);
    assert_eq!((out(&r.output, 0), out(&r.output, 2)), (7, 7));
    assert_eq!(
        seen.only(),
        [read(7, 7, false, true), own(read(8, 8, false, true)), read(7, 7, false, true)],
        "every read reproduced"
    );
    factory_end.unwrap()
}

/// A constructor that fails takes back the write its call made: one that reverts, and one whose
/// code the deposit refuses after its init code ran to the end. `CREATE` pushes zero, and
/// `FACTORY` returns.
#[test]
fn oracle_service_own_write_a_failed_constructor_takes_back_stays_in_the_read() {
    for end in [Constructor::Reverts, Constructor::DepositsEf] {
        let factory = creating(&init_calling_sys(end), End::Return);
        let (created, factory_succeeded) = assert_creation_takes_back_the_own_write(&factory);
        assert_eq!((created, factory_succeeded), (Address::ZERO, 1), "{end:?}: no creation");
    }
}

/// A creation that succeeds, its write standing with it, is taken back by its caller's revert:
/// `FACTORY` creates and then reverts with the address it created.
#[test]
fn oracle_service_own_write_of_a_creation_its_caller_takes_back_stays_in_the_read() {
    let factory = creating(&init_calling_sys(Constructor::Deploys), End::Revert);
    let (created, factory_succeeded) = assert_creation_takes_back_the_own_write(&factory);
    assert_eq!((created, factory_succeeded), (FACTORY.create(1), 0), "created, then taken back");
}

/// A keyless deployment whose constructor fails takes back the write its call made: one that
/// reverts (answered `ExecutionReverted`), and one that returns no code, which the deployment's
/// code deposit refuses (`EmptyCodeDeployed`). The constructor's call has `SYS` write 8 and read
/// it: a served 8 needs nothing persisted, a served 42 leaves the transaction out, as a replay
/// over 42 with no service pushes the write.
#[test]
fn oracle_service_own_write_a_failed_keyless_creation_takes_back_stays_in_the_read() {
    let sys = caller(&[oracle(set_slot(8)), oracle(get_slot())], End::Return);
    let cases = [
        (Constructor::Reverts, "ExecutionReverted(uint64,bytes)"),
        (Constructor::DepositsNothing, "EmptyCodeDeployed(uint64)"),
    ];
    for (end, error) in cases {
        let (raw, _) = common::keyless_tx(&init_calling_sys(end));
        let input = common::keyless_deploy(&raw, 300_000).to_vec();
        for answer in [8, 42] {
            let seen = Arc::new(Seen::default());
            let state = system_state(&sys, None, ON_CHAIN);
            let mut evm = evm_with(state, Some(answering(answer)), Some(seen.clone()));
            let r = transact(&mut evm, &legacy(SENDER, 0, KEYLESS, 0, input.clone()));
            assert!(r.status, "{:?}", r.stop);
            assert_eq!(common::error_data(&r.output)[..4], selector(error), "{end:?}");
            assert_eq!(
                seen.only(),
                [own(read(answer, 8, true, true))],
                "{end:?} answered {answer}; no write: it did not stand"
            );
        }

        let seen = Arc::new(Seen::default());
        let mut evm = evm_with(system_state(&sys, None, 42), None, Some(seen.clone()));
        let r = transact(&mut evm, &legacy(SENDER, 0, KEYLESS, 0, input));
        assert!(r.status, "{:?}", r.stop);
        assert_eq!(seen.only(), [own(read(8, 8, false, true))], "{end:?}: the replay");
    }
}

// ---------------------------------------------------------------------------------------------
// The observer and the block session

/// The observer changes nothing the transaction does: the same transaction, observed or not, has
/// the same outcome.
#[test]
fn oracle_service_observer_changes_no_outcome() {
    let top = || {
        caller(
            &[oracle(get_slot()), oracle(send_hint(&[0xaa])), oracle(multi_call(&[get_slot()]))],
            End::Return,
        )
    };
    let outcome = |observer: Option<Arc<Seen>>| {
        let mut evm = evm_with(db(Some(top())), Some(answering(42)), observer);
        let r = transact(&mut evm, &legacy(SENDER, 0, TOP, 0, Vec::new()));
        (r.status, r.output.clone(), r.tx_gas_used(), r.logs.clone(), evm.ext().usage(r.status))
    };
    let seen = Arc::new(Seen::default());
    assert_eq!(outcome(Some(seen.clone())), outcome(None));
    assert_eq!(seen.only(), [read(42, ON_CHAIN, true, true), read(42, ON_CHAIN, true, true)]);
}

/// A block session hands each attempt to the observer at its end, with the index the transaction
/// takes in the block: an executed candidate at its outcome, a refused one as not executed, and a
/// candidate executed again after a refusal at the same index.
#[test]
fn oracle_service_block_session_delivers_every_attempt() {
    let service = answering(42);
    let seen = Arc::new(Seen::default());
    let oracle = OracleServices { service: Some(service.clone()), observer: Some(seen.clone()) };
    let mut block = block_reading_the_oracle(oracle);
    let first =
        block.execute_candidate(&legacy(SENDER, 0, ORACLE, 0, get_slot()), &[]).expect("executed");
    block.commit_candidate(first).expect("committed");
    let stale = legacy(SENDER, 0, ORACLE, 0, get_slot());
    assert!(block.execute_candidate(&stale, &[]).is_err(), "the nonce is spent");
    let next = legacy(SENDER, 1, ORACLE, 0, get_slot());
    block.execute_candidate(&next, &[]).expect("executed");
    block.execute_candidate(&next, &[]).expect("executed again");
    let after_pre_block = seen.all().split_off(2);
    assert_eq!(
        after_pre_block,
        [
            (
                OracleBoundary::Transaction { index: 0, executed: true },
                vec![read(42, ON_CHAIN, true, true)]
            ),
            (OracleBoundary::Transaction { index: 1, executed: false }, vec![]),
            (
                OracleBoundary::Transaction { index: 1, executed: true },
                vec![read(42, ON_CHAIN, true, true)]
            ),
            (
                OracleBoundary::Transaction { index: 1, executed: true },
                vec![read(42, ON_CHAIN, true, true)]
            ),
        ]
    );
}

/// The sequence a builder runs (see `OracleObserver`): a candidate's served read (42 over the
/// chain's 3) asks for 42 to be persisted before it; the builder executes and commits the Mega
/// System Transaction that writes it, then executes the candidate again, at the next index, where
/// the read is served 42 over a slot that holds 42 and asks for nothing more.
#[test]
fn oracle_service_block_session_executes_again_after_the_persisting_mst() {
    let system = Address::with_last_byte(0x52);
    let mut state = db(None);
    state.insert_account_info(&system, AccountInfo::default().with_balance(U256::from(1)));
    with_registry(&mut state, system);
    let seen = Arc::new(Seen::default());
    let services = OracleServices { service: Some(answering(42)), observer: Some(seen.clone()) };
    let input = common::block_input(1, 1_000, 30_000_000, Some(B256::repeat_byte(0x11)));
    let services = BlockServices { oracle: services, ..common::services(B256::ZERO) };
    let mut block =
        common::factory().start_block(&input, Db::new(state), services).expect("the block starts");
    let candidate = legacy(SENDER, 0, ORACLE, 0, get_slot());
    block.execute_candidate(&candidate, &[]).expect("executed");
    let mst = block
        .execute_candidate(&legacy(system, 0, ORACLE, 0, set_slot(42)), &[])
        .expect("executed");
    assert!(mst.is_system_transaction());
    block.commit_candidate(mst).expect("committed");
    let again = block.execute_candidate(&candidate, &[]).expect("executed again");
    assert_eq!(out(again.result().output.as_ref(), 0), 42);
    block.commit_candidate(again).expect("committed");
    let transactions = seen
        .all()
        .into_iter()
        .filter(|(b, _)| matches!(b, OracleBoundary::Transaction { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        transactions,
        [
            (
                OracleBoundary::Transaction { index: 0, executed: true },
                vec![read(42, ON_CHAIN, true, true)]
            ),
            (OracleBoundary::Transaction { index: 0, executed: true }, vec![write(42)]),
            (
                OracleBoundary::Transaction { index: 1, executed: true },
                vec![read(42, 42, true, true)]
            ),
        ]
    );
}

/// An account whose read the database cannot serve.
const BROKEN: Address = Address::with_last_byte(0xdd);

#[derive(Debug)]
struct Unavailable;

impl core::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("account unavailable")
    }
}

impl std::error::Error for Unavailable {}

/// A database that fails every read of [`BROKEN`]'s account and serves the rest from `inner`.
#[derive(Debug)]
struct Broken(InMemoryDB);

impl Database for Broken {
    type Error = DatabaseError;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        if *address == BROKEN {
            return Err(DatabaseError::new(Unavailable, true));
        }
        self.0.get_account(address)
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        self.0.get_code_by_hash(code_hash)
    }

    fn get_storage(&mut self, address: &Address, key: &Word) -> Result<Word, Self::Error> {
        self.0.get_storage(address, key)
    }

    fn get_block_hash(&mut self, number: &Word) -> Result<B256, Self::Error> {
        self.0.get_block_hash(number)
    }
}

/// An attempt that fails with an error reaches no outcome: the block session hands it over as not
/// executed, with the reads it made and none of its writes, though the system address's code had
/// written 8 in a frame that returned before the error (a database that cannot serve the next
/// account the transaction calls).
#[test]
fn oracle_service_attempt_failing_with_an_error_reports_no_write() {
    let sys = caller(&[oracle(set_slot(8))], End::Return);
    let call = |to| Call { to, value: 0, gas: None, input: Vec::new() };
    let top = caller(&[call(SYS), oracle(get_slot()), call(BROKEN)], End::Return);
    let mut state = db(Some(top));
    state.insert_account_info(&SYS, contract(&sys));
    with_registry(&mut state, SYS);
    let seen = Arc::new(Seen::default());
    let oracle = OracleServices { service: None, observer: Some(seen.clone()) };
    let input = common::block_input(1, 1_000, 30_000_000, Some(B256::repeat_byte(0x11)));
    let services = BlockServices { oracle, ..common::services(B256::ZERO) };
    let mut block = common::factory()
        .start_block(&input, Db::new(Broken(state)), services)
        .expect("the block starts");
    let error = block.execute_candidate(&legacy(SENDER, 0, TOP, 0, Vec::new()), &[]);
    assert!(error.is_err(), "the read of the broken account fails the attempt");
    let last = seen.all().pop().expect("the attempt handed over");
    assert_eq!(
        last,
        (
            OracleBoundary::Transaction { index: 0, executed: false },
            vec![own(read(8, 8, false, true))]
        )
    );
}
