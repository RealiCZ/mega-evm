//! The give-back rule and the legacy share on the engine (`SATIN-HIST-012`, `SATIN-POOL-014`,
//! `SATIN-STATE-012`): the trajectories and worked examples of `mega_evm2_scenarios::gb`, each held
//! to the figures the rules give for it, derived by hand from the specification's schedule:
//!
//! - a cold fresh slot's write costs 12,100 of regular gas (2,100 access, 10,000 write), its state
//!   97,920 × `m` (a 10,000 legacy share from regular gas first) and its record 3,520 of history;
//! - a value call to a cold new account costs 3,000 of access, 11,300 of value transfer (its 2,300
//!   stipend comes back with the callee's gas), 183,600 × `m` of state (a 25,000 legacy share
//!   first), and 3,520 of history per record its start makes;
//! - a frame keeps `⌊available / 50⌋` of what it forwards from, and `⌊reservoir × 2 / 100⌋` of its
//!   reservoir.
//!
//! The comparison with the other implementation holds every case to the old rule run again
//! (`gate::RESERVOIR_GIVE_BACK`); these tests hold the engine to the new one.

use mega_evm2_diff::{
    driver::{run, scenarios},
    gate,
};
use serde_json::Value;

const SLOT: u64 = 97_920;
const ACCOUNT: u64 = 183_600;
const RECORD: u64 = 3_520;
const SLOT_SHARE: u64 = 10_000;
const ACCOUNT_SHARE: u64 = 25_000;
const COLD_WRITE: u64 = 2_100 + 10_000;
const COLD_ACCOUNT: u64 = 3_000;
const VALUE: u64 = 11_300;
const STIPEND: u64 = 2_300;
/// The body's 310 bytes of history.
const BODY: u64 = 27_280;
/// The reservoir of [`scenarios::gb::RESERVOIR_GAS`]: 300,000 less the body's history.
const RESERVOIR: u64 = 300_000 - BODY;

/// The scenario's inspected record, after checking that its plain run renders the same.
fn record(name: &str) -> Value {
    let s = scenarios::by_name(name).unwrap_or_else(|| panic!("no scenario {name}"));
    let plain = run(&s, false);
    let inspected = run(&s, true);
    assert!(plain.get("error").is_none(), "{name} refused: {plain}");
    assert_eq!(gate::same_engine_differences(&plain, &inspected), Vec::<String>::new(), "{name}");
    inspected
}

fn u(r: &Value, key: &str) -> u64 {
    r[key].as_u64().unwrap_or_else(|| panic!("{key}: {}", r[key]))
}

/// The transaction's output as 32-byte words.
fn words(r: &Value) -> Vec<u64> {
    let out = r["output"].as_str().unwrap();
    let bytes = alloy_primitives::hex::decode(out).unwrap();
    bytes
        .chunks(32)
        .map(|w| alloy_primitives::U256::from_be_slice(w).saturating_to::<u64>())
        .collect()
}

/// Frame `i` (in the order the frames opened): `(status, gas left, reservoir left)`.
fn frame(r: &Value, i: usize) -> (String, u64, u64) {
    let f = &r["frames"][i];
    (f["status"].as_str().unwrap().to_owned(), u(f, "gas_left"), u(f, "reservoir_left"))
}

fn frame_start(r: &Value, i: usize) -> (u64, u64) {
    let f = &r["frames"][i];
    (u(f, "gas_limit"), u(f, "reservoir"))
}

fn ok(s: &str) -> String {
    s.to_owned()
}

/// The slot write `PUSH1 1; PUSH1 1; SSTORE` measured: its pushes, its regular price and the
/// second `GAS`.
fn measured_write(regular: u64) -> u64 {
    3 + 3 + regular + 2
}

/// The value call `CALL(0, new, 1, 0, 0, 0, 0); POP` measured: its seven pushes, `POP`, the second
/// `GAS`, and what it takes from regular gas net of the returned stipend.
fn measured_new_account(regular: u64) -> u64 {
    (2 * 4 + 3 + 3 + 2) + 2 + 2 + regular - STIPEND
}

#[test]
fn test_t1_a_failure_handler_pays_the_legacy_share_from_regular_gas() {
    // No reservoir: every state and history charge from regular gas (`SATIN-POOL-008`).
    for (name, m) in [("gb_t1_handler_m1", 1), ("gb_t1_handler_m4", 4)] {
        let r = record(name);
        assert_eq!(
            words(&r),
            [
                measured_write(COLD_WRITE + SLOT * m + RECORD),
                measured_new_account(COLD_ACCOUNT + VALUE + ACCOUNT * m + 2 * RECORD)
            ],
            "{name}"
        );
        assert_eq!(u(&r, "reservoir_remaining"), 0);
    }
    // 113,540 without a reservoir at `m = 1` (`gas-model.md`, Security Considerations).
    assert_eq!(COLD_WRITE + SLOT + RECORD, 113_540);

    // With a reservoir at `m = 1` the slot takes 22,100 of regular gas and the account 39,300
    // (`SATIN-STATE-012`, `SATIN-HIST-010`), the rest from the reservoir.
    let r = record("gb_t1_handler_res_m1");
    assert_eq!(
        words(&r),
        [measured_write(22_100), measured_new_account(COLD_ACCOUNT + VALUE + ACCOUNT_SHARE)]
    );
    assert_eq!(COLD_WRITE + SLOT_SHARE, 22_100);
    assert_eq!(COLD_ACCOUNT + VALUE + ACCOUNT_SHARE, 39_300);
    // A keeps 2 per cent of its reservoir at the starved call, which returns the rest.
    assert_eq!(frame_start(&r, 1).1, RESERVOIR - RESERVOIR * 2 / 100);
    let left = RESERVOIR - (SLOT - SLOT_SHARE + RECORD) - (ACCOUNT - ACCOUNT_SHARE + 2 * RECORD);
    assert_eq!(u(&r, "reservoir_remaining"), left);
    assert_eq!(left, 15_640);

    // At `m = 4` the reservoir does not reach: the slot's 381,680 beyond its share take the 272,720
    // the reservoir holds and spill 108,960, and its record is paid from regular gas.
    let r = record("gb_t1_handler_res_m4");
    let spill = SLOT * 4 - SLOT_SHARE - RESERVOIR;
    assert_eq!(spill, 108_960);
    assert_eq!(
        words(&r),
        [
            measured_write(COLD_WRITE + SLOT_SHARE + spill + RECORD),
            measured_new_account(COLD_ACCOUNT + VALUE + ACCOUNT * 4 + 2 * RECORD)
        ]
    );
    assert_eq!(COLD_WRITE + SLOT_SHARE + spill + RECORD, 134_580);
}

#[test]
fn test_t2_a_lock_cleared_after_a_starved_call_does_not_raise_gas() {
    for (name, m) in [
        ("gb_t2_lock_m1", 1),
        ("gb_t2_lock_m4", 4),
        ("gb_t2_lock_res_m1", 1),
        ("gb_t2_lock_res_m4", 4),
    ] {
        let r = record(name);
        // The clear costs `PUSH0; PUSH0` and 100 and gives nothing to regular gas; the record
        // takes 22,100 of regular gas, the reservoir the clear filled paying the rest.
        assert_eq!(words(&r), [2 + 2 + 100 + 2, measured_write(22_100)], "{name}");
        // Without a reservoir, the clear's give-back less the record's draw stays.
        if !name.contains("res") {
            assert_eq!(
                u(&r, "reservoir_remaining"),
                (SLOT * m + RECORD) - (SLOT * m - SLOT_SHARE + RECORD)
            );
        }
    }
    // With a reservoir at `m = 1`: the set and the record each draw 91,440, and the clear gives
    // 101,440 back.
    let r = record("gb_t2_lock_res_m1");
    assert_eq!(u(&r, "reservoir_remaining"), RESERVOIR - 91_440 + 101_440 - 91_440);
    // At `m = 4` the set empties the reservoir, the clear gives 395,200, the record draws 385,200.
    let r = record("gb_t2_lock_res_m4");
    assert_eq!(u(&r, "reservoir_remaining"), 10_000);

    // The least gas A completes the lock from, and one less (`gb::T2_THRESHOLD_M1`): A ends with
    // no regular gas left, or halts on the record's legacy share.
    for (name, gas, status) in [
        ("gb_t2_threshold_m1", 1_227_160, "success"),
        ("gb_t2_threshold_below_m1", 1_227_159, "halt"),
        ("gb_t2_threshold_m4", 1_520_920, "success"),
        ("gb_t2_threshold_below_m4", 1_520_919, "halt"),
    ] {
        let r = record(name);
        assert_eq!(frame_start(&r, 1).0, gas, "{name}");
        let (got, left, _) = frame(&r, 1);
        assert_eq!(got, status, "{name}");
        assert_eq!(left, 0, "{name}");
    }
    assert_eq!(scenarios::gb::T2_THRESHOLD_M1, 1_227_160);
    assert_eq!(scenarios::gb::T2_THRESHOLD_M4, 1_520_920);
}

/// T3's D on `forward`: what its write (`PUSH1 1; PUSH0; SSTORE`) took from regular gas, the call's
/// seven pushes and cold access, the 2 per cent it keeps, then `POP; PUSH0; PUSH0; REVERT`.
fn t3_d_left(forward: u64, write: u64) -> u64 {
    (forward - (3 + 2 + write) - 15 - COLD_ACCOUNT) / 50 - 6
}

#[test]
fn test_t3_a_reverting_intermediary_returns_its_charges_as_reservoir() {
    // A's call to D: seven pushes, POP, the cold access, the forward less what D returns, `GAS`.
    let call_cost = |forward: u64, d_left: u64| 16 + 2 + COLD_ACCOUNT + forward - d_left + 2;
    for (name, forward, m) in [("gb_t3_slot_m1", 500_000, 1), ("gb_t3_slot_m4", 2_000_000, 4)] {
        let r = record(name);
        let d_left = t3_d_left(forward, COLD_WRITE + SLOT * m + RECORD);
        assert_eq!(frame(&r, 1), (ok("revert"), d_left, SLOT * m + RECORD), "{name}");
        assert_eq!(words(&r), [call_cost(forward, d_left)], "{name}");
        assert_eq!(u(&r, "reservoir_remaining"), SLOT * m + RECORD, "{name}");
    }
    assert_eq!(t3_d_left(500_000, COLD_WRITE + SLOT + RECORD), 7_662);

    // With a reservoir: D receives 267,266 and gives back into it what its write charged, the
    // legacy share and any spill included.
    let share = RESERVOIR - RESERVOIR * 2 / 100;
    let r = record("gb_t3_slot_res_m1");
    assert_eq!(frame_start(&r, 1), (500_000, share));
    let d_left = t3_d_left(500_000, COLD_WRITE + SLOT_SHARE);
    assert_eq!(frame(&r, 1), (ok("revert"), d_left, share + SLOT_SHARE));
    assert_eq!(u(&r, "reservoir_remaining"), RESERVOIR + SLOT_SHARE);
    let r = record("gb_t3_slot_res_m4");
    assert_eq!(frame(&r, 1).2, SLOT * 4 + RECORD);
    assert_eq!(u(&r, "reservoir_remaining"), RESERVOIR * 2 / 100 + SLOT * 4 + RECORD);

    // A 10,240-byte log: 903,936 of history (`SATIN-HIST-002`), all of it in A's reservoir.
    let log = (32 + 10_240) * 88;
    assert_eq!(log, 903_936);
    let log_regular = 3 + 2 + 375 + 8 * 10_240 + (3 * 320 + 320 * 320 / 512);
    let r = record("gb_t3_log");
    let d_left = (2_000_000 - log_regular - log - 15 - COLD_ACCOUNT) / 50 - 6;
    assert_eq!(frame(&r, 1), (ok("revert"), d_left, log));
    assert_eq!(u(&r, "reservoir_remaining"), log);
    let r = record("gb_t3_log_res");
    assert_eq!(frame(&r, 1).2, log);
    assert_eq!(u(&r, "reservoir_remaining"), RESERVOIR * 2 / 100 + log);
}

#[test]
fn test_t4_a_descendant_takes_back_into_its_reservoir() {
    // C's write back to 7 costs `PUSH1; PUSH0` and 100 (returned as its word), and gives P's
    // record's 3,520 into C's reservoir.
    let r = record("gb_t4_descendant_kept");
    assert_eq!(frame(&r, 2).2, RECORD);
    assert_eq!(frame(&r, 1), (ok("success"), frame(&r, 1).1, RECORD));
    assert_eq!(r["frames"][2]["output"], format!("0x{:064x}", 3 + 2 + 100 + 2));
    assert_eq!((u(&r, "kv"), u(&r, "reservoir_remaining")), (0, RECORD));
    // Reverted: C keeps `3,520 + (2,816 − 3,520) = 2,816`, P after its own revert 6,336.
    let r = record("gb_t4_descendant_reverted");
    assert_eq!(frame(&r, 2).2, 2_816);
    assert_eq!(frame(&r, 1).2, 6_336);
    assert_eq!(r["frames"][2]["output"], format!("0x{:064x}", 3 + 2 + 100 + 2));
    // P halts: its reservoir is the zero it received.
    let r = record("gb_t4_descendant_halted");
    assert_eq!(frame(&r, 1), (ok("halt"), 0, 0));
    // With a reservoir every charge was the reservoir's: a revert gives each frame back what it
    // received, a success the 3,520 more.
    let r = record("gb_t4_descendant_kept_res");
    let (p_share, c_share) = (frame_start(&r, 1).1, frame_start(&r, 2).1);
    assert_eq!(p_share, RESERVOIR - RESERVOIR * 2 / 100);
    let p_left = p_share - RECORD;
    assert_eq!(c_share, p_left - p_left * 2 / 100);
    assert_eq!(frame(&r, 2).2, c_share - 2_816 + RECORD);
    let r = record("gb_t4_descendant_reverted_res");
    assert_eq!(frame(&r, 2).2, c_share);
    assert_eq!(frame(&r, 1).2, p_share);
    assert_eq!(u(&r, "reservoir_remaining"), RESERVOIR);
}

#[test]
fn test_t5_a_failed_frame_gives_its_charges_back_into_its_reservoir() {
    // `SATIN-TAKE-011`'s example: 100,000 of reservoir and 50,000 of regular gas; the write's
    // pushes (5) and its 12,100 and 10,000 share from regular gas, 87,920 and the 3,520 record
    // from the reservoir; `PUSH0; PUSH0; REVERT`.
    let r = record("gb_t5_reverted_res_m1");
    assert_eq!(frame_start(&r, 1), (50_000, 100_000));
    assert_eq!(frame(&r, 1), (ok("revert"), 50_000 - 5 - 22_100 - 4, 110_000));
    assert_eq!(u(&r, "reservoir_remaining"), 2_040 + 110_000);
    let r = record("gb_t5_halted_res_m1");
    assert_eq!(frame(&r, 1), (ok("halt"), 0, 100_000));
    assert_eq!(u(&r, "reservoir_remaining"), 102_040);
    // At `m = 4` the reservoir pays 100,000 of the 381,680 beyond the share; the revert gives
    // back all 395,200.
    let r = record("gb_t5_reverted_res_m4");
    let spill = SLOT * 4 - SLOT_SHARE - 100_000;
    assert_eq!(
        frame(&r, 1),
        (ok("revert"), 500_000 - 5 - 22_100 - spill - RECORD - 4, SLOT * 4 + RECORD)
    );
    let r = record("gb_t5_halted_res_m4");
    assert_eq!(frame(&r, 1), (ok("halt"), 0, 100_000));
    // Without a reservoir: everything from regular gas, all of it into the reservoir on a revert,
    // consumed on a halt.
    for (name, forward, m) in [("gb_t5_reverted_m1", 150_000, 1), ("gb_t5_reverted_m4", 500_000, 4)]
    {
        let r = record(name);
        assert_eq!(
            frame(&r, 1),
            (ok("revert"), forward - 5 - COLD_WRITE - SLOT * m - RECORD - 4, SLOT * m + RECORD),
            "{name}"
        );
    }
    for name in ["gb_t5_halted_m1", "gb_t5_halted_m4"] {
        assert_eq!(frame(&record(name), 1), (ok("halt"), 0, 0), "{name}");
    }
}

#[test]
fn test_example_pool_012_a_returned_share_joins_the_kept_part() {
    let r = record("gb_ex_pool_share_returns");
    assert_eq!(frame_start(&r, 1).1, 98);
    assert_eq!(frame(&r, 1).2, 98);
    assert_eq!(u(&r, "reservoir_remaining"), 100);
}

#[test]
fn test_example_pool_019_clearing_a_cold_slot() {
    let r = record("gb_ex_pool_clear_ledgers");
    assert_eq!((u(&r, "regular"), u(&r, "history"), u(&r, "state")), (27_106, 30_800, 0));
    assert_eq!(u(&r, "gas_used"), 46_325);
    assert_eq!(27_106 + 30_800 - 11_581, 46_325);
}

#[test]
fn test_example_state_012_a_selfdestructs_new_beneficiary() {
    // A: `PUSH20; BALANCE` (a cold 3,000), `POP`, `PUSH20`, then 5,000 + 9,000 + the 16,000 share.
    let own = 3 + 3_000 + 2 + 3 + (5_000 + 9_000 + 16_000);
    let r = record("gb_ex_selfdestruct_share_res");
    let (limit, reservoir) = frame_start(&r, 1);
    // The rest of the 183,600 and the two records from the reservoir.
    assert_eq!(
        frame(&r, 1),
        (ok("success"), limit - own, reservoir - (ACCOUNT - 16_000) - 2 * RECORD)
    );
    assert_eq!(own - 3_008, 30_000);
    let r = record("gb_ex_selfdestruct_share");
    let (limit, _) = frame_start(&r, 1);
    assert_eq!(frame(&r, 1), (ok("success"), limit - own - (ACCOUNT - 16_000) - 2 * RECORD, 0));
}

#[test]
fn test_example_hist_019_022_the_allowance_pays_a_stipend_callees_log() {
    let r = record("gb_ex_allowance_log");
    // The child: `PUSH0; PUSH0; LOG0` on the 2,300 stipend; the 2,816 of history from the
    // allowance.
    assert_eq!(frame(&r, 1), (ok("success"), STIPEND - (2 + 2 + 375), 0));
    // The body, the start's two records and the log's 32 bytes.
    assert_eq!(u(&r, "history_bytes"), 310 + 2 * 40 + 32);
}

/// A deposit with empty calldata that makes no write reports 310 bytes of data size and 310
/// history bytes, its body, whose 27,280 of history gas it pays as any transaction does
/// (`SATIN-HIST-016`, `SATIN-HIST-022`).
#[test]
fn test_example_hist_022_a_deposits_body() {
    let r = record("gb_ex_deposit_body");
    assert_eq!((u(&r, "data_size"), u(&r, "history_bytes"), u(&r, "history")), (310, 310, 27_280));
}

#[test]
fn test_example_det_013_compute_exceeds_regular_gas_by_the_stipends() {
    // One turn of the loop: the call's 100 and 11,300 (its 2,300 stipend returned) and the loop's
    // own 58; compute counts the stipend too (`SATIN-DET-010`).
    let turn = scenarios::gb::STIPEND_LOOP_TURN;
    assert_eq!(turn, 58);
    let (many, fewer) =
        (record("gb_ex_det_stipend_calls"), record("gb_ex_det_stipend_calls_fewer"));
    let (w, f) = (words(&many), words(&fewer));
    // Word 1 is the regular gas left, word 0 `remainingComputeGas()`: 1,000 more calls spend 9,158
    // more each and count 11,458 more each.
    assert_eq!(f[1] - w[1], 1_000 * (9_100 + turn));
    assert_eq!(f[0] - w[0], 1_000 * (11_400 + turn));
    // 18,000 calls: 163,800,000 of net regular gas and 205,200,000 of compute, with the loop's own.
    assert_eq!(18_000 * 9_100, 163_800_000);
    assert_eq!(18_000 * 11_400, 205_200_000);
    // The answer: the 210,000,000 cap less the compute since `TIMESTAMP`: `POP`, the counter's
    // `PUSH2`, the turns, the last test (20), the first call's cold access, and before the
    // interceptor's frame `JUMPDEST; POP`, the selector's `PUSH32; PUSH0; MSTORE` (3 + 3), the
    // call's six pushes and its 100 and cold 2,900; the first call's two records (7,040) are
    // history, not compute.
    let compute = 2 +
        3 +
        18_000 * (11_400 + turn) +
        20 +
        2_900 +
        (1 + 2) +
        (3 + 2 + 6) +
        (3 + 2 + 3 + 2 + 3 + 2) +
        3_000;
    assert_eq!(w[0], scenarios::gb::DET_EXAMPLE_CAP - compute);
}

#[test]
fn test_example_take_006_sent_back() {
    // Q is given P's two records' 7,040 into its reservoir, which joins P's.
    let r = record("gb_ex_sent_back");
    assert_eq!(frame(&r, 1).2, 2 * RECORD);
    assert_eq!((u(&r, "kv"), u(&r, "data_size")), (0, 310 + 2 * 160));
    assert_eq!(u(&r, "reservoir_remaining"), 2 * RECORD);
    // P's nonce changed first: only Q's record, 3,520.
    let r = record("gb_ex_sent_back_nonce");
    assert_eq!(frame(&r, 2).2, RECORD);
    // Q sent P 1 wei first: Q, called back with 200,000 and left 5,000 after its forward, cannot
    // pay the 7,040 of the two records the call makes again.
    let r = record("gb_ex_sent_back_earlier");
    assert_eq!(frame_start(&r, 4).0, 200_000 + STIPEND);
    assert_eq!(frame(&r, 4), (ok("halt"), 0, 0));
    assert_eq!(u(&r, "reservoir_remaining"), 0);
}

#[test]
fn test_example_take_007_a_destroyed_creation() {
    // The creation and constructor charged 532,440 of state and 19,360 of history, all given back
    // into the destroying frame's reservoir.
    let state = ACCOUNT + 2 * SLOT + 100 * 1_530;
    let history = 3 * RECORD + 100 * 88;
    assert_eq!((state, history, state + history), (532_440, 19_360, 551_800));
    let r = record("gb_ex_destroyed_creation");
    assert_eq!(frame(&r, 2).2, state + history);
    // As if C never existed, but for P's nonce record.
    assert_eq!(
        (u(&r, "state"), u(&r, "history"), u(&r, "kv"), u(&r, "growth")),
        (0, BODY + RECORD, 1, 0)
    );
    assert_eq!(u(&r, "data_size"), 310 + 40);
    // With 1 wei to a warm beneficiary empty under EIP-161: 30,000 of regular gas, the other
    // 167,600 from regular gas (the reservoir is empty before the give-back), then the 551,800
    // back, then the beneficiary's record from them.
    let r = record("gb_ex_destroyed_creation_beneficiary");
    let (limit, _) = frame_start(&r, 2);
    let own = 3 + 3_000 + 2 + 3 + 30_000;
    assert_eq!(
        frame(&r, 2),
        (ok("success"), limit - own - (ACCOUNT - 16_000), state + history - RECORD)
    );
}

#[test]
fn test_example_take_008_set_then_clear() {
    let r = record("gb_ex_set_then_clear");
    // The new account and the indicator, once; one record and one unit of growth.
    assert_eq!(u(&r, "state"), ACCOUNT + 35_190);
    assert_eq!((u(&r, "kv"), u(&r, "growth")), (1, 1));
    let a = r["accounts"][format!("{}", scenarios::gb::AUTHORITY)].as_str().unwrap().to_owned();
    assert!(a.ends_with(&format!("/n2/{}", alloy_primitives::KECCAK256_EMPTY)), "{a}");
}

#[test]
fn test_example_take_009_a_reverted_log() {
    // `LOG2` of 64 bytes: 160 bytes, 14,080 of history, back into the frame's reservoir.
    assert_eq!((32 + 2 * 32 + 64) * 88, 14_080);
    assert_eq!(frame(&record("gb_ex_log_reverted"), 1).2, 14_080);
    // The allowance paid it: nothing comes back.
    assert_eq!(frame(&record("gb_ex_log_reverted_allowance"), 1).2, 0);
}

#[test]
fn test_example_take_010_an_answer_stopped_start() {
    let r = record("gb_ex_answer_stopped");
    let mut stop = alloy_primitives::keccak256("MegaLimitExceeded(uint8,uint64)")[..4].to_vec();
    stop.extend_from_slice(&alloy_primitives::U256::from(1).to_be_bytes::<32>());
    stop.extend_from_slice(&[0u8; 32]);
    assert_eq!(r["output"], alloy_primitives::hex::encode_prefixed(&stop));
    // The new account's state gas, its 25,000 share included, and the two records' history.
    assert_eq!(u(&r, "reservoir_remaining"), ACCOUNT + 2 * RECORD);
    assert_eq!(u(&r, "kv"), 2);
}

#[test]
fn test_example_take_012_a_failed_deposit() {
    let r = record("gb_ex_failed_deposit");
    assert_eq!((u(&r, "regular"), u(&r, "state"), u(&r, "history")), (1_000_000, 0, 0));
    // No history gas, but the body and calldata it keeps as its history bytes (`SATIN-TAKE-012`).
    assert_eq!((u(&r, "growth"), u(&r, "kv"), u(&r, "history_bytes")), (1, 0, 310));
    assert_eq!(u(&r, "data_size"), 310);
}

#[test]
fn test_example_key_015_keyless_gas_used() {
    let r = record("gb_ex_keyless_revert");
    assert_eq!(frame(&r, 1), (ok("revert"), 386_451, SLOT + RECORD));
    assert_eq!(words(&r)[0], 12_109);
    let r = record("gb_ex_keyless_destroy");
    assert_eq!(frame(&r, 1), (ok("success"), 494_998, ACCOUNT + RECORD));
    assert_eq!(words(&r)[0], 5_002);
}
