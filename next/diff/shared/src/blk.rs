//! Scenarios of group "blk": blocks of several transactions whose later transactions depend on
//! what the earlier ones wrote, used or created (`Scenario::prior`, [`crate::block`]). Both
//! harnesses run the transactions of a block on one EVM, in order, commit each outcome, and the
//! gates compare every transaction's record and the block's changes so far. Each block is a family
//! `<name>_t1`, `<name>_t2`, …
//!
//! What a block can show that a single transaction cannot: state a transaction leaves (storage,
//! nonces, balances, created accounts and code), the per-transaction state that must not leak into
//! the next transaction (Oracle hints, the access switch, detention, the limits' counters and the
//! gas ledgers), and what the earlier transactions made warm for SALT's capacity reads.
#![allow(dead_code)]
use super::{
    A, Account, Asm, B, BELOW_CAP, C, D, EMPTY, KEYLESS, Limits, ORACLE, SENDER, Scenario, acct,
    block, funded, op, scenario, spin, spin_after,
};
use crate::{
    edge::ONE_ETHER,
    q8::{ACCESS_CONTROL, keyless_call, mem_call, sel, send_hint},
};
use alloy_primitives::{Address, U256};

type Edit = Box<dyn Fn(&mut Scenario)>;

/// A counter: `SSTORE(1, SLOAD(1) + 1)`.
fn counter() -> Vec<u8> {
    Asm::new().push(1).op(op::SLOAD).push(1).op(op::ADD).push(1).op(op::SSTORE).stop().build()
}

fn blk(name: &'static str, rules: &[&'static str], accounts: Vec<Account>, gas: u64) -> Scenario {
    let mut s = scenario(name, accounts, gas, Limits::default());
    s.group = "blk";
    s.rules = rules.to_vec();
    s
}

fn to(a: Address) -> Edit {
    Box::new(move |s| s.to = a)
}

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    v.extend(state_carried());
    v.extend(per_transaction_state());
    v.extend(mechanisms());
    v
}

/// What one transaction leaves for the next: nonces, storage, balances, accounts and code.
fn state_carried() -> Vec<Scenario> {
    let mut v = Vec::new();
    // Three calls of one counter from one sender: the nonce counts up and the storage carries, the
    // first write is a fresh slot and the later ones are not.
    let first = blk(
        "blk_counter",
        &["SATIN-STATE-008", "SATIN-ACCT-016"],
        vec![acct(A, counter())],
        BELOW_CAP,
    );
    v.extend(block(first, vec![Box::new(|_| {}), Box::new(|_| {})]));
    // A fresh slot is written, rewritten to its original value in the next transaction (a slot
    // written back to what the transaction before it left is not a restore), and cleared in the
    // third.
    let writer = Asm::new().op(op::CALLDATASIZE).push(1).op(op::SSTORE).stop().build();
    let first = blk(
        "blk_slot_across_transactions",
        &["SATIN-STATE-008", "SATIN-ACCT-016"],
        vec![acct(A, writer)],
        BELOW_CAP,
    );
    let mut first = first;
    first.data = vec![1, 2, 3];
    v.extend(block(
        first,
        vec![
            Box::new(|s| s.data = vec![1, 2, 3, 4]),
            Box::new(|s| s.data = vec![]),
            Box::new(|s| s.data = vec![9]),
        ],
    ));
    // A nonce gap: the second transaction (nonce 5) is refused and leaves nothing; the third
    // (nonce 1) is the next in line.
    let first = blk(
        "blk_nonce_gap",
        &["SATIN-VAL-003", "SATIN-ACCT-006"],
        vec![acct(A, counter())],
        BELOW_CAP,
    );
    v.extend(block(first, vec![Box::new(|s| s.nonce = 5), Box::new(|s| s.nonce = 1)]));
    // A replayed transaction (the same nonce twice) is refused the second time.
    let first = blk("blk_nonce_replayed", &["SATIN-VAL-003"], vec![acct(A, counter())], BELOW_CAP);
    v.extend(block(first, vec![Box::new(|s| s.nonce = 0)]));
    // Balances: the sender sends nearly everything, then cannot send as much again.
    let mut first = blk(
        "blk_sender_balance",
        &["SATIN-ACCT-005"],
        vec![funded(SENDER, vec![], 500_000_000), acct(B, vec![])],
        BELOW_CAP,
    );
    first.to = B;
    first.value = U256::from(400_000_000u64);
    first.gas_price = 1;
    first.gas_limit = 400_000;
    v.extend(block(
        first,
        vec![
            Box::new(|s| s.value = U256::from(40_000_000u64)),
            Box::new(|s| s.value = U256::from(400_000_000u64)),
        ],
    ));
    // The recipient of a value transfer is created by the first transaction (a fresh leaf), and
    // the second pays it again, a leaf that exists now (no creation charge, no growth).
    let mut first = blk(
        "blk_value_to_new_then_existing",
        &["SATIN-ACCT-013", "SATIN-ACCT-015", "SATIN-HIST-010"],
        vec![],
        BELOW_CAP,
    );
    first.to = EMPTY;
    first.value = U256::from(7u64);
    v.extend(block(first, vec![Box::new(|_| {}), Box::new(|_| {})]));
    // A creation transaction, then calls of what it created.
    let init = {
        let runtime = Asm::new()
            .push(1)
            .op(op::SLOAD)
            .push(1)
            .op(op::ADD)
            .push(1)
            .op(op::SSTORE)
            .stop()
            .build();
        let mut code = Asm::new();
        for (i, chunk) in runtime.chunks(32).enumerate() {
            let mut w = [0u8; 32];
            w[..chunk.len()].copy_from_slice(chunk);
            code = code.push_bytes(&w).push(32 * i as u64).op(op::MSTORE);
        }
        code.push(runtime.len() as u64).push(0).op(op::RETURN).build()
    };
    let mut first =
        blk("blk_create_then_call", &["SATIN-SCHED-011", "SATIN-STATE-004"], vec![], BELOW_CAP);
    first.create = true;
    first.data = init;
    let created = SENDER.create(0);
    v.extend(block(
        first,
        vec![
            Box::new(move |s| {
                s.create = false;
                s.data = Vec::new();
                s.to = created;
            }),
            Box::new(move |s| {
                s.create = false;
                s.data = Vec::new();
                s.to = created;
            }),
        ],
    ));
    // A deposit creates its caller (a fresh account pays the new-account charge); the second
    // transaction is that caller's own, whose account now exists.
    let mut first = blk(
        "blk_deposit_creates_caller",
        &["SATIN-DEP-003", "SATIN-ACCT-020"],
        vec![acct(A, counter())],
        BELOW_CAP,
    );
    first.sender = D;
    first.deposit = Some(1_000_000_000_000);
    first.accounts.retain(|a| a.address != D);
    v.extend(block(
        first,
        vec![Box::new(|s| {
            s.sender = D;
            s.deposit = None;
            s.gas_price = 0;
            // The deposit left its caller at nonce 1.
            s.nonce = 1;
        })],
    ));
    // Two deposits from one fresh caller: the second finds the account the first created.
    let mut first = blk(
        "blk_two_deposits_one_caller",
        &["SATIN-DEP-003", "SATIN-ACCT-020"],
        vec![acct(A, counter())],
        BELOW_CAP,
    );
    first.sender = D;
    first.deposit = Some(0);
    first.accounts.retain(|a| a.address != D);
    v.extend(block(first, vec![Box::new(|_| {})]));
    // A contract creates a contract and destroys it in one transaction; the next transaction
    // sends value to the destroyed address and calls it.
    let child_init = Asm::new().push(0).op(op::SELFDESTRUCT).build();
    let creator = Asm::new().create(&child_init, 0).op(op::POP).stop().build();
    let mut first = blk(
        "blk_destroyed_then_called",
        &["SATIN-SD-001", "SATIN-SD-004"],
        vec![acct(A, creator)],
        BELOW_CAP,
    );
    first.gas_limit = 5_000_000;
    let destroyed = A.create(1);
    v.extend(block(
        first,
        vec![Box::new(move |s| {
            s.to = destroyed;
            s.value = U256::from(5u64);
        })],
    ));
    v
}

/// What must not leak from one transaction into the next.
fn per_transaction_state() -> Vec<Scenario> {
    let mut v = Vec::new();
    // Detention: the first transaction reads the timestamp and so is detained; the second only
    // computes, and is not.
    let reads_time = spin_after(Asm::new().op(op::TIMESTAMP).op(op::POP), 2_000);
    let plain = spin(2_000);
    let mut first = blk(
        "blk_detention_not_carried",
        &["SATIN-DET-012", "SATIN-DET-001"],
        vec![acct(A, reads_time), acct(B, plain)],
        BELOW_CAP,
    );
    first.limits.block_env_cap = Some(10_000);
    v.extend(block(first, vec![to(B), to(A), to(B)]));
    // The beneficiary: a transaction from the beneficiary is detained from its start; the next
    // one from an ordinary sender is not.
    let mut first = blk(
        "blk_beneficiary_sender_then_user",
        &["SATIN-DET-008", "SATIN-DET-007"],
        vec![acct(A, counter()), funded(C, vec![], ONE_ETHER)],
        BELOW_CAP,
    );
    first.beneficiary = C;
    first.sender = C;
    first.limits.block_env_cap = Some(1_000);
    v.extend(block(first, vec![Box::new(|s| s.sender = SENDER), Box::new(|s| s.sender = C)]));
    // The access switch of MegaAccessControl dies with its frame; the next transaction reads.
    let disable = sel("disableVolatileDataAccess()");
    let disabler = mem_call(Asm::new(), op::CALL, ACCESS_CONTROL, 0, 100_000, &disable)
        .op(op::POP)
        .op(op::TIMESTAMP)
        .op(op::POP)
        .stop()
        .build();
    let reader = Asm::new().op(op::TIMESTAMP).sstore(2, 1).stop().build();
    let first = blk(
        "blk_access_switch_not_carried",
        &["SATIN-MAC-001", "SATIN-MAC-003"],
        vec![acct(A, disabler), acct(B, reader)],
        BELOW_CAP,
    );
    v.extend(block(first, vec![to(B), to(A), to(B)]));
    // Resource limits count per transaction: the first is stopped at its growth limit, the
    // second (same limit) starts from zero and fits.
    let writes = Asm::new().sstore(1, 1).sstore(2, 1).sstore(3, 1).stop().build();
    let mut first = blk(
        "blk_limits_reset",
        &["SATIN-LIM-001", "SATIN-STOP-009"],
        vec![acct(A, writes), acct(B, Asm::new().sstore(1, 1).stop().build())],
        BELOW_CAP,
    );
    first.limits.growth = Some(2);
    v.extend(block(first, vec![to(B), Box::new(|_| {})]));
    // A refund (a slot written back to its original value) in one transaction does not carry:
    // the next transaction's own write of the same slot is a restore of its own original.
    let flip =
        Asm::new().push(1).op(op::SLOAD).op(op::ISZERO).push(1).op(op::SSTORE).stop().build();
    let first = blk(
        "blk_slot_flip_flop",
        &["SATIN-ACCT-016", "SATIN-HIST-012"],
        vec![acct(A, flip)],
        BELOW_CAP,
    );
    v.extend(block(first, vec![Box::new(|_| {}), Box::new(|_| {}), Box::new(|_| {})]));
    v
}

/// The mechanisms with per-transaction state of their own: Oracle hints, the Oracle's reads, the
/// keyless deployment.
fn mechanisms() -> Vec<Scenario> {
    let mut v = Vec::new();
    let oracle = |code: Vec<u8>| Account {
        address: ORACLE,
        code,
        storage: vec![(5, 9)],
        ..Account::default()
    };
    let oracle_stop = oracle(Asm::new().push(1).push(0).op(op::LOG0).stop().build());
    let hint = send_hint(7, &[0xab; 64]);
    let sends_hint =
        mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint).record_call(1).stop().build();
    // A hint in the first transaction; none in the next two. The record of a transaction holds
    // its own hints only.
    let first = blk(
        "blk_hint_then_none",
        &["SATIN-ORCL-006", "SATIN-ORCL-009"],
        vec![acct(A, sends_hint.clone()), acct(B, counter()), oracle_stop.clone()],
        BELOW_CAP,
    );
    v.extend(block(first, vec![to(B), to(A), to(B)]));
    // The hint's bytes count on its own transaction's data-size limit only: with a limit that
    // fits one hint, every transaction that sends one fits.
    let mut first = blk(
        "blk_hint_limit_per_transaction",
        &["SATIN-ORCL-010", "SATIN-LIM-012"],
        vec![acct(A, sends_hint.clone()), oracle_stop.clone()],
        BELOW_CAP,
    );
    first.limits.data = Some(310 + 100 + 164);
    v.extend(block(first, vec![Box::new(|_| {}), Box::new(|_| {})]));
    // A hint sent in a child that reverts, then the next transaction's child that reverts without
    // one (a caller resumes from a failed child): the first transaction's hint must not make the
    // second's resume a check against the limits.
    let b_hint_revert =
        mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint).op(op::POP).revert0().build();
    let b_revert = Asm::new().sstore(9, 9).revert0().build();
    let a_calls = Asm::new()
        .call_all(op::CALL, B, 0)
        .record_call(1)
        .call_all(op::CALL, D, 0)
        .record_call(3)
        .stop()
        .build();
    let mut first = blk(
        "blk_hint_in_failed_child_then_failed_child",
        &["SATIN-STOP-012", "SATIN-STOP-006"],
        vec![
            acct(A, a_calls),
            acct(B, b_hint_revert),
            acct(D, b_revert.clone()),
            oracle_stop.clone(),
        ],
        BELOW_CAP,
    );
    first.limits.data = Some(310 + 164 + 60);
    v.extend(block(
        first,
        vec![Box::new(|s| {
            // Swap the hinting child for the plain failing one.
            if let Some(b) = s.accounts.iter_mut().find(|a| a.address == B) {
                b.code = Asm::new().sstore(9, 9).revert0().build();
            }
        })],
    ));
    // The Oracle's storage read (through the service) in one transaction, none in the next.
    let reads_oracle = Asm::new()
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push(0)
        .push_addr(ORACLE)
        .op(op::GAS)
        .op(op::CALL)
        .record_call(1)
        .store_returndata_word(3)
        .stop()
        .build();
    let oracle_reader = oracle(
        Asm::new()
            .push(5)
            .op(op::SLOAD)
            .push(0)
            .op(op::MSTORE)
            .push(32)
            .push(0)
            .op(op::RETURN)
            .build(),
    );
    let mut first = blk(
        "blk_oracle_read_then_plain",
        &["SATIN-ORCL-002", "SATIN-ORCL-003"],
        vec![acct(A, reads_oracle), acct(B, counter()), oracle_reader],
        BELOW_CAP,
    );
    first.oracle = vec![(5, 1234)];
    v.extend(block(first, vec![to(B), to(A)]));
    // KeylessDeploy twice: the second finds the address occupied.
    let (call, signer, _deploy) = keyless_call("ok", 0);
    let mut first = blk(
        "blk_keyless_twice",
        &["SATIN-KEY-002", "SATIN-KEY-005"],
        vec![funded(signer, vec![], 100_000_000_000_000_000)],
        10_000_000,
    );
    first.to = KEYLESS;
    first.data = call;
    first.gas_limit = 5_000_000;
    v.extend(block(first, vec![Box::new(|_| {})]));
    v
}
