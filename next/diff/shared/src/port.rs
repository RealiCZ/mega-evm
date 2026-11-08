//! Scenarios of group "port": the rest of the Satin implementation's own tests that the earlier
//! ports left out, and the inputs the node's simulation switches make possible. The specification
//! lets a node turn off two checks for a user's transaction in a simulation, the nonce check and
//! EIP-3607 (`SATIN-MST-005`, `SATIN-KEY-007`), and never the limits or the detention caps
//! (`SATIN-ACT-009`); `Scenario::skip_nonce_check` and `Scenario::skip_eip3607` set them.
#![allow(dead_code)]
use super::{
    A, Account, Asm, Auth, B, BELOW_CAP, D, KEYLESS, Limits, ORACLE, SENDER, Scenario, acct,
    funded, op, scenario,
};
use crate::{
    edge::{from_system, system_account},
    q8::keyless_call,
};
use alloy_primitives::{Address, U256};

fn port(name: &'static str, rules: &[&'static str], accounts: Vec<Account>, gas: u64) -> Scenario {
    let mut s = scenario(name, accounts, gas, Limits::default());
    s.group = "port";
    s.rules = rules.to_vec();
    s
}

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(switches());
    v
}

fn counter() -> Vec<u8> {
    Asm::new().sstore(1, 1).stop().build()
}

/// The two switches on user transactions, the Mega System Transaction and the keyless deployment.
fn switches() -> Vec<Scenario> {
    let mut v = Vec::new();
    let sender_at =
        |nonce: u64| Account { nonce, ..funded(SENDER, vec![], 1_000_000_000_000_000_000) };
    // --- the nonce check
    // On (control): a nonce that is not the sender's is refused. Off: the transaction runs, and
    // the sender's nonce becomes the one after the transaction's.
    for (name, switch, tx_nonce) in [
        ("port_nonce_high_check_on", false, 5u64),
        ("port_nonce_high_check_off", true, 5),
        ("port_nonce_low_check_on", false, 0),
        ("port_nonce_low_check_off", true, 0),
    ] {
        let mut s =
            port(name, &["SATIN-BASE-001", "SATIN-MST-005"], vec![acct(A, counter())], BELOW_CAP);
        if name.contains("low") {
            s.accounts.push(sender_at(3));
        }
        s.nonce = tx_nonce;
        s.skip_nonce_check = switch;
        v.push(s);
    }
    // A creation whose sender's nonce cannot be bumped: with the check off the transaction
    // creates nothing and succeeds (SATIN-LIM-015); a deposit does the same with or without it.
    let init = crate::init_returning(1);
    for (name, switch) in
        [("port_create_max_nonce_check_off", true), ("port_create_max_nonce_check_on", false)]
    {
        let mut s =
            port(name, &["SATIN-LIM-015", "SATIN-DEP-010"], vec![sender_at(u64::MAX)], 20_000_000);
        s.create = true;
        s.data = init.clone();
        s.value = U256::from(7u64);
        s.skip_nonce_check = switch;
        v.push(s);
    }
    let mut s = port(
        "port_create_max_nonce_deposit",
        &["SATIN-DEP-010", "SATIN-LIM-015"],
        vec![Account { nonce: u64::MAX, ..funded(D, vec![], 1_000_000_000) }],
        BELOW_CAP,
    );
    s.sender = D;
    s.deposit = Some(0);
    s.create = true;
    s.data = init;
    v.push(s);
    // --- EIP-3607
    // A sender that holds code is refused; with the check off it sends.
    v.push(port(
        "port_sender_code_3607_on",
        &["SATIN-BASE-001", "SATIN-MST-005"],
        vec![funded(SENDER, vec![op::STOP], 1_000_000_000_000_000_000), acct(A, counter())],
        BELOW_CAP,
    ));
    // --- the Mega System Transaction follows the same switches (SATIN-MST-005)
    let sys_rules = ["SATIN-MST-005", "SATIN-VAL-007"];
    for (name, nonce, code, n_switch, c_switch) in [
        ("port_system_nonce_stale_checks_on", 3u64, vec![], false, false),
        ("port_system_nonce_stale_check_off", 3, vec![], true, false),
        ("port_system_code_checks_on", 0, vec![op::STOP], false, false),
        ("port_system_code_3607_off", 0, vec![op::STOP], false, true),
        ("port_system_code_and_stale_nonce_both_off", 3, vec![op::STOP], true, true),
    ] {
        let mut s = from_system(name, system_account(code, nonce), ORACLE);
        s.group = "port";
        s.rules = sys_rules.to_vec();
        s.chain_id = Some(1);
        s.skip_nonce_check = n_switch;
        s.skip_eip3607 = c_switch;
        // The transaction's nonce is the one the switch-off case does not match.
        s.nonce = 0;
        v.push(s);
    }
    // --- the keyless deployment's step 9 follows EIP-3607 (SATIN-KEY-007)
    let (data, signer, _) = keyless_call("ok", 1_000_000);
    for (name, switch) in
        [("port_keyless_signer_code_3607_on", false), ("port_keyless_signer_code_3607_off", true)]
    {
        let mut s = port(
            name,
            &["SATIN-KEY-005", "SATIN-KEY-007"],
            vec![funded(signer, vec![op::STOP], 100_000_000_000_000_000)],
            5_000_000,
        );
        s.to = KEYLESS;
        s.data = data.clone();
        s.skip_eip3607 = switch;
        v.push(s);
    }
    // A signer holding a delegation indicator is accepted either way.
    let mut delegation = vec![0xef, 0x01, 0x00];
    delegation.extend_from_slice(B.as_slice());
    let mut s = port(
        "port_keyless_signer_delegated",
        &["SATIN-KEY-005"],
        vec![funded(signer, delegation, 100_000_000_000_000_000)],
        5_000_000,
    );
    s.to = KEYLESS;
    s.data = data;
    v.push(s);
    v.extend(protocol_origin());
    v.extend(beneficiary_creations());
    v.extend(hold_caller());
    v.extend(compute_gas());
    v.extend(code_sizes());
    v.extend(create2_operands());
    v.extend(data_size_boundaries());
    let _ = (Address::ZERO, U256::ZERO);
    v
}

/// `SYSTEM_CALLER` (the caller of the pre-block calls).
const SYSTEM_CALLER: Address = Address::new([0xff; 20]);

/// A transaction from `SYSTEM_CALLER` is the protocol's, in a block and out of one
/// (`SATIN-MST-007`): it pays no history gas and no per-transaction limit stops it. A transaction
/// from an ordinary sender, with the same body, is the control.
fn protocol_origin() -> Vec<Scenario> {
    let mut v = Vec::new();
    let sender = |a: Address| funded(a, vec![], 1_000_000_000_000_000_000);
    for (name, from) in [
        ("port_protocol_caller_call", Address::new([0xff; 20])),
        ("port_ordinary_caller_call", Address::with_last_byte(0xee)),
    ] {
        let mut from_addr = from;
        if name.contains("protocol") {
            from_addr.0[19] = 0xfe;
        }
        let mut s = port(
            name,
            &["SATIN-MST-007", "SATIN-MST-008"],
            vec![sender(from_addr), acct(A, Asm::new().sstore(1, 1).sstore(2, 1).stop().build())],
            BELOW_CAP,
        );
        s.sender = from_addr;
        s.limits.kv = Some(1);
        v.push(s);
    }
    v
}

/// A creation whose created address is the block beneficiary: the transaction is not detained
/// from its start (`SATIN-DET-006`, `SATIN-DET-008`): a creation's address is no sender or call
/// recipient.
fn beneficiary_creations() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (name, beneficiary) in [
        ("port_create_at_beneficiary", SENDER.create(0)),
        ("port_create_beneficiary_elsewhere", Address::with_last_byte(0x77)),
        // The control: the sender is the beneficiary, detained from the start.
        ("port_create_by_beneficiary", SENDER),
    ] {
        let mut s = port(name, &["SATIN-DET-006", "SATIN-DET-008"], vec![], 1_000_000);
        s.create = true;
        s.data = crate::spin(2_000);
        s.limits.block_env_cap = Some(10_000);
        s.beneficiary = beneficiary;
        v.push(s);
    }
    v
}

/// The program the Oracle account holds for the scenarios below: a call with no data (the
/// transaction's) runs `body`; any call with data (the hint) stops.
fn oracle_program(body: Vec<u8>) -> Vec<u8> {
    let mut code =
        vec![op::CALLDATASIZE, op::ISZERO, op::PUSH1, 6, op::JUMPI, op::STOP, op::JUMPDEST];
    code.extend(body);
    code
}

/// A caller resuming from a child that sent an Oracle hint and failed is held to the limits the
/// hint left it over (`SATIN-STOP-006`, `SATIN-STOP-012`): in the data-size dimension only, where
/// the hint's bytes stay counted. A system-originated transaction is held to no limit.
fn hold_caller() -> Vec<Scenario> {
    use crate::q8::{mem_call, send_hint};
    let mut v = Vec::new();
    let hint = send_hint(7, &[0xab; 64]);
    let b = mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint).op(op::POP).revert0().build();
    // A Mega System Transaction whose child sends a hint and fails, on a data limit far below its
    // size: exempt from the limits, it carries on.
    let body = Asm::new().call_all(op::CALL, B, 0).record_call(1).stop().build();
    let mut s = from_system("port_hold_caller_system_exempt", system_account(vec![], 0), ORACLE);
    s.group = "port";
    s.rules = vec!["SATIN-STOP-006", "SATIN-STOP-012", "SATIN-MST-008", "SATIN-LIM-012"];
    s.chain_id = Some(1);
    s.accounts.push(acct(ORACLE, oracle_program(body)));
    s.accounts.push(acct(B, b.clone()));
    s.limits.data = Some(100);
    v.push(s);
    // A user transaction with a new EIP-7702 authority (a unit of state growth on the
    // transaction, no frame's), whose frame calls a child that sends a hint and fails, and does
    // nothing after: its data size at the resume is its data size at the end, and a limit equal to
    // it holds; one byte lower stops the caller as it resumes.
    for (name, limit) in [
        ("port_hold_caller_at_limit", HOLD_AT_LIMIT),
        ("port_hold_caller_one_over_limit", HOLD_AT_LIMIT - 1),
        ("port_hold_caller_ample", 1_000_000),
    ] {
        let a = Asm::new().call_all(op::CALL, B, 0).op(op::POP).stop().build();
        let mut s = port(
            name,
            &["SATIN-STOP-006", "SATIN-STOP-012", "SATIN-LIM-012", "SATIN-AUTH-008"],
            vec![acct(A, a), acct(B, b.clone()), acct(ORACLE, Asm::new().stop().build())],
            BELOW_CAP,
        );
        s.auths = vec![Auth { chain_id: 1, address: D, nonce: 0, authority: Some(X_AUTHORITY) }];
        s.limits.data = Some(limit);
        v.push(s);
    }
    v
}

/// The new authority of the scenarios above.
const X_AUTHORITY: Address = Address::new([0x72; 20]);
/// The data size the user scenario above counts: its limit.
const HOLD_AT_LIMIT: u64 = 615;

const WORK: u64 = 2_000;

/// `n` rounds of `PUSH1 a PUSH1 b <op> POP`.
fn rounds(mut a: Asm, n: u64, x: u8, y: u8, o: u8) -> Asm {
    for _ in 0..n {
        a = a.ops(&[op::PUSH1, x, op::PUSH1, y, o, op::POP]);
    }
    a
}

/// Compute gas: every opcode is computed as it runs, state gas stays off the figure, past the
/// execution cap an expansion is an ordinary out of gas (tests/satin/compute_gas.rs).
fn compute_gas() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-DET-009", "SATIN-POOL-021", "SATIN-POOL-007"];
    let arith = rounds(rounds(Asm::new(), 1, 1, 2, op::ADD), 1, 3, 4, op::MUL).stop().build();
    let sum = rounds(
        rounds(
            rounds(Asm::new(), 100, 1, 2, op::ADD)
                .call_with(op::CALL, Address::with_last_byte(2), 0, 100_000, 32)
                .op(op::POP),
            100,
            3,
            4,
            op::MUL,
        ),
        0,
        0,
        0,
        0,
    )
    .stop()
    .build();
    // PUSH1 1 PUSH4 11199968 MSTORE: about 240M of memory.
    let big_memory = Asm::new().push(1).push(11_199_968).op(op::MSTORE).stop().build();
    for (tag, gas) in [("10m", 10_000_000u64), ("300m", 300_000_000)] {
        for (name, code) in [("arith", &arith), ("sum", &sum)] {
            let name: &'static str = Box::leak(format!("port_cg_{name}_{tag}").into_boxed_str());
            v.push(port(name, &rules, vec![acct(A, code.clone())], gas));
        }
        for m in [1u64, 4] {
            let name: &'static str =
                Box::leak(format!("port_cg_state_gas_m{m}_{tag}").into_boxed_str());
            let a = Asm::new().push_addr(super::EMPTY).op(op::SELFDESTRUCT).build();
            let mut s = port(name, &rules, vec![funded(A, a, 5)], gas);
            s.salt = vec![(super::EMPTY, None, m)];
            v.push(s);
        }
    }
    for (name, gas) in [
        ("port_cg_memory_past_cap_250m", 250_000_000u64),
        ("port_cg_memory_past_cap_300m", 300_000_000),
    ] {
        v.push(port(name, &rules, vec![acct(A, big_memory.clone())], gas));
    }
    let a = Asm::new().call_all(op::CALL, B, 0).sstore(0, 1).stop().build();
    v.push(port(
        "port_cg_nested_memory_past_cap",
        &rules,
        vec![acct(A, a), acct(B, big_memory)],
        300_000_000,
    ));
    let _ = WORK;
    v
}

/// The code and initcode sizes (tests/satin/contract_size.rs): creation transactions at and one
/// byte past each limit, and the same through `CREATE`.
fn code_sizes() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-SIZE-001", "SATIN-SIZE-002", "SATIN-SIZE-004", "SATIN-OUT-003"];
    let gas = 5_000_000_000u64;
    let returns = |n: u64| {
        // PUSH3 n PUSH1 0 RETURN, right-padded: returns n zero bytes.
        let mut c = vec![0x62, (n >> 16) as u8, (n >> 8) as u8, n as u8, op::PUSH1, 0, op::RETURN];
        c.resize(32, 0);
        c
    };
    for (name, len) in [
        ("port_size_initcode_at_limit", 1_048_576usize),
        ("port_size_initcode_one_over", 1_048_577),
    ] {
        let mut s = port(name, &rules, vec![], gas);
        s.create = true;
        s.data = vec![op::STOP; len];
        v.push(s);
    }
    for (name, n) in [("port_size_code_at_limit", 524_288u64), ("port_size_code_one_over", 524_289)]
    {
        let mut s = port(name, &rules, vec![], gas);
        s.create = true;
        s.data = returns(n);
        v.push(s);
    }
    for (name, n) in [
        ("port_size_create_code_at_limit", 524_288u64),
        ("port_size_create_code_one_over", 524_289),
    ] {
        let init = &returns(n)[..7];
        let a = Asm::new().create(init, 0).push(1).op(op::SSTORE).stop().build();
        v.push(port(name, &rules, vec![acct(A, a)], gas));
    }
    v
}

/// `CREATE2` operand corners (tests/satin/create_opcodes.rs).
fn create2_operands() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-SIZE-003", "SATIN-SIZE-005", "SATIN-SIZE-006"];
    let max = [0xffu8; 32];
    let c2 = |salt: Option<u64>, len: Vec<u8>, offset: Vec<u8>| {
        let mut a = Asm::new();
        if let Some(s) = salt {
            a = a.push(s);
        }
        a = a.push_bytes(&len).push_bytes(&offset).push(0).op(op::CREATE2).stop();
        a.build()
    };
    let cases: [(&'static str, Vec<u8>, u64); 11] = [
        ("port_create2_len0_offset_max", c2(Some(0), vec![0], max.to_vec()), 100_000_000),
        ("port_create2_len1_offset_max", c2(Some(0), vec![1], max.to_vec()), 100_000_000),
        ("port_create2_len0_offset_large", c2(Some(0), vec![0], vec![0x40, 0, 0, 0]), 100_000_000),
        ("port_create2_32kib", c2(Some(0), vec![0x80, 0], vec![0]), 100_000_000),
        ("port_create2_32bytes", c2(Some(0), vec![32], vec![0]), 100_000_000),
        ("port_create2_init_at_limit", c2(Some(0), vec![0x10, 0, 0], vec![0]), 400_000_000),
        ("port_create2_init_one_over", c2(Some(0), vec![0x10, 0, 1], vec![0]), 400_000_000),
        ("port_create2_init_10mib", c2(Some(0), vec![0xa0, 0, 0], vec![0]), 400_000_000),
        (
            "port_create2_init_one_over_far_offset",
            c2(Some(0), vec![0x10, 0, 1], vec![0xff; 16]),
            400_000_000,
        ),
        ("port_create2_memory_unpayable", c2(Some(0), vec![0x07, 0xa1, 0x20], vec![0]), 227_280),
        (
            "port_create2_memory_unpayable_no_salt",
            c2(None, vec![0x07, 0xa1, 0x20], vec![0]),
            227_280,
        ),
    ];
    for (name, code, gas) in cases {
        v.push(port(name, &rules, vec![acct(A, code)], gas));
    }
    v
}

/// Data-size boundaries (tests/satin/data_size.rs, data_size_counts.rs): a record one byte over the
/// limit stops on that record, one at the limit does not.
fn data_size_boundaries() -> Vec<Scenario> {
    let mut v = Vec::new();
    let rules = ["SATIN-ACCT-001", "SATIN-ACCT-002", "SATIN-LIM-001", "SATIN-STOP-001"];
    let log_then_write = Asm::new().push(10).push(0).op(op::LOG0).sstore(1, 1).stop().build();
    let write_then_log = Asm::new().sstore(0, 1).push(10).push(0).op(op::LOG0).stop().build();
    let write = Asm::new().sstore(0, 1).stop().build();
    for (name, code, limit) in [
        ("port_ds_log_fits", &log_then_write, 310 + 82),
        ("port_ds_log_one_over", &log_then_write, 310 + 41),
        ("port_ds_write_fits", &write_then_log, 310 + 82),
        ("port_ds_write_one_over", &write_then_log, 310 + 39),
    ] {
        let mut s = port(name, &rules, vec![acct(A, code.clone())], 20_000_000);
        s.limits.data = Some(limit);
        v.push(s);
    }
    for (name, limit, gas) in [
        ("port_ds_slot_349_oog", 349u64, 152_305u64),
        ("port_ds_slot_349_stop", 349, 152_306),
        ("port_ds_slot_350_oog", 350, 155_825),
        ("port_ds_slot_350_ok", 350, 155_826),
    ] {
        let mut s = port(name, &rules, vec![acct(A, write.clone())], gas);
        s.limits.data = Some(limit);
        v.push(s);
    }
    for n in [0usize, 100, 1000] {
        let name: &'static str =
            Box::leak(format!("port_ds_limit_equal_kept_calldata{n}").into_boxed_str());
        let mut s = port(name, &rules, vec![acct(A, write.clone())], 20_000_000);
        s.data = vec![0xab; n];
        s.limits.data = Some(310 + n as u64 + 40);
        v.push(s);
    }
    v
}
