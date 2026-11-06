//! Fuzz mode 14: two mechanisms in one transaction. Every review round's high findings sat where
//! two mechanisms meet in one transaction (a keyless signer that is the sender; an over-bound
//! precompile under detention; a zero limit with an answered first frame; a delegated sender in a
//! failed deposit), a combination no single-mechanism generator draws.
//!
//! The mechanisms ([`MECHANISMS`]): a keyless deployment (as the transaction, with its signer
//! absent, funded, at nonce 1, the sender or the beneficiary, its deploy address funded, the
//! beneficiary or occupied, or called from a contract), an EIP-7702 list (authorities fresh, the
//! sender, the beneficiary, the recipient; a pre-existing delegation), a deposit (mint, value,
//! creation, an absent, funded or delegated sender), detention after a volatile read (each
//! block-environment read, the beneficiary's balance), a precompile at an edge input (ModExp at and
//! over EIP-7823's bound, KZG at its 100,000 price, a forward around each price), a creation
//! (CREATE, CREATE2, a creation transaction, onto an occupied address), a `SELFDESTRUCT` with
//! value, a non-default limit (0, around its use), state charges (fresh slots, new accounts, SALT
//! multipliers), the first frame's ending (a halt, a revert, a destruction, a refund), the system
//! contracts and the Mega System Transaction, and the Oracle (reads, hints, its cap).
//!
//! Seed `s` draws the unordered pair [`pair`]`(s)`: the seeds cycle through all 66 pairs, so the
//! first `66 k` seeds draw every pair `k` times. Each mechanism's parameters are random. One of the
//! two may take the transaction's envelope (a keyless call, a list, a deposit, a creation, a
//! precompile or a destroyed account as the recipient, a Mega System Transaction); the other then
//! joins it in the envelope's own form when there is one (a deposit to `KeylessDeploy`, a list
//! whose authority is the keyless signer, a deposit from a delegated sender, ...), and otherwise
//! adds its code to whatever runs as the first frame, and its state.

use super::Rng;
use crate::{
    A, Account, Asm, Auth, B, C, D, EMPTY, IDENTITY, KEYLESS, LIMIT_CONTROL, Limits, ORACLE,
    SENDER, Scenario, init_returning, op,
    q8::{ACCESS_CONTROL, KEYLESS_TXS, X1, X2, keyless_call, sel, send_hint},
    scenario,
};
use alloy_primitives::{Address, U256, keccak256};

/// The mechanisms mode 14 pairs, by index.
pub const MECHANISMS: [&str; 12] = [
    "keyless",
    "authorization",
    "deposit",
    "detention",
    "precompile",
    "creation",
    "selfdestruct",
    "limit",
    "state",
    "ending",
    "system",
    "oracle",
];

/// The number of unordered pairs of [`MECHANISMS`].
pub const PAIRS: u64 = 66;

const DEPOSITOR: Address = crate::q2::DEPOSITOR;
const SYSTEM: Address = crate::edge::SYSTEM;
const EMPTY2: Address = Address::with_last_byte(0xe7);
const ONE_ETHER: u64 = 1_000_000_000_000_000_000;

/// The unordered pair of mechanisms seed `seed` draws (indices into [`MECHANISMS`], the smaller
/// first): seeds `1..=66` draw each pair once, and so does every later run of 66 seeds.
pub fn pair(seed: u64) -> (usize, usize) {
    let mut k = (seed.saturating_sub(1) % PAIRS) as usize;
    for i in 0..MECHANISMS.len() {
        let n = MECHANISMS.len() - 1 - i;
        if k < n {
            return (i, i + 1 + k);
        }
        k -= n;
    }
    unreachable!("66 pairs")
}

/// What runs as the transaction's first frame, where the mechanisms' code goes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Holder {
    /// A call of A: A's code.
    A,
    /// A creation (a creation transaction or a deposit's): the init code.
    Init,
    /// A Mega System Transaction: the Oracle's code.
    Oracle,
    /// A call of D, which destroys itself: D's code before its `SELFDESTRUCT`.
    D,
    /// A keyless call or a precompile: no code of the scenario runs first.
    None,
}

struct Build {
    s: Scenario,
    code: Vec<u8>,
    /// How the first frame ends.
    tail: Vec<u8>,
    holder: Holder,
    /// The heir D's `SELFDESTRUCT` names, when D is in the scenario.
    heir: Address,
    /// Whether D is in the scenario (a destroying contract).
    has_d: bool,
}

impl Build {
    fn account(&mut self, a: Account) {
        self.s.accounts.retain(|x| x.address != a.address);
        self.s.accounts.push(a);
    }

    /// Changes the account at `address` (created when absent) with `f`.
    fn edit(&mut self, address: Address, f: impl FnOnce(&mut Account)) {
        if !self.s.accounts.iter().any(|x| x.address == address) {
            self.s.accounts.push(Account { address, ..Account::default() });
        }
        f(self.s.accounts.iter_mut().find(|x| x.address == address).unwrap());
    }

    fn sender(&mut self, a: Account) {
        self.s.sender = a.address;
        self.account(a);
    }

    fn fragment(&mut self, code: Vec<u8>, first: bool) {
        if first {
            let mut c = code;
            c.extend(core::mem::take(&mut self.code));
            self.code = c;
        } else {
            self.code.extend(code);
        }
    }
}

/// The scenario of seed `seed` of mode 14.
pub(super) fn generate(seed: u64) -> Scenario {
    let mut r = Rng::for_seed(seed ^ (14u64 << 48));
    let (a, b) = pair(seed);
    let (first, second) = if r.chance(50) { (a, b) } else { (b, a) };
    let gas = r.pick(&[1_000_000u64, 3_000_000, 30_000_000, 200_100_000, 210_000_000]);
    // A gas limit one below or at a charge before the first frame (the intrinsic gas, the body's
    // history, the start record, a deposit's created caller), set once the transaction is built.
    let tight = r.chance(20).then(|| r.pick(&[0u64, 1, 2, 3, 4, 5]));
    let mut build = Build {
        s: scenario("", Vec::new(), gas, Limits::default()),
        code: Vec::new(),
        tail: vec![op::STOP],
        holder: Holder::A,
        heir: Address::ZERO,
        has_d: false,
    };
    build.account(Account {
        address: B,
        code: Asm::new().sstore(7, 7).stop().build(),
        ..Account::default()
    });
    build.account(Account {
        address: C,
        code: vec![op::STOP],
        storage: vec![(1, 1)],
        ..Account::default()
    });
    // The envelope: a deposit's, or the first mechanism's that wants one; the other joins it.
    let deposit = [first, second].into_iter().find(|m| MECHANISMS[*m] == "deposit");
    let mut envelope =
        deposit.or_else(|| [first, second].into_iter().find(|m| wants_envelope(*m, &mut r)));
    // A keyless or precompile envelope runs no code of the scenario first, so a partner whose
    // part is code would do nothing: most such draws make both parts code of the first frame.
    if let Some(e) = envelope {
        let other = if e == first { second } else { first };
        if matches!(MECHANISMS[e], "keyless" | "precompile") &&
            MECHANISMS[other] != "limit" &&
            r.chance(60)
        {
            envelope = None;
        }
    }
    match envelope {
        Some(e) => {
            set_envelope(e, &mut r, &mut build);
            let other = if e == first { second } else { first };
            if !join_envelope(e, other, &mut r, &mut build) {
                apply(other, &mut r, &mut build);
            }
        }
        None => {
            apply(first, &mut r, &mut build);
            apply(second, &mut r, &mut build);
        }
    }
    let mut s = finish(build);
    if let Some(k) = tight {
        s.gas_limit = tight_gas(&s, k);
    }
    s
}

/// The gas limit around the charges a transaction makes before its first frame, by `k`: one
/// below its validation minimum (0), at it (1), one below and at the start record (2, 3), one
/// below and at a deposit's created caller or a value's new recipient (4, 5).
fn tight_gas(s: &Scenario, k: u64) -> u64 {
    let value = !s.value.is_zero();
    let base = 12_000 + if s.create { 12_000 } else { 3_000 + if value { 6_000 } else { 0 } };
    let tokens: u64 = s.data.iter().map(|b| if *b == 0 { 4 } else { 16 }).sum();
    let initcode = if s.create { 2 * (s.data.len() as u64).div_ceil(32) } else { 0 };
    let auths = s.auths.len() as u64;
    let mut needs = base + tokens + initcode + 7_816 * auths;
    if s.deposit.is_none() {
        needs += (310 + s.data.len() as u64 + 101 * auths) * 88;
    }
    let start = if value || s.create { 3_520 } else { 0 };
    let state = 183_600;
    match k {
        0 => needs - 1,
        1 => needs,
        2 => needs + start.max(1) - 1,
        3 => needs + start,
        4 => needs + start + state - 1,
        _ => needs + start + state,
    }
}

fn wants_envelope(m: usize, r: &mut Rng) -> bool {
    match MECHANISMS[m] {
        "keyless" => r.chance(70),
        "authorization" => r.chance(80),
        "deposit" => true,
        "precompile" => r.chance(25),
        "creation" => r.chance(30),
        "selfdestruct" => r.chance(25),
        "system" => r.chance(30),
        _ => false,
    }
}

/// Places the scenario's code where its first frame runs it.
fn finish(mut b: Build) -> Scenario {
    let mut code = core::mem::take(&mut b.code);
    match b.holder {
        Holder::A => {
            code.extend(b.tail.clone());
            b.edit(A, |a| a.code = code);
        }
        Holder::Init => {
            code.extend(b.tail.clone());
            b.s.data = code;
        }
        Holder::Oracle => {
            code.extend(b.tail.clone());
            b.edit(ORACLE, |o| o.code = code);
        }
        Holder::D => {
            code.extend(Asm::new().push_addr(b.heir).op(op::SELFDESTRUCT).build());
            b.edit(D, |d| d.code = code);
            b.has_d = false;
        }
        Holder::None => {}
    }
    if b.has_d {
        let destroy = Asm::new().push_addr(b.heir).op(op::SELFDESTRUCT).build();
        b.edit(D, |d| d.code = destroy);
    }
    b.s
}

/// Sets mechanism `m`'s envelope.
fn set_envelope(m: usize, r: &mut Rng, b: &mut Build) {
    match MECHANISMS[m] {
        "keyless" => keyless_envelope(r, b),
        "authorization" => {
            b.s.auths = authorizations(r, b);
            b.s.to = r.pick(&[A, A, A, X1]);
            if b.s.to == X1 {
                b.holder = Holder::None;
            }
        }
        "deposit" => {
            let mint = r.pick(&[0u64, 1_000_000]);
            b.s.deposit = Some(mint);
            b.s.value = U256::from(r.pick(&[0u64, 0, 1, 5]));
            match r.below(3) {
                0 => b.s.sender = DEPOSITOR,
                1 => b.sender(Account {
                    address: DEPOSITOR,
                    balance: U256::from(ONE_ETHER),
                    ..Account::default()
                }),
                _ => b.sender(Account {
                    address: DEPOSITOR,
                    balance: U256::from(10),
                    code: delegation(r.pick(&[B, C])),
                    ..Account::default()
                }),
            }
            b.s.accounts.retain(|x| x.address != SENDER);
            if r.chance(25) {
                b.s.create = true;
                b.holder = Holder::Init;
                b.tail = init_returning(r.pick(&[0u64, 1, 32]));
            } else if r.chance(25) {
                // A first frame answered without code: an absent account, the identity precompile.
                b.s.to = r.pick(&[EMPTY, IDENTITY]);
                b.holder = Holder::None;
            }
        }
        "precompile" => {
            let (pc, input, price) = precompile_input(r);
            b.s.to = pc;
            b.s.data = input;
            b.holder = Holder::None;
            if let Some(p) = price {
                let tokens: u64 = b.s.data.iter().map(|x| if *x == 0 { 1 } else { 4 }).sum();
                let needs = 15_000 + 4 * tokens + (310 + b.s.data.len() as u64) * 88;
                if r.chance(60) {
                    b.s.gas_limit = (needs + p + r.below(81)).saturating_sub(40);
                }
            }
        }
        "creation" => {
            b.s.create = true;
            b.holder = Holder::Init;
            b.tail = init_tail(r);
            if r.chance(30) {
                b.account(occupied(SENDER.create(0)));
            }
            b.s.value = U256::from(r.pick(&[0u64, 0, 3]));
        }
        "selfdestruct" => {
            b.s.to = D;
            b.holder = Holder::D;
            b.heir = heir(r, b);
            b.s.value = U256::from(r.pick(&[0u64, 5]));
        }
        "system" => {
            b.account(crate::edge::registry_naming(SYSTEM));
            b.sender(Account {
                address: SYSTEM,
                balance: U256::from(ONE_ETHER),
                ..Account::default()
            });
            b.s.accounts.retain(|x| x.address != SENDER);
            b.s.to = ORACLE;
            b.s.chain_id = Some(1);
            b.holder = Holder::Oracle;
        }
        other => unreachable!("{other} takes no envelope"),
    }
}

/// Joins mechanism `m` to the envelope of mechanism `e` in the envelope's own form; `false` when
/// there is none (`m` then adds its code and state).
fn join_envelope(e: usize, m: usize, r: &mut Rng, b: &mut Build) -> bool {
    match (MECHANISMS[e], MECHANISMS[m]) {
        ("deposit", "keyless") => {
            keyless_envelope(r, b);
            true
        }
        ("deposit", "creation") if !b.s.create => {
            b.s.create = true;
            b.holder = Holder::Init;
            b.tail = init_tail(r);
            if r.chance(40) {
                b.account(occupied(b.s.sender.create(0)));
            }
            true
        }
        ("deposit", "authorization") => {
            // A delegated sender, or a recipient that delegates.
            if r.chance(50) {
                let sender = b.s.sender;
                b.sender(Account {
                    address: sender,
                    balance: U256::from(r.pick(&[0u64, 10])),
                    code: delegation(r.pick(&[B, C, A])),
                    ..Account::default()
                });
            } else {
                b.account(Account { address: X2, code: delegation(A), ..Account::default() });
                b.s.to = X2;
            }
            true
        }
        ("deposit" | "authorization", "precompile") => {
            let (pc, input, _) = precompile_input(r);
            b.s.to = pc;
            b.s.data = input;
            b.holder = Holder::None;
            true
        }
        ("deposit" | "authorization", "selfdestruct") => {
            b.s.to = D;
            b.holder = Holder::D;
            b.heir = heir(r, b);
            true
        }
        ("deposit" | "authorization", "system") if r.chance(50) => {
            // From the live system address: a failed deposit, a refused list.
            b.account(crate::edge::registry_naming(SYSTEM));
            let balance = U256::from(ONE_ETHER);
            b.sender(Account { address: SYSTEM, balance, ..Account::default() });
            true
        }
        ("authorization", "keyless") => {
            keyless_envelope(r, b);
            if let Some(signer) = keyless_signer(&b.s.data) {
                let authority = r.pick(&[signer, signer, signer.create(0), b.s.sender]);
                let nonce = if authority == b.s.sender { 1 } else { 0 };
                b.s.auths.push(Auth { chain_id: 1, address: B, nonce, authority: Some(authority) });
            }
            true
        }
        ("deposit", "limit") => {
            // A created caller (the transaction's unit of state growth) under limits at and
            // around their use: state growth 0, and maybe a KV or data-size limit the start
            // crosses first.
            if r.chance(60) {
                b.s.accounts.retain(|x| x.address != b.s.sender);
                b.s.limits.growth = Some(r.pick(&[0u64, 0, 1]));
            } else {
                b.s.limits.growth = Some(r.pick(&[0u64, 1, 2]));
            }
            match r.below(3) {
                0 => b.s.limits.kv = Some(r.pick(&[0u64, 1])),
                1 => b.s.limits.data = Some(r.pick(&[310u64, 410, 500])),
                _ => {}
            }
            if r.chance(50) {
                b.s.to = r.pick(&[EMPTY, IDENTITY, A]);
                b.s.value = U256::from(r.pick(&[0u64, 1]));
                if b.s.to != A {
                    b.holder = Holder::None;
                }
            }
            true
        }
        ("keyless", "creation") => {
            if let Some(signer) = keyless_signer(&b.s.data) {
                b.account(occupied(signer.create(0)));
            }
            true
        }
        _ => false,
    }
}

/// Mechanism `m`'s code and state on a transaction whose envelope another mechanism (or none) set.
fn apply(m: usize, r: &mut Rng, b: &mut Build) {
    let first = r.chance(50);
    let runs = b.holder != Holder::None;
    match MECHANISMS[m] {
        "keyless" => {
            // From a contract: not dispatched, the account's own code runs.
            let (data, _, _) = keyless_call(r.pick(&["ok", "revert", "writes"]), 1_000_000);
            let value = if r.chance(20) { 1 } else { 0 };
            if runs {
                b.fragment(
                    mem_call(Asm::new(), op::CALL, KEYLESS, value, 300_000, &data)
                        .op(op::POP)
                        .build(),
                    first,
                );
            }
        }
        "authorization" => {
            // A pre-existing delegation, called; or the sender delegated.
            let target = r.pick(&[B, C, D, Address::ZERO]);
            b.account(Account {
                address: X2,
                code: delegation(target),
                nonce: 1,
                balance: U256::from(r.pick(&[0u64, 3])),
                ..Account::default()
            });
            if target == D {
                b.has_d = true;
                b.heir = heir(r, b);
            }
            if b.s.sender == SENDER && r.chance(40) {
                b.sender(Account {
                    address: SENDER,
                    balance: U256::from(ONE_ETHER),
                    code: delegation(r.pick(&[B, C])),
                    ..Account::default()
                });
            }
            if runs {
                let value = if r.chance(30) { 1 } else { 0 };
                let gas = r.pick(&[30_000u64, 1_000_000]);
                b.fragment(
                    Asm::new().call_with(op::CALL, X2, value, gas, 0).op(op::POP).build(),
                    first,
                );
            }
        }
        "deposit" => unreachable!("a deposit always takes the envelope"),
        "detention" => {
            b.s.limits.block_env_cap =
                Some(r.pick(&[1u64, 100, 2_000, 3_000, 20_000, 100_000, 1_000_000]));
            if r.chance(25) {
                b.s.beneficiary = r.pick(&[b.s.sender, A, B, X2]);
            }
            let read = match r.below(6) {
                0 => Asm::new().op(op::TIMESTAMP),
                1 => Asm::new().op(op::NUMBER),
                2 => Asm::new().op(op::COINBASE),
                3 => Asm::new().push_addr(b.s.beneficiary).op(op::BALANCE),
                4 => Asm::new().push(1).op(op::BLOCKHASH),
                _ => Asm::new().op(0x48),
            };
            if runs {
                b.fragment(read.op(op::POP).build(), r.chance(70));
            }
        }
        "precompile" => {
            let (pc, input, price) = precompile_input(r);
            let forward = match price {
                Some(p) => r.pick(&[p.saturating_sub(1), p, p + 1, 2 * p, 1_000_000]),
                None => r.pick(&[499u64, 500, 501, 30_000, 1_000_000]),
            };
            let kind = r.pick(&[op::CALL, op::CALL, op::STATICCALL]);
            let value = if kind == op::CALL && r.chance(15) { 1 } else { 0 };
            if runs {
                b.fragment(
                    mem_call(Asm::new(), kind, pc, value, forward, &input).op(op::POP).build(),
                    first,
                );
            }
        }
        "creation" => {
            let mut init = Vec::new();
            if r.chance(30) {
                init.extend(Asm::new().sstore(1, 1).build());
            }
            if r.chance(20) {
                init.extend(Asm::new().op(op::TIMESTAMP).op(op::POP).build());
            }
            init.extend(match r.below(5) {
                0 => Asm::new().revert0().build(),
                1 => vec![op::INVALID],
                2 => Asm::new().push_addr(SENDER).op(op::SELFDESTRUCT).build(),
                _ => init_returning(r.pick(&[0u64, 1, 64])),
            });
            let value = if r.chance(30) { 1 } else { 0 };
            let creator = match b.holder {
                Holder::A => Some(A),
                Holder::Oracle => Some(ORACLE),
                Holder::D => Some(D),
                _ => None,
            };
            let code = if r.chance(50) {
                if r.chance(30) &&
                    let Some(c) = creator
                {
                    b.account(occupied(c.create(0)));
                }
                Asm::new().create(&init, value).op(op::POP).build()
            } else {
                let salt = r.pick(&[0u64, 1, 7]);
                if r.chance(30) &&
                    let Some(c) = creator
                {
                    b.account(occupied(
                        c.create2(U256::from(salt).to_be_bytes::<32>(), keccak256(&init)),
                    ));
                }
                create2(&init, value, salt)
            };
            if runs {
                b.fragment(code, first);
            }
        }
        "selfdestruct" => {
            b.has_d = true;
            b.heir = heir(r, b);
            let balance = U256::from(r.pick(&[0u64, 7]));
            b.edit(D, |d| d.balance = balance);
            if runs {
                let value = r.pick(&[0u64, 1]);
                let gas = r.pick(&[10_000u64, 1_000_000]);
                b.fragment(
                    Asm::new().call_with(op::CALL, D, value, gas, 0).op(op::POP).build(),
                    first,
                );
            }
        }
        "limit" => match r.below(3) {
            0 => b.s.limits.data = Some(r.pick(&[0u64, 310, 311, 330, 350, 400, 470, 500, 1_000])),
            1 => b.s.limits.kv = Some(r.pick(&[0u64, 1, 2, 3])),
            _ => b.s.limits.growth = Some(r.pick(&[0u64, 1, 2])),
        },
        "state" => {
            for _ in 0..r.below(3) + 1 {
                let site = match r.below(4) {
                    0 => (A, Some(10 + r.below(3))),
                    1 => (EMPTY2, None),
                    2 => (A.create(0), None),
                    _ => (b.s.sender, None),
                };
                b.s.salt.push((site.0, site.1, r.pick(&[2u64, 8, 64])));
            }
            if runs {
                let mut code = Asm::new();
                for k in 0..r.below(3) {
                    code = code.sstore(10 + k, 1);
                }
                if r.chance(50) {
                    code = code.call_with(op::CALL, EMPTY2, 1, 0, 0).op(op::POP);
                }
                b.fragment(code.build(), first);
                b.edit(A, |a| a.balance = U256::from(10));
            }
        }
        "ending" => {
            if b.holder == Holder::Init {
                b.tail = init_tail(r);
                return;
            }
            b.tail = match r.below(6) {
                0 => vec![op::INVALID],
                1 => Asm::new().revert0().build(),
                2 => Asm::new()
                    .push_addr(r.pick(&[SENDER, b.s.beneficiary, EMPTY]))
                    .op(op::SELFDESTRUCT)
                    .build(),
                3 => {
                    // A refund: a slot the transaction found set, cleared.
                    let holder = match b.holder {
                        Holder::Oracle => ORACLE,
                        Holder::D => D,
                        _ => A,
                    };
                    b.edit(holder, |h| h.storage.push((3, 1)));
                    Asm::new().sstore(3, 0).stop().build()
                }
                4 => Asm::new().push(32).push(0).op(op::RETURN).build(),
                _ => vec![op::STOP],
            };
            if b.holder == Holder::A && r.chance(30) {
                b.s.value = U256::from(1);
            }
        }
        "system" => {
            let call = match r.below(4) {
                0 => (ACCESS_CONTROL, sel("disableVolatileDataAccess()")),
                1 => (ACCESS_CONTROL, sel("enableVolatileDataAccess()")),
                2 => (ACCESS_CONTROL, sel("isVolatileDataAccessDisabled()")),
                _ => (LIMIT_CONTROL, sel("remainingComputeGas()")),
            };
            let value = if r.chance(15) { 1 } else { 0 };
            if runs {
                b.fragment(
                    mem_call(Asm::new(), op::CALL, call.0, value, r.pick(&[0u64, 50_000]), &call.1)
                        .op(op::POP)
                        .build(),
                    first,
                );
            }
        }
        "oracle" => {
            if r.chance(60) {
                b.s.oracle = vec![(5, r.pick(&[0u64, 1, 77]))];
            }
            if r.chance(40) {
                b.s.limits.oracle_cap = Some(r.pick(&[1_000u64, 20_000, 300_000]));
            }
            let oracle_code = Asm::new()
                .push(5)
                .op(op::SLOAD)
                .op(op::POP)
                .push(0)
                .push(0)
                .op(op::LOG0)
                .stop()
                .build();
            if b.holder == Holder::Oracle {
                b.fragment(Asm::new().push(5).op(op::SLOAD).op(op::POP).build(), first);
                b.edit(ORACLE, |o| o.storage.push((5, 9)));
            } else {
                b.edit(ORACLE, |o| {
                    o.code = oracle_code;
                    o.storage.push((5, 9));
                });
            }
            if runs {
                let code = if r.chance(50) {
                    Asm::new()
                        .call_with(op::CALL, ORACLE, 0, r.pick(&[3_000u64, 100_000]), 0)
                        .op(op::POP)
                        .build()
                } else {
                    let hint = send_hint(r.below(3), &vec![0x5a; r.pick(&[0usize, 32, 300])]);
                    mem_call(
                        Asm::new(),
                        op::CALL,
                        ORACLE,
                        0,
                        r.pick(&[0u64, 60_000, 1_000_000]),
                        &hint,
                    )
                    .op(op::POP)
                    .build()
                };
                b.fragment(code, first);
            }
        }
        other => unreachable!("{other}"),
    }
}

/// A keyless deployment as the transaction, in one of its roles.
fn keyless_envelope(r: &mut Rng, b: &mut Build) {
    let (name, _, _) = KEYLESS_TXS[r.below(KEYLESS_TXS.len() as u64) as usize];
    let (data, signer, deploy) = keyless_call(name, r.pick(&[100_000u64, 1_000_000, 5_000_000]));
    b.s.to = KEYLESS;
    b.s.data = data;
    b.holder = Holder::None;
    if r.chance(10) {
        b.s.value = U256::from(1);
    }
    match r.below(8) {
        0 => {}
        1 => b.account(Account { address: signer, balance: U256::from(10), ..Account::default() }),
        2 => b.account(Account { address: signer, nonce: 1, ..Account::default() }),
        3 => {
            b.s.accounts.retain(|x| x.address != SENDER);
            b.sender(Account {
                address: signer,
                balance: U256::from(ONE_ETHER),
                ..Account::default()
            });
        }
        4 => b.s.beneficiary = signer,
        5 => b.account(Account { address: deploy, balance: U256::from(3), ..Account::default() }),
        6 => b.s.beneficiary = deploy,
        _ => b.account(occupied(deploy)),
    }
}

/// The signer of the keyless call `data` builds, if it is one of `KEYLESS_TXS`'.
fn keyless_signer(data: &[u8]) -> Option<Address> {
    KEYLESS_TXS
        .iter()
        .map(|(name, _, _)| keyless_call(name, 0))
        .find(|(d, _, _)| d.get(68..) == data.get(68..))
        .map(|(_, signer, _)| signer)
}

/// An EIP-7702 list of 1 to 3 entries: authorities fresh, the sender, the beneficiary or the
/// recipient, delegating to B, C, D or clearing; nonces mostly right.
fn authorizations(r: &mut Rng, b: &mut Build) -> Vec<Auth> {
    let mut v = Vec::new();
    for _ in 0..r.below(3) + 1 {
        let authority = match r.below(6) {
            0 | 1 => X1,
            2 => b.s.sender,
            3 => {
                b.s.beneficiary = X1;
                X1
            }
            4 => A,
            _ => B,
        };
        let nonce = if authority == b.s.sender { 1 } else { r.pick(&[0u64, 0, 0, 1]) };
        let address = r.pick(&[B, C, D, Address::ZERO]);
        if address == D {
            b.has_d = true;
            b.heir = heir(r, b);
        }
        v.push(Auth {
            chain_id: r.pick(&[1u64, 1, 1, 0]),
            address,
            nonce,
            authority: Some(authority),
        });
    }
    v
}

fn heir(r: &mut Rng, b: &Build) -> Address {
    r.pick(&[b.s.beneficiary, b.s.sender, EMPTY, D, A])
}

fn delegation(target: Address) -> Vec<u8> {
    let mut c = vec![0xef, 0x01, 0x00];
    c.extend_from_slice(target.as_slice());
    c
}

fn occupied(address: Address) -> Account {
    Account { address, nonce: 1, code: vec![op::STOP], ..Account::default() }
}

/// How a creation's init code ends: its runtime returned, a revert, a halt, a destruction.
fn init_tail(r: &mut Rng) -> Vec<u8> {
    match r.below(5) {
        0 => Asm::new().revert0().build(),
        1 => vec![op::INVALID],
        2 => Asm::new().push_addr(SENDER).op(op::SELFDESTRUCT).build(),
        _ => init_returning(r.pick(&[0u64, 1, 64])),
    }
}

/// `CREATE2` of `init` (copied into memory at 0) with `value` and `salt`; pops the address.
fn create2(init: &[u8], value: u64, salt: u64) -> Vec<u8> {
    let mut a = Asm::new();
    for (i, chunk) in init.chunks(32).enumerate() {
        let mut word = [0u8; 32];
        word[..chunk.len()].copy_from_slice(chunk);
        a = a.push_bytes(&word).push(32 * i as u64).op(op::MSTORE);
    }
    a.push(salt).push(init.len() as u64).push(0).push(value).op(op::CREATE2).op(op::POP).build()
}

/// Writes all of `input` into memory at 0 (zero words too), then a call of `kind` to `to`.
fn mem_call(a: Asm, kind: u8, to: Address, value: u64, gas: u64, input: &[u8]) -> Asm {
    let mut a = a;
    for (i, chunk) in input.chunks(32).enumerate() {
        let mut w = [0u8; 32];
        w[..chunk.len()].copy_from_slice(chunk);
        a = a.push_bytes(&w).push(32 * i as u64).op(op::MSTORE);
    }
    a.call_with(kind, to, value, gas, input.len() as u64)
}

/// A precompile's edge input and, where it is fixed by the input, its price.
fn precompile_input(r: &mut Rng) -> (Address, Vec<u8>, Option<u64>) {
    let pc = |x: u16| {
        let mut a = [0u8; 20];
        a[18] = (x >> 8) as u8;
        a[19] = x as u8;
        Address::new(a)
    };
    let lengths = |b: u64, e: u64, m: u64| {
        let mut input = Vec::new();
        for len in [b, e, m] {
            input.extend(U256::from(len).to_be_bytes::<32>());
        }
        input
    };
    match r.below(8) {
        0 | 1 => {
            // ModExp at, one over and far over EIP-7823's bound in each length, and small.
            let over = r.pick(&[1_024u64, 1_025, 20_000, u64::MAX]);
            let mut input = match r.below(3) {
                0 => lengths(over, 1, 1),
                1 => lengths(1, over, 1),
                _ => lengths(1, 1, over),
            };
            if over == 1_024 {
                input.resize(96 + 1_026, 0xff);
            }
            (pc(5), input, None)
        }
        2 => {
            let input = if r.chance(50) { super::classes::kzg_valid_input() } else { vec![0; 192] };
            (pc(0x0a), input, Some(100_000))
        }
        3 => (pc(1), vec![0x11; 128], Some(3_000)),
        4 => {
            let n = r.pick(&[0usize, 32, 33, 1_000]);
            (IDENTITY, vec![0xab; n], Some(15 + 3 * (n as u64).div_ceil(32)))
        }
        5 => {
            let rounds: u32 = r.pick(&[0u32, 12, 20_000]);
            let mut input = rounds.to_be_bytes().to_vec();
            input.extend([0u8; 208]);
            input.push(r.pick(&[0u8, 1]));
            (pc(9), input, Some(u64::from(rounds)))
        }
        6 => (pc(0x100), vec![0; 160], None),
        _ => {
            let (addr, len) = r.pick(&[(0x0bu16, 256usize), (0x0c, 160), (0x0e, 288), (0x10, 64)]);
            (pc(addr), vec![0; len], None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each run of 66 seeds draws every unordered pair once: the sample's 330 seeds draw each
    /// pair 5 times, the full batch's 3,300 seeds 50 times.
    #[test]
    fn test_every_pair_is_drawn_in_each_run_of_66_seeds() {
        for (seeds, times) in [(330u64, 5usize), (3_300, 50)] {
            let mut counts = std::collections::BTreeMap::new();
            for seed in 1..=seeds {
                let (a, b) = pair(seed);
                assert!(a < b && b < MECHANISMS.len(), "seed {seed}: {a} {b}");
                *counts.entry((a, b)).or_insert(0usize) += 1;
            }
            assert_eq!(counts.len(), PAIRS as usize);
            assert!(counts.values().all(|n| *n == times), "{counts:?}");
        }
    }
}
