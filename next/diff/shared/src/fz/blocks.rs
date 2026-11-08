//! Fuzz mode 15: blocks of several transactions. Seed `s` draws one block of two to four
//! transactions over one world of small programs (a counter, a toggled slot, an Oracle hint, a
//! read of the block environment under detention, a plain loop, a value transfer, a creation, a
//! chain of calls) and the block's environment (limits, caps, SALT multipliers, Oracle answers,
//! beneficiary); each transaction draws its envelope (a call, a value transfer, a creation, a
//! type-2 transaction, a deposit from a fresh caller, a wrong nonce) and gas. The block's
//! transactions run on one EVM in order, each committed ([`crate::block`]), so what a transaction
//! leaves (storage, nonces, balances, created accounts) is what the next one starts from and the
//! per-transaction state (hints, the access switch, detention, the limits' counters) must not
//! leak. A step is a scenario named `<mode prefix><seed>_t<k>`.

use super::Rng;
use crate::{
    A, Account, Asm, B, C, D, EMPTY, IDENTITY, Limits, ORACLE, SENDER, Scenario, acct, block,
    funded, init_returning, op,
    q8::{ACCESS_CONTROL, mem_call, sel, send_hint},
    scenario, spin, spin_after,
};
use alloy_primitives::{Address, U256};

const PEER: Address = Address::with_last_byte(0xe1);
const W1: Address = Address::with_last_byte(0xa1);
const W2: Address = Address::with_last_byte(0xa2);
const W3: Address = Address::with_last_byte(0xa3);
const W4: Address = Address::with_last_byte(0xa4);
const W5: Address = Address::with_last_byte(0xa5);
const ONE_ETHER: u64 = 1_000_000_000_000_000_000;

/// The programs a transaction can call.
fn programs() -> Vec<(Address, Vec<u8>)> {
    let counter =
        Asm::new().push(1).op(op::SLOAD).push(1).op(op::ADD).push(1).op(op::SSTORE).stop().build();
    let flip =
        Asm::new().push(2).op(op::SLOAD).op(op::ISZERO).push(2).op(op::SSTORE).stop().build();
    let hint = send_hint(7, &[0xcd; 64]);
    let sends_hint =
        mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint).record_call(1).stop().build();
    let sends_hint_reverts =
        mem_call(Asm::new(), op::CALL, ORACLE, 0, 200_000, &hint).op(op::POP).revert0().build();
    let reads_time = spin_after(Asm::new().op(op::TIMESTAMP).op(op::POP), 1_500);
    let plain = spin(1_500);
    let pays = Asm::new().call_all(op::CALL, EMPTY, 1).record_call(3).stop().build();
    let child = Asm::new().create(&init_returning(1), 0).push(4).op(op::SSTORE).stop().build();
    let chain = Asm::new()
        .call_all(op::CALL, A, 0)
        .op(op::POP)
        .call_all(op::CALL, B, 0)
        .op(op::POP)
        .call_all(op::CALL, W3, 0)
        .record_call(5)
        .stop()
        .build();
    let _ = (sends_hint_reverts.clone(), IDENTITY, ACCESS_CONTROL, sel("x()"));
    vec![
        (A, counter),
        (B, flip),
        (C, sends_hint),
        (D, reads_time),
        (W1, plain),
        (W2, pays),
        (W3, child),
        (W4, chain),
        (W5, sends_hint_reverts),
    ]
}

/// The step count of seed `seed`.
pub fn steps_of(seed: u64) -> u64 {
    2 + Rng::for_seed(seed.wrapping_mul(31).wrapping_add(7)).below(3)
}

/// The block of seed `seed`: its steps in order.
pub fn generate(seed: u64) -> Vec<Scenario> {
    let mut r = Rng::for_seed(seed);
    let progs = programs();
    let mut accounts: Vec<Account> = progs.iter().map(|(a, c)| acct(*a, c.clone())).collect();
    accounts.push(funded(PEER, Vec::new(), ONE_ETHER));
    accounts.push(funded(W2, progs[5].1.clone(), 50));
    accounts.push(Account {
        address: ORACLE,
        code: Asm::new()
            .push(5)
            .op(op::SLOAD)
            .push(0)
            .op(op::MSTORE)
            .push(32)
            .push(0)
            .op(op::RETURN)
            .build(),
        storage: vec![(5, 9)],
        ..Account::default()
    });
    // Existing storage the programs rewrite.
    if let Some(a) = accounts.iter_mut().find(|a| a.address == B) {
        a.storage = vec![(2, 1 + r.below(2))];
    }
    let mut s = scenario(
        "",
        accounts,
        r.pick(&[450_000u64, 700_000, crate::BELOW_CAP, 5_000_000]),
        Limits::default(),
    );
    // The block's environment.
    if r.chance(30) {
        s.limits.data = Some(400 + r.below(1_200));
    }
    if r.chance(25) {
        s.limits.kv = Some(1 + r.below(3));
    }
    if r.chance(25) {
        s.limits.growth = Some(1 + r.below(3));
    }
    if r.chance(40) {
        s.limits.block_env_cap = Some(r.pick(&[3_000u64, 12_000, 30_000, 60_000]));
    }
    if r.chance(25) {
        s.limits.oracle_cap = Some(r.pick(&[3_000u64, 12_000, 30_000]));
    }
    if r.chance(35) {
        s.salt = vec![
            (A, Some(1), r.pick(&[2u64, 4, 16])),
            (B, Some(2), r.pick(&[2u64, 8])),
            (EMPTY, None, r.pick(&[2u64, 4])),
        ];
    }
    if r.chance(30) {
        s.oracle = vec![(5, 100 + r.below(1_000))];
    }
    s.beneficiary = r.pick(&[Address::ZERO, A, SENDER, PEER, W1]);
    let targets: Vec<Address> = progs.iter().map(|(a, _)| *a).collect();
    // The first transaction and the edits of the others.
    let n = steps_of(seed) as usize;
    let mut txs: Vec<crate::Edit> = Vec::new();
    let mut first = s.clone();
    let draw = |r: &mut Rng, into_first: bool| -> Box<dyn Fn(&mut Scenario)> {
        let kind = r.below(100);
        let target = r.pick(&targets);
        let gas = if r.chance(8) {
            120_000
        } else {
            r.pick(&[450_000u64, 700_000, crate::BELOW_CAP, crate::ABOVE_CAP, 40_000_000])
        };
        let value = r.pick(&[0u64, 0, 1, 7]);
        let bad_nonce = r.chance(8);
        let from_e1 = r.chance(15);
        let edit: Box<dyn Fn(&mut Scenario)> = Box::new(move |s: &mut Scenario| {
            // Every transaction starts from the defaults of the transaction fields.
            s.create = false;
            s.data = Vec::new();
            s.tx_type = 0;
            s.gas_price = 0;
            s.deposit = None;
            s.value = U256::ZERO;
            s.gas_limit = gas;
            s.sender = if from_e1 { PEER } else { SENDER };
            match kind {
                0..=64 => {
                    s.to = target;
                    s.value = U256::from(if target == W2 || target == C || target == A {
                        0
                    } else {
                        value
                    });
                }
                65..=76 => {
                    // A value transfer to a fresh or an existing account.
                    s.to = if kind.is_multiple_of(2) { EMPTY } else { PEER };
                    s.value = U256::from(1 + value);
                }
                77..=84 => {
                    s.create = true;
                    s.data = init_returning(2);
                }
                85..=91 => {
                    s.tx_type = 2;
                    s.to = target;
                    s.gas_price = 1;
                }
                _ => {
                    // A deposit from a caller the block does not hold yet.
                    s.sender = D;
                    s.deposit = Some(r_mint(kind));
                    s.to = target;
                }
            }
            if bad_nonce {
                s.nonce = 3;
            }
        });
        let _ = into_first;
        edit
    };
    // The first transaction takes its fields from the first draw.
    let first_edit = draw(&mut r, true);
    first_edit(&mut first);
    // nonce 3 on the first transaction would be an unexplained refusal: the first is valid.
    if first.nonce == 3 {
        first.nonce = 0;
    }
    for _ in 1..n {
        txs.push(draw(&mut r, false));
    }
    let mut steps = block(first, txs);
    for (k, step) in steps.iter_mut().enumerate() {
        step.name = Box::leak(format!("{}_t{}", super::name(15, seed), k + 1).into_boxed_str());
        for p in step.prior.iter_mut() {
            p.group = "blk";
        }
    }
    steps
}

fn r_mint(kind: u64) -> u64 {
    (kind + 1) * 1_000_000_000
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seed is a block of two to four steps, named after the seed and the position, each
    /// carrying the steps before it; the same seed draws the same block.
    #[test]
    fn test_a_seed_is_a_block_of_named_steps() {
        for seed in 1..=60 {
            let a = generate(seed);
            assert_eq!(a.len() as u64, steps_of(seed));
            assert!((2..=4).contains(&a.len()));
            for (k, s) in a.iter().enumerate() {
                assert_eq!(s.name, format!("{}_t{}", super::super::name(15, seed), k + 1));
                assert_eq!(s.prior.len(), k);
                assert_eq!(super::super::parse_name(s.name), Some((15, seed)));
            }
            let b = generate(seed);
            assert_eq!(format!("{a:?}"), format!("{b:?}"));
        }
        assert_eq!(super::super::parse_name("fxb0001_t9"), None);
    }
}
