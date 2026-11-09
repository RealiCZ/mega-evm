//! The migration checks of Satin's give-back rule (`SATIN-HIST-012`, `SATIN-POOL-014`,
//! `SATIN-STATE-012`, `SATIN-DET-009`, `SATIN-MLC-002`, `SATIN-KEY-015`): every give-back of state
//! or history gas credits a reservoir and never regular gas, and the state an opcode or a keyless
//! deployment creates takes its legacy share from regular gas first. Each check is a transaction
//! the rule's text works through, with the figures it gives, at `m = 1` and without a reservoir
//! (every transaction's gas limit is below the 200,000,000 execution cap).

mod common;

use alloy_primitives::{Bytes, U256, keccak256};
use common::*;
use evm2::{evm::InMemoryDB, interpreter::InstrStop};

/// Check 1: a transaction below the execution cap reads `TIMESTAMP`, writes a zero slot to 1 and
/// back to 0 at `m = 1`, then calls `remainingComputeGas()`. The 101,440 its regular gas paid
/// (the slot's 97,920, its 10,000 legacy share among them, and the record's 3,520) stay out of its
/// regular gas and sit in its reservoir (`SATIN-HIST-012`), and they are no compute
/// (`SATIN-DET-009`): the answer is the 20,000,000 cap less the regular charges since the read.
#[test]
fn migration_check_1_detention_leaves_out_state_regular_gas_paid_after_its_give_back() {
    let mut selector = [0u8; 32];
    selector[..4].copy_from_slice(&keccak256("remainingComputeGas()")[..4]);
    let mut code = vec![op::TIMESTAMP, op::POP, op::GAS];
    code.extend([op::PUSH1, 1, op::PUSH0, op::SSTORE]); // 0 -> 1
    code.extend([op::PUSH0, op::PUSH0, op::SSTORE]); // 1 -> 0
    code.extend([op::GAS, op::SWAP1, op::SUB, op::PUSH1, 0x20, op::MSTORE]);
    code.push(op::PUSH32);
    code.extend_from_slice(&selector);
    code.extend([op::PUSH0, op::MSTORE]);
    // STATICCALL(GAS, MegaLimitControl, 0, 4, 0x40, 0x20): the answer at 0x40.
    code.extend([op::PUSH1, 0x20, op::PUSH1, 0x40, op::PUSH1, 4, op::PUSH0, op::PUSH20]);
    code.extend_from_slice(LIMIT_CONTROL.as_slice());
    code.extend([op::GAS, op::STATICCALL, op::POP, op::PUSH1, 0x60, op::PUSH0, op::RETURN]);
    let mut db = InMemoryDB::default();
    db.insert_account_info(&TOP, contract(&code));
    let r = run(db, TOP, Bytes::new(), 30_000_000);
    assert!(r.success);

    // Between the two `GAS` readings: the pushes (3 + 2 and 2 + 2), the cold write's 12,100, the
    // warm clear's 100 and the second `GAS`'s 2, and the 101,440 that the clear gives back into
    // the reservoir and not into regular gas.
    assert_eq!(r.word(1), 3 + 2 + 12_100 + 2 + 2 + 100 + 2 + SLOT_STATE + RECORD);
    assert_eq!(r.reservoir_left, SLOT_STATE + RECORD);
    // The regular charges after the read: POP, GAS, the two writes as above, GAS, SWAP1, SUB,
    // PUSH1 and the MSTORE that expands memory to two words (3 + 6), the selector's PUSH32, PUSH0
    // and MSTORE, the call's six pushes, and the STATICCALL's warm access, its expansion to three
    // words and its cold surcharge.
    let since_read = 2 +
        2 +
        (3 + 2 + 12_100) +
        (2 + 2 + 100) +
        2 +
        3 +
        3 +
        3 +
        9 +
        3 +
        2 +
        3 +
        (3 + 3 + 3 + 2 + 3 + 2) +
        (100 + 3 + 2_900);
    assert_eq!(r.word(2), 20_000_000 - since_read);
}

/// Check 2: a keyless creation whose constructor is `PUSH1 1; PUSH0; SSTORE; PUSH0; PUSH0; REVERT`,
/// on 500,000 of forward and no reservoir, pays its 12,109 of regular charges and the slot's
/// 101,440 from regular gas; its revert gives the 101,440 back into its reservoir
/// (`SATIN-POOL-014`), so it returns 386,451 of regular gas and 101,440 of reservoir, and its
/// `gasUsed`, both pools netted together (`SATIN-KEY-015`), is 12,109.
#[test]
fn migration_check_2_keyless_gas_used_nets_both_pools() {
    let (raw, _) =
        keyless_tx(&[op::PUSH1, 1, op::PUSH0, op::SSTORE, op::PUSH0, op::PUSH0, op::REVERT]);
    let r = run(InMemoryDB::default(), KEYLESS, keyless_deploy(&raw, 500_000), 2_000_000);
    assert!(r.success);
    let creation = r.frame(1);
    assert_eq!((creation.gas_limit, creation.reservoir), (500_000, 0));
    assert_eq!(creation.stop, Some(InstrStop::Revert));
    assert_eq!((creation.gas_left, creation.reservoir_left), (386_451, SLOT_STATE + RECORD));
    assert_eq!(r.word(0), 12_109);
    let mut reverted = keccak256("ExecutionReverted(uint64,bytes)")[..4].to_vec();
    reverted.extend_from_slice(&U256::from(12_109).to_be_bytes::<32>());
    reverted.extend_from_slice(&U256::from(64).to_be_bytes::<32>());
    reverted.extend_from_slice(&[0u8; 32]);
    assert_eq!(error_data(&r.output), reverted.as_slice());
}

/// Check 3: P, whose slot 0 holds 7, writes 8 (its record's 3,520 from regular gas) and makes a
/// `DELEGATECALL` to C; C emits an empty `LOG0` (2,816) and writes the slot back to 7, which gives
/// the 3,520 into C's reservoir, then reverts: C's reservoir becomes `3,520 + (2,816 − 3,520) =
/// 2,816` and joins P's. P then reverts after its own record: its reservoir becomes `2,816 +
/// 3,520 = 6,336` (`SATIN-POOL-014`, `SATIN-TAKE-005`).
#[test]
fn migration_check_3_a_frame_reverting_after_a_childs_revert_keeps_the_childs_reservoir() {
    let mut p = vec![op::PUSH1, 8, op::PUSH0, op::SSTORE];
    p.extend([op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH20]);
    p.extend_from_slice(C.as_slice());
    p.extend([op::GAS, op::DELEGATECALL, op::POP, op::PUSH0, op::PUSH0, op::REVERT]);
    let c = [
        op::PUSH0,
        op::PUSH0,
        op::LOG0,
        op::PUSH1,
        7,
        op::PUSH0,
        op::SSTORE,
        op::PUSH0,
        op::PUSH0,
        op::REVERT,
    ];
    // TOP calls P with all its gas, so that P's own frame is recorded as it ends.
    let mut top = vec![op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH20];
    top.extend_from_slice(P.as_slice());
    top.extend([op::GAS, op::CALL, op::STOP]);
    let mut db = InMemoryDB::default();
    db.insert_account_info(&TOP, contract(&top));
    db.insert_account_info(&P, contract(&p));
    db.insert_account_storage(&P, &U256::ZERO, &U256::from(7));
    db.insert_account_info(&C, contract(&c));
    let r = run(db, TOP, Bytes::new(), 1_000_000);
    assert!(r.success);
    let (p, c) = (r.frame(1), r.frame(2));
    assert_eq!((p.reservoir, c.reservoir), (0, 0));
    assert_eq!((c.stop, c.reservoir_left), (Some(InstrStop::Revert), LOG0));
    assert_eq!((p.stop, p.reservoir_left), (Some(InstrStop::Revert), LOG0 + RECORD));
    assert_eq!(p.reservoir_left, 6_336);
}

/// Check 4: a keyless creation whose constructor is `ORIGIN; SELFDESTRUCT`, on 500,000 of forward
/// and no reservoir, with a zero balance, spends 2 + 5,000 and receives back into its reservoir
/// the created account's 183,600 and its record's 3,520, which the call charged at its step 14
/// (`SATIN-SD-005`, `SATIN-SD-007`): it returns 494,998 of regular gas and 187,120 of reservoir,
/// and `gasUsed` adds those give-backs back, 5,002 + (0 − 187,120) + 187,120 = 5,002
/// (`SATIN-KEY-015`); the call answers `EmptyCodeDeployed(5002)`. The returned reservoir joins the
/// call's and returns to the sender at the settlement.
#[test]
fn migration_check_4_keyless_gas_used_adds_back_the_destruction_of_step_14s_charges() {
    let (raw, _) = keyless_tx(&[op::ORIGIN, op::SELFDESTRUCT]);
    let r = run(InMemoryDB::default(), KEYLESS, keyless_deploy(&raw, 500_000), 2_000_000);
    assert!(r.success);
    let creation = r.frame(1);
    assert_eq!((creation.gas_limit, creation.reservoir), (500_000, 0));
    assert_eq!(creation.stop, Some(InstrStop::SelfDestruct));
    assert_eq!((creation.gas_left, creation.reservoir_left), (494_998, ACCOUNT_STATE + RECORD));
    assert_eq!(r.word(0), 5_002);
    assert_eq!(error_data(&r.output), error_u64("EmptyCodeDeployed(uint64)", 5_002).as_slice());
    assert_eq!(r.reservoir_left, ACCOUNT_STATE + RECORD);
}
