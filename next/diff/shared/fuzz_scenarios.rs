//! Seeded random compositions (group "fz"): programs built from mechanism fragments (fresh and
//! restoring writes, logs, value/zero-value calls of every scheme to code, empty accounts and
//! precompiles, creations with deposits, block-environment reads, memory, account opcodes,
//! reverts, halts, loops, self-destruction), random caps, limits, SALT multipliers, gas
//! (below/above the execution cap, thin reservoirs) and deposits. Same bytes on both engines.
#![allow(dead_code)]
use super::{
    A, Account, Asm, Auth, B, C, D, EMPTY, IDENTITY, KEYLESS, Limits, ORACLE, Scenario,
    init_returning, op,
    q8::{ACCESS_CONTROL, KEYLESS_TXS, X1, X2, keyless_call, mem_call, sel, send_hint},
    scenario,
};
use alloy_primitives::{Address, U256};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<T: Copy>(&mut self, v: &[T]) -> T {
        v[self.below(v.len() as u64) as usize]
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

const SHA256: Address = Address::with_last_byte(2);
const EMPTY2: Address =
    Address::new([0x10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xe2]);

/// One stack-neutral fragment. `depth` 0 = A, 1 = B, 2 = C (C makes no calls).
fn harsh() -> bool {
    std::env::var("MEGA_EVM2_FUZZ_MODE").is_ok_and(|m| m == "2" || m == "4")
}

/// `MEGA_EVM2_FUZZ_MODE=3` (default generator) and `4` (harsh) add the later mechanisms: EIP-7702
/// lists, keyless deployments, `MegaAccessControl`, Oracle hints and reads, the precompile
/// catalogue. Modes 1 and 2 stay byte-identical to the original generator's.
fn mechanisms_mode() -> bool {
    std::env::var("MEGA_EVM2_FUZZ_MODE").is_ok_and(|m| m == "3" || m == "4")
}

/// A later-mechanism fragment (stack-neutral).
fn mechanism_fragment(r: &mut Rng) -> Asm {
    let a = Asm::new();
    match r.below(7) {
        6 => {
            // A call of X2, which may hold a pre-existing EIP-7702 delegation.
            let value = if r.chance(20) { 1 } else { 0 };
            a.call_with(
                r.pick(&[op::CALL, op::STATICCALL, op::DELEGATECALL]),
                X2,
                0,
                r.pick(&[30_000u64, 1_000_000]),
                0,
            )
            .op(op::POP)
            .ops(&if value == 1 {
                Asm::new().call_with(op::CALL, X2, 1, 50_000, 0).op(op::POP).build()
            } else {
                Vec::new()
            })
        }
        0 => {
            let s = r.pick(&[
                "disableVolatileDataAccess()",
                "disableVolatileDataAccess()",
                "enableVolatileDataAccess()",
                "isVolatileDataAccessDisabled()",
            ]);
            let value = if r.chance(10) { 1 } else { 0 };
            mem_call(a, op::CALL, ACCESS_CONTROL, value, r.pick(&[0u64, 50_000]), &sel(s))
                .op(op::POP)
        }
        1 | 2 => {
            let len = r.pick(&[0usize, 1, 32, 100, 300]);
            let hint = send_hint(r.below(3), &vec![0x5a; len]);
            let kind = r.pick(&[op::CALL, op::CALL, op::STATICCALL, op::DELEGATECALL]);
            let value = if kind == op::CALL && r.chance(10) { 1 } else { 0 };
            mem_call(a, kind, ORACLE, value, r.pick(&[0u64, 60_000, 1_000_000]), &hint).op(op::POP)
        }
        3 => a.call_with(op::CALL, ORACLE, 0, r.pick(&[3_000u64, 100_000]), 0).op(op::POP),
        4 => {
            let pc = |b: u16| {
                let mut x = [0u8; 20];
                x[18] = (b >> 8) as u8;
                x[19] = b as u8;
                Address::new(x)
            };
            let (addr, len) = r.pick(&[
                (0x0bu16, 256usize),
                (0x0c, 160),
                (0x0c, 480),
                (0x0e, 288),
                (0x0f, 384),
                (0x10, 64),
                (0x11, 128),
                (0x08, 192),
                (0x09, 213),
                (0x100, 160),
                (0x05, 96),
            ]);
            let mut input = vec![0u8; len];
            if addr == 0x09 {
                input[..4].copy_from_slice(&r.pick(&[12u32, 5_000, 40_000]).to_be_bytes());
            }
            if addr == 0x05 {
                input[31] = 1;
                input[63] = r.pick(&[1u8, 32, 64]);
                input[95] = 1;
                input.resize(96 + 2 + input[63] as usize, 0xff);
            }
            mem_call(a, op::CALL, pc(addr), 0, r.pick(&[30_000u64, 1_000_000]), &input).op(op::POP)
        }
        _ => {
            // keylessDeploy from a contract: not dispatched (the contract's own code runs).
            let (data, _, _) = keyless_call(r.pick(&["ok", "revert"]), 1_000_000);
            mem_call(a, op::CALL, KEYLESS, 0, 300_000, &data).op(op::POP)
        }
    }
}

fn fragment(r: &mut Rng, depth: u8) -> Asm {
    let a = Asm::new();
    if mechanisms_mode() && r.chance(25) {
        return mechanism_fragment(r);
    }
    match r.below(if depth >= 2 { 9 } else { 13 }) {
        0 => a.op(op::TIMESTAMP).op(op::POP),
        1 | 2 => a.sstore(r.below(6) + 1, r.pick(&[0, 1, 1, 2])),
        3 => {
            let len = r.pick(&[0u64, 32, 100, 200]);
            match r.below(3) {
                0 => a.push(len).push(0).op(op::LOG0),
                1 => a.push(9).push(len).push(0).op(op::LOG1),
                _ => a.push(3).push(2).push(1).push(len).push(0).op(op::LOG3),
            }
        }
        4 => a.push(r.pick(&[0u64, 64, 1024, 4096])).op(op::MLOAD).op(op::POP),
        5 => a
            .push_addr(r.pick(&[B, C, EMPTY, EMPTY2, Address::ZERO]))
            .op(r.pick(&[op::BALANCE, op::EXTCODESIZE, 0x3f]))
            .op(op::POP),
        6 => a.push(r.below(8)).op(op::SLOAD).op(op::POP),
        7 => a.op(op::NUMBER).op(op::POP).push(r.below(3)).push(r.below(6) + 1).op(op::SSTORE),
        8 => a.push(r.pick(&[1u64, 7, 64])).push(0).op(op::LOG0),
        _ => {
            // a call or a creation
            if r.chance(25) {
                let n = r.pick(&[0u64, 1, 10, 100]);
                let mut init =
                    if r.chance(30) { Asm::new().sstore(1, 1).build() } else { Vec::new() };
                if r.chance(15) {
                    init.extend(Asm::new().op(op::TIMESTAMP).op(op::POP).build());
                }
                init.extend(init_returning(n));
                let value = if r.chance(20) { 1 } else { 0 };
                return a.create(&init, value).op(op::POP);
            }
            let to = if depth == 0 {
                r.pick(&[B, B, C, EMPTY, EMPTY2, IDENTITY, SHA256])
            } else if harsh() {
                // deeper: B and C may call each other (and A), bounded by gas and depth
                r.pick(&[A, B, C, C, EMPTY, IDENTITY, SHA256])
            } else {
                r.pick(&[C, C, EMPTY, IDENTITY])
            };
            let kind = r.pick(&[
                op::CALL,
                op::CALL,
                op::CALL,
                op::STATICCALL,
                op::DELEGATECALL,
                op::CALLCODE,
            ]);
            let value = if matches!(kind, op::CALL | op::CALLCODE) && r.chance(40) { 1 } else { 0 };
            let gas = r.pick(&[0u64, 30_000, 200_000, 1_000_000_000]);
            let in_len = r.pick(&[0u64, 0, 64, 3200]);
            a.call_with(kind, to, value, gas, in_len).op(op::POP)
        }
    }
}

fn program(r: &mut Rng, depth: u8) -> Vec<u8> {
    let mut a = Asm::new();
    for _ in 0..(r.below(6) + 1) {
        a = a.ops(&fragment(r, depth).build());
    }
    let mut code = a.build();
    match r.below(12) {
        0 => code.extend(Asm::new().revert0().build()),
        1 => code.push(op::INVALID),
        2 => {
            let jd = code.len();
            code.extend_from_slice(&[op::JUMPDEST, op::PUSH1, (jd & 0xff) as u8, op::JUMP]);
            if jd > 0xff {
                code.truncate(jd);
                code.push(op::STOP);
            }
        }
        3 => code
            .extend(Asm::new().push_addr(r.pick(&[A, EMPTY, EMPTY2])).op(op::SELFDESTRUCT).build()),
        _ => code.push(op::STOP),
    }
    code
}

pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    // `MEGA_EVM2_FUZZ=first..=last` selects other seeds (both engines read it); default 1..=400.
    let (first, last) = std::env::var("MEGA_EVM2_FUZZ")
        .ok()
        .and_then(|v| {
            let (a, b) = v.split_once("..=")?;
            Some((a.parse().ok()?, b.parse().ok()?))
        })
        .unwrap_or((1u64, 400u64));
    for seed in first..=last {
        let mut r = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let h = harsh();
        let gas = if h {
            r.pick(&[300_000u64, 1_000_000, 3_000_000, 200_020_000, 200_060_000, 200_300_000])
        } else {
            r.pick(&[1_000_000u64, 3_000_000, 210_000_000, 200_100_000, 200_500_000])
        };
        let mut limits = Limits::default();
        if r.chance(if h { 75 } else { 40 }) {
            limits.block_env_cap = Some(if h {
                r.pick(&[500u64, 1_200, 3_000, 8_000, 25_000, 90_000])
            } else {
                r.pick(&[2_000u64, 15_000, 60_000, 250_000, 1_000_000])
            });
        }
        if r.chance(if h { 30 } else { 15 }) {
            limits.growth = Some(r.below(4));
        }
        if r.chance(if h { 25 } else { 10 }) {
            limits.kv = Some(r.below(5) + 1);
        }
        if r.chance(if h { 25 } else { 10 }) {
            limits.data = Some(310 + r.pick(&[100u64, 400, 1000]));
        }
        let name: &'static str = Box::leak(
            match (mechanisms_mode(), h) {
                (true, true) => format!("fqh{seed:04}"),
                (true, false) => format!("fq{seed:04}"),
                (false, true) => format!("fh{seed:04}"),
                (false, false) => format!("fz{seed:03}"),
            }
            .into_boxed_str(),
        );
        let mut s = scenario(
            name,
            vec![
                Account {
                    address: A,
                    code: program(&mut r, 0),
                    balance: U256::from(r.pick(&[0u64, 10])),
                    ..Account::default()
                },
                Account {
                    address: B,
                    code: program(&mut r, 1),
                    balance: U256::from(r.pick(&[0u64, 10])),
                    ..Account::default()
                },
                Account {
                    address: C,
                    code: program(&mut r, if h { 1 } else { 2 }),
                    storage: vec![(1, 1), (2, 5)],
                    ..Account::default()
                },
            ],
            gas,
            limits,
        );
        s.group = "fz";
        if r.chance(15) {
            s.value = U256::from(1);
        }
        for _ in 0..r.below(4) {
            let site = match r.below(4) {
                0 => (A, Some(r.below(6) + 1)),
                1 => (B, Some(r.below(6) + 1)),
                2 => (r.pick(&[EMPTY, EMPTY2]), None),
                _ => (r.pick(&[A.create(0), B.create(0), A.create(1)]), None),
            };
            s.salt.push((site.0, site.1, r.pick(&[1u64, 2, 8])));
        }
        if r.chance(if h { 20 } else { 10 }) {
            s.deposit = Some(r.pick(&[0u64, 1_000_000]));
            s.sender = Address::new([0x7e; 20]);
        }
        if mechanisms_mode() {
            mechanism_transaction(&mut r, &mut s, h);
        }
        v.push(s);
    }
    v
}

/// the transaction-level mechanisms on top of a random composition: the Oracle (code that
/// reads slot 5 and logs, an answer or none, its own cap), an EIP-7702 list, or a keyless
/// deployment as the transaction itself.
fn mechanism_transaction(r: &mut Rng, s: &mut Scenario, h: bool) {
    let oracle_code =
        Asm::new().push(5).op(op::SLOAD).op(op::POP).push(0).push(0).op(op::LOG0).stop().build();
    s.accounts.push(Account {
        address: ORACLE,
        code: oracle_code,
        storage: vec![(5, 9)],
        ..Account::default()
    });
    if r.chance(50) {
        s.oracle = vec![(5, r.pick(&[0u64, 1, 77]))];
    }
    if r.chance(if h { 40 } else { 20 }) {
        s.limits.oracle_cap = Some(r.pick(&[1_000u64, 20_000, 300_000]));
    }
    if r.chance(25) {
        // X2 holds a pre-existing delegation (to B, C, D or the beneficiary).
        let target = r.pick(&[B, C, D, super::q8::BENEFICIARY]);
        let mut code = vec![0xef, 0x01, 0x00];
        code.extend_from_slice(target.as_slice());
        s.accounts.push(Account {
            address: X2,
            code,
            nonce: 1,
            balance: U256::from(r.pick(&[0u64, 3])),
            ..Account::default()
        });
        if !s.accounts.iter().any(|a| a.address == D) {
            s.accounts.push(Account { address: D, code: program(r, 2), ..Account::default() });
        }
    }
    if r.chance(10) {
        // the beneficiary is a contract of the composition (its reads are volatile; a
        // frame whose access is off is refused them).
        s.beneficiary = r.pick(&[B, C, X1]);
    }
    if s.deposit.is_some() {
        return;
    }
    match r.below(10) {
        0..=2 => {
            // An EIP-7702 list: 1-3 entries over X1, X2, B and the sender, delegating to B, C, D
            // (D's code is random too) or clearing; nonces mostly right; another chain, an
            // invalid signature; the transaction may call an authority.
            s.accounts.push(Account { address: D, code: program(r, 1), ..Account::default() });
            for _ in 0..r.below(3) + 1 {
                let authority =
                    r.pick(&[Some(X1), Some(X1), Some(X2), Some(B), Some(s.sender), None]);
                let nonce = match authority {
                    Some(a) if a == s.sender => 1,
                    _ => r.pick(&[0u64, 0, 0, 1, 3]),
                };
                let chain_id = r.pick(&[1u64, 1, 1, 0, 2]);
                let address = r.pick(&[D, D, C, B, Address::ZERO]);
                s.auths.push(Auth { chain_id, address, nonce, authority });
            }
            if r.chance(30) {
                s.accounts.push(Account {
                    address: X1,
                    nonce: r.pick(&[0u64, 1, 3]),
                    balance: U256::from(r.pick(&[0u64, 5])),
                    ..Account::default()
                });
            }
            if r.chance(50) {
                s.to = r.pick(&[X1, X2, A]);
            }
            if r.chance(10) {
                s.auths.push(Auth {
                    chain_id: 1,
                    address: D,
                    nonce: 0,
                    authority: Some(super::q8::BENEFICIARY),
                });
            }
        }
        3 => {
            // A keyless deployment as the transaction.
            let (name, _, _) = KEYLESS_TXS[r.below(KEYLESS_TXS.len() as u64) as usize];
            let (data, signer, deploy) =
                keyless_call(name, r.pick(&[100_000u64, 1_000_000, 5_000_000]));
            s.to = KEYLESS;
            s.data = data;
            s.value = U256::from(if r.chance(10) { 1 } else { 0 });
            match r.below(5) {
                0 => s.accounts.push(Account {
                    address: signer,
                    balance: U256::from(10),
                    ..Account::default()
                }),
                1 => s.accounts.push(Account { address: signer, nonce: 1, ..Account::default() }),
                2 => s.accounts.push(Account {
                    address: deploy,
                    balance: U256::from(3),
                    ..Account::default()
                }),
                _ => {}
            }
            if r.chance(15) {
                s.beneficiary = signer;
            }
            if r.chance(30) {
                s.salt.push((signer, None, r.pick(&[2u64, 8])));
                s.salt.push((deploy, None, r.pick(&[1u64, 4])));
            }
        }
        _ => {}
    }
}
