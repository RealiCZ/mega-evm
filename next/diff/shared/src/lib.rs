//! The synthetic scenarios of the Satin differential gates, engine-neutral: the reference
//! (`next/reference`, on revm) and the Satin engine on evm2 (`next/diff`) run the same inputs and
//! render the same outcome record ([`record`]). Only `alloy_primitives` and `std` are used.
//!
//! `all()` is the base scenario list (group "p5"); the modules add the regression sets (`p6`,
//! `p7`), the interaction scenarios (`q2`), the fuzz generator (`fz`) and the scenarios of the
//! later mechanisms (`q8`: EIP-7702, keyless deployment, access control, Oracle hints, the
//! precompile catalogue), the gas limits above the EIP-8037 total cap (`cap`) and the edge inputs
//! of single rules (`edge`: the live system address's envelopes, a destroyed leaf a transfer
//! created, a transaction-level detained precompile, SALT capacities below the minimum bucket,
//! limits at or above 2^63, deposits whose first frame cannot start) and the refusal boundaries
//! (`bd`). Group names are part of the reference outputs. [`probe`] reads a single input (a
//! scenario by name with overrides, or a whole scenario) from a line of JSON.

#![allow(dead_code)]

use alloy_primitives::{Address, U256};

pub const SENDER: Address = Address::new([0x50; 20]);
pub const A: Address = addr(0xa0);
pub const B: Address = addr(0xb0);
pub const C: Address = addr(0xc0);
pub const D: Address = addr(0xd0);
/// No code, no balance: empty under EIP-161.
pub const EMPTY: Address = addr(0xe0);
/// The identity precompile.
pub const IDENTITY: Address = Address::with_last_byte(0x04);

pub const BELOW_CAP: u64 = 1_000_000;
/// Satin's minimum SALT bucket size, the unit of [`Scenario::salt`]; both drivers check that
/// their engine's constant equals it.
pub const MIN_BUCKET: u64 = 256;

/// The capacity in entries of `m` minimum buckets ([`Scenario::salt`]), or `None` when it is not
/// a `u64`.
pub const fn salt_capacity(m: u64) -> Option<u64> {
    MIN_BUCKET.checked_mul(m)
}
/// Satin's `KeylessDeploy` (`keylessDeploy(bytes,uint256)`).
pub const KEYLESS: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03]);
/// A signed pre-EIP-155 creation, keyed by 0x42..42: gas price 100 gwei, gas limit 200,000, init
/// code storing ORIGIN@1, GASPRICE@2, CALLER@3 and returning one byte.
pub const KEYLESS_SIGNED_TX: &str = "f8628085174876e80083030d40808091326001553a6002553360035560016000f31ca02327bc65dfaefd3c65ae4f03ed8045b517d7a99e2b80026e1703dfc1131baf39a00750cc742e3f22b826b0f71e060d1181bf0e0d29d292f6f93e91803001cdb5d8";
pub const KEYLESS_SIGNER: Address = Address::new([
    0x17, 0xc5, 0x18, 0x51, 0x67, 0x40, 0x1e, 0xd0, 0x0c, 0xf5, 0xf5, 0xb2, 0xfc, 0x97, 0xd9, 0xbb,
    0xfd, 0xb7, 0xd0, 0x25,
]);

pub fn keyless_deployed() -> Address {
    KEYLESS_SIGNER.create(0)
}

/// ABI-encoded `keylessDeploy(bytes signedTx, uint256 gasLimitOverride)`.
pub fn keyless_calldata(gas_limit_override: u64) -> Vec<u8> {
    let tx = alloy_primitives::hex::decode(KEYLESS_SIGNED_TX).unwrap();
    let mut c = alloy_primitives::keccak256("keylessDeploy(bytes,uint256)")[..4].to_vec();
    let word = |v: u64| U256::from(v).to_be_bytes::<32>();
    c.extend_from_slice(&word(0x40));
    c.extend_from_slice(&word(gas_limit_override));
    c.extend_from_slice(&word(tx.len() as u64));
    let mut padded = tx.clone();
    padded.resize(tx.len().div_ceil(32) * 32, 0);
    c.extend_from_slice(&padded);
    c
}

/// Satin's `MegaLimitControl` (interceptor: `remainingComputeGas()`).
pub const LIMIT_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05]);
/// 10M above Satin's 200M execution cap: 10M goes to the EIP-8037 reservoir.
pub const ABOVE_CAP: u64 = 210_000_000;
/// The gas limit of the block both drivers run every scenario in (the `cap_*_block` scenarios
/// declare exactly this gas limit).
pub const BLOCK_GAS_LIMIT: u64 = 10_000_000_000;

const fn addr(b: u8) -> Address {
    let mut a = [0u8; 20];
    a[0] = 0x10;
    a[19] = b;
    Address::new(a)
}

/// Opcodes used here.
pub mod op {
    pub const STOP: u8 = 0x00;
    pub const ADD: u8 = 0x01;
    pub const MUL: u8 = 0x02;
    pub const SUB: u8 = 0x03;
    pub const LT: u8 = 0x10;
    pub const ISZERO: u8 = 0x15;
    pub const ADDRESS: u8 = 0x30;
    pub const CALLER: u8 = 0x33;
    pub const RETURNDATASIZE: u8 = 0x3d;
    pub const RETURNDATACOPY: u8 = 0x3e;
    pub const TIMESTAMP: u8 = 0x42;
    pub const NUMBER: u8 = 0x43;
    pub const POP: u8 = 0x50;
    pub const MLOAD: u8 = 0x51;
    pub const MSTORE: u8 = 0x52;
    pub const SLOAD: u8 = 0x54;
    pub const SSTORE: u8 = 0x55;
    pub const JUMP: u8 = 0x56;
    pub const JUMPI: u8 = 0x57;
    pub const GAS: u8 = 0x5a;
    pub const JUMPDEST: u8 = 0x5b;
    pub const PUSH0: u8 = 0x5f;
    pub const PUSH1: u8 = 0x60;
    pub const DUP1: u8 = 0x80;
    pub const SWAP1: u8 = 0x90;
    pub const CREATE: u8 = 0xf0;
    pub const CALL: u8 = 0xf1;
    pub const CALLCODE: u8 = 0xf2;
    pub const RETURN: u8 = 0xf3;
    pub const DELEGATECALL: u8 = 0xf4;
    pub const STATICCALL: u8 = 0xfa;
    pub const REVERT: u8 = 0xfd;
    pub const CREATE2: u8 = 0xf5;
    pub const SELFDESTRUCT: u8 = 0xff;
    pub const LOG0: u8 = 0xa0;
    pub const LOG1: u8 = 0xa1;
    pub const LOG3: u8 = 0xa3;
    pub const CALLVALUE: u8 = 0x34;
    pub const BALANCE: u8 = 0x31;
    pub const EXTCODESIZE: u8 = 0x3b;
    pub const COINBASE: u8 = 0x41;
    pub const BLOCKHASH: u8 = 0x40;
    pub const SELFBALANCE: u8 = 0x47;
    pub const CODECOPY: u8 = 0x39;
    pub const CALLDATASIZE: u8 = 0x36;
    pub const CALLDATACOPY: u8 = 0x37;
    pub const INVALID: u8 = 0xfe;
    pub const DUPN: u8 = 0xe6;
    pub const SWAPN: u8 = 0xe7;
    pub const EXCHANGE: u8 = 0xe8;
    pub const SLOTNUM: u8 = 0x4b;
}

/// A tiny assembler.
#[derive(Clone, Default)]
pub struct Asm(pub Vec<u8>);

impl Asm {
    pub fn new() -> Self {
        Self(Vec::new())
    }
    pub fn op(mut self, o: u8) -> Self {
        self.0.push(o);
        self
    }
    pub fn ops(mut self, o: &[u8]) -> Self {
        self.0.extend_from_slice(o);
        self
    }
    pub fn push(mut self, v: u64) -> Self {
        if v == 0 {
            self.0.push(op::PUSH0);
            return self;
        }
        let bytes = v.to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        let n = 8 - skip;
        self.0.push(op::PUSH1 + n as u8 - 1);
        self.0.extend_from_slice(&bytes[skip..]);
        self
    }
    pub fn push_addr(mut self, a: Address) -> Self {
        self.0.push(op::PUSH1 + 19);
        self.0.extend_from_slice(a.as_slice());
        self
    }
    pub fn push_bytes(mut self, b: &[u8]) -> Self {
        assert!(!b.is_empty() && b.len() <= 32);
        self.0.push(op::PUSH1 + b.len() as u8 - 1);
        self.0.extend_from_slice(b);
        self
    }
    /// SSTORE(key, value).
    pub fn sstore(self, key: u64, value: u64) -> Self {
        self.push(value).push(key).op(op::SSTORE)
    }
    /// `CALL`-family opcode to `to` with `value`, all gas (`GAS`), no input, no output buffer.
    /// Leaves the success word on the stack.
    pub fn call_all(self, kind: u8, to: Address, value: u64) -> Self {
        let s = self.push(0).push(0).push(0).push(0);
        let s = if kind == op::CALL || kind == op::CALLCODE { s.push(value) } else { s };
        s.push_addr(to).op(op::GAS).op(kind)
    }
    /// Like [`call_all`] with `gas` instead of `GAS` and `in_len` bytes of memory at 0 as input.
    pub fn call_with(self, kind: u8, to: Address, value: u64, gas: u64, in_len: u64) -> Self {
        let s = self.push(0).push(0).push(in_len).push(0);
        let s = if kind == op::CALL || kind == op::CALLCODE { s.push(value) } else { s };
        s.push_addr(to).push(gas).op(kind)
    }
    /// Stores the success word on the stack at `slot`, then RETURNDATASIZE at `slot + 1`.
    pub fn record_call(self, slot: u64) -> Self {
        self.push(slot).op(op::SSTORE).op(op::RETURNDATASIZE).push(slot + 1).op(op::SSTORE)
    }
    /// `CREATE` of `init` (copied into memory at 0 with MSTOREs), value `value`; leaves the
    /// address (or 0) on the stack.
    pub fn create(mut self, init: &[u8], value: u64) -> Self {
        for (i, chunk) in init.chunks(32).enumerate() {
            let mut word = [0u8; 32];
            word[..chunk.len()].copy_from_slice(chunk);
            self.0.push(op::PUSH1 + 31);
            self.0.extend_from_slice(&word);
            self = self.push(32 * i as u64).op(op::MSTORE);
        }
        self.push(init.len() as u64).push(0).push(value).op(op::CREATE)
    }
    /// MSTORE(0, selector || zeros).
    pub fn mstore_selector(self, sel: [u8; 4]) -> Self {
        let mut w = [0u8; 32];
        w[..4].copy_from_slice(&sel);
        self.push_bytes(&w).push(0).op(op::MSTORE)
    }
    /// SSTORE(slot, first word of the return data) (0 when there is none).
    pub fn store_returndata_word(self, slot: u64) -> Self {
        self.push(0)
            .push(0)
            .op(op::MSTORE)
            .op(op::RETURNDATASIZE)
            .push(0)
            .push(0)
            .op(op::RETURNDATACOPY)
            .push(0)
            .op(op::MLOAD)
            .push(slot)
            .op(op::SSTORE)
    }
    pub fn stop(self) -> Self {
        self.op(op::STOP)
    }
    pub fn revert0(self) -> Self {
        self.push(0).push(0).op(op::REVERT)
    }
    pub fn build(self) -> Vec<u8> {
        self.0
    }
}

/// Init code returning `n` bytes of code (zeros, the first byte 0x00, i.e. STOP).
pub fn init_returning(n: u64) -> Vec<u8> {
    // PUSHn n PUSH0 RETURN : returns memory[0..n] (zeros).
    Asm::new().push(n).push(0).op(op::RETURN).build()
}

#[derive(Clone, Debug, Default)]
pub struct Account {
    pub address: Address,
    pub code: Vec<u8>,
    pub balance: U256,
    pub nonce: u64,
    /// Initial storage `(key, value)`.
    pub storage: Vec<(u64, u64)>,
}

/// Per-transaction limits; `None` keeps the Satin default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Limits {
    pub data: Option<u64>,
    pub kv: Option<u64>,
    pub growth: Option<u64>,
    /// Detention cap for the block environment (compute after a read).
    pub block_env_cap: Option<u64>,
    /// Detention cap for the Oracle's storage.
    pub oracle_cap: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Scenario {
    pub name: &'static str,
    pub accounts: Vec<Account>,
    pub to: Address,
    pub value: U256,
    pub data: Vec<u8>,
    pub gas_limit: u64,
    pub limits: Limits,
    /// "p5", "p6", "p7" (regression), "q2" (interaction), "fz" (fuzz), "q8" (later mechanisms),
    /// "cap" (gas limits above the EIP-8037 total cap), "edge" (edge inputs of single rules) or
    /// "bd" (refusal boundaries).
    pub group: &'static str,
    /// The transaction's sender.
    pub sender: Address,
    /// A creation transaction (`to` ignored).
    pub create: bool,
    /// An OP deposit (type 0x7e) with this mint, instead of a legacy transaction.
    pub deposit: Option<u64>,
    /// SALT bucket capacities, in minimum buckets: `(address, slot or account leaf, m)`.
    pub salt: Vec<(Address, Option<u64>, u64)>,
    /// SALT bucket capacities in entries, set after `salt`: `(address, slot or account leaf,
    /// capacity)`; any value, also one below the minimum bucket or between two multiples of it.
    pub salt_capacity: Vec<(Address, Option<u64>, u64)>,
    /// Non-zero L1 block info `(l1 base fee, base fee scalar)` and enveloped bytes for the L1 fee.
    pub l1_fee: Option<(u64, u64, Vec<u8>)>,
    /// Gas price (base fee is 0).
    pub gas_price: u64,
    /// The oracle service's answers `(slot, value)`.
    pub oracle: Vec<(u64, u64)>,
    /// an EIP-7702 authorization list (non-empty: a type-4 transaction, chain id 1, nonce 0,
    /// max fee = priority fee = `gas_price`), with pre-recovered authorities.
    pub auths: Vec<Auth>,
    /// the block beneficiary (default zero).
    pub beneficiary: Address,
    /// An EIP-2930 (1) or EIP-1559 (2) transaction instead of a legacy one (0): chain id 1, nonce
    /// 0, `gas_price` as its fee cap and priority fee, no access list. Not with `auths` or
    /// `deposit`.
    pub tx_type: u8,
    /// A legacy transaction's chain id (`None`: one signed without EIP-155).
    pub chain_id: Option<u64>,
}

/// one EIP-7702 authorization, its authority recovered already (`None`: an invalid
/// signature).
#[derive(Clone, Copy, Debug)]
pub struct Auth {
    pub chain_id: u64,
    pub address: Address,
    pub nonce: u64,
    pub authority: Option<Address>,
}

pub fn acct(address: Address, code: Vec<u8>) -> Account {
    Account { address, code, balance: U256::ZERO, ..Account::default() }
}

pub fn funded(address: Address, code: Vec<u8>, wei: u64) -> Account {
    Account { address, code, balance: U256::from(wei), ..Account::default() }
}

pub fn sender() -> Account {
    funded(SENDER, Vec::new(), 1_000_000_000_000_000_000)
}

pub fn scenario(
    name: &'static str,
    accounts: Vec<Account>,
    gas_limit: u64,
    limits: Limits,
) -> Scenario {
    let mut accounts = accounts;
    accounts.insert(0, sender());
    Scenario {
        name,
        accounts,
        to: A,
        value: U256::ZERO,
        data: Vec::new(),
        gas_limit,
        limits,
        group: "p5",
        sender: SENDER,
        create: false,
        deposit: None,
        salt: Vec::new(),
        salt_capacity: Vec::new(),
        l1_fee: None,
        gas_price: 0,
        oracle: Vec::new(),
        auths: Vec::new(),
        beneficiary: Address::ZERO,
        tx_type: 0,
        chain_id: None,
    }
}

pub fn growth(n: u64) -> Limits {
    Limits { growth: Some(n), ..Limits::default() }
}

/// B writes three fresh slots.
pub fn three_writes() -> Vec<u8> {
    Asm::new().sstore(1, 1).sstore(2, 1).sstore(3, 1).stop().build()
}

/// The hand-written scenarios: the base list, then the `p6`, `p7`, `q2`, `q8`, `cap`, `edge` and
/// `bd` groups, each with the chain's accounts ([`add_chain_accounts`]).
pub fn hand_written() -> Vec<Scenario> {
    let mut v = all();
    v.extend(p6::all());
    v.extend(p7::all());
    v.extend(q2::all());
    v.extend(q8::all());
    v.extend(cap::all());
    v.extend(edge::all());
    v.extend(bd::all());
    v.iter_mut().for_each(add_chain_accounts);
    v
}

/// The scenario named `name`: a hand-written one, or a fuzz seed of any generator mode
/// ([`fz::find`]).
pub fn by_name(name: &str) -> Option<Scenario> {
    hand_written().into_iter().find(|s| s.name == name).or_else(|| fz::find(name))
}

/// Gives `s` the accounts a real chain has: the L1Block predeploy and the SequencerRegistry, each
/// with a code byte so that their slots are read below the journal on both engines (an absent
/// account's slots are not read by evm2). An account `s` already holds at either address stays.
pub fn add_chain_accounts(s: &mut Scenario) {
    for a in [L1_BLOCK, SEQUENCER_REGISTRY] {
        if !s.accounts.iter().any(|x| x.address == a) {
            s.accounts.push(Account { address: a, code: vec![0xfe], ..Account::default() });
        }
    }
}

/// Every scenario of a gate run, as the environment selects them: the base list, the `p6`, `p7`
/// and `q2` groups, the fuzz seeds of `MEGA_EVM2_FUZZ_MODE` and `MEGA_EVM2_FUZZ` ([`fz::all`]),
/// then the `q8`, `cap`, `edge` and `bd` groups, each with the chain's accounts. `MEGA_EVM2_ONLY`
/// (a comma-separated list of names) keeps the named ones, `MEGA_EVM2_FUZZ_ONLY=1` the fuzz seeds.
pub fn everything() -> Vec<Scenario> {
    let mut v = all();
    v.extend(p6::all());
    v.extend(p7::all());
    v.extend(q2::all());
    v.extend(fz::all());
    v.extend(q8::all());
    v.extend(cap::all());
    v.extend(edge::all());
    v.extend(bd::all());
    v.iter_mut().for_each(add_chain_accounts);
    if let Ok(only) = std::env::var("MEGA_EVM2_ONLY") {
        v.retain(|s| only.split(',').any(|name| name == s.name));
    }
    if std::env::var("MEGA_EVM2_FUZZ_ONLY").as_deref() == Ok("1") {
        v.retain(|s| s.group == "fz");
    }
    v
}

pub const ORACLE: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01]);
pub const L1_BLOCK: Address =
    Address::new([0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x15]);
pub const SEQUENCER_REGISTRY: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x06]);

pub mod bd;
pub mod cap;
pub mod edge;
pub mod fz;
mod json;
pub mod p6;
pub mod p7;
pub mod probe;
pub mod q2;
pub mod q8;
pub mod record;

/// The base scenarios (group "p5").
pub fn all() -> Vec<Scenario> {
    let mut v = Vec::new();
    // --- controls: plain gas calibration
    v.push(scenario(
        "ctl_sstore",
        vec![acct(A, Asm::new().sstore(1, 1).stop().build())],
        BELOW_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "ctl_sstore_res",
        vec![acct(A, Asm::new().sstore(1, 1).stop().build())],
        ABOVE_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "ctl_call_chain_res",
        vec![
            acct(A, Asm::new().call_all(op::CALL, B, 0).op(op::POP).stop().build()),
            acct(B, Asm::new().call_all(op::CALL, C, 0).op(op::POP).stop().build()),
            acct(C, Asm::new().sstore(1, 1).stop().build()),
        ],
        ABOVE_CAP,
        Limits::default(),
    ));

    // --- revert-class stop at a mutation site and the caller resuming
    let a_calls_b = Asm::new().call_all(op::CALL, B, 0).record_call(0x10).stop().build();
    v.push(scenario(
        "s1_child_stopped_caught",
        vec![acct(A, a_calls_b.clone()), acct(B, three_writes())],
        BELOW_CAP,
        growth(3),
    ));
    v.push(scenario(
        "s1_child_stopped_caught_res",
        vec![acct(A, a_calls_b.clone()), acct(B, three_writes())],
        ABOVE_CAP,
        growth(3),
    ));
    v.push(scenario(
        "s1_top_stopped",
        vec![acct(
            A,
            Asm::new().sstore(1, 1).sstore(2, 1).sstore(3, 1).sstore(4, 1).stop().build(),
        )],
        BELOW_CAP,
        growth(3),
    ));
    v.push(scenario(
        "s1_top_stopped_res",
        vec![acct(
            A,
            Asm::new().sstore(1, 1).sstore(2, 1).sstore(3, 1).sstore(4, 1).stop().build(),
        )],
        ABOVE_CAP,
        growth(3),
    ));
    // B is stopped; A resumes and then crosses its own budget with fresh writes (no latch).
    let a_resumes_and_crosses = Asm::new()
        .call_all(op::CALL, B, 0)
        .record_call(0x10)
        .sstore(0x20, 1)
        .sstore(0x21, 1)
        .sstore(0x22, 1)
        .stop()
        .build();
    v.push(scenario(
        "s1_caller_crosses_after_resume",
        vec![acct(A, a_resumes_and_crosses), acct(B, three_writes())],
        BELOW_CAP,
        growth(3),
    ));
    // Two levels: A -> B -> C; C is stopped; B catches and writes; A catches.
    let b_calls_c = Asm::new().call_all(op::CALL, C, 0).record_call(0x10).stop().build();
    v.push(scenario(
        "s1_grandchild_stopped",
        vec![acct(A, a_calls_b.clone()), acct(B, b_calls_c), acct(C, three_writes())],
        ABOVE_CAP,
        growth(4),
    ));

    // --- frame start stop: a value call to an empty account and to a precompile
    let a_value_calls =
        |to: Address| Asm::new().call_all(op::CALL, to, 1).record_call(0x10).stop().build();
    v.push(scenario(
        "s2_value_to_empty_stopped",
        vec![funded(A, a_value_calls(EMPTY), 10)],
        BELOW_CAP,
        growth(1),
    ));
    v.push(scenario(
        "s2_value_to_empty_stopped_res",
        vec![funded(A, a_value_calls(EMPTY), 10)],
        ABOVE_CAP,
        growth(1),
    ));
    v.push(scenario(
        "s2_value_to_empty_ok",
        vec![funded(A, a_value_calls(EMPTY), 10)],
        BELOW_CAP,
        growth(3),
    ));
    v.push(scenario(
        "s2_value_to_empty_ok_res",
        vec![funded(A, a_value_calls(EMPTY), 10)],
        ABOVE_CAP,
        growth(3),
    ));
    v.push(scenario(
        "s2_value_to_precompile_stopped",
        vec![funded(A, a_value_calls(IDENTITY), 10)],
        BELOW_CAP,
        growth(1),
    ));
    v.push(scenario(
        "s2_value_to_precompile_ok",
        vec![funded(A, a_value_calls(IDENTITY), 10)],
        BELOW_CAP,
        growth(3),
    ));
    // A value call to a contract with code whose start crosses: stopped before its first
    // instruction.
    v.push(scenario(
        "s2_value_to_new_code_stopped",
        vec![funded(A, a_value_calls(B), 10), acct(B, Asm::new().sstore(1, 1).stop().build())],
        BELOW_CAP,
        Limits { kv: Some(1), ..Limits::default() },
    ));

    // --- start ledger: reservoir share, budgets, start charge given back
    v.push(scenario(
        "s3_value_call_reverting_child_res",
        vec![funded(A, a_value_calls(B), 10), acct(B, Asm::new().sstore(1, 1).revert0().build())],
        ABOVE_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "s3_value_call_ok_child_res",
        vec![funded(A, a_value_calls(B), 10), acct(B, Asm::new().sstore(1, 1).stop().build())],
        ABOVE_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "s3_nested_value_calls_res",
        vec![
            funded(A, a_value_calls(B), 10),
            funded(
                B,
                Asm::new().sstore(1, 1).call_all(op::CALL, C, 1).record_call(0x10).stop().build(),
                10,
            ),
            acct(C, Asm::new().sstore(1, 1).sstore(2, 2).revert0().build()),
        ],
        ABOVE_CAP,
        Limits { growth: Some(50), kv: Some(100), ..Limits::default() },
    ));
    v.push(scenario(
        "s3_create_res",
        vec![acct(
            A,
            Asm::new().create(&init_returning(10), 0).push(0x10).op(op::SSTORE).stop().build(),
        )],
        ABOVE_CAP,
        Limits::default(),
    ));

    // --- code-deposit admission: data-size budget of the creation
    // body = 310 bytes; tx data limit = 310 + 1000 -> A's budget 1000, creation's 980.
    for (name, n) in [
        ("s6_deposit_fits", 800u64),
        ("s6_creator_crosses_on_resume", 930),
        ("s6_deposit_refused", 950),
    ] {
        v.push(scenario(
            name,
            vec![acct(
                A,
                Asm::new()
                    .create(&init_returning(n), 0)
                    .push(0x10)
                    .op(op::SSTORE)
                    .op(op::RETURNDATASIZE)
                    .push(0x11)
                    .op(op::SSTORE)
                    .stop()
                    .build(),
            )],
            ABOVE_CAP,
            Limits { data: Some(310 + 1000), ..Limits::default() },
        ));
    }
    // --- detention (B9, E1). cap 30,000 after a block-environment read (TIMESTAMP).
    let det = |cap: u64| Limits { block_env_cap: Some(cap), ..Limits::default() };
    // An endless loop after the read: JUMPDEST PUSH1 <2> JUMP.
    let read_then_loop =
        Asm::new().op(op::TIMESTAMP).op(op::POP).op(op::JUMPDEST).push(2).op(op::JUMP).build();
    v.push(scenario(
        "d1_top_frame_crosses",
        vec![acct(A, read_then_loop.clone())],
        BELOW_CAP,
        det(30_000),
    ));
    v.push(scenario(
        "d1_top_frame_crosses_res",
        vec![acct(A, read_then_loop.clone())],
        ABOVE_CAP,
        det(30_000),
    ));
    v.push(scenario(
        "d2_child_crosses_caller_resumes",
        vec![acct(A, a_calls_b.clone()), acct(B, read_then_loop.clone())],
        BELOW_CAP,
        det(30_000),
    ));
    // GAS reads both parts after a read; the SSTORE's charges fit the allowance.
    v.push(scenario(
        "d3_gas_reads_full_gas",
        vec![acct(
            A,
            Asm::new()
                .op(op::TIMESTAMP)
                .op(op::POP)
                .op(op::GAS)
                .push(0x10)
                .op(op::SSTORE)
                .stop()
                .build(),
        )],
        BELOW_CAP,
        det(30_000),
    ));
    // The forward draws the withheld part first: B gets 98/100 of A's whole gas and reads it.
    v.push(scenario(
        "d4_forward_draws_withheld",
        vec![
            acct(
                A,
                Asm::new()
                    .op(op::TIMESTAMP)
                    .op(op::POP)
                    .call_all(op::CALL, B, 0)
                    .record_call(0x10)
                    .stop()
                    .build(),
            ),
            acct(B, Asm::new().op(op::GAS).push(1).op(op::SSTORE).stop().build()),
        ],
        BELOW_CAP,
        det(30_000),
    ));
    // A halting child burns its forward, which is not compute; then A loops to the crossing.
    let a_calls_invalid_then_loops = Asm::new()
        .op(op::TIMESTAMP)
        .op(op::POP)
        .call_with(op::CALL, B, 0, 40_000, 0)
        .op(op::POP)
        .op(op::JUMPDEST)
        .push(0)
        .op(op::JUMP) // patched below
        .build();
    let mut code = a_calls_invalid_then_loops;
    let jd = code.iter().rposition(|b| *b == op::JUMPDEST).unwrap();
    let n = code.len();
    code[n - 2] = 0; // placeholder for PUSH0 -> replaced by PUSH1 jd
    let mut code: Vec<u8> = code[..n - 2].to_vec();
    code.extend_from_slice(&[op::PUSH1, jd as u8, op::JUMP]);
    v.push(scenario(
        "d5_halt_burns_no_compute",
        vec![acct(A, code), acct(B, vec![0xfe])],
        BELOW_CAP,
        det(30_000),
    ));
    // A read in a child, then the caller loops: the caller is held on resume.
    v.push(scenario(
        "d6_read_in_child_caller_loops",
        vec![
            acct(A, {
                let mut c = Asm::new().call_all(op::CALL, B, 0).op(op::POP).build();
                let jd = c.len();
                c.extend_from_slice(&[op::JUMPDEST, op::PUSH1, jd as u8, op::JUMP]);
                c
            }),
            acct(B, Asm::new().op(op::NUMBER).op(op::POP).stop().build()),
        ],
        BELOW_CAP,
        det(30_000),
    ));
    // d7: a value CALL to a cold empty account after the read: the transfer charge fits the
    // allowance, the cold-access + new-account charge does not.
    v.push(scenario(
        "d7_second_charge_of_an_opcode_crosses",
        vec![funded(
            A,
            Asm::new()
                .op(op::TIMESTAMP)
                .op(op::POP)
                .call_with(op::CALL, EMPTY, 1, 0, 0)
                .record_call(0x10)
                .stop()
                .build(),
            10,
        )],
        BELOW_CAP,
        det(12_000),
    ));
    // d8: the CALL's own cold-access charge crosses: Satin stops A at the charge, before B.
    v.push(scenario(
        "d8_call_charge_crosses_before_child",
        vec![
            acct(
                A,
                Asm::new()
                    .op(op::TIMESTAMP)
                    .op(op::POP)
                    .call_all(op::CALL, B, 0)
                    .record_call(0x10)
                    .stop()
                    .build(),
            ),
            acct(B, Asm::new().sstore(1, 1).stop().build()),
        ],
        BELOW_CAP,
        det(2_000),
    ));
    // d9: SHA256 on 3200 bytes (price 60 + 12 * 100 = 1260) past the allowance, within the forward:
    // Satin answers the call with the stop without running it; A catches it and stops.
    v.push(scenario(
        "d9_precompile_price_crosses",
        vec![acct(
            A,
            Asm::new()
                .op(op::TIMESTAMP)
                .op(op::POP)
                .call_with(op::CALL, Address::with_last_byte(2), 0, 900_000, 3200)
                .op(op::POP)
                .stop()
                .build(),
        )],
        BELOW_CAP,
        det(1_500),
    ));
    // --- interceptors: MegaLimitControl.remainingComputeGas()
    let sel = remaining_compute_gas_selector();
    let call_limit = |kind: u8, value: u64, s: [u8; 4]| {
        Asm::new()
            .mstore_selector(s)
            .call_with(kind, LIMIT_CONTROL, value, 500_000, 4)
            .record_call(0x10)
            .store_returndata_word(0x20)
            .stop()
            .build()
    };
    v.push(scenario(
        "i1_limit_call",
        vec![acct(A, call_limit(op::CALL, 0, sel))],
        BELOW_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "i2_limit_staticcall",
        vec![acct(A, call_limit(op::STATICCALL, 0, sel))],
        BELOW_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "i3_limit_delegatecall_not_intercepted",
        vec![acct(A, call_limit(op::DELEGATECALL, 0, sel))],
        BELOW_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "i4_limit_callcode_not_intercepted",
        vec![acct(A, call_limit(op::CALLCODE, 0, sel))],
        BELOW_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "i5_unknown_selector_falls_through",
        vec![acct(A, call_limit(op::CALL, 0, [0xde, 0xad, 0xbe, 0xef]))],
        BELOW_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "i6_value_call_refused",
        vec![funded(A, call_limit(op::CALL, 1, sel), 10)],
        BELOW_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "i7_unfunded_value_call",
        vec![acct(A, call_limit(op::CALL, 1, sel))],
        BELOW_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "i8_value_unknown_selector_falls_through",
        vec![funded(A, call_limit(op::CALL, 1, [0xde, 0xad, 0xbe, 0xef]), 10)],
        BELOW_CAP,
        Limits::default(),
    ));
    v.push(scenario(
        "i9_limit_call_detained",
        vec![acct(
            A,
            Asm::new()
                .op(op::TIMESTAMP)
                .op(op::POP)
                .mstore_selector(sel)
                .call_with(op::CALL, LIMIT_CONTROL, 0, 500_000, 4)
                .record_call(0x10)
                .store_returndata_word(0x20)
                .stop()
                .build(),
        )],
        BELOW_CAP,
        Limits { block_env_cap: Some(200_000), ..Limits::default() },
    ));
    // --- first-frame settlement: the transaction's own value transfer
    for (name, code, gas) in [
        ("c1_value_tx_ok", Asm::new().stop().build(), BELOW_CAP),
        ("c1_value_tx_ok_res", Asm::new().stop().build(), ABOVE_CAP),
        ("c2_value_tx_revert", Asm::new().sstore(1, 1).revert0().build(), BELOW_CAP),
        ("c2_value_tx_revert_res", Asm::new().sstore(1, 1).revert0().build(), ABOVE_CAP),
        ("c3_value_tx_halt", Asm::new().sstore(1, 1).op(0xfe).build(), BELOW_CAP),
        ("c3_value_tx_halt_res", Asm::new().sstore(1, 1).op(0xfe).build(), ABOVE_CAP),
    ] {
        let mut c = scenario(name, vec![acct(A, code)], gas, Limits::default());
        c.value = U256::from(5);
        v.push(c);
    }
    let mut c4 = scenario("c4_value_tx_to_empty_res", vec![], ABOVE_CAP, Limits::default());
    c4.to = EMPTY;
    c4.value = U256::from(5);
    v.push(c4);

    // --- keyless deployment as a native creation
    let mut k1 = scenario("k1_keyless_deploy", vec![], BELOW_CAP, Limits::default());
    k1.to = KEYLESS;
    k1.data = keyless_calldata(300_000);
    v.push(k1.clone());
    let mut k2 = k1.clone();
    k2.name = "k2_keyless_with_value_refused";
    k2.value = U256::from(1);
    v.push(k2);
    v
}

/// `remainingComputeGas()`'s selector.
pub fn remaining_compute_gas_selector() -> [u8; 4] {
    alloy_primitives::keccak256("remainingComputeGas()")[..4].try_into().unwrap()
}

#[cfg(test)]
mod tests {
    /// A deposit carries no gas price: the engine's deposit envelope has none, so a scenario that
    /// gave one would run as a different transaction on each side. Likewise a scenario has one
    /// envelope: a deposit, an EIP-7702 list or a typed transaction.
    #[test]
    fn test_no_deposit_carries_a_gas_price() {
        let mut all = super::hand_written();
        for mode in super::fz::MODES.iter().map(|m| m.0) {
            all.extend(super::fz::generate(mode, 1, 300));
        }
        let priced: Vec<_> = all
            .iter()
            .filter(|s| s.deposit.is_some() && s.gas_price != 0)
            .map(|s| s.name)
            .collect();
        assert!(priced.is_empty(), "{priced:?}");
        // One envelope each (`Scenario::tx_type`: not with `auths` or `deposit`).
        let mixed: Vec<_> = all
            .iter()
            .filter(|s| {
                [s.deposit.is_some(), !s.auths.is_empty(), s.tx_type != 0]
                    .iter()
                    .filter(|e| **e)
                    .count() >
                    1
            })
            .map(|s| s.name)
            .collect();
        assert!(mixed.is_empty(), "{mixed:?}");
    }
}
