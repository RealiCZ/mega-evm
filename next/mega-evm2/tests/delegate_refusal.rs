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
//! The engine tests here check refusal before precompile dispatch and retention of the completed
//! target read in a replay witness. Mega EEST's `satin/access-control/delegate-refusal/*` and
//! `satin/detention/delegate-refusal/*` cases check the gas, warmth, returned state and refusal
//! status on ordinary calls.

mod witness_fixture;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
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
const STATICCALL: u8 = 0xfa;

// ---------------------------------------------------------------------------------------------
// Programs

/// `STATICCALL(0, to, 0, 0, 0, 0)` with 13 gas for the operand pushes.
fn staticcall(to: Address) -> (Vec<u8>, u64) {
    let mut code = vec![PUSH0; 4];
    code.extend(asm::push_address(to));
    code.extend([PUSH0, STATICCALL, STOP]);
    (code, 13)
}

/// How [`TOP`] runs a case.
struct Setup {
    /// [`F`]'s code, and the gas [`TOP`] forwards to it.
    f_code: Vec<u8>,
    f_gas: u64,
}

/// [`TOP`] disables volatile-data access, calls [`F`] and can probe the delegator afterward.
fn top(setup: &Setup, probe: bool) -> Vec<u8> {
    let mut code = Vec::new();
    // PUSH4 selector, PUSH1 0xe0, SHL, PUSH0, MSTORE (17 of gas, memory included)
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
    if probe {
        // GAS, BALANCE(DELEGATOR), POP, GAS. The difference at word 1 is BALANCE's charge + 7.
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

fn chain(setup: &Setup, probe: bool) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&TOP, common::contract(&top(setup, probe)));
    db.insert_account_info(&F, common::contract(&setup.f_code));
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

impl End {
    /// [`F`] halted: no return data and none of its gas handed back.
    fn assert_f_halted(&self) {
        assert!(self.stop.is_halt(), "{self:?}");
        assert_eq!(self.gas_left, 0, "{self:?}");
        assert!(self.output.is_empty(), "{self:?}");
    }

    /// [`F`] was refused with `VolatileDataAccessDisabled(10)`, handing back `left`.
    fn assert_refused(&self, left: u64) {
        assert_eq!(self.stop, InstrStop::Revert, "{self:?}");
        assert_eq!(self.output.as_ref(), refusal().as_slice(), "{self:?}");
        assert_eq!(self.gas_left, left, "{self:?}");
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

fn run_on(setup: &Setup, edit: impl Fn(&mut InMemoryDB)) -> End {
    let out = execute(setup, &edit, false);
    let probed = execute(setup, &edit, true);
    assert_eq!(
        (probed.stop, probed.gas_left, &probed.output),
        (out.stop, out.gas_left, &out.output)
    );
    out
}

fn execute(setup: &Setup, edit: &impl Fn(&mut InMemoryDB), probe: bool) -> End {
    let mut db = chain(setup, probe);
    edit(&mut db);
    let simulation = Simulation { limits: Some(TxLimits::SATIN), ..Simulation::default() };
    let mut evm = common::simulated_evm(block(), Db::new(db), &simulation);
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let tx = TxLegacy {
        chain_id: Some(common::CHAIN_ID),
        nonce: 0,
        gas_price: 0,
        gas_limit: 1_000_000,
        to: TxKind::Call(TOP),
        value: U256::ZERO,
        input: Bytes::new(),
    };
    let tx = Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER);
    evm.set_inspector(Ends::default());
    let out = evm.transact(&tx).expect("an outcome").detach();
    let ends = evm.clear_inspector_as::<Ends>().expect("the inspector").0;
    assert!(out.result.status, "TOP returns: {:?}", out.result);
    ends.into_iter().find(|e| e.depth == 1 && e.gas_limit == setup.f_gas).expect("F's frame")
}

fn run(setup: &Setup) -> End {
    run_on(setup, |_| {})
}

/// [`F`] making `call` (its code, its pushes' gas) with `left` at the call.
fn with_left(call: (Vec<u8>, u64), left: u64) -> Setup {
    Setup { f_code: call.0, f_gas: left + call.1 }
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
    assert_eq!(out.stop, InstrStop::Stop, "{out:?}");
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
