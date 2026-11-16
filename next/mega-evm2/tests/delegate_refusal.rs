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

/// The compute [`TOP`] has made when [`F`] starts, under detention: `TIMESTAMP` and `POP` (4),
/// the access switch (17 + 17 + 3,000 + 2) and the call of [`F`] (16 + 3,000).
const COMPUTE_AT_F: u64 = 4 + 3_036 + 3_016;

/// The block-environment cap that leaves [`F`] `spendable` of regular gas when it reaches its call
/// after `pushes` of gas: the limit is the compute at `TIMESTAMP` (2) plus the cap.
const fn cap_for(spendable: u64, pushes: u64) -> u64 {
    spendable + pushes + COMPUTE_AT_F - 2
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

    /// [`F`] was stopped with `MegaLimitExceeded(2, limit)`, handing back `left`.
    fn assert_stopped(&self, limit: u64, left: u64) {
        assert_eq!(self.f.stop, InstrStop::Revert, "{self:?}");
        assert_eq!(self.f.output.as_ref(), limit_stop(limit).as_slice(), "{self:?}");
        assert_eq!(self.f.gas_left, left, "{self:?}");
    }
}

/// `VolatileDataAccessDisabled(10)`.
fn refusal() -> Vec<u8> {
    common::error_u64("VolatileDataAccessDisabled(uint8)", 10)
}

/// `MegaLimitExceeded(2, limit)`.
fn limit_stop(limit: u64) -> Vec<u8> {
    let mut stop = keccak256("MegaLimitExceeded(uint8,uint64)")[..4].to_vec();
    stop.extend_from_slice(&U256::from(2).to_be_bytes::<32>());
    stop.extend_from_slice(&U256::from(limit).to_be_bytes::<32>());
    stop
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
// Direct refusals stay free

/// Case 1: `F` holds 50 at `CALL(0, BENEFICIARY, 1, 0, 1 MiB, 0, 0)`, a call it could fund neither
/// in value nor in memory: refused before anything, all 50 handed back (`SATIN-MAC-003`,
/// `SATIN-MAC-004`).
#[test]
fn delegate_refusal_direct_beneficiary_call_is_refused_for_nothing() {
    let call = call_op(CALL, BENEFICIARY, 1, 0x10_0000, 0);
    let out = run(&with_left(call, 50));
    out.assert_refused(50);
    // A `DELEGATECALL` of the beneficiary too, below its static cost.
    let out = run(&with_left(call_op(DELEGATECALL, BENEFICIARY, 0, 0, 0), 10));
    out.assert_refused(10);
}

// ---------------------------------------------------------------------------------------------
// A cold delegator: 2,999 / 3,050 / 3,100 left at the call (case 2)

/// 2,999: the static cost leaves 2,899, below the cold surcharge, so the target's load is skipped:
/// `F` halts and reads nothing.
#[test]
fn delegate_refusal_cold_delegator_2999_halts_without_reading_the_target() {
    let out = run(&with_left(staticcall(DELEGATOR), 2_999));
    out.assert_f_halted();
    assert!(!out.read, "a skipped cold load reads nothing");
    assert_eq!(out.balance_after(), 3_000);
}

/// 3,050: the target is loaded (2,950 cover its 2,900), then the delegation guard (2,900 + 100)
/// halts the call: `F` halts, its target read.
#[test]
fn delegate_refusal_cold_delegator_3050_halts_at_the_guard_after_reading_the_target() {
    let out = run(&with_left(staticcall(DELEGATOR), 3_050));
    out.assert_f_halted();
    assert!(out.read, "the target's load was made");
}

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
// A warm delegator: 199 / 200 left at the call (case 3)

/// 199: the static cost leaves 99, below the guard's 100: `F` halts.
#[test]
fn delegate_refusal_warm_delegator_199_halts_at_the_guard() {
    let setup = Setup { access_list: true, ..with_left(staticcall(DELEGATOR), 199) };
    run(&setup).assert_f_halted();
}

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
// Value (case 4(b)) and memory

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

/// Case 4(a): `CALLCODE(0, DELEGATOR, 0, 0, 1 MiB, 0, 0)` with 5,000 left halts on its memory
/// expansion, before its target's load: nothing read.
#[test]
fn delegate_refusal_memory_expansion_halts_before_the_target_is_read() {
    let out = run(&with_left(call_op(CALLCODE, DELEGATOR, 0, 0x10_0000, 0), 5_000));
    out.assert_f_halted();
    assert!(!out.read);
}

/// The call's own memory expansion stays charged: an output range of 1,024 bytes (32 words: 96 +
/// 2 = 98) on a warm delegator costs 100 + 98; with value and a cold delegator 100 + 98 + 2,900,
/// the 9,000 given back; input and output ranges of 1,024 and 2,048 bytes grow memory to 64 words
/// together (192 + 8 = 200), 100 + 200.
#[test]
fn delegate_refusal_memory_stays_charged() {
    let warm = |call, left| Setup { access_list: true, ..with_left(call, left) };
    let out = run(&warm(call_op(STATICCALL, DELEGATOR, 0, 0, 1_024), 10_000));
    out.assert_refused(10_000 - 100 - 98);
    let out = run(&with_left(call_op(CALL, DELEGATOR, 1, 0, 1_024), 20_000));
    out.assert_refused(20_000 - 100 - 98 - 2_900);
    let out = run(&warm(call_op(STATICCALL, DELEGATOR, 0, 1_024, 2_048), 10_000));
    out.assert_refused(10_000 - 100 - 200);
}

/// A range already within the frame's memory adds nothing: `F` first stores a word at 992 (memory
/// to 32 words, 106 of gas with its pushes), then the call with an output range of 1,024 bytes on
/// a warm delegator pays its 100 alone.
#[test]
fn delegate_refusal_memory_stays_charged_and_held_memory_costs_nothing() {
    let (call, pushes) = call_op(STATICCALL, DELEGATOR, 0, 0, 1_024);
    let code = [vec![PUSH0, 0x61, 0x03, 0xe0, 0x52], call].concat();
    let setup = Setup {
        f_code: code,
        f_gas: 106 + pushes + 10_000,
        access_list: true,
        probe: true,
        ..Setup::default()
    };
    run(&setup).assert_refused(10_000 - 100);
}

// ---------------------------------------------------------------------------------------------
// Gas detention (cases 5 and 5b)

/// Case 5b: `F` holds 200 spendable and 5,000 withheld at `STATICCALL(0, DELEGATOR, 0, 0, 0, 0)`
/// with a cold delegator: the whole 5,100 left after the static cost loads the target and passes
/// the guard; refused at the delegate, the call's 2,900 cold surcharge is the crossing: `F` stops
/// with `MegaLimitExceeded(2, limit)` and hands back 5,100, the withheld part included, its target
/// read. Spendable 3,000 at the call pays the surcharge: refused, 3,000 spent, 2,200 back; 2,999
/// crosses.
#[test]
fn delegate_refusal_detention_crossing_on_the_cold_surcharge() {
    let (call, pushes) = staticcall(DELEGATOR);
    let detained = |spendable| {
        let cap = cap_for(spendable, pushes);
        let setup = Setup {
            f_code: call.clone(),
            f_gas: 5_200 + pushes,
            cap: Some(cap),
            ..Setup::default()
        };
        (run(&setup), cap + 2)
    };
    let (out, limit) = detained(200);
    out.assert_stopped(limit, 5_100);
    assert!(out.read, "the target's read stays when its surcharge crosses");
    let (out, limit) = detained(2_999);
    out.assert_stopped(limit, 5_100);
    let (out, _) = detained(3_000);
    out.assert_refused(5_200 - 3_000);
    assert!(out.read);
}

/// The crossing comes after the memory expansion is charged again, so memory stays charged at the
/// stop: an output range of 1,024 bytes (98) with 300 spendable at the call stops on the cold
/// surcharge, handing back 5,200 − 100 − 98.
#[test]
fn delegate_refusal_detention_crossing_keeps_the_memory_charged() {
    let (call, pushes) = call_op(STATICCALL, DELEGATOR, 0, 0, 1_024);
    let cap = cap_for(300, pushes);
    let setup = Setup { f_code: call, f_gas: 5_200 + pushes, cap: Some(cap), ..Setup::default() };
    let out = run(&setup);
    out.assert_stopped(cap + 2, 5_200 - 100 - 98);
    assert!(out.read);
}

/// Case 5: under the default cap, `F` forwarded 40,000,000 runs `STATICCALL(0, DELEGATOR, 0,
/// 0x320000, 0, 0)` (14 in pushes): its 20,787,200 of memory is more than its spendable part and
/// less than its whole gas, so the expansion is the crossing: `F` stops and hands back the whole
/// less the static cost, and its target is never read.
#[test]
fn delegate_refusal_detention_crossing_on_memory_reads_nothing() {
    let (call, pushes) = call_op(STATICCALL, DELEGATOR, 0, 0x32_0000, 0);
    assert_eq!(pushes, 14);
    let setup = Setup {
        f_code: call,
        f_gas: 40_000_000,
        cap: Some(TxLimits::SATIN.cap),
        tx_gas: 50_000_000,
        ..Setup::default()
    };
    let out = run(&setup);
    out.assert_stopped(TxLimits::SATIN.cap + 2, 40_000_000 - pushes - 100);
    assert!(!out.read, "the call stopped before its target's load");
}

// ---------------------------------------------------------------------------------------------
// Warmth

/// The frame's end rolls warmth back to its start: `F` warms the delegator itself (`BALANCE`, its
/// 3,005 with the push and the pop), so its call finds the target warm and pays 100; `F`'s revert
/// then takes that warming back, and the delegator is cold for `F`'s caller.
#[test]
fn delegate_refusal_warmth_taken_in_the_frame_rolls_back() {
    let (call, pushes) = staticcall(DELEGATOR);
    let code = [asm::balance(DELEGATOR), call].concat();
    let setup =
        Setup { f_code: code, f_gas: 3_005 + pushes + 1_000, probe: true, ..Setup::default() };
    let out = run(&setup);
    out.assert_refused(1_000 - 100);
    assert_eq!(out.balance_after(), 3_000);
}

/// A warmth from before the frame started stays: [`TOP`] reads the delegator's balance first, so
/// the call pays 100, and the delegator is warm for [`TOP`] after `F`'s revert.
#[test]
fn delegate_refusal_warmth_from_before_the_frame_stays() {
    let setup = Setup { warm_in_top: true, ..with_left(staticcall(DELEGATOR), 200) };
    let out = run(&setup);
    out.assert_refused(100);
    assert_eq!(out.balance_after(), 100);
}

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
