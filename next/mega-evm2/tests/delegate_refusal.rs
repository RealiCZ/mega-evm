//! A call refused at its target's EIP-7702 delegate in a frame whose volatile-data access is off
//! (`SATIN-MAC-003`, `SATIN-MAC-004`, `SATIN-MAC-007`).
//!
//! A call whose target operand is the block beneficiary is refused before it charges anything. A
//! call whose target operand is not makes its steps as with access on — its static cost, the
//! static-context check, its memory expansion, the value-transfer cost, the load of its target's
//! account and code (skipped when the target is cold and the whole regular gas cannot pay its cold
//! surcharge), the delegation guard — and is refused at its delegate's load when that delegate is
//! the beneficiary. It then keeps its static cost and its memory expansion charged, gets the
//! value-transfer cost back, and pays its target's cold surcharge, last, when the target was not
//! warm; that surcharge may be gas detention's crossing, a stop with `MegaLimitExceeded(2, limit)`
//! (`SATIN-OUT-008`, `SATIN-STOP-020`, `SATIN-DET-018`). The frame's end rolls EIP-2929's warmth
//! back to the frame's start, and the completed read of the target stays in the read set and the
//! witness (`SATIN-WIT-012`). A precompile address holding a delegation to the beneficiary is
//! refused before dispatch (`SATIN-PCMP-008`).
//!
//! Every case runs the call in a child frame `F` of a frame that switched its volatile-data access
//! off; "left at the call" is the regular gas `F` holds when it reaches the call opcode. The
//! figures are those of the proposal's conformance cases 1, 2, 3, 4(b), 5 and 5b, and the
//! specification's charges: `WARM_STORAGE_READ_COST` = 100, `COLD_ACCOUNT_ACCESS` = 3,000,
//! `VALUE_TRANSFER_COST` = 9,000, and memory at `3·w + w²/512` for `w` words.
//!
//! The conformance suite's `satin/access-control/delegate-refusal/*` and
//! `satin/detention/delegate-refusal/*` cases hold the direct refusals, the guard's edges, the
//! memory, the crossings and the warmth a frame takes or finds; the tests here hold what no case
//! observes yet: the target read after a refused `CALL`, `CALLCODE` or `DELEGATECALL`, the read
//! of an access-listed target, the coldness after a reverted warming, the precompile control and
//! the witness.

mod witness_fixture;

use alloy_consensus::{SignableTransaction, TxEip2930, TxLegacy, transaction::Recovered};
use alloy_eips::eip2930::{AccessList, AccessListItem};
use alloy_primitives::{Address, Bytes, Signature, TxKind, U256, keccak256};
use evm2::{
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    inspector::Inspector,
    interpreter::{InstrStop, Interpreter, Message, MessageResult},
};
use mega_evm2::{
    MegaTypes, TxLimits, factory::Simulation, oracle::OracleServices, witness::Evidence,
};
use op_evm2::OpTxEnvelope;
use witness_fixture::{Produced, Tx, asm, common};

const SENDER: Address = Address::with_last_byte(0x50);
/// Switches its volatile-data access off, then calls [`F`].
const TOP: Address = Address::with_last_byte(0xa0);
/// Runs the call under test.
const F: Address = Address::with_last_byte(0xc0);
/// Takes [`DELEGATOR`]'s balance, then reverts.
const WARMER: Address = Address::with_last_byte(0xc1);
/// Holds the EIP-7702 delegation indicator `0xef0100 ‖ BENEFICIARY`.
const DELEGATOR: Address = Address::with_last_byte(0xd0);
const BENEFICIARY: Address = Address::with_last_byte(0xbe);
/// The IDENTITY precompile.
const IDENTITY: Address = Address::with_last_byte(0x04);
const ACCESS_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04]);

const PUSH0: u8 = 0x5f;
const STOP: u8 = 0x00;
const CALL: u8 = 0xf1;
const CALLCODE: u8 = 0xf2;
const DELEGATECALL: u8 = 0xf4;
const STATICCALL: u8 = 0xfa;

// ---------------------------------------------------------------------------------------------
// Programs

/// The gas `PUSH`ing `v` costs: `PUSH0` 2, any other push 3.
fn push_cost(v: u64) -> u64 {
    if v == 0 { 2 } else { 3 }
}

/// A call opcode `op` of `to` with a gas operand of 0, `value` (for `CALL` and `CALLCODE`), the
/// input range `0..input` and the output range `0..output`; with the gas its pushes cost.
fn call_op(op: u8, to: Address, value: u64, input: u64, output: u64) -> (Vec<u8>, u64) {
    let mut code = [asm::push(output), vec![PUSH0], asm::push(input), vec![PUSH0]].concat();
    let mut pushes = push_cost(output) + 2 + push_cost(input) + 2;
    if op == CALL || op == CALLCODE {
        code.extend(asm::push(value));
        pushes += push_cost(value);
    }
    code.extend(asm::push_address(to));
    code.extend([PUSH0, op, STOP]);
    (code, pushes + 3 + 2)
}

/// `STATICCALL(0, to, 0, 0, 0, 0)`: 13 of gas in pushes.
fn staticcall(to: Address) -> (Vec<u8>, u64) {
    call_op(STATICCALL, to, 0, 0, 0)
}

/// How [`TOP`] runs a case.
#[derive(Clone, Debug, Default)]
struct Setup {
    /// [`F`]'s code, and the gas [`TOP`] forwards to it.
    f_code: Vec<u8>,
    f_gas: u64,
    /// Gas detention: [`TOP`] reads `TIMESTAMP` first, under this block-environment cap.
    cap: Option<u64>,
    /// [`TOP`] takes [`DELEGATOR`]'s balance before it switches its access off.
    warm_in_top: bool,
    /// [`TOP`] calls [`WARMER`], whose warming of [`DELEGATOR`] its revert takes back.
    warm_in_reverted_frame: bool,
    /// The transaction's access list holds [`DELEGATOR`].
    access_list: bool,
    /// After [`F`], [`TOP`] measures what `BALANCE(DELEGATOR)` costs it (not under detention:
    /// the cold read would cross the limit).
    probe: bool,
    /// The transaction's gas limit (1,000,000 when zero).
    tx_gas: u64,
}

/// [`TOP`]'s code: `TIMESTAMP` under detention, the warmings asked for, then
/// `disableVolatileDataAccess()`, the call of [`F`], and the return of
/// `(F's success, the probe's gas, F's return data)`.
fn top(setup: &Setup) -> Vec<u8> {
    let mut code = Vec::new();
    if setup.cap.is_some() {
        code.extend([0x42, 0x50]);
    }
    if setup.warm_in_top {
        code.extend(asm::balance(DELEGATOR));
    }
    if setup.warm_in_reverted_frame {
        code.extend(asm::call(WARMER, Some(50_000), 0));
    }
    // PUSH4 selector, PUSH1 0xe0, SHL, PUSH0, MSTORE (17 of gas, memory included); then
    // CALL(100_000, ACCESS_CONTROL, 0, 0, 4, 0, 0) (17 in pushes, 100 and 2,900 cold), POP (2).
    code.push(0x63);
    code.extend_from_slice(&keccak256("disableVolatileDataAccess()")[..4]);
    code.extend([0x60, 0xe0, 0x1b, PUSH0, 0x52]);
    code.extend([PUSH0, PUSH0, 0x60, 0x04, PUSH0, PUSH0]);
    code.extend(asm::push_address(ACCESS_CONTROL));
    code.extend([0x62, 0x01, 0x86, 0xa0, CALL, 0x50]);
    // CALL(f_gas, F, 0, 0, 0, 0, 0) (16 in pushes, 100 and 2,900 cold); its success at word 0.
    code.extend([PUSH0, PUSH0, PUSH0, PUSH0, PUSH0]);
    code.extend(asm::push_address(F));
    code.push(0x63);
    code.extend_from_slice(&u32::try_from(setup.f_gas).expect("a 4-byte gas").to_be_bytes());
    code.extend([CALL, PUSH0, 0x52]);
    if setup.probe {
        // GAS, BALANCE(DELEGATOR), POP, GAS: the difference at word 1 is BALANCE's charge + 7.
        code.push(0x5a);
        code.extend(asm::balance(DELEGATOR));
        code.extend([0x5a, 0x90, 0x03, 0x60, 0x20, 0x52]);
    }
    // RETURNDATACOPY(64, 0, RETURNDATASIZE); RETURN(0, 64 + RETURNDATASIZE).
    code.extend([0x3d, PUSH0, 0x60, 0x40, 0x3e, 0x3d, 0x60, 0x40, 0x01, PUSH0, 0xf3]);
    code
}

fn delegation() -> Bytecode {
    Bytecode::new_eip7702(BENEFICIARY)
}

/// The chain the cases run on: [`TOP`], [`F`], [`WARMER`] and [`DELEGATOR`].
fn chain(setup: &Setup) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&TOP, common::contract(&top(setup)));
    db.insert_account_info(&F, common::contract(&setup.f_code));
    let warmer = [asm::balance(DELEGATOR), vec![PUSH0, PUSH0, 0xfd]].concat();
    db.insert_account_info(&WARMER, common::contract(&warmer));
    db.insert_account_info(
        &DELEGATOR,
        AccountInfo::default().with_nonce(1).with_code(delegation()),
    );
    db
}

// ---------------------------------------------------------------------------------------------
// Running a case

/// How a frame ended, as the inspector saw it.
#[derive(Clone, Debug)]
struct End {
    depth: u16,
    gas_limit: u64,
    stop: InstrStop,
    /// The gas the frame handed back.
    gas_left: u64,
    output: Bytes,
}

/// Records how each call frame ended.
#[derive(Debug, Default)]
struct Ends(Vec<End>);

impl Inspector<MegaTypes> for Ends {
    fn call_end(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        m: &Message<MegaTypes>,
        r: &mut MessageResult<MegaTypes>,
    ) {
        self.0.push(End {
            depth: m.depth,
            gas_limit: m.gas_limit,
            stop: r.stop,
            gas_left: r.gas.remaining(),
            output: r.output.clone(),
        });
    }
}

/// What a case left.
#[derive(Debug)]
struct Outcome {
    /// How [`F`] ended.
    f: End,
    /// Whether the transaction's read set holds [`DELEGATOR`], in a run without the probe (which
    /// reads it).
    read: bool,
    /// What `BALANCE(DELEGATOR)` cost [`TOP`] after [`F`], when probed.
    balance_after: Option<u64>,
}

impl Outcome {
    /// What `BALANCE(DELEGATOR)` cost [`TOP`] after [`F`].
    fn balance_after(&self) -> u64 {
        self.balance_after.expect("a probed case")
    }

    /// [`F`] halted: no return data and none of its gas handed back.
    fn assert_f_halted(&self) {
        assert!(self.f.stop.is_halt(), "{self:?}");
        assert_eq!(self.f.gas_left, 0, "{self:?}");
        assert!(self.f.output.is_empty(), "{self:?}");
    }

    /// [`F`] was refused with `VolatileDataAccessDisabled(10)`, handing back `left`.
    fn assert_refused(&self, left: u64) {
        assert_eq!(self.f.stop, InstrStop::Revert, "{self:?}");
        assert_eq!(self.f.output.as_ref(), refusal().as_slice(), "{self:?}");
        assert_eq!(self.f.gas_left, left, "{self:?}");
    }
}

/// `VolatileDataAccessDisabled(10)`.
fn refusal() -> Vec<u8> {
    common::error_u64("VolatileDataAccessDisabled(uint8)", 10)
}

fn block() -> BlockEnv<MegaTypes> {
    BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(10_000_000_000u64),
        beneficiary: BENEFICIARY,
        ..BlockEnv::<MegaTypes>::default()
    }
}

/// Runs `setup` on its chain, changed by `edit`: without the probe, for the read set and `F`'s
/// frame, then, when `setup` asks for it, with the probe, whose `F` must end the same.
fn run_on(setup: &Setup, edit: impl Fn(&mut InMemoryDB)) -> Outcome {
    let mut out = execute(&Setup { probe: false, ..setup.clone() }, &edit).0;
    if setup.probe {
        let (probed, output) = execute(setup, &edit);
        assert_eq!(
            (probed.f.stop, probed.f.gas_left, &probed.f.output),
            (out.f.stop, out.f.gas_left, &out.f.output)
        );
        let probe = U256::from_be_slice(&output[32..64]).to::<u64>();
        out.balance_after = Some(probe - 7);
    }
    out
}

/// One run of `setup` on its chain, changed by `edit`.
fn execute(setup: &Setup, edit: &impl Fn(&mut InMemoryDB)) -> (Outcome, Bytes) {
    let mut db = chain(setup);
    edit(&mut db);
    let mut limits = TxLimits::SATIN;
    if let Some(cap) = setup.cap {
        limits.cap = cap;
    }
    let simulation = Simulation { limits: Some(limits), ..Simulation::default() };
    let mut evm = common::simulated_evm(block(), Db::new(db), &simulation);
    let gas_limit = if setup.tx_gas == 0 { 1_000_000 } else { setup.tx_gas };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let envelope = if setup.access_list {
        let item = AccessListItem { address: DELEGATOR, storage_keys: Vec::new() };
        let tx = TxEip2930 {
            chain_id: common::CHAIN_ID,
            nonce: 0,
            gas_price: 0,
            gas_limit,
            to: TxKind::Call(TOP),
            value: U256::ZERO,
            access_list: AccessList(vec![item]),
            input: Bytes::new(),
        };
        OpTxEnvelope::from_signed(tx.into_signed(signature))
    } else {
        let tx = TxLegacy {
            chain_id: Some(common::CHAIN_ID),
            nonce: 0,
            gas_price: 0,
            gas_limit,
            to: TxKind::Call(TOP),
            value: U256::ZERO,
            input: Bytes::new(),
        };
        OpTxEnvelope::from_signed(tx.into_signed(signature))
    };
    let tx = Recovered::new_unchecked(envelope, SENDER);
    evm.set_inspector(Ends::default());
    let out = evm.transact(&tx).expect("an outcome").detach();
    let ends = evm.clear_inspector_as::<Ends>().expect("the inspector").0;
    // Under detention a frame that spent its allowance leaves TOP none for its own tail.
    if setup.cap.is_none() {
        assert!(out.result.status, "TOP returns: {:?}", out.result);
    }
    let f =
        ends.into_iter().find(|e| e.depth == 1 && e.gas_limit == setup.f_gas).expect("F's frame");
    let read = out.pending_state.account_changes_for(&DELEGATOR).is_some();
    (Outcome { f, read, balance_after: None }, out.result.output.clone())
}

fn run(setup: &Setup) -> Outcome {
    run_on(setup, |_| {})
}

/// [`F`] making `call` (its code, its pushes' gas) with `left` at the call.
fn with_left(call: (Vec<u8>, u64), left: u64) -> Setup {
    Setup { f_code: call.0, f_gas: left + call.1, probe: true, ..Setup::default() }
}

// ---------------------------------------------------------------------------------------------
// A cold delegator: 3,100 left at the call (case 2)

/// 3,100: the call reaches its delegate's load and is refused: it pays 100 and the target's cold
/// 2,900, 3,000 in all, and hands back 100; the target was read, and is cold again for `F`'s
/// caller, as the revert took the call's warming back. Each of the four call opcodes, with no
/// value: nothing is given back that was not charged.
#[test]
fn delegate_refusal_cold_delegator_3100_pays_its_targets_cold_access() {
    for op in [STATICCALL, CALL, CALLCODE, DELEGATECALL] {
        let out = run(&with_left(call_op(op, DELEGATOR, 0, 0, 0), 3_100));
        out.assert_refused(100);
        assert!(out.read);
        assert_eq!(out.balance_after(), 3_000);
    }
}

// ---------------------------------------------------------------------------------------------
// A warm delegator: 200 left at the call (case 3)

/// 200: refused, paying the warm 100 alone; the access list's warmth stays.
#[test]
fn delegate_refusal_warm_delegator_200_pays_its_targets_warm_access() {
    let setup = Setup { access_list: true, ..with_left(staticcall(DELEGATOR), 200) };
    let out = run(&setup);
    out.assert_refused(100);
    assert!(out.read);
    assert_eq!(out.balance_after(), 100);
}

// ---------------------------------------------------------------------------------------------
// Value (case 4(b))

/// Case 4(b): `CALL(0, DELEGATOR, 1, 0, 0, 0, 0)` with 15,000 left, from an `F` that holds no
/// balance: the call pays the 9,000 value-transfer cost and reaches its delegate before any
/// balance check, is refused, gets the 9,000 back and pays 3,000: 12,000 handed back. A
/// `CALLCODE` the same.
#[test]
fn delegate_refusal_value_transfer_cost_is_given_back() {
    for op in [CALL, CALLCODE] {
        let out = run(&with_left(call_op(op, DELEGATOR, 1, 0, 0), 15_000));
        out.assert_refused(12_000);
        assert!(out.read);
    }
}

// ---------------------------------------------------------------------------------------------
// Warmth

/// A warming a reverted frame took back does not count: [`WARMER`] reads the delegator's balance
/// and reverts, so the call finds its target cold and pays 3,000.
#[test]
fn delegate_refusal_warmth_a_revert_took_back_does_not_count() {
    let setup = Setup { warm_in_reverted_frame: true, ..with_left(staticcall(DELEGATOR), 3_100) };
    let out = run(&setup);
    out.assert_refused(100);
    assert_eq!(out.balance_after(), 3_000);
}

// ---------------------------------------------------------------------------------------------
// Precompile addresses (`SATIN-PCMP-008`)

/// IDENTITY's address holding a delegation to the beneficiary: a call of it reaches its delegate's
/// load, never dispatch, and is refused (200 left: the precompile is warm from the transaction's
/// start, so 100 is paid; 199 halts at the guard). Without the delegation the same call reaches
/// dispatch and runs the precompile (which its gas operand of 0 cannot pay), and `F` stops.
#[test]
fn delegate_refusal_precompile_address_is_refused_before_dispatch() {
    let delegated = |db: &mut InMemoryDB| {
        db.insert_account_info(&IDENTITY, AccountInfo::default().with_code(delegation()))
    };
    let out = run_on(&with_left(staticcall(IDENTITY), 200), delegated);
    out.assert_refused(100);
    run_on(&with_left(staticcall(IDENTITY), 199), delegated).assert_f_halted();
    let out = run(&with_left(staticcall(IDENTITY), 200));
    assert_eq!(out.f.stop, InstrStop::Stop, "{out:?}");
}

// ---------------------------------------------------------------------------------------------
// The witness (`SATIN-WIT-012`)

/// A block whose transaction switches its access off and calls [`F`] with `f_gas`, `F` making
/// `STATICCALL(0, DELEGATOR, 0, 0, 0, 0)`; paid to [`BENEFICIARY`].
fn produce_case(f_gas: u64) -> Produced {
    let mut parent = witness_fixture::chain(witness_fixture::L1::Present);
    let mut word = [0u8; 32];
    word[..4].copy_from_slice(&keccak256("disableVolatileDataAccess()")[..4]);
    let caller = [
        asm::mstore32(0, word),
        asm::call(ACCESS_CONTROL, Some(100_000), 4),
        asm::call(F, Some(f_gas), 0),
        vec![STOP],
    ]
    .concat();
    parent.insert_account_info(&TOP, witness_fixture::contract(caller));
    parent.insert_account_info(&F, witness_fixture::contract(staticcall(DELEGATOR).0));
    parent.insert_account_info(
        &DELEGATOR,
        AccountInfo::default().with_nonce(1).with_code(delegation()),
    );
    let mut input = witness_fixture::input();
    input.header.beneficiary = BENEFICIARY;
    let txs: [Tx; 1] = [witness_fixture::call(witness_fixture::SENDER, 0, TOP)];
    witness_fixture::produce_block(
        common::factory(),
        input,
        &parent,
        Db::new(parent.clone()),
        OracleServices::default(),
        &witness_fixture::included(&txs),
    )
}

/// A call refused at its delegate (3,100 left) and one halted by the guard (3,050) keep their
/// completed read of the target's account and code in the witness, which the replay needs; one
/// whose cold load was skipped (2,999) leaves the target out of it.
#[test]
fn delegate_refusal_witness_keeps_the_completed_target_read() {
    for (left, kept) in [(3_100, true), (3_050, true), (2_999, false)] {
        let built = produce_case(left + 13);
        assert!(built.output.receipts[0].status);
        assert_eq!(built.witness.account(&DELEGATOR).is_some(), kept, "{left}");
        let read = witness_fixture::assert_exact_replay(&built);
        assert_eq!(read.contains(&Evidence::Account(DELEGATOR)), kept, "{left}");
        assert_eq!(read.contains(&Evidence::Code(delegation().hash_slow())), kept, "{left}");
    }
}

#[test]
fn delegate_refusal_detention_crossing_keeps_target_in_pending_state() {
    let (f_code, pushes) = staticcall(DELEGATOR);
    // Leave 200 spendable after the access switch, the call into F and its pushes.
    // The effective limit adds the 2 gas spent at TIMESTAMP back to the cap.
    let cap = 200 + pushes + 4 + 3_036 + 3_016 - 2;
    let out = run(&Setup { f_code, f_gas: 5_200 + pushes, cap: Some(cap), ..Setup::default() });

    let mut error = common::error_u64("MegaLimitExceeded(uint8,uint64)", 2);
    error.extend_from_slice(&U256::from(cap + 2).to_be_bytes::<32>());
    assert_eq!(out.f.stop, InstrStop::Revert);
    assert_eq!(out.f.output.as_ref(), error.as_slice());
    assert_eq!(out.f.gas_left, 5_100);
    assert!(out.read, "the crossing must preserve the delegator in pending state");
}

#[test]
fn delegate_refusal_surcharge_crossing_after_memory_keeps_target_in_pending_state() {
    let (f_code, pushes) = call_op(STATICCALL, DELEGATOR, 0, 0, 1_024);
    // The static cost and 98 gas for memory fit within the 300 spendable gas.
    let cap = 300 + pushes + 4 + 3_036 + 3_016 - 2;
    let out = run(&Setup { f_code, f_gas: 5_200 + pushes, cap: Some(cap), ..Setup::default() });

    let mut error = common::error_u64("MegaLimitExceeded(uint8,uint64)", 2);
    error.extend_from_slice(&U256::from(cap + 2).to_be_bytes::<32>());
    assert_eq!(out.f.stop, InstrStop::Revert);
    assert_eq!(out.f.output.as_ref(), error.as_slice());
    assert_eq!(out.f.gas_left, 5_200 - 100 - 98);
    assert!(out.read, "the surcharge crossing must preserve the delegator in pending state");
}

#[test]
fn delegate_refusal_memory_crossing_leaves_target_out_of_pending_state() {
    let (f_code, pushes) = call_op(STATICCALL, DELEGATOR, 0, 0x32_0000, 0);
    assert_eq!(pushes, 14);
    let cap = TxLimits::SATIN.cap;
    let setup =
        Setup { f_code, f_gas: 40_000_000, cap: Some(cap), tx_gas: 50_000_000, ..Setup::default() };
    let out = run(&setup);

    let mut error = common::error_u64("MegaLimitExceeded(uint8,uint64)", 2);
    error.extend_from_slice(&U256::from(cap + 2).to_be_bytes::<32>());
    assert_eq!(out.f.stop, InstrStop::Revert);
    assert_eq!(out.f.output.as_ref(), error.as_slice());
    assert_eq!(out.f.gas_left, 40_000_000 - pushes - 100);
    assert!(!out.read, "the memory crossing must leave the delegator out of pending state");
}
