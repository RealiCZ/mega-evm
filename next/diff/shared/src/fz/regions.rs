//! Fuzz modes 11 to 13: random inputs in regions no earlier mode drew, where the engine and the
//! reference were found apart.
//!
//! - 11 (`fxf`), the first frame's gas window: every envelope (legacy with and without a chain id,
//!   EIP-2930, EIP-1559, EIP-7702, a deposit from an existing or an absent caller, a creation by a
//!   user and by a deposit, a Mega System Transaction from an existing or an absent system address)
//!   to a recipient that is absent, an existing account, an existing empty account, a contract, an
//!   account delegating (EIP-7702) to a contract, the sender itself, or the authority an EIP-7702
//!   list delegates, with and without a value, at SALT prices of one to eight minimum buckets, the
//!   sender or the recipient sometimes the block beneficiary. The gas limit is drawn around each
//!   charge made before and at the first frame: the validation minimum (the intrinsic gas and the
//!   body's history), the start record, the recipient's new-account or the created account's state,
//!   the frame's first work; for a deposit, its intrinsic gas, its created caller's account state,
//!   then the first frame; also above the execution cap, where the reservoir pays history first. A
//!   first frame whose state charge cannot be paid is evm2 #871's halted first frame, which the
//!   gate accepts by its shape (`halted_first_frame`, `known-differences.toml`).
//! - 12 (`fxv`), an instruction reads volatile data, then fails: the instructions whose reads the
//!   gate's refusal sweep lists (the account opcodes and EXTCODECOPY on the block beneficiary, the
//!   call family to it or to an account delegating to it, SELFDESTRUCT to it), the beneficiary's
//!   own SELFBALANCE and SELFDESTRUCT, the Oracle's SLOAD and the block-environment opcodes, each
//!   in a frame whose gas is drawn across the charges before and after its load, on an ordinary or
//!   a full stack, under limits small enough that its records cross them; the caller then writes,
//!   which a detention its child left would stop.
//! - 13 (`fxk`), short stacks under detention: every instruction that pops operands and makes a
//!   dynamic charge, and BLOCKHASH (which pops before it reads), with its last operands missing
//!   after a volatile read, under a cap its charges may cross, in the transaction's frame or a
//!   child; with every operand, and undetained, as controls.
#![allow(dead_code)]
use super::Rng;
use crate::{
    A, Account, Asm, Auth, BELOW_CAP, C, D, EMPTY, Limits, ORACLE, SENDER, Scenario, acct,
    bd::{self, BEN, P, PLENTY, STACK_LIMIT, delegation, fill},
    edge::{AUTHORITY, SYSTEM, registry_naming, store_one},
    funded, init_returning, op,
    q2::EMPTY2,
    q8::X1,
    scenario,
};
use alloy_primitives::{Address, U256};

/// The history of one write record (40 bytes at 88 gas a byte): a start record.
const RECORD: u64 = 40 * 88;
/// The account state gas of one minimum bucket (120 bytes at 1,530): a new account, a created
/// account, a deposit's created caller.
const ACCOUNT_STATE: u64 = 120 * 1_530;
/// The body every envelope has, in bytes, and the history of a byte.
const BODY: u64 = 310;
const HISTORY: u64 = 88;
const EXECUTION_CAP: u64 = 200_000_000;
/// A deposit's caller that does not exist before it.
const FRESH: Address = Address::with_last_byte(0xd1);

const EXP: u8 = 0x0a;
const KECCAK256: u8 = 0x20;
const EXTCODECOPY: u8 = 0x3c;
const EXTCODEHASH: u8 = 0x3f;
const MSTORE8: u8 = 0x53;
const TSTORE: u8 = 0x5d;
const MCOPY: u8 = 0x5e;

/// The scenario of seed `seed` of region mode `mode` (11 to 13), not yet named.
pub(super) fn generate(mode: u8, seed: u64) -> Scenario {
    let mut r = Rng::for_seed(seed ^ (u64::from(mode) << 48));
    match mode {
        11 => first_frame(&mut r),
        12 => read_then_fail(&mut r),
        13 => short_stack(&mut r),
        _ => panic!("unknown fuzz mode {mode}"),
    }
}

/// A gas limit around `at`: mostly within a few gas, else within a few hundred, else a few
/// thousand below to many thousand above.
fn near(r: &mut Rng, at: u64) -> u64 {
    let d = match r.below(10) {
        0..=4 => r.below(5) as i64 - 2,
        5..=7 => r.below(201) as i64 - 100,
        _ => r.below(12_001) as i64 - 2_000,
    };
    (at as i64 + d).max(0) as u64
}

/// Calldata gas: 4 a token, a non-zero byte four tokens.
fn calldata(data: &[u8]) -> u64 {
    data.iter().map(|b| if *b == 0 { 4 } else { 16 }).sum()
}

/// The first frame's gas window.
fn first_frame(r: &mut Rng) -> Scenario {
    let mut s = scenario("", Vec::new(), BELOW_CAP, Limits::default());
    let envelope = r.below(11);
    let deposit = matches!(envelope, 5 | 6 | 9);
    let create = matches!(envelope, 7..=9);
    let fresh_caller = envelope == 6 || (envelope == 9 && r.chance(50));
    if fresh_caller {
        s.accounts.retain(|a| a.address != SENDER);
        s.sender = FRESH;
    }
    if envelope == 10 {
        // A Mega System Transaction: a legacy call to the Oracle from the live system address,
        // promoted to a deposit (no history; a created caller is counted outside the frames).
        s.accounts.retain(|a| a.address != SENDER);
        s.accounts.push(registry_naming(SYSTEM));
        s.accounts.push(acct(ORACLE, store_one()));
        if r.chance(60) {
            s.accounts.push(funded(SYSTEM, Vec::new(), r.pick(&[0u64, 10])));
        }
        s.sender = SYSTEM;
        s.to = ORACLE;
        s.chain_id = Some(1);
        s.value = U256::from(r.pick(&[0u64, 0, 1]));
        s.beneficiary = r.pick(&[Address::ZERO, Address::ZERO, SYSTEM, ORACLE]);
        let at = r.pick(&[21_000u64, 21_000 + ACCOUNT_STATE, 21_000 + 22_100, 21_000 + 50_000]);
        s.gas_limit = near(r, at).max(1);
        return s;
    }
    match envelope {
        0 => s.chain_id = None,
        1 => s.chain_id = Some(1),
        2 => s.tx_type = 1,
        3 | 8 => s.tx_type = 2,
        4 => {
            let authority = r.pick(&[Some(AUTHORITY), Some(SENDER), None]);
            s.auths.push(Auth {
                chain_id: 1,
                address: r.pick(&[C, A]),
                nonce: if authority == Some(SENDER) { 1 } else { 0 },
                authority,
            });
        }
        _ => {}
    }
    let value = r.pick(&[0u64, 1, 1, 5]);
    s.value = U256::from(value);
    let m = r.pick(&[1u64, 1, 2, 8]);
    // The recipient, and whether it is empty (its value's new account is charged).
    let mut empty_recipient = false;
    if create {
        s.create = true;
        s.data = match r.below(4) {
            0 => Vec::new(),
            1 => init_returning(1),
            2 => {
                let mut c = Asm::new().sstore(1, 1).build();
                c.extend(init_returning(1));
                c
            }
            _ => vec![op::STOP],
        };
        // Both callers are at nonce 0.
        let target = s.sender.create(0);
        empty_recipient = true;
        if r.chance(20) {
            // A balance-only account where the creation lands: it is not created again.
            s.accounts.push(funded(target, Vec::new(), 3));
            empty_recipient = false;
        }
        s.salt.push((target, None, m));
    } else {
        let to = match r.below(7) {
            0 => {
                empty_recipient = true;
                EMPTY
            }
            1 => {
                s.accounts.push(funded(A, Vec::new(), 1));
                A
            }
            2 => {
                // An existing empty account: it exists, yet a value creates it again.
                s.accounts.push(Account { address: EMPTY2, ..Account::default() });
                empty_recipient = true;
                EMPTY2
            }
            3 => {
                s.accounts.push(acct(A, store_one()));
                A
            }
            4 => {
                s.accounts.push(acct(A, delegation(C)));
                s.accounts.push(acct(C, store_one()));
                A
            }
            5 => s.sender,
            // The authority the list delegates: loaded and changed before the first frame.
            6 if !s.auths.is_empty() => s.auths[0].authority.unwrap_or(AUTHORITY),
            _ => {
                s.accounts.push(acct(A, vec![op::STOP]));
                A
            }
        };
        s.to = to;
        s.salt.push((to, None, m));
    }
    if deposit {
        let mint = r.pick(&[0u64, value, value + 1, 1_000]);
        s.deposit = Some(mint);
        if fresh_caller {
            s.salt.push((FRESH, None, r.pick(&[1u64, 1, 8])));
        }
    } else if r.chance(50) {
        s.gas_price = 1;
    }
    s.beneficiary = r.pick(&[Address::ZERO, Address::ZERO, Address::ZERO, s.sender, s.to]);
    // What each charge before the first frame needs, in order.
    let to_other = create || (value > 0 && s.to != s.sender);
    let state = if (create || value > 0) && empty_recipient { ACCOUNT_STATE * m } else { 0 };
    let mut anchors = Vec::new();
    if deposit {
        let intrinsic = 21_000 + calldata(&s.data) + if create { 32_000 } else { 0 };
        let caller =
            if fresh_caller { ACCOUNT_STATE * s.salt.last().map_or(1, |x| x.2) } else { 0 };
        anchors.extend([intrinsic, intrinsic + caller, intrinsic + caller + state]);
        anchors.push(intrinsic + caller + state + r.below(30_000));
    } else {
        let base = if create {
            24_000
        } else if value > 0 {
            21_000
        } else {
            15_000
        };
        let words = s.data.len().div_ceil(32) as u64;
        let body = BODY + s.data.len() as u64 + 101 * s.auths.len() as u64;
        let intrinsic = base +
            calldata(&s.data) +
            if create { 2 * words } else { 0 } +
            25_000 * s.auths.len() as u64;
        let validation = intrinsic + body * HISTORY;
        let record = if to_other { RECORD } else { 0 };
        anchors.extend([validation, validation + record, validation + record + state]);
        anchors.push(validation + record + state + r.below(30_000));
        if r.chance(12) {
            // Above the execution cap: the reservoir pays the body's history, then the record.
            let over = r.pick(&anchors) - intrinsic;
            anchors = vec![EXECUTION_CAP + intrinsic + over.saturating_sub(body * HISTORY)];
        }
    }
    let at = r.pick(&anchors);
    s.gas_limit = near(r, at).max(1);
    s
}

/// An instruction reads volatile data, then fails.
fn read_then_fail(r: &mut Rng) -> Scenario {
    let probe_balance = r.pick(&[0u64, 0, 10]);
    let mut s = match r.below(10) {
        0..=4 => {
            // An instruction of the refusal sweep, with access on: it reads, then may fail on a
            // charge after its load, its limits, or its stack.
            let probes = bd::probes();
            let p = &probes[r.below(probes.len() as u64) as usize];
            let (warm, extra) = if p.variants.is_empty() {
                (Vec::new(), Vec::new())
            } else {
                let v = &p.variants[r.below(p.variants.len() as u64) as usize];
                (v.1.clone(), v.2.clone())
            };
            let boundaries: Vec<u64> = p.boundaries.iter().chain(&extra).map(|b| b.1).collect();
            let last = boundaries.iter().copied().max().unwrap_or(0);
            let full = r.chance(15);
            let fill_gas = if full { 2 * (STACK_LIMIT - p.operands.len()) as u64 } else { 0 };
            // After the load: a new account's state, a record's history, the forward.
            let after = match r.below(5) {
                0 => 0,
                1 => r.below(64),
                2 => r.below(8_000),
                3 => ACCOUNT_STATE + r.below(8_000),
                _ => r.below(60_000),
            };
            let at = if r.chance(30) { r.pick(&boundaries) } else { last + after };
            let gas = near(r, at + fill_gas);
            let mut a = Asm::new();
            for w in &warm {
                a = a.push_addr(*w).op(op::BALANCE).op(op::POP);
            }
            let a = a.call_with(op::CALL, P, 0, gas, 0).record_call(3).sstore(5, 1).stop().build();
            let ben = if r.chance(50) { vec![op::STOP] } else { Vec::new() };
            let mut accounts =
                vec![acct(A, a), funded(P, p.code(full), probe_balance), acct(X1, delegation(BEN))];
            if !ben.is_empty() || r.chance(50) {
                accounts.push(funded(BEN, ben, r.pick(&[0u64, 1])));
            }
            scenario("", accounts, BELOW_CAP, Limits::default())
        }
        5 => {
            // The Oracle's SLOAD in its own frame: its cold access charged after the load.
            let at = r.pick(&[3 + 100u64, 3 + 100 + bd::COLD_SLOT]);
            let gas = near(r, at);
            let a = Asm::new().call_with(op::CALL, ORACLE, 0, gas, 0).record_call(3).sstore(5, 1);
            let mut s = scenario(
                "",
                vec![acct(A, a.stop().build()), bd::oracle()],
                BELOW_CAP,
                Limits::default(),
            );
            if r.chance(50) {
                s.oracle = vec![(5, r.pick(&[0u64, 1, 77]))];
            }
            s
        }
        6 => {
            // A block-environment read on a full stack, at its static gas, or with plenty.
            let ops =
                [op::TIMESTAMP, op::NUMBER, op::COINBASE, op::SLOTNUM, 0x44, 0x45, 0x48, 0x4a];
            let o = r.pick(&ops);
            let full = r.chance(50);
            let code = if full { fill(Asm::new(), STACK_LIMIT) } else { Asm::new() };
            let code = code.op(o).stop().build();
            let fill_gas = if full { 2 * STACK_LIMIT as u64 } else { 0 };
            let gas = if r.chance(20) { PLENTY } else { near(r, fill_gas + 2) };
            let a = Asm::new().call_with(op::CALL, P, 0, gas, 0).record_call(3).sstore(5, 1);
            scenario(
                "",
                vec![acct(A, a.stop().build()), acct(P, code)],
                BELOW_CAP,
                Limits::default(),
            )
        }
        7..=8 => {
            // SELFDESTRUCT to the beneficiary (absent, or an account), directly or through a
            // DELEGATECALL, with the frame gas around its charges and its records' history (two
            // records, 7,040), and limits its records may cross.
            let delegated = r.chance(25);
            let destroy = Asm::new().push_addr(BEN).op(op::SELFDESTRUCT).stop().build();
            let at = 3 +
                5_000 +
                r.pick(&[0u64, 3_000]) +
                r.pick(&[0u64, ACCOUNT_STATE]) +
                r.pick(&[0u64, 3_520, 7_040]) +
                if delegated { 2 + 2 + 2 + 2 + 3 + 100 + 2_600 + 2 } else { 0 };
            let gas = near(r, at);
            let a = Asm::new().call_with(op::CALL, P, 0, gas, 0).record_call(3).sstore(5, 1);
            let mut accounts = vec![acct(A, a.stop().build())];
            if delegated {
                let p = Asm::new().call_all(op::DELEGATECALL, C, 0).record_call(1).stop().build();
                accounts.push(funded(P, p, r.pick(&[0u64, 7])));
                accounts.push(acct(C, destroy));
            } else {
                accounts.push(funded(P, destroy, r.pick(&[0u64, 7])));
            }
            if r.chance(60) {
                accounts.push(funded(BEN, Vec::new(), 1));
            }
            scenario("", accounts, BELOW_CAP, Limits::default())
        }
        _ => {
            // The beneficiary's own frame: SELFBALANCE on a full stack, or its destruction.
            let (rest, at) = match r.below(3) {
                0 => {
                    (fill(Asm::new(), STACK_LIMIT).op(op::SELFBALANCE), 2 * STACK_LIMIT as u64 + 5)
                }
                1 => (Asm::new().op(op::SELFBALANCE).push(1).op(op::SSTORE), 5 + 3 + 22_100),
                _ => (Asm::new().push_addr(r.pick(&[C, EMPTY])).op(op::SELFDESTRUCT), 3 + 5_000),
            };
            let extra = r.pick(&[0u64, 0, 2_600, 25_000, ACCOUNT_STATE, 7_040]);
            let gas = near(r, at + extra);
            let a = Asm::new().call_with(op::CALL, BEN, 0, gas, 0).record_call(3).sstore(5, 1);
            let own = rest.stop().build();
            scenario(
                "",
                vec![acct(A, a.stop().build()), funded(BEN, own, 7), funded(C, Vec::new(), 1)],
                BELOW_CAP,
                Limits::default(),
            )
        }
    };
    s.beneficiary = BEN;
    s.limits.block_env_cap = r.pick(&[None, Some(100u64), Some(1_000), Some(6_000), Some(50_000)]);
    s.limits.oracle_cap = r.pick(&[None, None, Some(100u64), Some(6_000)]);
    if r.chance(40) {
        s.limits.kv = Some(r.pick(&[1u64, 2, 3, 4]));
    }
    if r.chance(20) {
        s.limits.growth = Some(r.below(2));
    }
    if r.chance(20) {
        s.limits.data = Some(r.pick(&[350u64, 400, 450, 600]));
    }
    s
}

/// An operand of a short-stack instruction.
#[derive(Clone, Copy)]
enum Opnd {
    W(u64),
    Ad(Address),
}

/// The instructions of mode 13, each with its operands in the order it pops them.
fn short_stack_table(r: &mut Rng) -> (u8, Vec<Opnd>) {
    use Opnd::{Ad, W};
    let len = r.pick(&[0x20u64, 0x8000, 0x8000, 0x10000, 1 << 20]);
    let at = r.pick(&[0u64, 0, 0x40, 0x10000]);
    let addr = r.pick(&[A, C, EMPTY, BEN]);
    let value = r.pick(&[0u64, 1]);
    match r.below(27) {
        0 => (KECCAK256, vec![W(at), W(len)]),
        1 => (op::CALLDATACOPY, vec![W(at), W(0), W(len)]),
        2 => (op::CODECOPY, vec![W(at), W(0), W(len)]),
        3 => (op::RETURNDATACOPY, vec![W(at), W(0), W(0)]),
        4 => (EXTCODECOPY, vec![Ad(addr), W(at), W(0), W(len)]),
        5 => (MCOPY, vec![W(at), W(0), W(len)]),
        6 => (op::MLOAD, vec![W(r.pick(&[0x40u64, 0x8000, 0x10000]))]),
        7 => (op::MSTORE, vec![W(r.pick(&[0x40u64, 0x8000, 0x10000])), W(1)]),
        8 => (MSTORE8, vec![W(r.pick(&[0x40u64, 0x8000, 0x10000])), W(1)]),
        9..=13 => {
            let n = r.below(5) as u8;
            let mut v = vec![W(at), W(len)];
            v.extend((0..n).map(|i| W(u64::from(i) + 1)));
            (op::LOG0 + n, v)
        }
        14 => (op::CREATE, vec![W(value), W(at), W(len)]),
        15 => (op::CREATE2, vec![W(value), W(at), W(len), W(0)]),
        16 | 17 => {
            let kind = r.pick(&[op::CALL, op::CALLCODE]);
            let gas = r.pick(&[0u64, 1_000, 100_000]);
            (kind, vec![W(gas), Ad(addr), W(value), W(at), W(len), W(0), W(0)])
        }
        18 | 19 => {
            let kind = r.pick(&[op::DELEGATECALL, op::STATICCALL]);
            let gas = r.pick(&[0u64, 1_000, 100_000]);
            (kind, vec![W(gas), Ad(addr), W(at), W(len), W(0), W(0)])
        }
        20 => (r.pick(&[op::RETURN, op::REVERT]), vec![W(at), W(len)]),
        21 => (op::SSTORE, vec![W(r.pick(&[1u64, 2])), W(1)]),
        22 => (r.pick(&[op::BALANCE, op::EXTCODESIZE, EXTCODEHASH, op::SLOAD]), vec![Ad(addr)]),
        23 => (EXP, vec![W(2), W(u64::MAX)]),
        24 => (op::SELFDESTRUCT, vec![Ad(addr)]),
        25 => (op::BLOCKHASH, vec![W(0)]),
        _ => (TSTORE, vec![W(1), W(1)]),
    }
}

/// Short stacks under detention.
fn short_stack(r: &mut Rng) -> Scenario {
    let (opcode, operands) = short_stack_table(r);
    let n = operands.len();
    let present = match r.below(10) {
        0..=5 => n - 1,
        6..=8 => r.below(n as u64) as usize,
        _ => n,
    };
    let detained = r.chance(85);
    let mut code = if detained { Asm::new().op(op::TIMESTAMP).op(op::POP) } else { Asm::new() };
    for o in operands[..present].iter().rev() {
        code = match *o {
            Opnd::W(v) => code.push(v),
            Opnd::Ad(a) => code.push_addr(a),
        };
    }
    let code = code.op(opcode).stop().build();
    let accounts_tail = vec![acct(C, store_one()), funded(BEN, Vec::new(), 1)];
    let mut s = if r.chance(65) {
        let gas = match r.below(4) {
            0 => r.below(200_000),
            1 => near(r, 30_000),
            2 => near(r, 3_000),
            _ => PLENTY,
        };
        let a = Asm::new().call_with(op::CALL, P, 0, gas, 0).record_call(3).sstore(5, 1);
        let mut accounts = vec![acct(A, a.stop().build()), funded(P, code, 10)];
        accounts.extend(accounts_tail);
        scenario("", accounts, BELOW_CAP, Limits::default())
    } else {
        let mut accounts = vec![funded(A, code, 10)];
        accounts.extend(accounts_tail);
        let gas = r.pick(&[BELOW_CAP, BELOW_CAP, 100_000, 60_000]);
        scenario("", accounts, gas, Limits::default())
    };
    s.beneficiary = r.pick(&[Address::ZERO, BEN, D]);
    s.limits.block_env_cap =
        r.pick(&[Some(1u64), Some(10), Some(100), Some(1_000), Some(10_000), Some(100_000), None]);
    s
}

#[cfg(test)]
mod tests {
    use super::super::generate;
    use crate::{EMPTY, SENDER};

    /// Mode 11 draws every envelope, absent recipients, and gas limits at the validation minimum
    /// and below the start record of a value call to another account.
    #[test]
    fn test_first_frame_mode_draws_its_windows() {
        let seeds = generate(11, 1, 600);
        let deposits = seeds.iter().filter(|s| s.deposit.is_some()).count();
        let creations = seeds.iter().filter(|s| s.create).count();
        let lists = seeds.iter().filter(|s| !s.auths.is_empty()).count();
        let typed = seeds.iter().filter(|s| s.tx_type != 0).count();
        let absent = seeds.iter().filter(|s| s.to == EMPTY && !s.create).count();
        let fresh = seeds.iter().filter(|s| s.sender != SENDER).count();
        for (what, n) in [
            ("deposits", deposits),
            ("creations", creations),
            ("lists", lists),
            ("typed", typed),
            ("absent recipients", absent),
            ("fresh callers", fresh),
        ] {
            assert!(n >= 20, "{what}: {n}");
        }
        // A legacy value call to an existing account at 48,280 + [0, 3,520): the start window.
        let window = seeds
            .iter()
            .filter(|s| {
                s.deposit.is_none() &&
                    s.auths.is_empty() &&
                    !s.create &&
                    !s.value.is_zero() &&
                    s.to != s.sender &&
                    (48_280..51_800).contains(&s.gas_limit)
            })
            .count();
        assert!(window >= 10, "start window: {window}");
    }

    /// Mode 13 draws short stacks, full stacks and undetained frames.
    #[test]
    fn test_short_stack_mode_draws_its_controls() {
        let seeds = generate(13, 1, 300);
        let detained = seeds
            .iter()
            .filter(|s| s.accounts.iter().any(|a| a.code.first() == Some(&crate::op::TIMESTAMP)))
            .count();
        assert!(detained > 200 && detained < 300, "detained: {detained}");
    }
}
