//! Satin's give-back rule and legacy share at the points the main-set scenarios do not hold to
//! their figures: a failed creation transaction's charges made before its frame (`SATIN-POOL-015`,
//! `SATIN-POOL-024`, `SATIN-HIST-005`), KeylessDeploy's steps 10 and 14 (`SATIN-KEY-005`), gas
//! detention across a child's halt and revert (`SATIN-DET-009`, `SATIN-DET-011`), and a legacy
//! share drawn from withheld gas (`SATIN-DET-017`), and a code deposit the data-size limit refuses
//! after its charges (`SATIN-SCHED-011`, `SATIN-POOL-014`). The comparison with the other
//! implementation holds these paths only to the old rule run again, so the figures here are the
//! new rule's.

mod common;

use alloy_primitives::{Bytes, TxKind, U256, keccak256};
use common::*;
use evm2::{evm::InMemoryDB, interpreter::InstrStop};

const INVALID: u8 = 0xfe;
/// The body's history: 310 bytes and each calldata byte, at 88.
fn body_history(calldata: usize) -> u64 {
    (310 + calldata as u64) * 88
}

/// A creation transaction whose frame halts or reverts, with 100,000 of reservoir once the body's
/// history is paid: the start record's 3,520 take the reservoir first, the created account's
/// 183,600 the 96,480 left and 87,120 of regular gas (no legacy share before the first frame).
/// A revert gives both charges back into the transaction's reservoir; a halt only the parts the
/// reservoir paid, the 87,120 being consumed as regular gas, so neither is on the state ledger.
#[test]
fn test_a_failed_creation_transaction_settles_its_charges_made_before_its_frame() {
    for (init, stop, reservoir) in [
        (vec![INVALID], InstrStop::InvalidFEOpcode, 100_000),
        (vec![op::PUSH0, op::PUSH0, op::REVERT], InstrStop::Revert, RECORD + ACCOUNT_STATE),
    ] {
        let body = body_history(init.len());
        let gas = 200_000_000 + body + 100_000;
        let r = run_with(InMemoryDB::default(), TxKind::Create, U256::ZERO, init.into(), gas);
        assert_eq!(r.stop, stop);
        assert_eq!(r.reservoir_left, reservoir, "{stop:?}");
        // Only the body's history stays charged as state or history gas.
        assert_eq!(r.state_gas_spent, body, "{stop:?}");
        if stop == InstrStop::InvalidFEOpcode {
            assert_eq!(r.total_gas_spent, gas - 100_000);
        }
    }
}

/// `ORIGIN; SELFDESTRUCT` deployed keylessly with 1,000,000 of reservoir: step 10 charges the
/// signer's new account, its 25,000 legacy share from regular gas and 158,600 from the reservoir;
/// step 14 the created account, its 20,000 share first and 163,600 from the reservoir, and the two
/// records' 7,040. The creation receives its share of the 670,760 left, and gets the created
/// account's 183,600 and its record's 3,520 back from the destruction; `gasUsed` stays 5,002.
#[test]
fn test_keyless_steps_take_their_legacy_shares() {
    let (raw, _) = keyless_tx(&[op::ORIGIN, op::SELFDESTRUCT]);
    let input = keyless_deploy(&raw, 500_000);
    let gas = 200_000_000 + body_history(input.len()) + 1_000_000;
    let r = run(InMemoryDB::default(), KEYLESS, input, gas);
    assert!(r.success);
    let left = 1_000_000 - (ACCOUNT_STATE - 25_000) - (ACCOUNT_STATE - 20_000) - 2 * RECORD;
    assert_eq!(left, 670_760);
    let share = left - left * 2 / 100;
    let creation = r.frame(1);
    assert_eq!((creation.gas_limit, creation.reservoir), (500_000, share));
    assert_eq!(creation.reservoir_left, share + ACCOUNT_STATE + RECORD);
    assert_eq!(r.reservoir_left, left + ACCOUNT_STATE + RECORD);
    assert_eq!(r.reservoir_left, 857_880);
    assert_eq!(r.word(0), 5_002);
}

/// `TIMESTAMP`, then a call of C with 200,000, then `remainingComputeGas()`: C writes a fresh slot
/// (its 12,100 and the pushes are compute, the slot's 97,920 and record's 3,520 its regular gas
/// paid are not) and halts, its burned remainder no compute either, or reverts (`PUSH0; PUSH0` more
/// of compute). The answer is the 20,000,000 cap less the compute since the read.
#[test]
fn test_a_childs_state_gas_is_no_compute_whether_it_halts_or_reverts() {
    let mut selector = [0u8; 32];
    selector[..4].copy_from_slice(&keccak256("remainingComputeGas()")[..4]);
    let mut top = vec![op::TIMESTAMP, op::POP];
    top.extend([op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH0, op::PUSH20]);
    top.extend_from_slice(C.as_slice());
    top.extend([0x62, 0x03, 0x0d, 0x40, op::CALL, op::POP, op::PUSH32]);
    top.extend_from_slice(&selector);
    top.extend([op::PUSH0, op::MSTORE, op::PUSH1, 0x20, op::PUSH1, 0x20, op::PUSH1, 4, op::PUSH0]);
    top.push(op::PUSH20);
    top.extend_from_slice(LIMIT_CONTROL.as_slice());
    top.extend([op::GAS, op::STATICCALL, op::POP, op::PUSH1, 0x20, op::PUSH1, 0x20, op::RETURN]);
    let write = [op::PUSH1, 1, op::PUSH0, op::SSTORE];
    for (end, child_compute) in [
        (vec![INVALID], 3 + 2 + 12_100),
        (vec![op::PUSH0, op::PUSH0, op::REVERT], 3 + 2 + 12_100 + 2 + 2),
    ] {
        let mut c = write.to_vec();
        c.extend(end);
        let mut db = InMemoryDB::default();
        db.insert_account_info(&TOP, contract(&top));
        db.insert_account_info(&C, contract(&c));
        let r = run(db, TOP, Bytes::new(), 30_000_000);
        assert!(r.success);
        // POP, the call's seven pushes and its warm 100 and cold 2,900, C, POP, the selector's
        // PUSH32, PUSH0 and MSTORE expanding memory to a word, the six pushes, and the STATICCALL's
        // 100, its expansion to two words and its cold 2,900.
        let since_read =
            2 + 16 + 3_000 + child_compute + 2 + (3 + 2 + 3 + 3) + 16 + (100 + 3 + 2_900);
        assert_eq!(r.word(0), 20_000_000 - since_read, "{child_compute}");
    }
}

/// After `TIMESTAMP` under a detention cap of 15,000, a fresh slot's write costs 12,100 of regular
/// gas, which the spendable part pays; its 10,000 legacy share, the 87,920 the absent reservoir
/// cannot pay and its record's 3,520 are taken from the withheld part first, so the write
/// completes. Under a cap of 12,000 the 12,100 cross the limit and stop the transaction.
#[test]
fn test_a_legacy_share_draws_the_withheld_gas_first() {
    let code = [op::TIMESTAMP, op::POP, op::PUSH1, 1, op::PUSH0, op::SSTORE, op::STOP];
    let mut db = InMemoryDB::default();
    db.insert_account_info(&TOP, contract(&code));
    let r = run_detained(
        db.clone(),
        TxKind::Call(TOP),
        U256::ZERO,
        Bytes::new(),
        1_000_000,
        Some(15_000),
    );
    assert_eq!(r.stop, InstrStop::Stop);
    let r = run_detained(db, TxKind::Call(TOP), U256::ZERO, Bytes::new(), 1_000_000, Some(12_000));
    assert_eq!(r.stop, InstrStop::Revert);
    let mut stopped = keccak256("MegaLimitExceeded(uint8,uint64)")[..4].to_vec();
    stopped.extend_from_slice(&U256::from(2).to_be_bytes::<32>());
    stopped.extend_from_slice(&U256::from(12_002).to_be_bytes::<32>());
    assert_eq!(r.output.as_ref(), stopped.as_slice());
}

/// A creating `PUSH2 n; PUSH0; RETURN` from memory (`CREATE(0, 0, 5)`) and returning `GAS` after
/// it: first `TIMESTAMP` when `read`, and `logs` × `LOG0(0, 16,384)` then `LOG0(0, 5,890)` when
/// `logs` is not zero.
fn refused_deposit(logs: usize, read: bool, n: u16) -> InMemoryDB {
    let mut a = Vec::new();
    if read {
        a.extend([op::TIMESTAMP, op::POP]);
    }
    if logs > 0 {
        for _ in 0..logs {
            a.extend([op::PUSH2, 0x40, 0x00, op::PUSH0, op::LOG0]);
        }
        a.extend([op::PUSH2, 0x17, 0x02, op::PUSH0, op::LOG0]);
    }
    let mut init = [0u8; 32];
    init[..5].copy_from_slice(&[op::PUSH2, (n >> 8) as u8, n as u8, op::PUSH0, op::RETURN]);
    a.push(op::PUSH32);
    a.extend_from_slice(&init);
    a.extend([op::PUSH0, op::MSTORE, op::PUSH1, 5, op::PUSH0, op::PUSH0, op::CREATE, op::POP]);
    a.extend([op::GAS, op::PUSH0, op::MSTORE, op::PUSH1, 32, op::PUSH0, op::RETURN]);
    let mut db = InMemoryDB::default();
    db.insert_account_info(&TOP, contract(&a));
    db
}

/// The deposit's charges of `n` bytes: its hashing, its state gas and its history.
fn deposit_charges(n: u64) -> (u64, u64) {
    (n.div_ceil(32) * 6, n * 1_530 + n * 88)
}

/// The review's input: at the default limits and a gas limit of 1,300,000,000, A's 798 logs of
/// 16,384 bytes and one of 5,890 (each with its 32-byte record) and the 310-byte body leave 1,000
/// of the 13,107,200 bytes of data size, and the creation's records 940 of them, below its deposit
/// of 950. The creation gets 40,665,629 of regular gas and no reservoir (the logs' history took
/// it), runs its 96 and pays the deposit's 180 of hashing, 1,453,500 of state gas and 83,600 of
/// history, all from regular gas, before the data size refuses it. The refusal reverts: the state
/// and the history go into its reservoir, the hashing stays spent. It returns (39,128,253,
/// 1,537,100), and A's `GAS` reads 39,951,119; restoring the gas from before the deposit returned
/// (40,665,533, 0) and let A read 41,488,399. The transaction keeps 1,258,324,671 and returns
/// 1,724,220 of reservoir: the deposit's 1,537,100, and the created account's 183,600 and its
/// record's 3,520 that the failed creation gives back.
#[test]
fn test_a_refused_code_deposit_gives_its_state_and_history_into_the_reservoir() {
    let r = run(refused_deposit(798, false, 950), TOP, Bytes::new(), 1_300_000_000);
    assert!(r.success);
    let (hashing, given) = deposit_charges(950);
    assert_eq!((hashing, given), (180, 1_537_100));
    let creation = r.frame(1);
    assert_eq!((creation.gas_limit, creation.reservoir), (40_665_629, 0));
    assert_eq!(creation.stop, Some(InstrStop::Revert));
    assert_eq!(creation.gas_left, 40_665_629 - 96 - hashing - given);
    assert_eq!((creation.gas_left, creation.reservoir_left), (39_128_253, 1_537_100));
    assert_eq!(r.word(0), 39_951_119);
    assert_eq!(r.total_gas_spent, 1_258_324_671);
    assert_eq!(r.reservoir_left, given + ACCOUNT_STATE + RECORD);
}

/// `s6_deposit_refused` at a gas limit of 4,000,000, below the execution cap (no reservoir), with
/// a data-size limit of 1,310: the body's 310 bytes leave A 1,000, the creation's records 940, and
/// its 950-byte deposit is refused after its charges. A has 3,762,100 at the forward, keeps a
/// fiftieth (75,242) and forwards 3,686,858; the creation's two records' history (7,040) then
/// come out of what it kept. The creation returns (3,686,858 − 96 − 180 − 1,537,100, 1,537,100) =
/// (2,149,482, 1,537,100), and A's `GAS` reads 68,202 + 2,149,482 − 2 (`POP`) − 2 (`GAS`) =
/// 2,217,680; restoring the gas from before the deposit returned (3,686,762, 0) and 3,754,960.
#[test]
fn test_a_refused_code_deposit_below_the_cap_keeps_its_charges_out_of_regular_gas() {
    let r = run_limited(
        refused_deposit(0, false, 950),
        TxKind::Call(TOP),
        U256::ZERO,
        Bytes::new(),
        4_000_000,
        |limits| limits.tx[0] = 310 + 1_000,
    );
    assert!(r.success);
    let (hashing, given) = deposit_charges(950);
    let creation = r.frame(1);
    assert_eq!((creation.gas_limit, creation.reservoir), (3_686_858, 0));
    assert_eq!(creation.stop, Some(InstrStop::Revert));
    assert_eq!(creation.gas_left, 3_686_858 - 96 - hashing - given);
    assert_eq!((creation.gas_left, creation.reservoir_left), (2_149_482, 1_537_100));
    assert_eq!(r.word(0), 2_217_680);
    assert_eq!(r.reservoir_left, given + ACCOUNT_STATE + RECORD);
}

/// `q62_det_refused_deposit_alone` returning `GAS`: after `TIMESTAMP` under a detention cap of
/// 400,000, a data-size limit of 400 refuses the creation's 100-byte deposit after its charges.
/// The deposit's 153,000 of state gas and 8,800 of history spill past the absent reservoir and are
/// drawn from the withheld part first; the revert gives them into the reservoir, so the withheld
/// part does not get them back as well. The creation, forwarded 2,706,855, returns (2,706,855 − 17
/// − 24 − 161,800, 161,800) = (2,545,014, 161,800), and A's `GAS`, which reads both parts, is
/// 55,241 − 7,040 + 2,545,014 − 4 = 2,593,211; handing the draw back to the withheld part as well
/// returned (2,706,814, 161,800) and 2,755,011.
#[test]
fn test_a_refused_code_deposit_after_a_read_does_not_hand_its_draw_back_to_the_withheld_gas() {
    let r = run_limited(
        refused_deposit(0, true, 100),
        TxKind::Call(TOP),
        U256::ZERO,
        Bytes::new(),
        3_000_000,
        |limits| {
            limits.tx[0] = 400;
            limits.cap = 400_000;
        },
    );
    assert!(r.success);
    let (hashing, given) = deposit_charges(100);
    assert_eq!((hashing, given), (24, 161_800));
    let creation = r.frame(1);
    assert_eq!((creation.gas_limit, creation.reservoir), (2_706_855, 0));
    assert_eq!(creation.stop, Some(InstrStop::Revert));
    assert_eq!(creation.gas_left, 2_706_855 - 17 - hashing - given);
    assert_eq!((creation.gas_left, creation.reservoir_left), (2_545_014, 161_800));
    assert_eq!(r.word(0), 2_593_211);
}
