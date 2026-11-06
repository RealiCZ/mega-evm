//! Fuzz modes 5 to 10: random inputs that each land in one class of edge inputs of single rules,
//! the classes in which inputs outside every gate showed the engine and the reference apart.
//!
//! - 5 (`fxs`), the live system address: the `SequencerRegistry` names the sender (mostly, else
//!   another address, or its code is not the trusted code), which sends every envelope (legacy
//!   with, without and with another chain id, EIP-2930, EIP-1559, EIP-7702, a deposit, a creation)
//!   to the whitelisted Oracle or elsewhere, from an account that is absent, plain, delegated
//!   (EIP-7702) or has code, with any nonce, balance, gas price and value.
//! - 6 (`fxc`), creations onto occupied addresses: CREATE, CREATE2, a creation transaction or a
//!   keyless deployment whose address holds code, a nonce or storage (or only a balance, or
//!   nothing: controls), with and without a volatile read before it, detained and not, in the
//!   transaction's frame or in a child that may revert, with cheap work, a read loop or a second
//!   creation after it.
//! - 7 (`fxp`), a precompile called at depth 0 by a transaction whose sender or recipient is the
//!   block beneficiary (or neither: control): every precompile, inputs of several sizes, the
//!   detention cap and the gas limit drawn around the precompile's price where it is fixed (the gas
//!   limit at what the call needs before the precompile runs, plus the price).
//! - 8 (`fxl`), limits and SALT capacities: a random composition (modes 1 to 4) whose data-size,
//!   KV, state-growth and detention limits and gas limit are drawn from small values up to
//!   `u64::MAX`, and whose touched buckets report capacities in entries, invalid ones (below the
//!   minimum bucket) and ones up to `u64::MAX` included.
//! - 9 (`fxd`), a creation address funded first: a value transfer (a call, a destroyed helper's
//!   balance, a third contract; or none or a zero-value call: controls) materializes the address of
//!   a later CREATE or CREATE2, which then lands there, and the contract is destroyed (by its
//!   constructor, or when called after), with any heir, limits and SALT price, in the transaction's
//!   frame or in a child that may revert.
//! - 10 (`fxr`), refusals of volatile data in a frame whose access is off (or on: control): the
//!   instructions of the refusal-boundary sweep (`bd`), each run with a frame gas drawn around one
//!   of the charges it makes before its load, the target warm or cold, the stack empty, ordinary,
//!   full or one below full, a static frame, a probe frame with or without a balance, detention.
//!
//! The first frame's own gas window (a value to an absent account, a creation, a created deposit
//! caller, each with little gas) is mode 11's (`regions.rs`); these modes keep their streams, so
//! their seeds stay the inputs the reference outputs were made from.
#![allow(dead_code)]
use super::{Gen, Rng, composition};
use crate::{
    A, Account, Asm, Auth, B, BELOW_CAP, BLOCK_GAS_LIMIT, C, EMPTY, KEYLESS, L1_BLOCK,
    LIMIT_CONTROL, Limits, ORACLE, SENDER, SEQUENCER_REGISTRY, Scenario, acct,
    bd::{self, BEN, OWN_PREFIX, P, PLENTY, STACK_LIMIT, access_off, delegation, fill},
    edge::{AUTHORITY, ONE_ETHER, SYSTEM, registry_naming, store_one},
    funded, init_returning, op,
    q2::EMPTY2,
    q8::{ACCESS_CONTROL, KEYLESS_TXS, X1, keyless_call, mem_call, sel},
    scenario,
};
use alloy_primitives::{Address, B256, U256, keccak256};

/// The scenario of seed `seed` of class mode `mode` (5 to 10), not yet named.
pub(super) fn generate(mode: u8, seed: u64) -> Scenario {
    // A stream of its own per mode: one seed is a different input in each.
    let mut r = Rng::for_seed(seed ^ (u64::from(mode) << 48));
    match mode {
        5 => system_sender(&mut r),
        6 => collision(&mut r),
        7 => top_precompile(&mut r),
        8 => wide_limits(&mut r),
        9 => fund_create_destroy(&mut r),
        10 => refusal(&mut r),
        _ => panic!("unknown fuzz mode {mode}"),
    }
}

/// Another address the registry may name instead of [`SYSTEM`].
const OTHER_SYSTEM: Address = Address::with_last_byte(0x5b);

/// The live system address, every envelope.
fn system_sender(r: &mut Rng) -> Scenario {
    let mut s = scenario(
        "",
        vec![acct(ORACLE, store_one()), acct(A, store_one())],
        BELOW_CAP,
        Limits::default(),
    );
    s.accounts.push(match r.below(10) {
        // Not the trusted code: no address is the system address.
        0 => Account { code: vec![op::INVALID, op::STOP], ..registry_naming(SYSTEM) },
        1 => registry_naming(OTHER_SYSTEM),
        _ => registry_naming(SYSTEM),
    });
    s.sender = if r.chance(85) { SYSTEM } else { r.pick(&[SENDER, OTHER_SYSTEM]) };
    if s.sender != SENDER {
        let code = match r.below(8) {
            0 => delegation(A),
            1 => delegation(ORACLE),
            2 => delegation(s.sender),
            3 => vec![op::STOP],
            4 => store_one(),
            _ => Vec::new(),
        };
        let nonce = r.pick(&[0u64, 0, 0, 1, 2]);
        let balance = r.pick(&[0u64, ONE_ETHER, ONE_ETHER]);
        if !r.chance(15) {
            s.accounts.push(Account {
                address: s.sender,
                code,
                nonce,
                balance: U256::from(balance),
                ..Account::default()
            });
        }
    }
    s.to = r.pick(&[
        ORACLE,
        ORACLE,
        ORACLE,
        A,
        SEQUENCER_REGISTRY,
        KEYLESS,
        ACCESS_CONTROL,
        LIMIT_CONTROL,
        EMPTY,
        L1_BLOCK,
    ]);
    // Little gas only without a value, a creation, a list or a deposit: the first frame's gas
    // window of those is mode 11's.
    let little = r.chance(15);
    s.gas_limit = if little {
        r.pick(&[21_000u64, 21_500, 25_000, 60_000])
    } else {
        r.pick(&[BELOW_CAP, BELOW_CAP, 30_000_000, 1 << 32, (1 << 32) + 7_000_000])
    };
    let priced = r.chance(25);
    if !little && r.chance(25) {
        s.value = U256::from(1);
    }
    s.data = match r.below(4) {
        0 => sel("disableVolatileDataAccess()").to_vec(),
        1 => vec![0x5a; 36],
        _ => Vec::new(),
    };
    match r.below(10) {
        4 => s.tx_type = 1,
        5 => s.tx_type = 2,
        6 if !little => {
            for _ in 0..r.below(2) + 1 {
                let authority = r.pick(&[Some(AUTHORITY), Some(s.sender), Some(SENDER), None]);
                let nonce = if authority == Some(s.sender) { 1 } else { 0 };
                s.auths.push(Auth {
                    chain_id: r.pick(&[1u64, 1, 0]),
                    address: r.pick(&[A, ORACLE, Address::ZERO]),
                    nonce,
                    authority,
                });
            }
        }
        7..=9 if !little => s.deposit = Some(r.pick(&[0u64, 1_000_000])),
        _ => s.chain_id = r.pick(&[Some(1), Some(1), None, Some(2)]),
    }
    // A deposit carries no gas price.
    if priced && s.deposit.is_none() {
        s.gas_price = 1;
    }
    if !little && s.auths.is_empty() && r.chance(20) {
        s.create = true;
        s.data = Asm::new().sstore(1, 1).build();
        s.data.extend(init_returning(1));
    }
    s.beneficiary = r.pick(&[Address::ZERO, Address::ZERO, SYSTEM, A]);
    s
}

/// The init codes of the creations of modes 6 and 9.
fn init_code(r: &mut Rng) -> Vec<u8> {
    match r.below(8) {
        0 => init_returning(1),
        1 => {
            let mut c = Asm::new().sstore(1, 1).build();
            c.extend(init_returning(2));
            c
        }
        2 => {
            let mut c = Asm::new().op(op::TIMESTAMP).op(op::POP).build();
            c.extend(init_returning(1));
            c
        }
        3 => Asm::new().revert0().build(),
        4 => vec![op::INVALID],
        5 => Asm::new().push_addr(A).op(op::SELFDESTRUCT).build(),
        6 => init_returning(0),
        _ => Vec::new(),
    }
}

/// CREATE2 of `init` (copied into memory at 0) with `value` and `salt`; leaves the address (or 0).
fn create2(mut a: Asm, init: &[u8], value: u64, salt: u64) -> Asm {
    for (i, chunk) in init.chunks(32).enumerate() {
        let mut w = [0u8; 32];
        w[..chunk.len()].copy_from_slice(chunk);
        a = a.push_bytes(&w).push(32 * i as u64).op(op::MSTORE);
    }
    a.push(salt).push(init.len() as u64).push(0).push(value).op(op::CREATE2)
}

fn create2_address(creator: Address, salt: u64, init: &[u8]) -> Address {
    creator.create2(B256::from(U256::from(salt)), keccak256(init))
}

/// A volatile read before a creation (or none).
fn volatile_read(r: &mut Rng, a: Asm) -> Asm {
    match r.below(7) {
        0 => a.op(op::TIMESTAMP).op(op::POP),
        1 => a.op(op::NUMBER).op(op::POP),
        2 => a.op(op::COINBASE).op(op::POP),
        3 => a.push_addr(BEN).op(op::BALANCE).op(op::POP),
        4 => a.call_with(op::CALL, ORACLE, 0, 50_000, 0).op(op::POP),
        _ => a,
    }
}

/// What occupies an address a creation lands on: code, a nonce, storage, both code and a nonce,
/// or (controls) only a balance, or nothing.
fn occupant(r: &mut Rng, address: Address) -> Option<Account> {
    let a = Account { address, ..Account::default() };
    match r.below(7) {
        0 => Some(Account { code: vec![0x00], ..a }),
        1 => Some(Account { nonce: 1, ..a }),
        2 => Some(Account { storage: vec![(5, 9)], ..a }),
        3 => Some(Account { code: vec![op::STOP], nonce: 1, ..a }),
        4 => Some(Account { balance: U256::from(5), ..a }),
        _ if r.chance(30) => None,
        _ => Some(Account { code: vec![0x00], ..a }),
    }
}

/// Creations onto occupied addresses.
fn collision(r: &mut Rng) -> Scenario {
    let mut limits = Limits::default();
    if r.chance(60) {
        limits.block_env_cap = Some(r.pick(&[1_000u64, 30_000, 50_000, 400_000, 2_000_000]));
    }
    if r.chance(15) {
        limits.growth = Some(r.below(2));
    }
    let gas = r.pick(&[1_000_000u64, 5_000_000, 40_000_000, 100_000_000, 210_000_000]);
    let mut s = scenario("", Vec::new(), gas, limits);
    let oracle = Account {
        address: ORACLE,
        code: Asm::new().push(5).op(op::SLOAD).op(op::POP).stop().build(),
        storage: vec![(5, 9)],
        ..Account::default()
    };
    s.accounts.push(oracle);
    s.beneficiary = r.pick(&[Address::ZERO, Address::ZERO, A, SENDER, BEN]);
    match r.below(10) {
        0..=6 => {
            // An opcode creation, by A or by B (which A calls and which may revert after it).
            let by_b = r.chance(20);
            let creator = if by_b { B } else { A };
            let nonce = r.pick(&[0u64, 1, 2]);
            let init = init_code(r);
            let value = if r.chance(25) { 1 } else { 0 };
            let salt = r.pick(&[0u64, 1, 0xff]);
            let use_create2 = r.chance(45);
            let target = if use_create2 {
                create2_address(creator, salt, &init)
            } else {
                creator.create(nonce)
            };
            let mut c = volatile_read(r, Asm::new());
            c = if use_create2 { create2(c, &init, value, salt) } else { c.create(&init, value) };
            c = c.push(0x20).op(op::SSTORE);
            c = match r.below(5) {
                0 => c.sstore(0x10, 1),
                1 => {
                    let pre = c.op(op::TIMESTAMP).op(op::POP);
                    let jd = pre.0.len() as u64;
                    pre.op(op::JUMPDEST).push(jd).op(op::JUMP)
                }
                2 if use_create2 => create2(c, &init, value, salt).push(0x21).op(op::SSTORE),
                2 => c.create(&init, value).push(0x21).op(op::SSTORE),
                3 => c.push(1).op(op::POP),
                _ => c,
            };
            let balance = U256::from(r.pick(&[0u64, 10]));
            if by_b {
                let b = if r.chance(50) { c.revert0() } else { c.stop() };
                s.accounts.push(Account {
                    address: B,
                    code: b.build(),
                    nonce,
                    balance,
                    ..Account::default()
                });
                let a = volatile_read(r, Asm::new()).call_all(op::CALL, B, 0).record_call(0x10);
                s.accounts.push(acct(A, a.stop().build()));
            } else {
                s.accounts.push(Account {
                    address: A,
                    code: c.stop().build(),
                    nonce,
                    balance,
                    ..Account::default()
                });
            }
            s.accounts.extend(occupant(r, target));
        }
        7 | 8 => {
            // The transaction's own creation frame collides (the sender's nonce is 0).
            s.create = true;
            s.data = init_code(r);
            if s.data.is_empty() {
                s.data = init_returning(1);
            }
            s.accounts.extend(occupant(r, SENDER.create(0)));
        }
        _ => {
            // A keyless deployment as the transaction, onto an occupied deployment address.
            let (name, _, _) = KEYLESS_TXS[r.below(KEYLESS_TXS.len() as u64) as usize];
            let (data, signer, deploy) =
                keyless_call(name, r.pick(&[100_000u64, 1_000_000, 5_000_000]));
            s.to = KEYLESS;
            s.data = data;
            s.gas_limit = s.gas_limit.max(5_000_000);
            s.accounts.extend(occupant(r, deploy));
            if r.chance(30) {
                s.accounts.push(funded(signer, Vec::new(), 10));
            }
            if r.chance(40) {
                s.beneficiary = signer;
            }
        }
    }
    s
}

/// A precompile's input and, where it is fixed by the input's length, its price.
fn precompile_input(r: &mut Rng) -> (Address, Vec<u8>, Option<u64>) {
    let pc = |b: u16| {
        let mut x = [0u8; 20];
        x[18] = (b >> 8) as u8;
        x[19] = b as u8;
        Address::new(x)
    };
    let words = |n: usize| n.div_ceil(32) as u64;
    match r.below(13) {
        0 => (pc(1), vec![if r.chance(50) { 0 } else { 0x11 }; 128], Some(3_000)),
        1..=3 => {
            let n = r.pick(&[0usize, 1, 31, 32, 33, 64, 1_000, 4_096]);
            let (addr, base, per_word) = r.pick(&[(2u16, 60u64, 12u64), (3, 600, 120), (4, 15, 3)]);
            (pc(addr), vec![0xab; n], Some(base + per_word * words(n)))
        }
        4 => {
            // MODEXP: base, exponent and modulus lengths, then the three numbers.
            let mut input = vec![0u8; 96];
            let (b, e, m) =
                r.pick(&[(1u8, 1u8, 1u8), (1, 32, 1), (32, 64, 32), (0, 0, 0), (64, 1, 64)]);
            input[31] = b;
            input[63] = e;
            input[95] = m;
            input.resize(96 + b as usize + e as usize + m as usize, 0xff);
            (pc(5), input, None)
        }
        5 => (pc(6), vec![0; 128], Some(150)),
        6 => (pc(7), vec![0; 96], Some(6_000)),
        7 => {
            let k = r.below(3);
            (pc(8), vec![0; 192 * k as usize], Some(45_000 + 34_000 * k))
        }
        8 => {
            // BLAKE2F: many rounds only with an invalid final flag, which it checks after its
            // charge (so the rounds are never run).
            let flag = r.pick(&[0u8, 1, 2]);
            let rounds: u32 = if flag == 2 {
                r.pick(&[0u32, 12, 2_000_000, 20_000_001])
            } else {
                r.pick(&[0u32, 1, 12, 5_000])
            };
            let mut input = rounds.to_be_bytes().to_vec();
            input.extend([0u8; 208]);
            input.push(flag);
            (pc(9), input, Some(u64::from(rounds)))
        }
        9 => (pc(0x0a), vec![0; 192], Some(50_000)),
        10 => {
            let (addr, len) = r.pick(&[
                (0x0bu16, 256usize),
                (0x0c, 160),
                (0x0d, 512),
                (0x0e, 288),
                (0x0f, 384),
                (0x10, 64),
                (0x11, 128),
            ]);
            (pc(addr), vec![0; len], None)
        }
        11 => (pc(0x100), vec![0; 160], None),
        // No precompile there: an empty account.
        _ => (pc(r.pick(&[0x12u16, 0x101])), vec![0x01; 4], Some(0)),
    }
}

/// A depth-0 precompile call with the sender or the recipient as the beneficiary.
fn top_precompile(r: &mut Rng) -> Scenario {
    let (pc, input, price) = precompile_input(r);
    let mut s = scenario("", Vec::new(), BELOW_CAP, Limits::default());
    s.to = pc;
    s.beneficiary = r.pick(&[SENDER, SENDER, pc, pc, B]);
    s.limits.block_env_cap = match price {
        Some(p) if r.chance(60) => Some(r.pick(&[p.saturating_sub(1), p, p + 1, p / 2, 2 * p + 1])),
        _ => r.pick(&[None, Some(1), Some(10), Some(720), Some(5_000), Some(100_000)]),
    };
    // The gas limit around what the call needs before the precompile runs plus its price, or
    // ample. What it needs: Satin's intrinsic gas, the EIP-2780 base of 15,000 and 4 gas per
    // calldata token, with the history of the body (310 bytes of envelope and the calldata, at
    // 88 gas a byte); the calldata floor lies below it. The window is a few dozen gas wide each
    // way, so that the transaction's other small charges (a chain id) do not move it off.
    let tokens: u64 = input.iter().map(|b| if *b == 0 { 1 } else { 4 }).sum();
    let needs = 15_000 + 4 * tokens + (310 + input.len() as u64) * 88;
    s.gas_limit = match price {
        Some(p) if r.chance(65) => (needs + p + r.below(81)).saturating_sub(40),
        _ => r.pick(&[100_000u64, BELOW_CAP, 100_000_000]),
    };
    s.data = input;
    if s.gas_limit >= BELOW_CAP && r.chance(15) {
        s.value = U256::from(1);
    }
    match r.below(8) {
        0 => s.tx_type = 2,
        1 => s.chain_id = Some(1),
        2 if s.gas_limit >= BELOW_CAP => s.deposit = Some(0),
        _ => {}
    }
    s
}

/// Values a limit is drawn from: small ones, the powers of two around the integer widths, and
/// the top of `u64`.
const LIMIT_VALUES: [u64; 20] = [
    0,
    1,
    2,
    3,
    7,
    100,
    310,
    311,
    1_000,
    65_535,
    1 << 31,
    (1 << 32) - 1,
    1 << 32,
    1 << 53,
    1 << 62,
    (1 << 63) - 1,
    1 << 63,
    (1 << 63) + 1,
    u64::MAX - 1,
    u64::MAX,
];

/// Capacities a SALT bucket reports, in entries: below the minimum bucket (invalid), at and
/// around it and its multiples, and up to `u64::MAX`.
const CAPACITIES: [u64; 16] = [
    0,
    1,
    255,
    256,
    257,
    300,
    511,
    512,
    700,
    2_048,
    65_535,
    1 << 20,
    1 << 32,
    1 << 40,
    u64::MAX >> 1,
    u64::MAX,
];

fn limit_value(r: &mut Rng) -> u64 {
    match r.below(3) {
        0 => r.pick(&LIMIT_VALUES),
        // Log-uniform over the whole range.
        1 => r.next() >> r.below(64),
        _ => r.below(2_000),
    }
}

/// A random composition under limits and SALT capacities drawn over the whole range.
fn wide_limits(r: &mut Rng) -> Scenario {
    let g = Gen { harsh: r.chance(50), mechanisms: r.chance(60) };
    let mut s = composition(g, r.next());
    for limit in [
        &mut s.limits.data,
        &mut s.limits.kv,
        &mut s.limits.growth,
        &mut s.limits.block_env_cap,
        &mut s.limits.oracle_cap,
    ] {
        if r.chance(50) {
            *limit = Some(limit_value(r));
        }
    }
    if r.chance(15) {
        s.gas_limit =
            r.pick(&[(1 << 32) - 1, 1 << 32, BLOCK_GAS_LIMIT, BLOCK_GAS_LIMIT + 1, u64::MAX]);
    }
    for _ in 0..r.below(5) {
        let (address, slot) = match r.below(6) {
            0 => (A, Some(r.below(6) + 1)),
            1 => (B, Some(r.below(6) + 1)),
            2 => (C, Some(r.below(2) + 1)),
            3 => (r.pick(&[EMPTY, EMPTY2]), None),
            4 => (r.pick(&[A.create(0), B.create(0), A.create(1)]), None),
            _ => (r.pick(&[A, B, C]), None),
        };
        s.salt_capacity.push((address, slot, r.pick(&CAPACITIES)));
    }
    // Often enough to be reached, a capacity below the minimum bucket on every slot the
    // composition's first account may write: its first fresh write fails the lookup.
    if r.chance(10) {
        let capacity = r.pick(&[0u64, 1, 255]);
        s.salt_capacity.extend((1..=6).map(|k| (A, Some(k), capacity)));
    }
    s
}

/// Init code returning `runtime` as the created contract's code.
fn deployer(runtime: &[u8]) -> Vec<u8> {
    let mut a = Asm::new();
    for (i, chunk) in runtime.chunks(32).enumerate() {
        let mut w = [0u8; 32];
        w[..chunk.len()].copy_from_slice(chunk);
        a = a.push_bytes(&w).push(32 * i as u64).op(op::MSTORE);
    }
    a.push(runtime.len() as u64).push(0).op(op::RETURN).build()
}

/// A creation address materialized by a value transfer, then created and destroyed.
fn fund_create_destroy(r: &mut Rng) -> Scenario {
    let nonce = r.pick(&[0u64, 1, 1, 2, 5]);
    let balance = r.pick(&[10u64, 10, 1_000]);
    let wrapped = r.chance(20);
    let creator = if wrapped { B } else { A };
    let use_create2 = r.chance(35);
    let salt = r.pick(&[0u64, 1]);
    // The heir of the destruction: an account, an absent one, or the contract itself.
    let heir = r.pick(&[Some(A), Some(A), Some(B), Some(EMPTY), None]);
    let destroy = |a: Asm| match heir {
        Some(h) => a.push_addr(h).op(op::SELFDESTRUCT),
        None => a.op(op::ADDRESS).op(op::SELFDESTRUCT),
    };
    // The constructor destroys (after a write, or after sending a wei on), or the runtime code
    // does when called.
    let destroyed_when_called = r.chance(30);
    let init = if destroyed_when_called {
        deployer(&destroy(Asm::new()).build())
    } else {
        let mut a = Asm::new();
        if r.chance(30) {
            a = a.sstore(1, 1);
        }
        if r.chance(15) {
            a = a.call_all(op::CALL, EMPTY2, 1).op(op::POP);
        }
        destroy(a).build()
    };
    let fund_value = r.pick(&[1u64, 5]);
    let funding = r.below(7);
    // A helper creation first takes the creator's nonce.
    let helper = funding == 2;
    let target = if use_create2 {
        create2_address(creator, salt, &init)
    } else {
        creator.create(nonce + u64::from(helper))
    };
    let mut c = Asm::new();
    let mut extra = Vec::new();
    c = match funding {
        0 | 1 => c.call_all(op::CALL, target, fund_value).op(op::POP),
        2 => {
            let to_target = Asm::new().push_addr(target).op(op::SELFDESTRUCT).build();
            c.create(&to_target, fund_value).op(op::POP)
        }
        3 => c.call_all(op::CALL, target, 0).op(op::POP),
        4 => c
            .call_all(op::CALL, target, fund_value)
            .op(op::POP)
            .call_all(op::CALL, target, 1)
            .op(op::POP),
        5 => {
            let fund = Asm::new().call_all(op::CALL, target, fund_value).op(op::POP).stop().build();
            extra.push(funded(C, fund, 10));
            c.call_all(op::CALL, C, 0).op(op::POP)
        }
        _ => c,
    };
    let value = r.pick(&[0u64, 0, 1]);
    c = if use_create2 { create2(c, &init, value, salt) } else { c.create(&init, value) };
    c = c.push(0x20).op(op::SSTORE);
    if destroyed_when_called && r.chance(70) {
        c = c.call_all(op::CALL, target, 0).op(op::POP);
    }
    c = match r.below(4) {
        0 => c.call_all(op::CALL, target, 1).op(op::POP),
        1 => c.push_addr(target).op(op::BALANCE).push(0x21).op(op::SSTORE),
        2 => c.push_addr(target).op(op::EXTCODESIZE).push(0x22).op(op::SSTORE),
        _ => c,
    };
    let mut accounts = Vec::new();
    if wrapped {
        let b = if r.chance(50) { c.revert0() } else { c.stop() };
        accounts.push(Account {
            address: B,
            code: b.build(),
            nonce,
            balance: U256::from(balance),
            ..Account::default()
        });
        accounts
            .push(acct(A, Asm::new().call_all(op::CALL, B, 0).record_call(0x10).stop().build()));
    } else {
        accounts.push(Account {
            address: A,
            code: c.stop().build(),
            nonce,
            balance: U256::from(balance),
            ..Account::default()
        });
    }
    accounts.extend(extra);
    match r.below(10) {
        // An account there at the start: empty but for a slot, or a balance.
        0 => {
            accounts.push(Account { address: target, storage: vec![(5, 9)], ..Account::default() })
        }
        1 => accounts.push(funded(target, Vec::new(), 3)),
        _ => {}
    }
    let mut limits = Limits {
        growth: r.pick(&[None, None, Some(0), Some(1), Some(2)]),
        kv: r.pick(&[None, None, Some(2), Some(4)]),
        ..Limits::default()
    };
    if r.chance(15) {
        limits.block_env_cap = Some(r.pick(&[5_000u64, 50_000]));
    }
    let gas = r.pick(&[2_000_000u64, BELOW_CAP, BELOW_CAP, 5_000_000]);
    let mut s = scenario("", accounts, gas, limits);
    if r.chance(40) {
        s.salt.push((target, None, r.pick(&[1u64, 2, 8])));
    }
    s.beneficiary = r.pick(&[Address::ZERO, Address::ZERO, Address::ZERO, EMPTY, target]);
    s
}

/// A frame gas around `at`: mostly within two of it, else within 150.
fn around(r: &mut Rng, at: u64) -> u64 {
    let d = if r.chance(60) { r.below(5) as i64 - 2 } else { r.below(301) as i64 - 150 };
    (at as i64 + d).max(0) as u64
}

/// `A`'s call of `MegaAccessControl`: `disableVolatileDataAccess()` (access off), or a view of the
/// same cost (access on, a control); its success word at slot 1.
fn access(a: Asm, off: bool) -> Asm {
    if off {
        access_off(a)
    } else {
        mem_call(a, op::CALL, ACCESS_CONTROL, 0, 100_000, &sel("isVolatileDataAccessDisabled()"))
            .record_call(1)
    }
}

/// Refusals of volatile data with the frame gas drawn around the charges before the load.
fn refusal(r: &mut Rng) -> Scenario {
    let off = !r.chance(12);
    let mut s = match r.below(12) {
        0..=8 => {
            let probes = bd::probes();
            let p = &probes[r.below(probes.len() as u64) as usize];
            let (warm, extra) = if p.variants.is_empty() {
                (Vec::new(), Vec::new())
            } else {
                let v = &p.variants[r.below(p.variants.len() as u64) as usize];
                (v.1.clone(), v.2.clone())
            };
            let boundaries: Vec<u64> = p.boundaries.iter().chain(&extra).map(|b| b.1).collect();
            let at = r.pick(&boundaries);
            let n = p.operands.len();
            let (code, fill_gas) = match r.below(10) {
                6 | 7 => (p.code(true), 2 * (STACK_LIMIT - n) as u64),
                8 => {
                    let mut a = fill(Asm::new(), STACK_LIMIT - n - 1);
                    for o in &p.operands {
                        a = o.push(a);
                    }
                    (a.op(p.opcode).stop().build(), 2 * (STACK_LIMIT - n - 1) as u64)
                }
                9 if n > 0 => (vec![p.opcode, op::STOP], 0),
                _ => (p.code(false), 0),
            };
            let gas = around(r, at + fill_gas);
            let kind = if r.chance(10) { op::STATICCALL } else { op::CALL };
            let mut a = Asm::new();
            for w in &warm {
                a = a.push_addr(*w).op(op::BALANCE).op(op::POP);
            }
            let a = access(a, off).call_with(kind, P, 0, gas, 0).record_call(3).stop().build();
            let probe = Account {
                address: P,
                code,
                balance: U256::from(r.pick(&[0u64, 0, 10])),
                ..Account::default()
            };
            scenario(
                "",
                vec![acct(A, a), probe, acct(BEN, vec![op::STOP]), acct(X1, delegation(BEN))],
                BELOW_CAP,
                Limits::default(),
            )
        }
        9 => {
            // The Oracle's slot, read in the Oracle's frame: SLOAD's static gas, or its cold check.
            let at = r.pick(&[3 + 100u64, 3 + 100 + bd::COLD_SLOT]);
            let gas = around(r, at);
            let a = access(Asm::new(), off).call_with(op::CALL, ORACLE, 0, gas, 0);
            let mut s = scenario(
                "",
                vec![acct(A, a.record_call(3).stop().build()), bd::oracle()],
                BELOW_CAP,
                Limits::default(),
            );
            if r.chance(50) {
                s.oracle = vec![(5, r.pick(&[0u64, 1, 77]))];
            }
            s
        }
        _ => {
            // The beneficiary's own frame: SELFBALANCE (on an ordinary or a full stack), or the
            // destruction of the beneficiary.
            let (rest, at) = match r.below(3) {
                0 => (Asm::new().op(op::SELFBALANCE), OWN_PREFIX + 5),
                1 => (
                    fill(Asm::new(), STACK_LIMIT).op(op::SELFBALANCE),
                    OWN_PREFIX + 2 * STACK_LIMIT as u64 + 5,
                ),
                _ => (Asm::new().push_addr(bd::HEIR).op(op::SELFDESTRUCT), OWN_PREFIX + 3 + 5_000),
            };
            let gas = if r.chance(10) { PLENTY } else { around(r, at) };
            let selector =
                if off { "disableVolatileDataAccess()" } else { "isVolatileDataAccessDisabled()" };
            let own = mem_call(Asm::new(), op::CALL, ACCESS_CONTROL, 0, 100_000, &sel(selector))
                .op(op::POP);
            let own = own.ops(&rest.build()).stop().build();
            let a = Asm::new().call_with(op::CALL, BEN, 0, gas, 0).record_call(3).stop().build();
            scenario("", vec![acct(A, a), funded(BEN, own, 7)], BELOW_CAP, Limits::default())
        }
    };
    s.beneficiary = BEN;
    if r.chance(15) {
        s.limits.block_env_cap = Some(r.pick(&[500u64, 5_000, 50_000]));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::super::{MODES, generate, name, parse_name};

    /// Every class mode is reproducible from its seed, names its seeds, and has seeds that
    /// differ from each other and from the other modes' seeds of the same number.
    #[test]
    fn test_class_modes_are_seeded_and_distinct() {
        for (mode, _, _) in MODES.iter().filter(|m| m.0 >= 5) {
            let a = generate(*mode, 1, 50);
            let b = generate(*mode, 1, 50);
            for (x, y) in a.iter().zip(&b) {
                assert_eq!(format!("{x:?}"), format!("{y:?}"), "mode {mode} is not reproducible");
            }
            assert_eq!(a[6].name, name(*mode, 7));
            assert_eq!(parse_name(a[6].name), Some((*mode, 7)));
            let distinct: std::collections::BTreeSet<String> = a
                .iter()
                .map(|s| format!("{:?}", crate::Scenario { name: "", ..s.clone() }))
                .collect();
            assert!(distinct.len() > 40, "mode {mode}: only {} distinct inputs", distinct.len());
        }
        let five = generate(5, 3, 3);
        let six = generate(6, 3, 3);
        assert_ne!(format!("{:?}", five[0].accounts), format!("{:?}", six[0].accounts));
    }
}
