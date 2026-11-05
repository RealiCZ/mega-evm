//! Differential probes of single mechanisms (group "p6"), as complete transactions in the shared
//! format. Addresses: caller 0xaa, contract 0xbb (balance 1,000,000), child 0xcc.
#![allow(dead_code)]
use super::{Account, Asm, Limits, Scenario, op};
use alloy_primitives::{Address, U256};

pub const CALLER: Address = Address::with_last_byte(0xaa);
pub const CONTRACT: Address = Address::with_last_byte(0xbb);
pub const CHILD: Address = Address::with_last_byte(0xcc);

fn base(name: &'static str, code: Vec<u8>, child: Option<Vec<u8>>, gas: u64) -> Scenario {
    let mut accounts = vec![
        Account { address: CALLER, balance: U256::from(10u64.pow(18)), ..Account::default() },
        Account {
            address: CONTRACT,
            code,
            balance: U256::from(1_000_000u64),
            ..Account::default()
        },
    ];
    if let Some(c) = child {
        accounts.push(Account { address: CHILD, code: c, ..Account::default() });
    }
    let mut s = super::scenario(name, Vec::new(), gas, Limits::default());
    s.accounts = accounts;
    s.sender = CALLER;
    s.to = CONTRACT;
    s.group = "p6";
    s
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    // --- base profile: STOP, ADD, the Amsterdam opcodes on the Osaka base.
    v.push(base("p6_stop", vec![op::STOP], None, 1_000_000));
    v.push(base(
        "p6_add",
        Asm::new().push(1).push(2).op(op::ADD).op(op::POP).stop().build(),
        None,
        1_000_000,
    ));
    // DUPN with a 17-deep stack (immediate 0 = depth 17), SWAPN, EXCHANGE, SLOTNUM.
    let deep = |tail: &[u8]| {
        let mut a = Asm::new();
        for i in 0..18u64 {
            a = a.push(i + 1);
        }
        a.ops(tail).stop().build()
    };
    v.push(base("p6_dupn", deep(&[op::DUPN, 0x00]), None, 1_000_000));
    v.push(base("p6_swapn", deep(&[op::SWAPN, 0x00]), None, 1_000_000));
    v.push(base("p6_exchange", deep(&[op::EXCHANGE, 0x01]), None, 1_000_000));
    v.push(base(
        "p6_slotnum",
        Asm::new().op(op::SLOTNUM).push(1).op(op::SSTORE).stop().build(),
        None,
        1_000_000,
    ));
    // --- 98/100 forward: the child stores GAS.
    v.push(base(
        "p6_forward_98",
        Asm::new().call_all(op::CALL, CHILD, 0).op(op::POP).stop().build(),
        Some(Asm::new().op(op::GAS).push(1).op(op::SSTORE).stop().build()),
        1_000_000,
    ));
    // --- SALT state prices and paired refunds at m = 1, 2, 8.
    let failed_call = Asm::new()
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push(1_000_001)
        .push_addr(CHILD)
        .push(100_000)
        .op(op::CALL)
        .stop()
        .build();
    let failed_create = Asm::new().create(&[op::PUSH0, op::PUSH0, op::REVERT], 0).stop().build();
    for m in [1u64, 2, 8] {
        let mut s = base(
            leak(format!("p6_salt_sstore_set_m{m}")),
            Asm::new().sstore(0, 1).stop().build(),
            None,
            1_000_000,
        );
        s.salt = vec![(CONTRACT, Some(0), m)];
        v.push(s);
        let mut s = base(
            leak(format!("p6_salt_sstore_restore_m{m}")),
            Asm::new().sstore(0, 1).sstore(0, 0).stop().build(),
            None,
            1_000_000,
        );
        s.salt = vec![(CONTRACT, Some(0), m)];
        v.push(s);
        let mut s = base(
            leak(format!("p6_salt_failed_value_call_m{m}")),
            failed_call.clone(),
            None,
            1_000_000,
        );
        s.salt = vec![(CHILD, None, m)];
        v.push(s);
        let mut s = base(
            leak(format!("p6_salt_failed_create_m{m}")),
            failed_create.clone(),
            None,
            1_000_000,
        );
        s.salt = vec![(CONTRACT.create(0), None, m)];
        v.push(s);
        let mut s =
            base(leak(format!("p6_salt_first_value_call_m{m}")), vec![op::STOP], None, 1_000_000);
        s.to = CHILD;
        s.value = U256::from(1);
        s.salt = vec![(CHILD, None, m)];
        v.push(s);
        let mut s =
            base(leak(format!("p6_salt_code_deposit_m{m}")), vec![op::STOP], None, 1_000_000);
        s.create = true;
        s.data = vec![op::PUSH1, 32, op::PUSH0, op::RETURN];
        s.salt = vec![(CALLER.create(0), None, m)];
        v.push(s);
    }
    // --- history: interleaved writes and logs, child reverts / root reverts.
    let writes_and_logs = Asm::new()
        .sstore(1, 1)
        .push(32)
        .push(0)
        .op(op::LOG0)
        .sstore(2, 2)
        .push(7)
        .push(64)
        .push(0)
        .op(op::LOG1)
        .build();
    let mut child_reverts = writes_and_logs.clone();
    child_reverts.extend(Asm::new().revert0().build());
    v.push(base(
        "p6_history_child_reverts",
        Asm::new()
            .sstore(5, 5)
            .call_all(op::CALL, CHILD, 0)
            .op(op::POP)
            .push(16)
            .push(0)
            .op(op::LOG0)
            .stop()
            .build(),
        Some(child_reverts),
        1_000_000,
    ));
    let mut root_reverts = writes_and_logs.clone();
    root_reverts.extend(Asm::new().revert0().build());
    v.push(base("p6_history_root_reverts", root_reverts, None, 1_000_000));
    let mut ok = writes_and_logs.clone();
    ok.push(op::STOP);
    v.push(base("p6_history_ok", ok.clone(), None, 1_000_000));
    let mut s = base("p6_history_ok_res", ok, None, 210_000_000);
    s.group = "p6";
    v.push(s);
    // --- a child refills an ancestor's slot (0 -> 1 in the parent, 1 -> 0 in the child).
    let child_restore = Asm::new().sstore(1, 0).stop().build();
    let child_restore_revert = Asm::new().sstore(1, 0).revert0().build();
    let parent = Asm::new()
        .sstore(1, 1)
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push_addr(CHILD)
        .op(op::GAS)
        .op(op::DELEGATECALL)
        .op(op::POP)
        .stop()
        .build();
    v.push(base("p6_child_refills_ancestor_ok", parent.clone(), Some(child_restore), 1_000_000));
    v.push(base(
        "p6_child_refills_ancestor_revert",
        parent.clone(),
        Some(child_restore_revert),
        1_000_000,
    ));
    let mut s = base(
        "p6_child_refills_ancestor_ok_res",
        parent,
        Some(Asm::new().sstore(1, 0).stop().build()),
        200_200_000,
    );
    s.group = "p6";
    v.push(s);
    // --- value-call history allowance: the child emits a LOG3 of one word.
    let log3 = Asm::new().push(3).push(2).push(1).push(32).push(0).op(op::LOG3).stop().build();
    for (name, kind, value) in [
        ("p6_allowance_value_call", op::CALL, 1u64),
        ("p6_allowance_zero_value_call", op::CALL, 0),
        ("p6_allowance_value_callcode", op::CALLCODE, 1),
    ] {
        let code =
            Asm::new().call_with(kind, CHILD, value, 0, 0).push(0x10).op(op::SSTORE).stop().build();
        v.push(base(name, code, Some(log3.clone()), 1_000_000));
    }
    // --- the 98/100 reservoir share: parent reservoir 101,440 / 103,510, child writes a slot.
    for (name, gas) in [
        ("p6_share_reservoir_101440", 200_000_000 + 101_440),
        ("p6_share_reservoir_103510", 200_000_000 + 103_510),
    ] {
        let code = Asm::new().call_all(op::CALL, CHILD, 0).push(0x10).op(op::SSTORE).stop().build();
        v.push(base(name, code, Some(Asm::new().sstore(1, 1).stop().build()), gas));
    }
    // --- D82/D85/D86: constructor destruction, and a deployment destroyed later.
    let ctor_destroys = Asm::new().sstore(1, 1).push_addr(CONTRACT).op(op::SELFDESTRUCT).build();
    for m in [1u64, 2, 8] {
        let mut s = base(
            leak(format!("p6_destroy_in_constructor_m{m}")),
            Asm::new().create(&ctor_destroys, 0).op(op::POP).stop().build(),
            None,
            1_000_000,
        );
        s.salt = vec![(CONTRACT.create(0), None, m)];
        v.push(s);
        let runtime = Asm::new().push_addr(CONTRACT).op(op::SELFDESTRUCT).build();
        let mut init = Asm::new().push_bytes(&runtime).push(0).op(op::MSTORE).build();
        init.extend(
            Asm::new()
                .push(runtime.len() as u64)
                .push(32 - runtime.len() as u64)
                .op(op::RETURN)
                .build(),
        );
        let code = Asm::new()
            .create(&init, 0)
            .push(0)
            .push(0)
            .push(0)
            .push(0)
            .push(0)
            .op(0x85)
            .op(op::GAS)
            .op(op::CALL)
            .op(op::POP)
            .stop()
            .build();
        let mut s = base(leak(format!("p6_destroy_after_deploy_m{m}")), code, None, 1_000_000);
        s.salt = vec![(CONTRACT.create(0), None, m)];
        v.push(s);
    }
    // --- a user deposit from a new caller: the created caller's account state gas at m.
    for m in [1u64, 2, 8] {
        let fresh = Address::with_last_byte(0xd1);
        let mut s =
            base(leak(format!("p6_deposit_new_caller_m{m}")), vec![op::STOP], None, 1_000_000);
        s.accounts.retain(|a| a.address != CALLER);
        s.sender = fresh;
        s.deposit = Some(0);
        s.salt = vec![(fresh, None, m)];
        v.push(s);
    }
    // --- the EIP-7623 floor with calldata (history pays the bytes at 88 > the floor's rate).
    for (name, byte) in [("p6_floor_zero_calldata", 0u8), ("p6_floor_nonzero_calldata", 1)] {
        let mut s = base(name, vec![op::STOP], None, 1_000_000);
        s.data = vec![byte; 1000];
        v.push(s);
    }
    v
}
