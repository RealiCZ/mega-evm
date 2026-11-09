//! Scenarios of group "par": the owner's decisions that bring Satin's system-contract and block
//! rules back to Rex6's, and the Oracle's no-service rule generalized to deposits and Mega System
//! Transactions. Each scenario names the rules it exercises; where the specification gives a worked
//! figure the scenario is that input.
//!
//! - deposits and Mega System Transactions pay history gas, a failed deposit reporting its body and
//!   calldata as history bytes (`par_dep_*`, `par_mst_*`);
//! - an Oracle read is made before its cold access, its warm base included, is charged
//!   (`par_orcl_read_*`);
//! - a refused volatile read costs nothing (`par_refused_*`);
//! - a keyless deployment is recognized only once its arguments decode (`par_keyless_*`);
//! - a hint is admitted whether its call carries value or not, a transaction's own `sendHint`
//!   forwarded its first frame's whole gas (`par_hint_*`);
//! - no Oracle read of a deposit or a Mega System Transaction asks the service (`par_orcl_dep_*`,
//!   `par_orcl_mst_*`, with a user's read as the control).
//!
//! The packing budgets a Mega System Transaction is held to are the block's, which no synthetic
//! case reaches (one transaction, or a block's transactions on one EVM without the block's
//! budgets): the engine's `mst_packing` test holds them, and the block gate the recorded blocks.
use super::{
    A, Account, Asm, B, BELOW_CAP, C, EMPTY, KEYLESS, LIMIT_CONTROL, Limits, ORACLE, Scenario,
    acct,
    bd::{BEN, P, access_off, delegation},
    edge::{SYSTEM, registry_naming, system_account},
    funded, init_returning, op,
    q8::{X1, mem_call, sel, send_hint},
    remaining_compute_gas_selector,
    rev::oracle_account,
    scenario,
};
use alloy_primitives::U256;

const EXTCODECOPY: u8 = 0x3c;
const LOG0: u8 = 0xa0;

fn par(name: &'static str, rules: &[&'static str], accounts: Vec<Account>, gas: u64) -> Scenario {
    let mut s = scenario(name, accounts, gas, Limits::default());
    s.group = "par";
    s.rules = rules.to_vec();
    s
}

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(deposit_history());
    v.extend(oracle_read_before_charge());
    v.extend(refusals());
    v.extend(keyless());
    v.extend(hints());
    v.extend(oracle_no_service());
    v
}

const HISTORY: &[&str] = &[
    "SATIN-HIST-016",
    "SATIN-HIST-022",
    "SATIN-DEP-001",
    "SATIN-DEP-005",
    "SATIN-MST-010",
    "SATIN-TAKE-012",
];

/// A deposit with mint `mint` of the scenario's transaction.
fn dep(name: &'static str, accounts: Vec<Account>, gas: u64, mint: u64) -> Scenario {
    let mut s = par(name, HISTORY, accounts, gas);
    s.deposit = Some(mint);
    s
}

/// The Oracle on a chain whose registry names [`SYSTEM`], running `oracle`, its slot 5 holding 9.
fn system_chain(oracle: Vec<u8>) -> Vec<Account> {
    vec![
        registry_naming(SYSTEM),
        system_account(vec![], 0),
        Account { address: ORACLE, code: oracle, storage: vec![(5, 9)], ..Account::default() },
    ]
}

/// A Mega System Transaction: a legacy call of the Oracle from the live system address, with the
/// chain id.
fn mst(name: &'static str, rules: &[&'static str], oracle: Vec<u8>) -> Scenario {
    let mut s = par(name, rules, system_chain(oracle), BELOW_CAP);
    s.sender = SYSTEM;
    s.to = ORACLE;
    s.chain_id = Some(1);
    s
}

/// The specification's expected results: an existing funded sender's deposit calling an existing
/// account with no value and empty calldata — 30,000 of gas cannot pay the intrinsic gas and the
/// body's 27,280 of history, a failed deposit; 100,000 pay it, a frame that halts still makes it
/// a failed deposit reporting no history gas and 310 history bytes, one that returns pays and
/// reports 27,280. Then a start record priced and given back on a revert, a write record and a log
/// priced, deposited code priced, and a Mega System Transaction's body and record priced.
fn deposit_history() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.push(dep("par_dep_short_of_body_history", vec![funded(B, vec![], 1)], 30_000, 5));
    v.last_mut().unwrap().to = B;
    v.push(dep("par_dep_halts", vec![acct(A, vec![op::INVALID])], 100_000, 0));
    v.push(dep("par_dep_returns", vec![acct(A, vec![op::STOP])], 100_000, 0));
    let mut s = dep("par_dep_start_record", vec![], 400_000, 1);
    s.to = EMPTY;
    s.value = U256::from(1);
    v.push(s);
    let mut s = dep(
        "par_dep_start_record_reverted",
        vec![acct(A, Asm::new().revert0().build())],
        400_000,
        1,
    );
    s.value = U256::from(1);
    v.push(s);
    let writes = Asm::new().sstore(1, 1).push(0).push(0).op(LOG0).stop().build();
    v.push(dep("par_dep_record_and_log", vec![acct(A, writes)], 1_000_000, 0));
    let mut s = dep("par_dep_deposited_code", vec![], 1_000_000, 0);
    s.create = true;
    s.data = init_returning(10);
    v.push(s);
    // A deposit whose created caller cannot pay its account once the body's history is paid.
    let mut s = dep("par_dep_created_caller_short", vec![acct(A, vec![op::STOP])], 200_000, 0);
    s.sender = C;
    v.push(s);
    let rules = &["SATIN-HIST-016", "SATIN-MST-008", "SATIN-HIST-022"];
    v.push(mst("par_mst_pays_history", rules, Asm::new().sstore(6, 1).stop().build()));
    v
}

/// The Oracle's code: `PUSH1 5; SLOAD; STOP`.
fn oracle_reads_slot_5() -> Vec<u8> {
    Asm::new().push(5).op(op::SLOAD).stop().build()
}

/// `A` calls the Oracle (`PUSH1 5; SLOAD; STOP`) with `forward`, then stores what
/// `remainingComputeGas()` answers in slot 0x20: the Oracle read's mark sets the limit.
fn oracle_then_remaining(forward: u64) -> Vec<u8> {
    Asm::new()
        .call_with(op::CALL, ORACLE, 0, forward, 0)
        .record_call(0x10)
        .mstore_selector(remaining_compute_gas_selector())
        .call_with(op::STATICCALL, LIMIT_CONTROL, 0, 500_000, 4)
        .op(op::POP)
        .store_returndata_word(0x20)
        .stop()
        .build()
}

/// An Oracle frame short of its read's cold access, `COLD_SLOAD_COST` (2,100, its warm base
/// included), still loads the slot, asks the service and marks the read, then runs out of gas:
/// forwarded 2,102 (2,099 left at the `SLOAD` once `PUSH1` is paid), 2,053 (2,050: the warm base
/// paid, not the rest, where the reference, charging the warm base first, runs out of gas before
/// the read) or 53 (50: short even of the warm base); with 4,102 the read completes. Either way the
/// mark is at the compute before the cold access, which `remainingComputeGas()` then shows. Last,
/// the Oracle's own code reached with too little gas for its read (`getSlot(5)` forwarded 250, 29
/// left at its `SLOAD`), its caller looping on: the mark stands, so the loop is stopped at the
/// compute limit, not run out of the transaction's 30,000,000 of gas.
fn oracle_read_before_charge() -> Vec<Scenario> {
    let rules = &["SATIN-ORCL-004", "SATIN-DET-003", "SATIN-WIT-012"];
    let mut v = Vec::new();
    for (name, forward) in [
        ("par_orcl_read_short_of_cold_access", 2_102),
        ("par_orcl_read_marked_before_charge", 4_102),
        ("par_orcl_read_past_warm_base_short_of_cold", 2_053),
        ("par_orcl_read_below_warm_base", 53),
    ] {
        let oracle = Account {
            address: ORACLE,
            code: oracle_reads_slot_5(),
            storage: vec![(5, 9)],
            ..Account::default()
        };
        let mut s =
            par(name, rules, vec![acct(A, oracle_then_remaining(forward)), oracle], BELOW_CAP);
        s.oracle = vec![(5, 42)];
        // Detention on, its Oracle cap 100,000 (below the gas left): the answer reads the limit
        // the mark sets.
        s.limits = Limits { oracle_cap: Some(100_000), ..Limits::default() };
        v.push(s);
    }
    let mut get_slot = sel("getSlot(uint256)").to_vec();
    get_slot.extend_from_slice(&U256::from(5).to_be_bytes::<32>());
    let mut a = mem_call(Asm::new(), op::CALL, ORACLE, 0, 250, &get_slot).op(op::POP).build();
    // JUMPDEST; PUSH1 <it>; JUMP
    let at = u8::try_from(a.len()).expect("a one-byte jump target");
    a.extend([op::JUMPDEST, op::PUSH1, at, op::JUMP]);
    let mut oracle = oracle_account();
    oracle.storage = vec![(5, 9)];
    let mut s = par(
        "par_orcl_real_read_below_warm_base_then_loop",
        rules,
        vec![acct(A, a), oracle],
        30_000_000,
    );
    s.oracle = vec![(5, 42)];
    v.push(s);
    v
}

/// `A` switches its access off and calls `P` (running `code`) with `gas`, `kind` a plain or a
/// static call; the call's success word at slot 3; the beneficiary is [`BEN`].
fn refused(
    name: &'static str,
    rules: &[&'static str],
    code: Vec<u8>,
    gas: u64,
    kind: u8,
) -> Scenario {
    let a = access_off(Asm::new()).call_with(kind, P, 0, gas, 0).record_call(3).stop().build();
    let mut s = par(
        name,
        rules,
        vec![acct(A, a), acct(P, code), acct(BEN, vec![op::STOP]), acct(X1, delegation(BEN))],
        BELOW_CAP,
    );
    s.beneficiary = BEN;
    s
}

/// A refused read costs nothing and comes before the opcode's operands (for one volatile whatever
/// its operands) or its body (for one volatile by what it reads): `SATIN-MAC-004`'s expected
/// `EXTCODECOPY(beneficiary, 0, 0, 1 MiB)` on 1,000 of gas keeps what it held; a `BLOCKHASH` on an
/// empty stack, a `TIMESTAMP` below its static gas, a call of a target delegating to the
/// beneficiary with a 1 MiB input, a `SELFDESTRUCT` to the beneficiary in a static frame and the
/// Oracle's `SLOAD` below its static gas are refused; an `EXTCODECOPY` short of its operands fails
/// on them as with access on.
fn refusals() -> Vec<Scenario> {
    let rules = &["SATIN-MAC-003", "SATIN-MAC-004"];
    let mut v = Vec::new();
    let copy =
        Asm::new().push(1 << 20).push(0).push(0).push_addr(BEN).op(EXTCODECOPY).stop().build();
    v.push(refused("par_refused_extcodecopy_1mib", rules, copy, 1_000, op::CALL));
    v.push(refused(
        "par_refused_blockhash_empty_stack",
        rules,
        vec![op::BLOCKHASH],
        1_000,
        op::CALL,
    ));
    v.push(refused("par_refused_timestamp_below_static", rules, vec![op::TIMESTAMP], 1, op::CALL));
    let call = Asm::new()
        .push(0)
        .push(0)
        .push(1 << 20)
        .push(0)
        .push(0)
        .push_addr(X1)
        .op(op::GAS)
        .op(op::CALL)
        .stop()
        .build();
    let delegate = &["SATIN-MAC-003", "SATIN-MAC-004", "SATIN-MAC-007"];
    v.push(refused("par_refused_call_delegate_1mib", delegate, call, 1_000, op::CALL));
    let sd = Asm::new().push_addr(BEN).op(op::SELFDESTRUCT).build();
    v.push(refused("par_refused_selfdestruct_static_frame", rules, sd, 50_000, op::STATICCALL));
    let short = Asm::new().push_addr(BEN).op(EXTCODECOPY).stop().build();
    v.push(refused("par_refused_extcodecopy_short_of_operands", rules, short, 50_000, op::CALL));
    // The Oracle's own frame reads its storage below the `SLOAD`'s static gas.
    let a =
        access_off(Asm::new()).call_with(op::CALL, ORACLE, 0, 50, 0).record_call(3).stop().build();
    let oracle = Account {
        address: ORACLE,
        code: oracle_reads_slot_5(),
        storage: vec![(5, 9)],
        ..Account::default()
    };
    v.push(par(
        "par_refused_oracle_sload_below_static",
        rules,
        vec![acct(A, a), oracle],
        BELOW_CAP,
    ));
    v
}

/// A `keylessDeploy` call whose arguments do not decode (an offset past the input) is not a
/// deployment: it runs KeylessDeploy's code (none here) and pays no overhead, with value or not.
fn keyless() -> Vec<Scenario> {
    let rules = &["SATIN-ICPT-006", "SATIN-KEY-005"];
    let mut input = sel("keylessDeploy(bytes,uint256)").to_vec();
    input.extend_from_slice(&U256::from(0x40).to_be_bytes::<32>());
    input.extend_from_slice(&U256::from(1_000_000u64).to_be_bytes::<32>());
    let mut v = Vec::new();
    for (name, value) in
        [("par_keyless_undecodable_arguments", 0u64), ("par_keyless_undecodable_with_value", 1)]
    {
        let mut s = par(name, rules, vec![], BELOW_CAP);
        s.to = KEYLESS;
        s.data = input.clone();
        s.value = U256::from(value);
        v.push(s);
    }
    v
}

/// A `sendHint` call carrying value is admitted, forwarded and counted (its bytes on the
/// transaction's data size), whether the Oracle's code then takes the value or reverts. A
/// transaction's own `sendHint` with value has no stipend: it is forwarded its first frame's whole
/// regular gas, so it is admitted with 1 of it, 2,300 and 2,301, as a legacy transaction or a
/// deposit, then the Oracle's own code reverts its non-payable call (or runs out of its one gas),
/// the hint staying counted; one whose input would take the data size over the limit is stopped
/// before the Oracle's code runs.
fn hints() -> Vec<Scenario> {
    let rules = &["SATIN-ORCL-006", "SATIN-ICPT-003"];
    let mut v = Vec::new();
    for (name, oracle) in [
        ("par_hint_with_value", vec![op::STOP]),
        ("par_hint_with_value_reverted", Asm::new().revert0().build()),
    ] {
        let a = mem_call(Asm::new(), op::CALL, ORACLE, 1, 50_000, &send_hint(7, &[0xaa]))
            .record_call(1)
            .stop()
            .build();
        v.push(par(name, rules, vec![funded(A, a, 10), acct(ORACLE, oracle)], BELOW_CAP));
    }
    // A legacy transaction with the 164 bytes of `sendHint(7, 0xab × 64)` pays 67,740 before its
    // first frame: its intrinsic gas, its body's history and the transfer's record.
    let top = &["SATIN-ORCL-006", "SATIN-ORCL-007", "SATIN-ORCL-009", "SATIN-FWD-003"];
    let top_hint = |name, frame_gas: u64| {
        let mut s = par(name, top, vec![oracle_account()], 67_740 + frame_gas);
        s.to = ORACLE;
        s.value = U256::from(1);
        s.data = send_hint(7, &[0xab; 64]);
        s
    };
    v.push(top_hint("par_hint_top_value_frame_1", 1));
    v.push(top_hint("par_hint_top_value_frame_2300", 2_300));
    v.push(top_hint("par_hint_top_value_frame_2301", 2_301));
    let mut s = top_hint("par_hint_top_value_deposit_frame_2300", 2_300);
    s.deposit = Some(0);
    s.rules.push("SATIN-ORCL-005");
    v.push(s);
    // The body's 474 bytes and the hint's 164 cross a limit of 600.
    let mut s = top_hint("par_hint_top_value_over_data_limit", 2_000);
    s.limits = Limits { data: Some(600), ..Limits::default() };
    s.rules.push("SATIN-ORCL-010");
    v.push(s);
    v
}

/// The Oracle's code returning its slot 5.
fn oracle_returns_slot_5() -> Vec<u8> {
    Asm::new().push(5).op(op::SLOAD).push(0).op(op::MSTORE).push(32).push(0).op(op::RETURN).build()
}

/// The service answers 42 for the Oracle's slot 5, which holds 9: a deposit's read and a Mega
/// System Transaction's push 9, a user's read (control) 42.
fn oracle_no_service() -> Vec<Scenario> {
    let rules = &["SATIN-ORCL-005", "SATIN-ORCL-003"];
    let a = Asm::new()
        .call_all(op::CALL, ORACLE, 0)
        .op(op::POP)
        .store_returndata_word(0x20)
        .stop()
        .build();
    let oracle = || Account {
        address: ORACLE,
        code: oracle_returns_slot_5(),
        storage: vec![(5, 9)],
        ..Account::default()
    };
    let mut v = Vec::new();
    let mut s =
        par("par_orcl_user_read_served", rules, vec![acct(A, a.clone()), oracle()], BELOW_CAP);
    s.oracle = vec![(5, 42)];
    v.push(s);
    let mut s = par("par_orcl_dep_read_on_chain", rules, vec![acct(A, a), oracle()], BELOW_CAP);
    s.deposit = Some(0);
    s.oracle = vec![(5, 42)];
    v.push(s);
    let stores_read = Asm::new().push(5).op(op::SLOAD).push(6).op(op::SSTORE).stop().build();
    let mut s = mst("par_orcl_mst_read_on_chain", rules, stores_read);
    s.oracle = vec![(5, 42)];
    v.push(s);
    v
}
