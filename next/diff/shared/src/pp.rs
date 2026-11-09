//! Helpers of the scenario modules that port the other implementation's remaining tests
//! (`port_*`): builders for a scenario of group "port" or "blk", and code fragments the ports
//! share (fresh writes, loops, calls).
#![allow(dead_code)]
use super::{Account, Asm, Limits, Scenario, acct, op, scenario};
use alloy_primitives::Address;

/// The data size of an account or slot write record.
pub const RECORD: u64 = 40;
/// The data size of a transfer log.
pub const TRANSFER_LOG: u64 = 160;
/// The data size every transaction carries before its calldata and records.
pub const BODY: u64 = 310;
/// Fresh-slot state gas at the minimum bucket.
pub const SLOT_STATE: u64 = 97_920;
/// A write record's history gas.
pub const RECORD_HISTORY: u64 = 3_520;

/// A scenario of group "port".
pub fn sc(
    name: &'static str,
    rules: &[&'static str],
    accounts: Vec<Account>,
    gas: u64,
) -> Scenario {
    let mut s = scenario(name, accounts, gas, Limits::default());
    s.group = "port";
    s.rules = rules.to_vec();
    s
}

/// A scenario named at run time.
pub fn scn(name: String, rules: &[&'static str], accounts: Vec<Account>, gas: u64) -> Scenario {
    sc(nm(name), rules, accounts, gas)
}

/// A name that lives for the whole run.
pub fn nm(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

pub fn lim(data: Option<u64>, kv: Option<u64>, growth: Option<u64>) -> Limits {
    Limits { data, kv, growth, ..Limits::default() }
}

/// `SSTORE(first + i, 1)` for `i` in `0..n`, straight-line.
pub fn writes(mut a: Asm, first: u64, n: u64) -> Asm {
    for i in 0..n {
        a = a.sstore(first + i, 1);
    }
    a
}

/// `n` fresh writes of the slots `base + 1..=base + n`, as a loop (the code stays small).
pub fn write_loop(a: Asm, base: u64, n: u64) -> Asm {
    let head = a.push(n);
    let dest = head.0.len() as u64;
    head.op(op::JUMPDEST)
        .op(op::DUP1)
        .op(op::DUP1)
        .push(base)
        .op(op::ADD)
        .op(op::SSTORE)
        .push(1)
        .op(op::SWAP1)
        .op(op::SUB)
        .op(op::DUP1)
        .push(dest)
        .op(op::JUMPI)
        .op(op::POP)
}

/// A loop that never ends.
pub fn endless(a: Asm) -> Asm {
    let d = a.0.len() as u64;
    a.op(op::JUMPDEST).push(d).op(op::JUMP)
}

/// `n` rounds of `PUSH1 1 PUSH1 2 ADD POP` (13 gas of compute per round).
pub fn work(mut a: Asm, n: u64) -> Asm {
    for _ in 0..n {
        a = a.ops(&[op::PUSH1, 1, op::PUSH1, 2, op::ADD, op::POP]);
    }
    a
}

/// `n` rounds of a counted loop (about 26 gas each): the loop of [`super::spin`] without its
/// closing write, so a caller can follow it.
pub fn spin_loop(a: Asm, n: u64) -> Asm {
    let head = a.push(n);
    let dest = head.0.len() as u64;
    head.op(op::JUMPDEST)
        .push(1)
        .op(op::SWAP1)
        .op(op::SUB)
        .op(op::DUP1)
        .push(dest)
        .op(op::JUMPI)
        .op(op::POP)
}

/// A `CALL` of `to` forwarding `gas` with `value`, its answer dropped.
pub fn call(a: Asm, to: Address, value: u64, gas: u64) -> Asm {
    a.call_with(op::CALL, to, value, gas, 0).op(op::POP)
}

/// A `CALL` of `to` forwarding all gas with `value`, its answer dropped.
pub fn call_all(a: Asm, to: Address, value: u64) -> Asm {
    a.call_all(op::CALL, to, value).op(op::POP)
}

/// A `CALL` of `to` forwarding all gas, its answer stored at `slot` (and the size of its return
/// data at `slot + 1`).
pub fn call_rec(a: Asm, to: Address, value: u64, slot: u64) -> Asm {
    a.call_all(op::CALL, to, value).record_call(slot)
}

/// A contract that calls `to` with all gas and stores the answer at slot 1 and 2.
pub fn caller_of(to: Address) -> Vec<u8> {
    call_rec(Asm::new(), to, 0, 1).stop().build()
}

/// `PUSH20 who SELFDESTRUCT`.
pub fn destruct(a: Asm, who: Address) -> Asm {
    a.push_addr(who).op(op::SELFDESTRUCT)
}

/// `LOG0` of `n` bytes of memory from 0.
pub fn log0(a: Asm, n: u64) -> Asm {
    a.push(n).push(0).op(op::LOG0)
}

/// A contract account.
pub fn code(address: Address, a: Asm) -> Account {
    acct(address, a.build())
}

/// Init code that stores `slots` fresh slots and returns `n` bytes of zeros.
pub fn init_with(slots: u64, n: u64) -> Vec<u8> {
    let a = write_loop(Asm::new(), 0, slots);
    a.push(n).push(0).op(op::RETURN).build()
}

/// Init code returning `n` bytes whose first byte is `first`.
pub fn init_first(first: u8, n: u64) -> Vec<u8> {
    Asm::new().push(first as u64).push(0).op(0x53).push(n).push(0).op(op::RETURN).build()
}

/// A contract that runs `main` (which must end in a stop) when called with no data and `other`
/// when called with data.
pub fn branch_on_data(main: Asm, other: Asm) -> Vec<u8> {
    let d = 4 + main.0.len() as u64;
    let mut a = Asm::new().op(op::CALLDATASIZE).push(d).op(op::JUMPI);
    a.0.extend_from_slice(&main.0);
    a.op(op::JUMPDEST).ops(&other.0).build()
}

/// Init code that runs `prefix`, then deploys `runtime`.
pub fn init_deploying(prefix: Asm, runtime: &[u8]) -> Vec<u8> {
    let off = prefix.0.len() as u64 + 10;
    let n = runtime.len() as u64;
    let mut c =
        prefix.push(n).push(off).push(0).op(op::CODECOPY).push(n).push(0).op(op::RETURN).build();
    // PUSH1 n PUSH1 off PUSH0 CODECOPY PUSH1 n PUSH0 RETURN is 10 bytes while n and off fit a byte
    assert!(n < 256 && off < 256, "a runtime and a prefix that fit PUSH1");
    c.extend_from_slice(runtime);
    c
}

/// The address a `CREATE2` of `init` by `from` with salt 0 makes.
pub fn create2_addr(from: Address, init: &[u8]) -> Address {
    from.create2(alloy_primitives::B256::ZERO, alloy_primitives::keccak256(init))
}

/// `CREATE2(value, init, salt 0)`, the address left on the stack.
pub fn create2_of(a: Asm, init: &[u8], value: u64) -> Asm {
    let mut a = a;
    for (i, chunk) in init.chunks(32).enumerate() {
        let mut word = [0u8; 32];
        word[..chunk.len()].copy_from_slice(chunk);
        a = a.push_bytes(&word).push(32 * i as u64).op(op::MSTORE);
    }
    a.push(0).push(init.len() as u64).push(0).push(value).op(op::CREATE2)
}

/// A value `CALL` of `to` (an address on the stack from `who`) forwarding all gas.
pub fn call_caller_value(a: Asm, value: u64) -> Asm {
    // retsize retoff insize inoff value CALLER GAS CALL
    a.push(0).push(0).push(0).push(0).push(value).op(op::CALLER).op(op::GAS).op(op::CALL)
}
