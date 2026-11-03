//! Interaction scenarios (group "q2"): each deliberately combines mechanisms that the base,
//! `p6` and `p7` scenarios exercise apart.
#![allow(dead_code)]
use super::{
    A, ABOVE_CAP, Asm, B, BELOW_CAP, C, EMPTY, IDENTITY, LIMIT_CONTROL, Limits, Scenario, acct,
    funded, init_returning, op, scenario,
};
use alloy_primitives::{Address, U256};

pub const EMPTY2: Address =
    Address::new([0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xe2]);
pub const SHA256: Address = Address::with_last_byte(2);
pub const DEPOSITOR: Address = Address::new([0x7e; 20]);

fn q(name: &'static str, accounts: Vec<super::Account>, gas: u64, limits: Limits) -> Scenario {
    let mut s = scenario(name, accounts, gas, limits);
    s.group = "q2";
    s
}

fn det(cap: u64) -> Limits {
    Limits { block_env_cap: Some(cap), ..Limits::default() }
}

/// TIMESTAMP POP: a block-environment read (detention starts).
fn read() -> Asm {
    Asm::new().op(op::TIMESTAMP).op(op::POP)
}

/// Init code that writes slot 1 and returns `n` zero bytes of code.
fn init_writes_returning(n: u64) -> Vec<u8> {
    let mut c = Asm::new().sstore(1, 1).build();
    c.extend(init_returning(n));
    c
}

/// After a CREATE that left the address on the stack: CALL it with all gas, pop the status.
fn call_created() -> Asm {
    Asm::new().push(0).push(0).push(0).push(0).push(0).op(0x85).op(op::GAS).op(op::CALL).op(op::POP)
}

/// An endless loop at the end of the code.
fn looping(mut code: Vec<u8>) -> Vec<u8> {
    let jd = code.len();
    code.extend_from_slice(&[op::JUMPDEST, op::PUSH1, jd as u8, op::JUMP]);
    code
}

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    let det_create =
        |n: u64| read().create(&init_returning(n), 0).push(0x10).op(op::SSTORE).stop().build();

    // q01/q02: a detained frame's CREATE whose account and code deposit spill state + history,
    // SALT m = 2 at the created address; below the cap (all of it spills, W first) and above it
    // (the 98/100 reservoir share pays).
    let mut s =
        q("q01_det_create_spill_salt2", vec![acct(A, det_create(32))], 3_000_000, det(150_000));
    s.salt = vec![(A.create(0), None, 2)];
    v.push(s);
    let mut s =
        q("q02_det_create_salt2_res", vec![acct(A, det_create(32))], ABOVE_CAP, det(150_000));
    s.salt = vec![(A.create(0), None, 2)];
    v.push(s);
    // q03: the same creation at m = 8 with a constructor write, below the cap.
    let mut s = q(
        "q03_det_create_ctor_write_salt8",
        vec![acct(
            A,
            read().create(&init_writes_returning(64), 0).push(0x10).op(op::SSTORE).stop().build(),
        )],
        8_000_000,
        det(150_000),
    );
    s.salt = vec![(A.create(0), None, 8), (A.create(0), Some(1), 2)];
    v.push(s);

    // q04: a detained value CALL to a new account (state spill W first, m = 2), then a value
    // call to B whose stipend-funded LOG0 draws the history allowance.
    let b_logs = Asm::new().push(32).push(0).op(op::LOG0).stop().build();
    let code = read()
        .call_with(op::CALL, EMPTY, 1, 0, 0)
        .push(0x10)
        .op(op::SSTORE)
        .call_with(op::CALL, B, 1, 0, 0)
        .push(0x11)
        .op(op::SSTORE)
        .stop()
        .build();
    let mut s = q(
        "q04_det_value_new_account_and_stipend_log",
        vec![funded(A, code.clone(), 10), acct(B, b_logs.clone())],
        3_000_000,
        det(150_000),
    );
    s.salt = vec![(EMPTY, None, 2)];
    v.push(s);
    let mut s = q(
        "q05_det_value_new_account_and_stipend_log_res",
        vec![funded(A, code, 10), acct(B, b_logs)],
        ABOVE_CAP,
        det(150_000),
    );
    s.salt = vec![(EMPTY, None, 2)];
    v.push(s);
    // q06: the stipend-funded LOG overruns the 160-byte allowance (LOG1 of 128 bytes = 192).
    let b_big_log = Asm::new().push(9).push(128).push(0).op(op::LOG1).stop().build();
    v.push(q(
        "q06_stipend_log_over_allowance",
        vec![
            funded(
                A,
                Asm::new().call_with(op::CALL, B, 1, 0, 0).push(0x11).op(op::SSTORE).stop().build(),
                10,
            ),
            acct(B, b_big_log.clone()),
        ],
        BELOW_CAP,
        Limits::default(),
    ));
    // q07: same with 30,000 forwarded on top of the stipend, detained.
    v.push(q(
        "q07_det_value_call_log_over_allowance",
        vec![
            funded(
                A,
                read()
                    .call_with(op::CALL, B, 1, 30_000, 0)
                    .push(0x11)
                    .op(op::SSTORE)
                    .stop()
                    .build(),
                10,
            ),
            acct(B, b_big_log),
        ],
        BELOW_CAP,
        det(100_000),
    ));

    // q08-q11: a precompile under detention: price crosses / fits / exceeds the forward; a value
    // call to a precompile whose price crosses.
    // The caller returns the success word (MSTORE + RETURN: cheaper than any cap here).
    let ret_word = || Asm::new().push(0).op(op::MSTORE).push(32).push(0).op(op::RETURN);
    let sha = |gas: u64, len: u64| {
        read().call_with(op::CALL, SHA256, 0, gas, len).ops(&ret_word().build()).build()
    };
    v.push(q(
        "q08_det_precompile_price_crosses",
        vec![acct(A, sha(900_000, 3200))],
        BELOW_CAP,
        det(1_500),
    ));
    v.push(q(
        "q09_det_precompile_price_fits",
        vec![acct(A, sha(900_000, 64))],
        BELOW_CAP,
        det(1_500),
    ));
    v.push(q(
        "q10_det_precompile_price_over_forward",
        vec![acct(A, sha(100, 3200))],
        BELOW_CAP,
        det(1_500),
    ));
    v.push(q(
        "q11_det_value_precompile_price_crosses",
        vec![funded(
            A,
            read().call_with(op::CALL, IDENTITY, 1, 900_000, 3200).ops(&ret_word().build()).build(),
            10,
        )],
        BELOW_CAP,
        det(400),
    ));
    // q12: same, but the caller cannot fund the value: revm refuses first, no hold.
    v.push(q(
        "q12_det_unfunded_value_precompile",
        vec![acct(
            A,
            read().call_with(op::CALL, IDENTITY, 1, 900_000, 3200).ops(&ret_word().build()).build(),
        )],
        BELOW_CAP,
        det(400),
    ));

    // q13/q14: a revert-class stop in a grandchild after history and state charges (fresh
    // slots at m = 2 and a LOG); B and A catch it and A writes.
    let c_writes = Asm::new()
        .sstore(1, 1)
        .push(32)
        .push(0)
        .op(op::LOG0)
        .sstore(2, 1)
        .sstore(3, 1)
        .sstore(4, 1)
        .stop()
        .build();
    let b_calls_c = Asm::new().call_all(op::CALL, C, 0).record_call(0x10).stop().build();
    let a_calls_b_writes =
        Asm::new().call_all(op::CALL, B, 0).record_call(0x10).sstore(0x20, 1).stop().build();
    for (name, gas) in [
        ("q13_grandchild_stop_after_history_state", BELOW_CAP),
        ("q14_grandchild_stop_after_history_state_res", ABOVE_CAP),
    ] {
        let mut s = q(
            name,
            vec![
                acct(A, a_calls_b_writes.clone()),
                acct(B, b_calls_c.clone()),
                acct(C, c_writes.clone()),
            ],
            gas,
            Limits { growth: Some(4), ..Limits::default() },
        );
        s.salt = vec![(C, Some(1), 2), (A, Some(0x20), 2)];
        v.push(s);
    }
    // q15: the same under a KV budget, detained.
    let mut s = q(
        "q15_det_grandchild_kv_stop",
        vec![
            acct(A, read().ops(&a_calls_b_writes).build()),
            acct(B, b_calls_c.clone()),
            acct(C, c_writes.clone()),
        ],
        BELOW_CAP,
        Limits { kv: Some(5), block_env_cap: Some(500_000), ..Limits::default() },
    );
    s.salt = vec![(C, Some(1), 2)];
    v.push(s);

    // q16/q17: an account created and destroyed inside a detained frame (destruction table,
    // m = 2), and one destroyed to a new beneficiary.
    let ctor_destroys =
        |to: Address| Asm::new().sstore(1, 1).push_addr(to).op(op::SELFDESTRUCT).build();
    let mut s = q(
        "q16_det_created_and_destroyed",
        vec![acct(A, read().create(&ctor_destroys(A), 0).push(0x10).op(op::SSTORE).stop().build())],
        3_000_000,
        det(200_000),
    );
    s.salt = vec![(A.create(0), None, 2), (A.create(0), Some(1), 2)];
    v.push(s);
    let mut s = q(
        "q17_det_created_destroyed_to_new_beneficiary",
        vec![funded(
            A,
            read().create(&ctor_destroys(EMPTY), 5).push(0x10).op(op::SSTORE).stop().build(),
            10,
        )],
        3_000_000,
        det(200_000),
    );
    s.salt = vec![(A.create(0), None, 2), (EMPTY, None, 2)];
    v.push(s);
    // q18: a pre-existing funded account SELFDESTRUCTs to a new beneficiary while detained:
    // the beneficiary's account state spills W first.
    let mut s = q(
        "q18_det_selfdestruct_to_new_beneficiary",
        vec![funded(A, read().push_addr(EMPTY).op(op::SELFDESTRUCT).build(), 10)],
        3_000_000,
        det(150_000),
    );
    s.salt = vec![(EMPTY, None, 2)];
    v.push(s);
    // q19: a created contract destroyed later in the same tx, at m = 8, detained.
    let runtime = Asm::new().push_addr(A).op(op::SELFDESTRUCT).build();
    let mut init = Asm::new().push_bytes(&runtime).push(0).op(op::MSTORE).build();
    init.extend(
        Asm::new()
            .push(runtime.len() as u64)
            .push(32 - runtime.len() as u64)
            .op(op::RETURN)
            .build(),
    );
    let mut s = q(
        "q19_det_deploy_then_destroy_salt8",
        vec![acct(A, read().create(&init, 0).ops(&call_created().build()).stop().build())],
        8_000_000,
        det(300_000),
    );
    s.salt = vec![(A.create(0), None, 8)];
    v.push(s);

    // q20-q22: deposits: a new depositor creates a contract (SALT on the caller and the created
    // address); a deposit writing slots at m = 2; a deposit whose first frame halts.
    let mut s = q("q20_deposit_new_caller_creates_salt", Vec::new(), 3_000_000, Limits::default());
    s.sender = DEPOSITOR;
    s.deposit = Some(1_000_000);
    s.create = true;
    s.data = init_writes_returning(10);
    s.salt = vec![
        (DEPOSITOR, None, 4),
        (DEPOSITOR.create(0), None, 2),
        (DEPOSITOR.create(0), Some(1), 2),
    ];
    v.push(s);
    let mut s = q(
        "q21_deposit_writes_salt2",
        vec![acct(
            A,
            Asm::new()
                .sstore(1, 1)
                .push(32)
                .push(0)
                .op(op::LOG0)
                .call_with(op::CALL, EMPTY, 1, 0, 0)
                .op(op::POP)
                .stop()
                .build(),
        )],
        BELOW_CAP,
        Limits::default(),
    );
    s.sender = DEPOSITOR;
    s.deposit = Some(10);
    s.salt = vec![(A, Some(1), 2), (EMPTY, None, 2)];
    v.push(s);
    s = q(
        "q22_deposit_first_frame_halts",
        vec![acct(A, Asm::new().sstore(1, 1).op(op::INVALID).build())],
        BELOW_CAP,
        Limits::default(),
    );
    s.sender = DEPOSITOR;
    s.deposit = Some(5);
    v.push(s);
    // q23: a detained deposit (deposits are held to every limit).
    let mut s = q(
        "q23_deposit_detained_crosses",
        vec![acct(A, looping(read().build()))],
        BELOW_CAP,
        det(30_000),
    );
    s.sender = DEPOSITOR;
    s.deposit = Some(0);
    v.push(s);

    // q24/q25: the reservoir runs out mid-frame, so state and history spill onto regular gas
    // while part of it is withheld.
    let three = read().sstore(1, 1).sstore(2, 1).sstore(3, 1).stop().build();
    v.push(q(
        "q24_det_reservoir_exhausted_spill",
        vec![acct(A, three.clone())],
        200_100_000,
        det(150_000),
    ));
    let mut s = q(
        "q25_det_reservoir_exhausted_spill_salt2",
        vec![acct(A, three)],
        200_100_000,
        det(150_000),
    );
    s.salt = vec![(A, Some(2), 2)];
    v.push(s);
    // q26: the spill exceeds even S + W: a real out-of-gas, not a crossing.
    v.push(q(
        "q26_det_spill_beyond_total_halts",
        vec![acct(A, read().sstore(1, 1).sstore(2, 1).sstore(3, 1).stop().build())],
        250_000,
        det(150_000),
    ));

    // q27/q28: detention plus a start stop: two value calls to new accounts under a state-growth
    // limit of 1; the second start is stopped (answer taken back), A catches and loops to a
    // compute crossing.
    let code = read()
        .call_with(op::CALL, EMPTY, 1, 0, 0)
        .push(0x10)
        .op(op::SSTORE)
        .call_with(op::CALL, EMPTY2, 1, 0, 0)
        .push(0x11)
        .op(op::SSTORE)
        .stop()
        .build();
    v.push(q(
        "q27_det_start_stop_new_accounts",
        vec![funded(A, code.clone(), 10)],
        3_000_000,
        Limits { growth: Some(2), block_env_cap: Some(400_000), ..Limits::default() },
    ));
    v.push(q(
        "q28_det_start_stop_new_accounts_res",
        vec![funded(A, code, 10)],
        ABOVE_CAP,
        Limits { growth: Some(2), block_env_cap: Some(400_000), ..Limits::default() },
    ));

    // q29/q30: CREATE under the 98/100 share at m = 2 whose creation reverts (start charge and
    // captured price come back), and a CREATE2 with an m = 8 deposit above the cap.
    let mut s = q(
        "q29_create_reverts_salt2_res",
        vec![acct(
            A,
            Asm::new()
                .create(&[op::PUSH0, op::PUSH0, op::REVERT], 0)
                .push(0x10)
                .op(op::SSTORE)
                .stop()
                .build(),
        )],
        ABOVE_CAP,
        Limits::default(),
    );
    s.salt = vec![(A.create(0), None, 2)];
    v.push(s);
    let mut c2 = Asm::new();
    let init = init_returning(48);
    let mut word = [0u8; 32];
    word[..init.len()].copy_from_slice(&init);
    c2 = c2
        .push_bytes(&word)
        .push(0)
        .op(op::MSTORE)
        .push(7)
        .push(init.len() as u64)
        .push(0)
        .push(0)
        .op(op::CREATE2)
        .push(0x10)
        .op(op::SSTORE)
        .stop();
    let salt_addr = A.create2(B256_7, alloy_primitives::keccak256(&init));
    let mut s =
        q("q30_create2_deposit_salt8_res", vec![acct(A, c2.build())], ABOVE_CAP, Limits::default());
    s.salt = vec![(salt_addr, None, 8)];
    v.push(s);

    // q31/q32: nested value calls with history allowances; the grandchild's LOG3 of one word
    // fits the allowance exactly, an extra LOG0 is charged; the middle frame reverts / not.
    let c_logs = Asm::new()
        .push(3)
        .push(2)
        .push(1)
        .push(32)
        .push(0)
        .op(op::LOG3)
        .push(0)
        .push(0)
        .op(op::LOG0)
        .stop()
        .build();
    let b_value_calls_c = |rev: bool| {
        let a = Asm::new().call_with(op::CALL, C, 1, 50_000, 0).push(0x10).op(op::SSTORE);
        if rev { a.revert0().build() } else { a.stop().build() }
    };
    for (name, rev) in [
        ("q31_nested_value_calls_allowance", false),
        ("q32_nested_value_calls_allowance_middle_reverts", true),
    ] {
        v.push(q(
            name,
            vec![
                funded(
                    A,
                    Asm::new()
                        .call_with(op::CALL, B, 1, 200_000, 0)
                        .push(0x10)
                        .op(op::SSTORE)
                        .stop()
                        .build(),
                    10,
                ),
                funded(B, b_value_calls_c(rev), 10),
                acct(C, c_logs.clone()),
            ],
            ABOVE_CAP,
            Limits::default(),
        ));
    }

    // q33-q35: code deposit under detention: admission refused (data size), the deposit's hash
    // charge crosses the compute cap, and fits it.
    let mut s = q(
        "q33_det_deposit_refused_data_size",
        vec![acct(A, det_create(950))],
        ABOVE_CAP,
        Limits { data: Some(310 + 1000), block_env_cap: Some(500_000), ..Limits::default() },
    );
    s.salt = vec![(A.create(0), None, 2)];
    v.push(s);
    // The creation frame itself reads, then returns 4,000 bytes (memory 405 + hash 750 regular).
    let reading_init =
        |n: u64| Asm::new().op(op::TIMESTAMP).op(op::POP).push(n).push(0).op(op::RETURN).build();
    // The creator returns the created address and RETURNDATASIZE (cheap: no SSTORE after).
    let creates = |n: u64| {
        Asm::new()
            .create(&reading_init(n), 0)
            .push(0)
            .op(op::MSTORE)
            .op(op::RETURNDATASIZE)
            .push(32)
            .op(op::MSTORE)
            .push(64)
            .push(0)
            .op(op::RETURN)
            .build()
    };
    v.push(q("q34_det_deposit_hash_crosses", vec![acct(A, creates(4000))], ABOVE_CAP, det(1_000)));
    v.push(q("q35_det_deposit_hash_fits", vec![acct(A, creates(4000))], ABOVE_CAP, det(5_000)));
    // q36: the same creation below the cap: the code-deposit state spill draws W first.
    let mut s = q(
        "q36_det_deposit_spill_withheld_first",
        vec![acct(A, creates(200))],
        3_000_000,
        det(5_000),
    );
    s.salt = vec![(A.create(0), None, 2)];
    v.push(s);

    // q37: the transaction's own value to a reverting contract at m = 2 on the recipient
    // (first-frame record given back), detained from the read.
    let mut s = q(
        "q37_first_frame_value_revert_detained",
        vec![acct(A, read().sstore(1, 1).revert0().build())],
        BELOW_CAP,
        det(100_000),
    );
    s.value = U256::from(5);
    s.salt = vec![(A, Some(1), 2)];
    v.push(s);
    // q38: a creation transaction whose init reads TIMESTAMP, writes and deploys, m = 2.
    let mut s = q("q38_create_tx_detained_salt2", Vec::new(), 3_000_000, det(200_000));
    s.create = true;
    let mut init = Asm::new().op(op::TIMESTAMP).op(op::POP).build();
    init.extend(init_writes_returning(16));
    s.data = init;
    s.salt = vec![(super::SENDER.create(0), None, 2), (super::SENDER.create(0), Some(1), 2)];
    v.push(s);
    // q39: MegaLimitControl after state charges, detained (answer is regular gas only).
    let sel = super::remaining_compute_gas_selector();
    let mut s = q(
        "q39_det_limit_control_after_state",
        vec![acct(
            A,
            read()
                .sstore(1, 1)
                .mstore_selector(sel)
                .call_with(op::CALL, LIMIT_CONTROL, 0, 500_000, 4)
                .record_call(0x10)
                .store_returndata_word(0x20)
                .stop()
                .build(),
        )],
        BELOW_CAP,
        det(200_000),
    );
    s.salt = vec![(A, Some(1), 2)];
    v.push(s);
    // q40: a halting child that spilled state under detention: its burn is not compute; A then
    // loops to the crossing.
    let mut code = read().call_with(op::CALL, B, 0, 200_000, 0).op(op::POP).build();
    code = looping(code);
    v.push(q(
        "q40_det_halting_child_spilled_then_loop",
        vec![acct(A, code), acct(B, Asm::new().sstore(1, 1).op(op::INVALID).build())],
        3_000_000,
        det(60_000),
    ));
    // q41: a grandchild crosses compute after a state spill; B catches it and writes a slot.
    let c_spills_loops = looping(Asm::new().sstore(1, 1).build());
    let b_catch_writes =
        Asm::new().call_all(op::CALL, C, 0).record_call(0x10).sstore(0x20, 1).stop().build();
    v.push(q(
        "q41_det_grandchild_crosses_after_spill",
        vec![
            acct(A, read().call_all(op::CALL, B, 0).record_call(0x10).stop().build()),
            acct(B, b_catch_writes),
            acct(C, c_spills_loops),
        ],
        3_000_000,
        det(400_000),
    ));
    // q42: an unfunded value CALL to a new account under detention (OutOfFunds: no state kept).
    let mut s = q(
        "q42_det_unfunded_value_new_account",
        vec![acct(
            A,
            read().call_with(op::CALL, EMPTY, 1, 0, 0).push(0x10).op(op::SSTORE).stop().build(),
        )],
        3_000_000,
        det(150_000),
    );
    s.salt = vec![(EMPTY, None, 2)];
    v.push(s);
    // q43/q44: a value CALL to SHA256 whose own charges fit the cap but whose price crosses the
    // allowance (held, answered with the stop), and the same call unfunded (refused first).
    let vsha =
        || read().call_with(op::CALL, SHA256, 1, 900_000, 3200).ops(&ret_word().build()).build();
    // SHA256's account is funded, so the transfer adds no account (no new-account charge).
    v.push(q(
        "q43_det_value_call_priced_precompile_crosses",
        vec![funded(A, vsha(), 10), funded(SHA256, Vec::new(), 1)],
        BELOW_CAP,
        det(12_700),
    ));
    v.push(q(
        "q44_det_unfunded_value_call_priced_precompile",
        vec![acct(A, vsha()), funded(SHA256, Vec::new(), 1)],
        BELOW_CAP,
        det(12_700),
    ));
    // q45/q46: a non-zero L1 data fee (L1Block slots: base fee, Ecotone scalars, blob base fee)
    // with 100 enveloped bytes; q46 adds a gas price (caller refund, base-fee vault) and SALT.
    let l1 = super::Account {
        address: super::L1_BLOCK,
        code: vec![0xfe],
        storage: vec![(1, 1_000_000_000), (7, 1)],
        // slot 3 (Ecotone scalars) is set by the drivers from `l1_fee.1` (it needs 256 bits).
        ..super::Account::default()
    };
    let mut s = q(
        "q45_l1_fee_nonzero",
        vec![acct(A, Asm::new().sstore(1, 1).stop().build()), l1.clone()],
        BELOW_CAP,
        Limits::default(),
    );
    s.l1_fee = Some((1_000_000_000, 1_000_000, vec![0x01; 100]));
    v.push(s);
    let mut s = q(
        "q46_l1_fee_gas_price_salt_detained",
        vec![acct(A, read().sstore(1, 1).stop().build()), l1],
        BELOW_CAP,
        det(100_000),
    );
    s.l1_fee = Some((1_000_000_000, 1_000_000, vec![0x02; 100]));
    s.gas_price = 7;
    s.salt = vec![(A, Some(1), 2)];
    v.push(s);
    // q47-q51: the account opcodes' and SELFDESTRUCT's skip-cold checks see spendable + withheld:
    // only the whole gas pays the cold access, so the account is loaded and the charge crosses.
    let skip = |tail: Asm| read().push_addr(C).ops(&tail.build()).build();
    v.push(q(
        "q47_det_balance_skip_cold_sees_total",
        vec![acct(A, skip(Asm::new().op(op::BALANCE).op(op::POP).stop())), acct(C, vec![op::STOP])],
        BELOW_CAP,
        det(1_005),
    ));
    v.push(q(
        "q48_det_extcodesize_skip_cold_sees_total",
        vec![
            acct(A, skip(Asm::new().op(op::EXTCODESIZE).op(op::POP).stop())),
            acct(C, vec![op::STOP]),
        ],
        BELOW_CAP,
        det(1_005),
    ));
    v.push(q(
        "q49_det_extcodehash_skip_cold_sees_total",
        vec![acct(A, skip(Asm::new().op(0x3f).op(op::POP).stop())), acct(C, vec![op::STOP])],
        BELOW_CAP,
        det(1_005),
    ));
    v.push(q(
        "q50_det_extcodecopy_skip_cold_sees_total",
        vec![
            acct(A, read().push(1).push(0).push(0).push_addr(C).op(0x3c).stop().build()),
            acct(C, vec![op::STOP]),
        ],
        BELOW_CAP,
        det(1_005),
    ));
    v.push(q(
        "q51_det_selfdestruct_skip_cold_sees_total",
        vec![funded(A, skip(Asm::new().op(op::SELFDESTRUCT)), 10), acct(C, vec![op::STOP])],
        BELOW_CAP,
        det(1_005),
    ));
    // q52/q53: the block beneficiary's account (0x0 here) is volatile: BALANCE of it, and a CALL
    // to it, detain the transaction; A then loops to the crossing.
    v.push(q(
        "q52_beneficiary_balance_detains",
        vec![acct(A, looping(Asm::new().push(0).op(op::BALANCE).op(op::POP).build()))],
        BELOW_CAP,
        det(30_000),
    ));
    v.push(q(
        "q53_beneficiary_call_detains",
        vec![acct(
            A,
            looping(Asm::new().call_with(op::CALL, Address::ZERO, 0, 0, 0).op(op::POP).build()),
        )],
        BELOW_CAP,
        det(30_000),
    ));
    // q54-q57: the Oracle's storage: an SLOAD in the Oracle's own frame asks the service, is
    // priced cold every time, and detains under the Oracle's cap.
    let oracle_code = Asm::new()
        .push(5)
        .op(op::SLOAD)
        .op(op::POP)
        .push(5)
        .op(op::SLOAD)
        .push(0)
        .op(op::MSTORE)
        .push(32)
        .push(0)
        .op(op::RETURN)
        .build();
    let a_calls_oracle = |tail: Asm| {
        Asm::new()
            .call_all(op::CALL, super::ORACLE, 0)
            .record_call(0x10)
            .store_returndata_word(0x20)
            .ops(&tail.build())
            .build()
    };
    let oracle_acct = |code: Vec<u8>| super::Account {
        address: super::ORACLE,
        code,
        storage: vec![(5, 9)],
        ..super::Account::default()
    };
    let mut s = q(
        "q54_oracle_answers_then_loop",
        vec![acct(A, looping(a_calls_oracle(Asm::new()))), oracle_acct(oracle_code.clone())],
        BELOW_CAP,
        Limits { oracle_cap: Some(60_000), ..Limits::default() },
    );
    s.oracle = vec![(5, 42)];
    v.push(s);
    v.push(q(
        "q55_oracle_no_answer_loaded_value",
        vec![acct(A, a_calls_oracle(Asm::new().stop())), oracle_acct(oracle_code.clone())],
        BELOW_CAP,
        Limits::default(),
    ));
    // q56: a detained Oracle frame that holds the gas but may not spend it asks, then crosses.
    let mut s = q(
        "q56_det_oracle_read_crosses",
        vec![
            acct(A, read().ops(&a_calls_oracle(Asm::new().stop())).build()),
            oracle_acct(oracle_code),
        ],
        BELOW_CAP,
        det(4_300),
    );
    s.oracle = vec![(5, 42)];
    v.push(s);
    // q57: the Oracle's answer written by A into a fresh slot at m = 2 (history + SALT), A
    // detained by the block environment and the Oracle at once.
    let mut s = q(
        "q57_oracle_answer_written_salt2_detained",
        vec![
            acct(A, read().ops(&a_calls_oracle(Asm::new().stop())).build()),
            oracle_acct(
                Asm::new()
                    .push(5)
                    .op(op::SLOAD)
                    .push(0)
                    .op(op::MSTORE)
                    .push(32)
                    .push(0)
                    .op(op::RETURN)
                    .build(),
            ),
        ],
        3_000_000,
        Limits { block_env_cap: Some(400_000), oracle_cap: Some(300_000), ..Limits::default() },
    );
    s.oracle = vec![(5, 77)];
    s.salt = vec![(A, Some(0x20), 2)];
    v.push(s);
    // q58/q59: self-recursion to the deepest reachable depth (98/100 forward: ~550 levels), each
    // level writing one slot; q59 detained from the top (the crossing happens deep down).
    let recurse = Asm::new()
        .push(1)
        .push(0)
        .op(op::SSTORE)
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .op(op::ADDRESS)
        .op(op::GAS)
        .op(op::CALL)
        .op(op::POP)
        .stop()
        .build();
    v.push(q(
        "q58_deep_self_recursion_res",
        vec![acct(A, recurse.clone())],
        ABOVE_CAP,
        Limits::default(),
    ));
    let mut deep = read().build();
    deep.extend(recurse);
    v.push(q("q59_deep_self_recursion_detained", vec![acct(A, deep)], ABOVE_CAP, det(1_000_000)));
    // q60: the cheapest self-call (no write): the deepest depth 98/100 lets 200M reach.
    let pure = Asm::new()
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .op(op::ADDRESS)
        .op(op::GAS)
        .op(op::CALL)
        .op(op::POP)
        .stop()
        .build();
    v.push(q("q60_deepest_self_call", vec![acct(A, pure)], ABOVE_CAP, Limits::default()));
    v
}

const B256_7: alloy_primitives::B256 = {
    let mut b = [0u8; 32];
    b[31] = 7;
    alloy_primitives::B256::new(b)
};
