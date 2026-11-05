//! Scenarios of group "bd": the refusal boundaries, and the creations that fail after their start.
//!
//! In a frame whose volatile access is off, Satin refuses an instruction's read of volatile data:
//! the block beneficiary's account (BALANCE, EXTCODESIZE, EXTCODEHASH, EXTCODECOPY, SELFBALANCE,
//! the call family's target or its EIP-7702 delegate, either end of SELFDESTRUCT), the block
//! environment (COINBASE, TIMESTAMP, NUMBER, PREVRANDAO, GASLIMIT, BASEFEE, BLOBBASEFEE, SLOTNUM,
//! BLOCKHASH) and the Oracle's storage. A refusal happens where the instruction would load what it
//! reads, after the charges it makes before that load, and reverts the frame on the gas it had
//! after the instruction's static gas.
//!
//! Each instruction runs in a probe frame started with an exact gas limit `G`, so that the frame
//! reaches the instruction with a known amount of gas. `G` is taken one below, at and one above
//! every charge the instruction makes before its load: its static gas, then each dynamic charge
//! (EXTCODECOPY's copy and memory expansion; a call's memory expansion and value transfer), then
//! the cold-access check and, for a target that delegates, the delegation guard. Below a charge
//! the frame halts on it; at the last one the load is reached and refused (or, for a delegating
//! target, the delegate's). The block beneficiary is warm from the transaction's start (EIP-3651)
//! and stays warm, so its loads have no cold side: their cold-access windows show no boundary
//! (refused throughout). The cold side of a load is exercised on the delegating target, warm and
//! cold, and on the Oracle's slot.
//! Then each instruction on an empty stack (its operands missing) and on a full one (1,024 items
//! at the instruction), and in a static frame where that check comes first.
//!
//! Two probe shapes, by who switches access off:
//! - [`probe`]: `A` switches access off for itself and the frames below, then calls the probe
//!   account `P` with `G`; `P`'s code is the instruction and its operands.
//! - [`own_probe`], for an instruction whose frame is the beneficiary's own (SELFBALANCE, a
//!   SELFDESTRUCT of the beneficiary): a frame whose access is off cannot call the beneficiary, so
//!   `A` calls it with `G` first and the beneficiary's code switches access off itself
//!   ([`OWN_PREFIX`] gas), then runs the instruction.
//!
//! `A` records the probe's success word and return-data size (a refusal returns 36 bytes, a halt
//! none).
//!
//! The rest: creations that fail after their start (a collision), by an opcode or as the
//! transaction's own frame, under detention, whose burned forward is not compute; a call of an
//! account delegating to the beneficiary across the cold and guard windows, reached by spinning the
//! frame's gas down; and volatile reads that push, on a full stack.
#![allow(dead_code)]
use super::{
    A, Account, Asm, B, BELOW_CAP, C, D, KEYLESS, Limits, ORACLE, Scenario, acct, funded,
    init_returning, op,
    q8::{ACCESS_CONTROL, X1, keyless_call, mem_call, sel},
    scenario,
};
use alloy_primitives::{Address, keccak256};

/// The block beneficiary of the refusal scenarios.
pub(crate) const BEN: Address = B;
/// The probe account, when the probe's frame is not the beneficiary's.
pub(crate) const P: Address = D;
/// Where a probe's SELFDESTRUCT sends the balance.
pub(crate) const HEIR: Address = C;

const EXTCODECOPY: u8 = 0x3c;
const EXTCODEHASH: u8 = 0x3f;
const PREVRANDAO: u8 = 0x44;
const GASLIMIT: u8 = 0x45;
const BASEFEE: u8 = 0x48;
const BLOBBASEFEE: u8 = 0x4a;

/// Satin's gas figures the windows are placed on.
pub(crate) const COLD_ACCOUNT: u64 = 2_900;
pub(crate) const COLD_SLOT: u64 = 2_000;
const SELFDESTRUCT_COLD: u64 = 3_000;
const WARM_READ: u64 = 100;
const TRANSFER_VALUE: u64 = 11_300;
/// EXTCODECOPY of 1,024 bytes: 32 words of copy, and memory from 0 to 32 words.
const COPY_LEN: u64 = 1_024;
const COPY_COST: u64 = 3 * 32;
const MEMORY_32_WORDS: u64 = 3 * 32 + 32 * 32 / 512;
/// What the beneficiary's own probe spends before its instruction: the `disableVolatileDataAccess`
/// call and the POP of its success word. MSTORE of the selector (PUSH32, PUSH0, MSTORE and one
/// word of memory: 3 + 2 + 3 + 3), the call's operands (2 + 2 + 3 + 2 + 2 + 3 + 3), CALL's static
/// gas and its cold target (100 + 2,900), POP (2). The interceptor answers on untouched gas.
pub(crate) const OWN_PREFIX: u64 = 11 + 17 + 100 + COLD_ACCOUNT + 2;
/// A gas limit no probe runs out of.
pub(crate) const PLENTY: u64 = 200_000;
pub(crate) const STACK_LIMIT: usize = 1_024;

fn bd(name: String, accounts: Vec<Account>, gas: u64, limits: Limits) -> Scenario {
    let mut s = scenario(Box::leak(name.into_boxed_str()), accounts, gas, limits);
    s.group = "bd";
    s
}

pub(crate) fn delegation(target: Address) -> Vec<u8> {
    let mut c = vec![0xef, 0x01, 0x00];
    c.extend_from_slice(target.as_slice());
    c
}

/// `A` switches its access off (`disableVolatileDataAccess()`, success word at slot 1).
pub(crate) fn access_off(a: Asm) -> Asm {
    mem_call(a, op::CALL, ACCESS_CONTROL, 0, 100_000, &sel("disableVolatileDataAccess()"))
        .record_call(1)
}

/// `n` PUSH0s.
pub(crate) fn fill(mut a: Asm, n: usize) -> Asm {
    for _ in 0..n {
        a = a.op(op::PUSH0);
    }
    a
}

/// Where a probe's frame is started from.
#[derive(Clone, Copy)]
enum Frame {
    Plain,
    Static,
}

/// A probe scenario: `A` loads `warm` (BALANCE, before access is off), switches access off, and
/// calls `P` (whose code is `code`) with `gas`; the probe's success word at slot 3.
fn probe(name: String, code: Vec<u8>, gas: u64, warm: &[Address], frame: Frame) -> Scenario {
    let mut a = Asm::new();
    for w in warm {
        a = a.push_addr(*w).op(op::BALANCE).op(op::POP);
    }
    let kind = match frame {
        Frame::Plain => op::CALL,
        Frame::Static => op::STATICCALL,
    };
    let a = access_off(a).call_with(kind, P, 0, gas, 0).record_call(3).stop().build();
    let mut s = bd(
        name,
        vec![acct(A, a), acct(P, code), acct(BEN, vec![op::STOP]), acct(X1, delegation(BEN))],
        BELOW_CAP,
        Limits::default(),
    );
    s.beneficiary = BEN;
    s
}

/// The beneficiary's own probe: `A` calls the beneficiary with `gas`; its code switches access
/// off ([`OWN_PREFIX`]) and runs `rest`.
fn own_probe(name: String, rest: Asm, gas: u64) -> Scenario {
    let own = mem_call(
        Asm::new(),
        op::CALL,
        ACCESS_CONTROL,
        0,
        100_000,
        &sel("disableVolatileDataAccess()"),
    )
    .op(op::POP);
    let own = own.ops(&rest.build()).stop().build();
    let a = Asm::new().call_with(op::CALL, BEN, 0, gas, 0).record_call(3).stop().build();
    let mut s = bd(name, vec![acct(A, a), funded(BEN, own, 7)], BELOW_CAP, Limits::default());
    s.beneficiary = BEN;
    s
}

/// The Oracle, reading its slot 5 for the probes of its storage.
pub(crate) fn oracle() -> Account {
    Account {
        address: ORACLE,
        code: Asm::new().push(5).op(op::SLOAD).stop().build(),
        storage: vec![(5, 9)],
        ..Account::default()
    }
}

/// `G` one below, at and one above `at`.
fn window(at: u64) -> [(u64, &'static str); 3] {
    [(at - 1, "m1"), (at, "at"), (at + 1, "p1")]
}

/// An instruction under test: its operands (pushed in order, the last on top), the instruction,
/// and the charges before its load, cumulative from the start of the probe's frame.
pub(crate) struct Probe {
    pub(crate) name: &'static str,
    pub(crate) operands: Vec<Operand>,
    pub(crate) opcode: u8,
    /// Named boundaries: `(name, gas the frame needs to pay everything up to and including it)`.
    pub(crate) boundaries: Vec<(&'static str, u64)>,
    /// The cold side, for a target whose warmth is set: `(warm list for A, extra boundaries)`.
    pub(crate) variants: Vec<Variant>,
}

/// A warmth variant of a probe: its name, the accounts `A` loads first, and the boundaries it adds.
pub(crate) type Variant = (&'static str, Vec<Address>, Vec<(&'static str, u64)>);

#[derive(Clone, Copy)]
pub(crate) enum Operand {
    Word(u64),
    Addr(Address),
}

impl Operand {
    pub(crate) fn push(self, a: Asm) -> Asm {
        match self {
            Self::Word(v) => a.push(v),
            Self::Addr(x) => a.push_addr(x),
        }
    }

    /// The gas of its push: PUSH0 for zero, a PUSHn otherwise.
    pub(crate) fn gas(self) -> u64 {
        match self {
            Self::Word(0) => 2,
            _ => 3,
        }
    }
}

impl Probe {
    pub(crate) fn code(&self, fill_to_full: bool) -> Vec<u8> {
        let mut a = Asm::new();
        if fill_to_full {
            a = fill(a, STACK_LIMIT - self.operands.len());
        }
        for o in &self.operands {
            a = o.push(a);
        }
        a.op(self.opcode).stop().build()
    }

    /// The gas of the operands' pushes.
    pub(crate) fn push_gas(&self) -> u64 {
        self.operands.iter().map(|o| o.gas()).sum()
    }
}

/// The probes of the instructions `A` can reach through a plain frame of its own.
pub(crate) fn probes() -> Vec<Probe> {
    use Operand::{Addr, Word};
    let mut v = Vec::new();
    // Account opcodes: the static gas, then the load. The beneficiary is warm, so the cold-access
    // window is no boundary.
    for (name, opcode, static_gas) in [
        ("balance", op::BALANCE, 100u64),
        ("extcodesize", op::EXTCODESIZE, 200),
        ("extcodehash", EXTCODEHASH, 100),
    ] {
        let at = 3 + static_gas;
        v.push(Probe {
            name,
            operands: vec![Addr(BEN)],
            opcode,
            boundaries: vec![("static", at), ("cold", at + COLD_ACCOUNT)],
            variants: Vec::new(),
        });
    }
    // EXTCODECOPY of 1,024 bytes to memory 0: the copy, then the memory expansion, then the load.
    let static_gas = 3 + 2 + 2 + 3 + 200;
    v.push(Probe {
        name: "extcodecopy",
        operands: vec![Word(COPY_LEN), Word(0), Word(0), Addr(BEN)],
        opcode: EXTCODECOPY,
        boundaries: vec![
            ("static", static_gas),
            ("copy", static_gas + COPY_COST),
            ("memory", static_gas + COPY_COST + MEMORY_32_WORDS),
            ("cold", static_gas + COPY_COST + MEMORY_32_WORDS + COLD_ACCOUNT),
        ],
        variants: Vec::new(),
    });
    // EXTCODECOPY of nothing: no copy and no memory charge, the load right after the static gas.
    v.push(Probe {
        name: "extcodecopy_len0",
        operands: vec![Word(0), Word(0), Word(0), Addr(BEN)],
        opcode: EXTCODECOPY,
        boundaries: vec![("static", 2 + 2 + 2 + 3 + 200)],
        variants: Vec::new(),
    });
    // The call family with the beneficiary as target: a 32-byte output range at 992 (32 words of
    // memory), a value of 1 for CALL and CALLCODE: the memory expansion, the value transfer, then
    // the target's load.
    for (name, opcode, value) in [
        ("call", op::CALL, true),
        ("callcode", op::CALLCODE, true),
        ("delegatecall", op::DELEGATECALL, false),
        ("staticcall", op::STATICCALL, false),
    ] {
        let mut operands = vec![Word(32), Word(992), Word(0), Word(0)];
        if value {
            operands.push(Word(1));
        }
        operands.extend([Addr(BEN), Word(0)]);
        let pushes: u64 = operands.iter().map(|o| o.gas()).sum();
        let at = pushes + 100;
        let mut boundaries = vec![("static", at), ("memory", at + MEMORY_32_WORDS)];
        let mut last = at + MEMORY_32_WORDS;
        if value {
            last += TRANSFER_VALUE;
            boundaries.push(("value", last));
        }
        boundaries.push(("cold", last + COLD_ACCOUNT));
        let name: &'static str = Box::leak(format!("{name}_target").into_boxed_str());
        v.push(Probe { name, operands, opcode, boundaries, variants: Vec::new() });
    }
    // The call family with a target delegating to the beneficiary, no memory and no value: the
    // target's load (a cold one needs a cold access payable), the delegation guard (a warm read
    // on top), then the delegate's load.
    for (name, opcode, value) in [
        ("call", op::CALL, true),
        ("callcode", op::CALLCODE, true),
        ("delegatecall", op::DELEGATECALL, false),
        ("staticcall", op::STATICCALL, false),
    ] {
        let mut operands = vec![Word(0), Word(0), Word(0), Word(0)];
        if value {
            operands.push(Word(0));
        }
        operands.extend([Addr(X1), Word(0)]);
        let pushes: u64 = operands.iter().map(|o| o.gas()).sum();
        let at = pushes + 100;
        let name: &'static str = Box::leak(format!("{name}_delegated").into_boxed_str());
        v.push(Probe {
            name,
            operands,
            opcode,
            boundaries: vec![("static", at)],
            variants: vec![
                ("warm", vec![X1], vec![("guard", at + WARM_READ)]),
                (
                    "cold",
                    vec![],
                    vec![("cold", at + COLD_ACCOUNT), ("guard", at + COLD_ACCOUNT + WARM_READ)],
                ),
            ],
        });
    }
    // SELFDESTRUCT to the beneficiary: the static gas, then the load (the beneficiary is warm, so
    // the cold-access window is no boundary).
    v.push(Probe {
        name: "selfdestruct_to",
        operands: vec![Addr(BEN)],
        opcode: op::SELFDESTRUCT,
        boundaries: vec![("static", 3 + 5_000), ("cold", 3 + 5_000 + SELFDESTRUCT_COLD)],
        variants: Vec::new(),
    });
    // The block environment: the static gas, then the read.
    for (name, opcode) in [
        ("coinbase", op::COINBASE),
        ("timestamp", op::TIMESTAMP),
        ("number", op::NUMBER),
        ("prevrandao", PREVRANDAO),
        ("gaslimit", GASLIMIT),
        ("basefee", BASEFEE),
        ("blobbasefee", BLOBBASEFEE),
        ("slotnum", op::SLOTNUM),
    ] {
        v.push(Probe {
            name,
            operands: vec![],
            opcode,
            boundaries: vec![("static", 2)],
            variants: Vec::new(),
        });
    }
    v.push(Probe {
        name: "blockhash",
        operands: vec![Word(0)],
        opcode: op::BLOCKHASH,
        boundaries: vec![("static", 2 + 20)],
        variants: Vec::new(),
    });
    v
}

/// The windows, stack cases and static frames of every probe.
fn refusals() -> Vec<Scenario> {
    let mut v = Vec::new();
    for p in probes() {
        let code = p.code(false);
        let variants = if p.variants.is_empty() {
            vec![("", Vec::new(), Vec::new())]
        } else {
            p.variants.iter().map(|(n, w, b)| (*n, w.clone(), b.clone())).collect()
        };
        for (variant, warm, extra) in &variants {
            let tag = if variant.is_empty() { String::new() } else { format!("_{variant}") };
            for (boundary, at) in p.boundaries.iter().chain(extra) {
                for (gas, d) in window(*at) {
                    let name = format!("bd_{}{tag}_{boundary}_{d}", p.name);
                    v.push(probe(name, code.clone(), gas, warm, Frame::Plain));
                }
            }
        }
        if !p.operands.is_empty() {
            let name = format!("bd_{}_empty_stack", p.name);
            v.push(probe(name, vec![p.opcode, op::STOP], PLENTY, &[], Frame::Plain));
        }
        let name = format!("bd_{}_full_stack", p.name);
        let gas = PLENTY + 2 * STACK_LIMIT as u64 + p.push_gas();
        v.push(probe(name, p.code(true), gas, &[], Frame::Plain));
    }
    // A static frame checks a value call and SELFDESTRUCT before any load.
    let call = probes().into_iter().find(|p| p.name == "call_target").unwrap();
    v.push(probe(
        "bd_call_target_value_static".into(),
        call.code(false),
        PLENTY,
        &[],
        Frame::Static,
    ));
    let sd = probes().into_iter().find(|p| p.name == "selfdestruct_to").unwrap();
    v.push(probe("bd_selfdestruct_to_static".into(), sd.code(false), PLENTY, &[], Frame::Static));
    // The Oracle's storage (its code: PUSH1 5, SLOAD): SLOAD's static gas, then the read (before
    // the cold check).
    for (boundary, at) in [("static", 3 + 100), ("cold", 3 + 100 + COLD_SLOT)] {
        for (gas, d) in window(at) {
            let name = format!("bd_oracle_sload_{boundary}_{d}");
            let a = access_off(Asm::new()).call_with(op::CALL, ORACLE, 0, gas, 0);
            let a = a.record_call(3).stop().build();
            let mut s = bd(name, vec![acct(A, a), oracle()], BELOW_CAP, Limits::default());
            s.beneficiary = BEN;
            v.push(s);
        }
    }
    // The beneficiary's own frame: SELFBALANCE, and SELFDESTRUCT of the beneficiary.
    for (gas, d) in window(OWN_PREFIX + 5) {
        let rest = Asm::new().op(op::SELFBALANCE);
        v.push(own_probe(format!("bd_selfbalance_static_{d}"), rest, gas));
    }
    let rest = fill(Asm::new(), STACK_LIMIT).op(op::SELFBALANCE);
    v.push(own_probe("bd_selfbalance_full_stack".into(), rest, PLENTY));
    for (gas, d) in window(OWN_PREFIX + 3 + 5_000) {
        let rest = Asm::new().push_addr(HEIR).op(op::SELFDESTRUCT);
        v.push(own_probe(format!("bd_selfdestruct_own_static_{d}"), rest, gas));
    }
    v
}

/// Creations that fail after their start, a volatile read before or after: the collision burns
/// the creation's forward, which is not compute.
fn collisions() -> Vec<Scenario> {
    let det = |cap: u64| Limits { block_env_cap: Some(cap), ..Limits::default() };
    let occupied =
        |nonce: u64| Account { address: A.create(nonce), code: vec![0x00], ..Account::default() };
    let rd = || Asm::new().op(op::TIMESTAMP).op(op::POP);
    let mut v = Vec::new();
    let mut push = |name: &'static str, accounts: Vec<Account>, gas: u64, limits: Limits| {
        v.push(bd(name.into(), accounts, gas, limits));
    };
    // x1: a read, then a colliding CREATE, then cheap work.
    let code = rd().create(&init_returning(1), 0).op(op::POP).push(1).op(op::POP).stop().build();
    push(
        "x1_det_read_then_create_collision",
        vec![acct(A, code.clone()), occupied(0)],
        40_000_000,
        det(400_000),
    );
    // x2: the same without the occupied address (the creation succeeds).
    push("x2_det_read_then_create_ok", vec![acct(A, code)], 40_000_000, det(400_000));
    // x3: the collision without a read.
    let code =
        Asm::new().create(&init_returning(1), 0).op(op::POP).push(1).op(op::POP).stop().build();
    push(
        "x3_create_collision_default_caps",
        vec![acct(A, code), occupied(0)],
        40_000_000,
        Limits::default(),
    );
    // x4: the collision first, then the read, then an endless loop: the stop's limit.
    let pre = Asm::new().create(&init_returning(1), 0).op(op::POP).op(op::TIMESTAMP).op(op::POP);
    let jd = pre.0.len() as u64;
    let code = pre.op(op::JUMPDEST).push(jd).op(op::JUMP).build();
    push(
        "x4_det_create_collision_then_read_loop",
        vec![acct(A, code), occupied(0)],
        40_000_000,
        det(30_000),
    );
    // x5: default caps (20M), a read, a colliding CREATE forwarding more than 20M, an SSTORE.
    let code = rd().create(&init_returning(1), 0).op(op::POP).sstore(0x10, 1).stop().build();
    push(
        "x5_default_caps_read_collision_sstore",
        vec![acct(A, code), occupied(0)],
        100_000_000,
        Limits::default(),
    );
    // w1-w8: an event after a read, then an endless loop (the stop exposes the compute).
    let looped = |pre: Asm| {
        let jd = pre.0.len() as u64;
        pre.op(op::JUMPDEST).push(jd).op(op::JUMP).build()
    };
    let cases: Vec<(&'static str, Vec<u8>, Vec<Account>, u64)> = vec![
        (
            "w1_det_create_out_of_funds_loop",
            looped(rd().create(&init_returning(1), 1).op(op::POP)),
            vec![],
            0,
        ),
        (
            "w2_det_call_out_of_funds_loop",
            looped(rd().call_all(op::CALL, B, 1).op(op::POP)),
            vec![acct(B, vec![0x00])],
            0,
        ),
        (
            "w3_det_create_empty_initcode_loop",
            looped(rd().push(0).push(0).push(0).op(op::CREATE).op(op::POP)),
            vec![],
            0,
        ),
        (
            "w4_det_create_init_reverts_loop",
            looped(rd().create(&Asm::new().revert0().build(), 0).op(op::POP)),
            vec![],
            0,
        ),
        (
            "w5_det_create_init_halts_loop",
            looped(rd().create(&[op::INVALID], 0).op(op::POP)),
            vec![],
            0,
        ),
        (
            "w6_det_create_collision_loop",
            looped(rd().create(&init_returning(1), 0).op(op::POP)),
            vec![occupied(0)],
            0,
        ),
        (
            "w7_det_call_value_new_account_loop",
            looped(rd().call_all(op::CALL, C, 1).op(op::POP)),
            vec![],
            10,
        ),
        (
            "w8_det_call_precompile_oog_loop",
            looped(rd().call_with(op::CALL, Address::with_last_byte(1), 0, 100, 0).op(op::POP)),
            vec![],
            0,
        ),
    ];
    for (name, code, extra, wei) in cases {
        let mut accounts = vec![funded(A, code, wei)];
        accounts.extend(extra);
        push(name, accounts, 5_000_000, det(50_000));
    }
    // A CREATE2 collision after a read, and before a read (the stop's limit).
    let init = init_returning(1);
    let create2 = |a: Asm| {
        let mut a = a;
        for (i, chunk) in init.chunks(32).enumerate() {
            let mut word = [0u8; 32];
            word[..chunk.len()].copy_from_slice(chunk);
            a = a.push_bytes(&word).push(32 * i as u64).op(op::MSTORE);
        }
        a.push(0).push(init.len() as u64).push(0).push(0).op(op::CREATE2)
    };
    let occupied2 = Account {
        address: A.create2(alloy_primitives::B256::ZERO, keccak256(&init)),
        code: vec![0x00],
        ..Account::default()
    };
    let code = create2(rd()).op(op::POP).sstore(0x10, 1).stop().build();
    push(
        "bd_det_read_then_create2_collision",
        vec![acct(A, code), occupied2.clone()],
        40_000_000,
        det(400_000),
    );
    let pre = create2(Asm::new()).op(op::POP).op(op::TIMESTAMP).op(op::POP);
    let jd = pre.0.len() as u64;
    let code = pre.op(op::JUMPDEST).push(jd).op(op::JUMP).build();
    push(
        "bd_det_create2_collision_then_read_loop",
        vec![acct(A, code), occupied2],
        40_000_000,
        det(30_000),
    );
    // A creation transaction whose address already has code: its own frame collides. Plain, with
    // no state growth allowed, and detained from its start (the sender is the beneficiary).
    for (name, limits, sender_is_beneficiary) in [
        ("bd_create_tx_collision", Limits::default(), false),
        (
            "bd_create_tx_collision_no_growth",
            Limits { growth: Some(0), ..Limits::default() },
            false,
        ),
        ("bd_create_tx_collision_detained", det(50_000), true),
    ] {
        let target =
            Account { address: super::SENDER.create(0), code: vec![0x00], ..Account::default() };
        let mut s = bd(name.into(), vec![target], 1_000_000, limits);
        s.create = true;
        s.data = init_returning(1);
        if sender_is_beneficiary {
            s.beneficiary = super::SENDER;
        }
        v.push(s);
    }
    // A keyless deployment whose address already has a nonce: its creation collides. Its signer
    // is the beneficiary, so the call is a volatile read, detained or not.
    for (name, cap) in [
        ("bd_keyless_deploy_address_with_nonce", None),
        ("bd_keyless_deploy_address_with_nonce_detained", Some(60_000)),
    ] {
        let (data, signer, deploy) = keyless_call("ok", 1_000_000);
        let mut s = bd(
            name.into(),
            vec![Account { address: deploy, nonce: 1, ..Account::default() }],
            5_000_000,
            Limits { block_env_cap: cap, ..Limits::default() },
        );
        s.to = KEYLESS;
        s.data = data;
        s.beneficiary = signer;
        v.push(s);
    }
    v
}

/// A call of `X1`, delegating to the beneficiary, from a frame whose access is off, its gas spun
/// down below a chosen figure first: the target warm or cold, around the cold access and the
/// delegation guard.
fn delegated_spins() -> Vec<Scenario> {
    let spin = |a: Asm, below: u64| {
        let start = a.clone().build().len() as u64;
        a.op(op::JUMPDEST).op(op::GAS).push(below).op(op::LT).push(start).op(op::JUMPI)
    };
    let tail = |a: Asm| {
        a.push(0).push(0).push(0).push(0).push(0).push_addr(X1).push(0).op(op::CALL).stop().build()
    };
    let warm_x1 = |a: Asm| a.push_addr(X1).op(op::BALANCE).op(op::POP);
    let warm_b = |a: Asm| a.push_addr(BEN).op(op::BALANCE).op(op::POP);
    let ac = |a: Asm| access_off(a);
    let cases: Vec<(&'static str, Vec<u8>)> = vec![
        ("y1_warm_target_cold_delegate_low_gas", tail(spin(warm_x1(ac(Asm::new())), 2_000))),
        ("y2_warm_target_delegation_unpayable", tail(spin(warm_x1(ac(Asm::new())), 160))),
        ("y3_cold_target_low_gas", tail(spin(ac(Asm::new()), 2_000))),
        (
            "y4_warm_target_warm_delegate_low_gas",
            tail(spin(warm_x1(ac(warm_b(Asm::new()))), 2_000)),
        ),
        ("y5_warm_target_cold_delegate", tail(warm_x1(ac(Asm::new())))),
        ("y6_warm_target_warm_delegate", tail(warm_x1(ac(warm_b(Asm::new()))))),
        ("y7_cold_target_guard_unpayable", tail(spin(ac(Asm::new()), 2_720))),
        ("y8_cold_target_just_payable", tail(spin(ac(Asm::new()), 2_800))),
        ("y9_warm_target_just_payable", tail(spin(warm_x1(ac(Asm::new())), 300))),
        ("z1_cold_target_3000", tail(spin(ac(Asm::new()), 3_000))),
        ("z2_cold_target_3600", tail(spin(ac(Asm::new()), 3_600))),
        ("z3_cold_target_4200", tail(spin(ac(Asm::new()), 4_200))),
        ("z4_cold_target_5400", tail(spin(ac(Asm::new()), 5_400))),
        ("z5_cold_target_8000", tail(spin(ac(Asm::new()), 8_000))),
        ("z6_warm_target_130", tail(spin(warm_x1(ac(Asm::new())), 130))),
        ("z7_warm_target_230", tail(spin(warm_x1(ac(Asm::new())), 230))),
        ("z8_warm_target_260", tail(spin(warm_x1(ac(Asm::new())), 260))),
        ("z9_cold_target_guard_window", tail(spin(ac(Asm::new()), 3_100))),
        ("z10_cold_target_above_guard", tail(spin(ac(Asm::new()), 3_200))),
    ];
    let mut v = Vec::new();
    for (name, a) in cases {
        let bcode = Asm::new().sstore(7, 7).stop().build();
        let mut s = bd(
            name.into(),
            vec![acct(A, a), acct(BEN, bcode), acct(X1, delegation(BEN))],
            BELOW_CAP,
            Limits::default(),
        );
        s.beneficiary = BEN;
        v.push(s);
    }
    v
}

/// Volatile reads that push, on a full stack: a refused read reverts before the push's stack
/// check, an allowed one overflows.
fn full_stack_reads() -> Vec<Scenario> {
    let cases: Vec<(&'static str, bool, u8, bool)> = vec![
        ("u1_full_stack_timestamp_refused", true, op::TIMESTAMP, false),
        ("u2_full_stack_selfbalance_refused", true, op::SELFBALANCE, true),
        ("u3_full_stack_timestamp", false, op::TIMESTAMP, false),
        ("u4_full_stack_coinbase_refused", true, op::COINBASE, false),
        ("u5_full_stack_selfbalance", false, op::SELFBALANCE, true),
    ];
    let mut v = Vec::new();
    for (name, off, opcode, self_beneficiary) in cases {
        let a = if off { access_off(Asm::new()) } else { Asm::new() };
        let a = fill(a, STACK_LIMIT).op(opcode).stop().build();
        let mut s = bd(name.into(), vec![acct(A, a)], BELOW_CAP, Limits::default());
        if self_beneficiary {
            s.beneficiary = A;
        }
        v.push(s);
    }
    v
}

pub fn all() -> Vec<Scenario> {
    let mut v = refusals();
    v.extend(collisions());
    v.extend(delegated_spins());
    v.extend(full_stack_reads());
    v
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_are_unique() {
        let all = super::all();
        let mut names: Vec<_> = all.iter().map(|s| s.name).collect();
        names.sort_unstable();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len());
    }
}
