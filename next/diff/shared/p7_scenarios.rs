//! Detention scenarios (group "p7"), as complete transactions:
//! each runs with TIMESTAMP first (detained) and PUSH0 first (no read), tx gas 10M, its cap.
#![allow(dead_code)]
use super::{Account, Limits, Scenario};
use alloy_primitives::{Address, U256};

pub const CALLER: [u8; 20] = addr(0xd00000);
pub const CONTRACT: [u8; 20] = addr(0xd00001);
pub const CHILD: [u8; 20] = addr(0xd00002);
pub const GRANDCHILD: [u8; 20] = addr(0xd00013);

/// The gas limit of every transaction (below Satin's 200M execution cap: no reservoir).
pub const TX_GAS: u64 = 10_000_000;
/// The gas the wrapper forwards to `CHILD`.
pub const CHILD_GAS: u32 = 1_000_000;

const fn addr(n: u32) -> [u8; 20] {
    let mut a = [0u8; 20];
    a[17] = (n >> 16) as u8;
    a[18] = (n >> 8) as u8;
    a[19] = n as u8;
    a
}

pub mod op {
    pub const STOP: u8 = 0x00;
    pub const ADD: u8 = 0x01;
    pub const POP: u8 = 0x50;
    pub const MLOAD: u8 = 0x51;
    pub const MSTORE: u8 = 0x52;
    pub const SLOAD: u8 = 0x54;
    pub const SSTORE: u8 = 0x55;
    pub const JUMP: u8 = 0x56;
    pub const GAS: u8 = 0x5a;
    pub const JUMPDEST: u8 = 0x5b;
    pub const PUSH0: u8 = 0x5f;
    pub const PUSH1: u8 = 0x60;
    pub const PUSH3: u8 = 0x62;
    pub const PUSH4: u8 = 0x63;
    pub const PUSH20: u8 = 0x73;
    pub const TIMESTAMP: u8 = 0x42;
    pub const RETURNDATASIZE: u8 = 0x3d;
    pub const RETURNDATACOPY: u8 = 0x3e;
    pub const CALL: u8 = 0xf1;
    pub const RETURN: u8 = 0xf3;
}
use op::*;

pub struct P7Scenario {
    pub name: &'static str,
    /// Detention's block-environment cap.
    pub cap: u64,
    /// `(address, code)`.
    pub contracts: Vec<([u8; 20], Vec<u8>)>,
    /// The emulation fixes this scenario exercises (for the negative controls).
    pub exercises: &'static str,
}

fn call_all(c: &mut Vec<u8>, to: [u8; 20]) {
    c.extend([PUSH0, PUSH0, PUSH0, PUSH0, PUSH0, PUSH20]);
    c.extend(to);
    c.extend([GAS, CALL]);
}

/// The transaction's frame: records `GAS` before the call (word 0), calls `CHILD` with
/// `CHILD_GAS` and records the status (word 1), `GAS` after (word 2), the return data size
/// (word 3) and the return data (from 0x80), and returns them.
pub fn wrapper() -> Vec<u8> {
    let mut c = vec![GAS, PUSH0, MSTORE];
    c.extend([PUSH0, PUSH0, PUSH0, PUSH0, PUSH0, PUSH20]);
    c.extend(CHILD);
    c.push(PUSH4);
    c.extend(CHILD_GAS.to_be_bytes());
    c.push(CALL);
    c.extend([PUSH1, 0x20, MSTORE]);
    c.extend([GAS, PUSH1, 0x40, MSTORE]);
    c.extend([RETURNDATASIZE, PUSH1, 0x60, MSTORE]);
    c.extend([RETURNDATASIZE, PUSH0, PUSH1, 0x80, RETURNDATACOPY]);
    c.extend([RETURNDATASIZE, PUSH1, 0x80, ADD, PUSH0, RETURN]);
    c
}

/// `PUSH3 offset MLOAD`: about 292,800 gas of memory expansion, above the cap's spendable part and
/// below the frame's whole gas.
const MID_MEM: [u8; 5] = [PUSH3, 0x05, 0x9d, 0x80, MLOAD];
/// A memory expansion to 16 MiB: far more than any frame here has.
const HUGE_MEM: [u8; 6] = [PUSH4, 0x01, 0x00, 0x00, 0x00, MLOAD];

pub fn p7_raw(first: u8) -> Vec<P7Scenario> {
    let w = || (CONTRACT, wrapper());
    let read = [first, POP];
    let mut v = Vec::new();

    let mut c = read.to_vec();
    c.extend([GAS, PUSH0, MSTORE, PUSH1, 0x20, PUSH0, RETURN]);
    v.push(P7Scenario {
        name: "gas_reads_total",
        cap: 50_000,
        contracts: vec![w(), (CHILD, c)],
        exercises: "GAS",
    });

    let mut c = read.to_vec();
    c.extend([JUMPDEST, PUSH1, 2, JUMP]);
    v.push(P7Scenario {
        name: "static_crossing",
        cap: 100_000,
        contracts: vec![w(), (CHILD, c)],
        exercises: "classify",
    });

    let mut c = read.to_vec();
    c.extend(MID_MEM);
    c.push(STOP);
    v.push(P7Scenario {
        name: "memory_crossing",
        cap: 100_000,
        contracts: vec![w(), (CHILD, c)],
        exercises: "classify",
    });

    let mut c = read.to_vec();
    c.extend(HUGE_MEM);
    c.push(STOP);
    v.push(P7Scenario {
        name: "real_oog_halts",
        cap: 100_000,
        contracts: vec![w(), (CHILD, c)],
        exercises: "classify (negative)",
    });

    let mut c = read.to_vec();
    call_all(&mut c, GRANDCHILD);
    c.extend([POP, RETURNDATASIZE, PUSH0, PUSH0, RETURNDATACOPY, RETURNDATASIZE, PUSH0, RETURN]);
    let gc = vec![GAS, PUSH0, MSTORE, PUSH1, 0x20, PUSH0, RETURN];
    v.push(P7Scenario {
        name: "forward_from_total",
        cap: 100_000,
        contracts: vec![w(), (CHILD, c), (GRANDCHILD, gc)],
        exercises: "forward",
    });

    let mut c = read.to_vec();
    call_all(&mut c, GRANDCHILD);
    c.extend([PUSH0, MSTORE, GAS, PUSH1, 0x20, MSTORE, RETURNDATASIZE, PUSH1, 0x40, MSTORE]);
    c.extend([PUSH1, 0x60, PUSH0, RETURN]);
    let mut gc = MID_MEM.to_vec();
    gc.push(STOP);
    v.push(P7Scenario {
        name: "grandchild_crossing_returns_all",
        cap: 100_000,
        contracts: vec![w(), (CHILD, c), (GRANDCHILD, gc)],
        exercises: "after (W back)",
    });

    let mut c = read.to_vec();
    c.extend([PUSH1, 1, PUSH0, SSTORE, STOP]);
    v.push(P7Scenario {
        name: "sstore_sentry_sees_total",
        cap: 1_007,
        contracts: vec![w(), (CHILD, c.clone())],
        exercises: "SSTORE copy (sentry)",
    });
    v.push(P7Scenario {
        name: "sstore_spill_withheld_first",
        cap: 20_007,
        contracts: vec![w(), (CHILD, c)],
        exercises: "SSTORE copy (spill)",
    });

    let mut c = read.to_vec();
    c.extend([PUSH0, SLOAD, STOP]);
    v.push(P7Scenario {
        name: "sload_skip_cold_sees_total",
        cap: 1_005,
        contracts: vec![w(), (CHILD, c)],
        exercises: "SLOAD copy (skip-cold)",
    });
    v
}

/// Every detention scenario, detained (TIMESTAMP) and not (PUSH0).
pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    for (first, suffix) in [(op::TIMESTAMP, "read"), (op::PUSH0, "noread")] {
        for p in p7_raw(first) {
            let mut accounts = vec![Account {
                address: Address::from(CALLER),
                balance: U256::from(1_000_000_000_000_000_000u64),
                ..Account::default()
            }];
            for (a, code) in p.contracts {
                accounts.push(Account { address: Address::from(a), code, ..Account::default() });
            }
            let name: &'static str =
                Box::leak(format!("p7_{}_{}", p.name, suffix).into_boxed_str());
            let mut s = super::scenario(
                name,
                Vec::new(),
                TX_GAS,
                Limits { block_env_cap: Some(p.cap), ..Limits::default() },
            );
            s.accounts = accounts;
            s.sender = Address::from(CALLER);
            s.to = Address::from(CONTRACT);
            s.group = "p7";
            v.push(s);
        }
    }
    v
}
