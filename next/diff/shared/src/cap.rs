//! The EIP-8037 total transaction gas cap (`TX_MAX_TOTAL_GAS_LIMIT` = 2^32 − 1) is not applied:
//! a transaction's gas limit is bounded by the block's gas limit alone (`BLOCK_GAS_LIMIT` here).
//! Each transaction kind the shared format expresses (legacy, OP deposit, EIP-7702) runs at gas
//! limit 2^32 − 1, 2^32 and the block's gas limit, calling a contract that stores 1 in slot 1, so
//! an accepted transaction shows in the compared fields.
//!
//! No scenario is a system-address transaction. Its promotion to a deposit happens in the
//! transaction handler (`prepare_legacy`, reached by plain `transact`), but only for the live
//! system address, which needs the `SequencerRegistry`'s trusted code; the scenario fixtures give
//! the registry a one-byte placeholder code, so no scenario has one. The recorded blocks cover the
//! path: every transaction of the corpus with a gas limit of 2^32 or more is a system-address call
//! to the Oracle (2,367 in the full corpus, 133 in the sample), compared by the replay and block
//! gates.
#![allow(dead_code)]
use super::{Account, Asm, Auth, BLOCK_GAS_LIMIT, Limits, Scenario, acct, op, scenario};
use alloy_primitives::Address;

const AUTHORITY: Address = Address::with_last_byte(0xc0);

fn cap(name: &'static str, gas: u64) -> Scenario {
    let code = Asm::new().sstore(1, 1).stop().build();
    let mut s = scenario(name, vec![acct(super::A, code)], gas, Limits::default());
    s.group = "cap";
    s
}

pub fn all() -> Vec<Scenario> {
    // (legacy, deposit, EIP-7702) names per gas limit.
    let rows: [(u64, [&'static str; 3]); 3] = [
        ((1 << 32) - 1, ["cap_legacy_2p32m1", "cap_deposit_2p32m1", "cap_7702_2p32m1"]),
        (1 << 32, ["cap_legacy_2p32", "cap_deposit_2p32", "cap_7702_2p32"]),
        (BLOCK_GAS_LIMIT, ["cap_legacy_block", "cap_deposit_block", "cap_7702_block"]),
    ];
    let mut v = Vec::new();
    for (gas, [legacy, deposit, eip7702]) in rows {
        v.push(cap(legacy, gas));
        let mut s = cap(deposit, gas);
        s.deposit = Some(0);
        v.push(s);
        let mut s = cap(eip7702, gas);
        s.auths =
            vec![Auth { chain_id: 1, address: super::D, nonce: 0, authority: Some(AUTHORITY) }];
        s.accounts.push(Account { address: super::D, code: vec![op::STOP], ..Account::default() });
        v.push(s);
    }
    v
}
