//! The Satin execution engine on evm2: Satin's frame, stop and start-ledger rules, history and
//! SALT pricing, destruction, and gas detention on a side ledger, composed on the thin evm2 fork's
//! hooks and op-evm2's OP layer, run through `Evm::transact`.
//!
//! Where each mechanism lives:
//! - type family, Satin `Version`, opcode table, SALT pricing (state-gas price hook): this file;
//! - start ledger, pools, stops, history ledger, SALT multipliers: `ledger.rs`;
//! - withheld-gas detention (side ledger), code-deposit and precompile sites: `detention.rs`;
//! - CALL copy, whose skip-cold check sees the withheld gas too: `call_emulation.rs`;
//! - interceptors (`MegaLimitControl`, `MegaAccessControl`, the Oracle's hints): `intercept.rs`;
//! - transaction handlers (body history, first frame, settlement, deposits, L1 fee): `tx.rs`;
//! - EIP-7702 transactions: `eip7702.rs`; keyless deployment: `keyless.rs`;
//! - per-transaction state of volatile-access control, hints and authorities: `satin_ext.rs`;
//! - block execution: `block.rs`.

#![allow(missing_docs)]

pub mod block;
pub mod call_emulation;
pub mod detention;
pub mod eip7702;
pub mod intercept;
pub mod keyless;
pub mod ledger;
#[cfg(feature = "salt-ahash")]
pub mod salt_hash;
pub mod satin_ext;
pub mod tx;

pub use crate::ledger::*;

use alloy_primitives::{Address, Bytes, U256};
use call_emulation::call_with;
use evm2::{
    Evm, EvmConfig, EvmConfigSelector, EvmFeatures, EvmTypes, EvmTypesHost, ExecutionConfig,
    MessageHooks, OpcodeConfig, SpecId, Version,
    interpreter::{
        Gas, GasTracker, InstrStop, InterpreterState, Message, MessageKind, Result, StackMut, Word,
        instructions::{self as ins, create_with_state_charge},
        op,
    },
    precompiles::PrecompileMap,
    utils::WordExt,
    version::{GasId, StateGasSite},
};
use evm2_macros::instruction;
use op_evm2::{OpSpecId, OpTxEnvelope, OpTxResultExt, OpTypes};
use std::sync::OnceLock;

/// Negative controls: `Q2_DISABLE=a,b` switches named fixes off (tests only; `compare` feature,
/// otherwise a constant `false` the compiler folds away).
#[cfg(feature = "compare")]
pub fn off(name: &str) -> bool {
    static OFF: OnceLock<Vec<String>> = OnceLock::new();
    OFF.get_or_init(|| {
        std::env::var("Q2_DISABLE")
            .map(|v| v.split(',').map(str::to_owned).collect())
            .unwrap_or_default()
    })
    .iter()
    .any(|n| n == name)
}

#[cfg(not(feature = "compare"))]
#[inline(always)]
pub fn off(_name: &str) -> bool {
    false
}

/// `Q2_DEBUG` traces, read once (a per-frame `getenv` would distort measurements; `compare`
/// feature only).
#[cfg(feature = "compare")]
pub fn debug() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("Q2_DEBUG").is_ok())
}

#[cfg(not(feature = "compare"))]
#[inline(always)]
pub fn debug() -> bool {
    false
}

pub trait LiteTypes: EvmTypes + EvmTypesHost<EvmExt = Lite, Tx = OpTxEnvelope> {}
impl<T: EvmTypes + EvmTypesHost<EvmExt = Lite, Tx = OpTxEnvelope>> LiteTypes for T {}

// ---------------------------------------------------------------------------------------------
// Type family and configuration

#[derive(Clone, Copy, Debug)]
pub struct MegaTypes;

impl EvmTypesHost for MegaTypes {
    type ConfigSelector = MegaSelector;
    type SpecId = MegaSpec;
    type Tx = OpTxEnvelope;
    type EvmExt = Lite;
    type MessageExt = ();
    type MessageResultExt = ();
    type TxEnvExt = ();
    type TxResultExt = OpTxResultExt;
    type BlockEnvExt = ();
    type Host<'a> = Evm<'a, Self>;
    type MessageHooks = StartHooks;

    /// SALT: `entry × m` for the bucket the charge lands in; code-deposit is state only
    /// (its history is charged at admission); a destroyed account's charges are free.
    fn state_gas_price(
        host: &mut Evm<'_, Self>,
        id: GasId,
        site: StateGasSite,
    ) -> core::result::Result<u64, evm2::ExecutionError> {
        let address = match site {
            StateGasSite::Account(a) | StateGasSite::Storage(a, _) => a,
        };
        if host.ext().is_destroyed(&address) {
            return Ok(0);
        }
        // Empty deposits must not read a SALT bucket.
        if id == GasId::CodeDepositState && core::mem::take(&mut host.ext_mut().q8.empty_deposit) {
            return Ok(0);
        }
        let price = reprice(host, id, site);
        if matches!(id, GasId::CreateState | GasId::CodeDepositState) {
            host.ext_mut().created_prices.push((address, id, price));
        }
        Ok(price)
    }
}

/// The SALT price of a state charge at `site`, as `state_gas_price` makes it for an account the
/// destruction table has not given back: the schedule's entry times the bucket's multiplier, read
/// through the transaction's cache (so the same price every time in a transaction).
pub fn reprice(host: &mut Evm<'_, MegaTypes>, id: GasId, site: StateGasSite) -> u64 {
    let m = if host.ext().is_system || off("salt") { 1 } else { multiplier(host.ext_mut(), site) };
    host.version().gas_params.get(id).saturating_mul(m)
}

impl OpTypes for MegaTypes {
    fn op_spec(_: MegaSpec) -> OpSpecId {
        OpSpecId::KARST
    }
    fn l1_block_info(ext: &mut Lite) -> &mut op_evm2::L1BlockInfo {
        &mut ext.l1
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MegaSpec {
    Satin,
}

impl From<MegaSpec> for SpecId {
    fn from(_: MegaSpec) -> Self {
        Self::OSAKA
    }
}

pub struct MegaConfig<const BASE: u32, const CUSTOM: u32>(());

impl<const BASE: u32, const CUSTOM: u32> EvmConfig<MegaTypes> for MegaConfig<BASE, CUSTOM> {
    const BASE_SPEC_ID: SpecId = SpecId::OSAKA;
    const OPCODE_CONFIG: &'static OpcodeConfig<MegaTypes> = &satin_table();
}

/// Osaka's table + Amsterdam opcodes and prices + Satin's wrappers.
pub const fn satin_table() -> OpcodeConfig<MegaTypes> {
    let mut c = OpcodeConfig::<MegaTypes>::base::<evm2::BaseEvmConfig<{ SpecId::OSAKA as u32 }>>();
    c.set_instruction::<ins::dupn<MegaTypes>>(op::DUPN, 3);
    c.set_instruction::<ins::swapn<MegaTypes>>(op::SWAPN, 3);
    c.set_instruction::<ins::exchange<MegaTypes>>(op::EXCHANGE, 3);
    c.set_instruction::<ins::slotnum<MegaTypes>>(op::SLOTNUM, 2);
    c.set_static_gas(op::EXTCODESIZE, 200);
    c.set_static_gas(op::EXTCODECOPY, 200);
    c.set_instruction::<mega_sstore>(op::SSTORE, c.static_gas(op::SSTORE));
    c.set_instruction::<mega_sload>(op::SLOAD, c.static_gas(op::SLOAD));
    c.set_instruction::<mega_balance>(op::BALANCE, c.static_gas(op::BALANCE));
    c.set_instruction::<mega_extcodesize>(op::EXTCODESIZE, c.static_gas(op::EXTCODESIZE));
    c.set_instruction::<mega_extcodehash>(op::EXTCODEHASH, c.static_gas(op::EXTCODEHASH));
    c.set_instruction::<mega_extcodecopy>(op::EXTCODECOPY, c.static_gas(op::EXTCODECOPY));
    c.set_instruction::<mega_selfbalance>(op::SELFBALANCE, c.static_gas(op::SELFBALANCE));
    c.set_instruction::<mega_call>(op::CALL, c.static_gas(op::CALL));
    c.set_instruction::<mega_callcode>(op::CALLCODE, c.static_gas(op::CALLCODE));
    c.set_instruction::<mega_delegatecall>(op::DELEGATECALL, c.static_gas(op::DELEGATECALL));
    c.set_instruction::<mega_staticcall>(op::STATICCALL, c.static_gas(op::STATICCALL));
    c.set_instruction::<mega_create>(op::CREATE, c.static_gas(op::CREATE));
    c.set_instruction::<mega_create2>(op::CREATE2, c.static_gas(op::CREATE2));
    c.set_instruction::<mega_log<0>>(op::LOG0, c.static_gas(op::LOG0));
    c.set_instruction::<mega_log<1>>(op::LOG1, c.static_gas(op::LOG1));
    c.set_instruction::<mega_log<2>>(op::LOG2, c.static_gas(op::LOG2));
    c.set_instruction::<mega_log<3>>(op::LOG3, c.static_gas(op::LOG3));
    c.set_instruction::<mega_log<4>>(op::LOG4, c.static_gas(op::LOG4));
    c.set_instruction::<mega_selfdestruct>(op::SELFDESTRUCT, c.static_gas(op::SELFDESTRUCT));
    c.set_instruction::<keyless::mega_keyless>(keyless::DISPATCHER, 0);
    detention::wrap_table(c)
}

pub struct MegaSelector(());

impl EvmConfigSelector<MegaTypes> for MegaSelector {
    type Config<const BASE_SPEC_ID: u32, const CUSTOM_SPEC_ID: u32> =
        MegaConfig<BASE_SPEC_ID, CUSTOM_SPEC_ID>;

    fn execution_config(_: MegaSpec) -> ExecutionConfig<MegaTypes> {
        ExecutionConfig::for_config::<MegaConfig<{ SpecId::OSAKA as u32 }, 0>>()
            .with_version(satin_version())
    }
}

/// Satin's `Version`: Osaka features + EIP-8037/2780/7708, Amsterdam gas, Satin's state prices,
/// 98/100 forward, 512 KiB / 1 MiB size limits, 200M execution cap, and no EIP-8037 total gas
/// cap: a transaction's gas limit is bounded by the block's gas limit alone.
pub fn satin_version() -> Version {
    let mut v = Version::new(SpecId::OSAKA);
    v.features.insert(EvmFeatures::EIP8037 | EvmFeatures::EIP2780 | EvmFeatures::EIP7708);
    v.gas_params = Version::new(SpecId::AMSTERDAM).gas_params;
    let p = &mut v.gas_params;
    p.set(GasId::SstoreSetState, (64 * CPSB) as u32);
    p.set(GasId::NewAccountState, (120 * CPSB) as u32);
    p.set(GasId::CreateState, (120 * CPSB) as u32);
    p.set(GasId::CodeDepositState, CPSB as u32);
    p.set(GasId::CallStipendReduction, 50);
    v.tx_gas_limit_cap = EXECUTION_CAP;
    v.tx_total_gas_cap = u64::MAX;
    v.max_code_size = 512 * 1024;
    v.max_initcode_size = 1024 * 1024;
    v.chain_id = 1;
    v
}

/// The Satin precompile set: op-evm2's Karst map, KZG repriced to 100,000, every entry the
/// catalog prices carrying its input-only gas quote.
pub fn precompile_map() -> &'static PrecompileMap<MegaTypes> {
    static MAP: OnceLock<PrecompileMap<MegaTypes>> = OnceLock::new();
    MAP.get_or_init(|| {
        let base = op_evm2::precompiles::op_precompile_map::<MegaTypes>(OpSpecId::KARST);
        let mut out = PrecompileMap::<MegaTypes>::new();
        for a in base.addresses() {
            let p = base.get(&a).unwrap();
            let p = if a == Address::with_last_byte(0x0a) {
                evm2::precompiles::Precompile::new(
                    a,
                    evm2::precompiles::PrecompileId::KzgPointEvaluation,
                    kzg100k,
                )
            } else {
                p
            };
            let p = match a.0[19] {
                1 => p.with_required_gas(|i| {
                    detention::precompile_price(&Address::with_last_byte(1), i).unwrap()
                }),
                2 => p.with_required_gas(|i| {
                    detention::precompile_price(&Address::with_last_byte(2), i).unwrap()
                }),
                3 => p.with_required_gas(|i| {
                    detention::precompile_price(&Address::with_last_byte(3), i).unwrap()
                }),
                4 => p.with_required_gas(|i| {
                    detention::precompile_price(&Address::with_last_byte(4), i).unwrap()
                }),
                6 => p.with_required_gas(|_| 150),
                7 => p.with_required_gas(|_| 6_000),
                10 => p.with_required_gas(|_| 100_000),
                _ => p,
            };
            out.insert(p);
        }
        out
    })
}

fn kzg100k(
    evm: &mut Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    gas: &mut GasTracker,
) -> evm2::precompiles::PrecompileResult {
    let _ = evm;
    gas.spend(100_000)
        .map_err(|_| evm2::PrecompileError::Halt(evm2::precompiles::PrecompileHalt::OutOfGas))?;
    let mut free = GasTracker::new(u64::MAX / 2);
    evm2::precompiles::kzg_point_evaluation::run(&msg.input, &mut free)
}

// ---------------------------------------------------------------------------------------------
// Completed writes: SSTORE, LOG, SELFDESTRUCT

/// SLOAD (copied body): the skip-cold check sees spendable + withheld gas.
#[instruction(EvmTypes = MegaTypes, dynamic_gas)]
pub fn mega_sload(cx: _, [key]: [Word]) -> Result<out> {
    let additional_cold_cost = cx.state.gas_params().get(GasId::ColdStorageAdditionalCost);
    let total = detention::available(cx.gas.tracker(), &cx.state.host().ext().detention);
    let total = if off("sload_copy") { cx.gas.remaining() } else { total };
    let skip_cold_load = cx.state.feature(EvmFeatures::EIP2929) && total < additional_cold_cost;
    let destination = cx.state.message().destination;
    if destination == ORACLE && crate::satin_ext::refused_here(cx.state) {
        return revert_with(
            cx.gas,
            cx.state,
            &crate::satin_ext::disabled_revert(crate::satin_ext::access::ORACLE),
        );
    }
    if destination == ORACLE {
        // Satin `oracle_sload`: no load when the whole gas cannot pay the cold access; otherwise
        // the slot is loaded through the journal, the service asked, the read priced cold
        // whatever its warmth, and committed as a volatile read under the Oracle's cap.
        if skip_cold_load {
            detention::no_debit_halt(cx.gas);
            return Err(InstrStop::OutOfGas);
        }
        let load = evm2::interpreter::Host::sload(cx.state.host(), &destination, key, false)
            .map_err(|error| cx.state.fail(error))?;
        let answer = cx.state.host().ext().cfg.oracle.get(key).copied();
        cx.state.host().ext_mut().oracle_reads += 1;
        cx.gas.spend(additional_cold_cost)?;
        let cap = cx.state.host().ext().detention.oracle_cap;
        detention::commit_read(cx.state.host().ext_mut(), cx.gas.tracker_mut(), cap, 0);
        out.write(answer.unwrap_or(load.value));
    } else {
        let load = match evm2::interpreter::Host::sload(
            cx.state.host(),
            &destination,
            key,
            skip_cold_load,
        ) {
            Ok(load) => load,
            Err(error) => {
                if skip_cold_load {
                    detention::no_debit_halt(cx.gas);
                }
                return Err(cx.state.fail(error));
            }
        };
        if load.is_cold {
            cx.gas.spend(additional_cold_cost)?;
        }
        out.write(load.value);
    }
}

/// An account opcode's load (copied `env.rs::load_account`): the skip-cold check sees
/// spendable + withheld gas; a load of the block beneficiary's account is a volatile read.
fn account_load(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    addr: Address,
    load_code: bool,
) -> Result<evm2::evm::AccountLoad> {
    if addr == state.host().block().beneficiary && crate::satin_ext::refused_here(state) {
        revert_with(
            gas,
            state,
            &crate::satin_ext::disabled_revert(crate::satin_ext::access::BENEFICIARY),
        )?;
    }
    let cold_load_gas = state.gas_params().cold_account_additional_cost();
    let total = detention::available(gas.tracker(), &state.host().ext().detention);
    let total = if off("sload_copy") { gas.remaining() } else { total };
    let account = match evm2::interpreter::Host::load_account(
        state.host(),
        &addr,
        load_code,
        total < cold_load_gas,
    ) {
        Ok(account) => account,
        Err(error) => {
            if total < cold_load_gas {
                detention::no_debit_halt(gas);
            }
            return Err(state.fail(error));
        }
    };
    if account.is_cold {
        gas.spend(cold_load_gas)?;
    }
    Ok(account)
}

/// Commits a read of the beneficiary's account once the opcode completed (`read_volatile`).
fn beneficiary_read(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>, addr: Address) {
    if addr == state.host().block().beneficiary && !off("beneficiary") {
        let cap = state.host().ext().detention.cap;
        detention::commit_read(state.host().ext_mut(), gas.tracker_mut(), cap, 0);
    }
}

#[instruction(EvmTypes = MegaTypes, dynamic_gas)]
pub fn mega_balance(cx: _, [addr]: [Word]) -> Result<out> {
    let a = addr.to_address();
    *out = account_load(cx.gas, cx.state, a, false)?.balance;
    beneficiary_read(cx.gas, cx.state, a);
}

#[instruction(EvmTypes = MegaTypes, dynamic_gas)]
pub fn mega_selfbalance(cx: _) -> Result<out> {
    let a = cx.state.message().destination;
    if a == cx.state.host().block().beneficiary && crate::satin_ext::refused_here(cx.state) {
        return revert_with(
            cx.gas,
            cx.state,
            &crate::satin_ext::disabled_revert(crate::satin_ext::access::BENEFICIARY),
        );
    }
    *out = evm2::interpreter::Host::load_account(cx.state.host(), &a, false, false)
        .map_err(|e| cx.state.fail(e))?
        .balance;
    beneficiary_read(cx.gas, cx.state, a);
}

#[instruction(EvmTypes = MegaTypes, dynamic_gas)]
pub fn mega_extcodesize(cx: _, [addr]: [Word]) -> Result<out> {
    let a = addr.to_address();
    out.write_u64(account_load(cx.gas, cx.state, a, true)?.code.len() as u64);
    beneficiary_read(cx.gas, cx.state, a);
}

#[instruction(EvmTypes = MegaTypes, dynamic_gas)]
pub fn mega_extcodehash(cx: _, [addr]: [Word]) -> Result<out> {
    let a = addr.to_address();
    let account = account_load(cx.gas, cx.state, a, false)?;
    *out = if account.is_empty { Word::ZERO } else { evm2::utils::b256_to_word(account.code_hash) };
    beneficiary_read(cx.gas, cx.state, a);
}

#[instruction(EvmTypes = MegaTypes, dynamic_gas)]
pub fn mega_extcodecopy(cx: _, [addr, memory_offset, code_offset, len]: [Word]) -> Result {
    let a = addr.to_address();
    let len = len.to_usize()?;
    cx.gas.spend(cx.state.gas_params().extcodecopy_cost(len))?;
    let memory_offset = if len != 0 {
        let memory_offset = memory_offset.to_usize()?;
        cx.state.resize_memory(cx.gas, memory_offset, len)?;
        memory_offset
    } else {
        0
    };
    let code = account_load(cx.gas, cx.state, a, true)?.code;
    cx.state.memory().set_data(
        memory_offset,
        code_offset.to_usize_saturated(),
        len,
        code.original_byte_slice(),
    );
    beneficiary_read(cx.gas, cx.state, a);
    Ok(())
}

#[instruction(EvmTypes = MegaTypes, dynamic_gas)]
pub fn mega_sstore(cx: _, [key, value]: [Word]) -> Result {
    detention::sstore_body(cx.gas, cx.state, key, value)?;
    detention::after_sstore(cx.gas, cx.state)
}

/// Satin's completed-write hook for SSTORE: the record and state growth are drawn and held to the
/// limits (a crossing is the stop), then the record's history is charged or refilled.
pub fn sstore_completed(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    key: &Word,
    s: evm2::evm::SStore,
) -> Result {
    let address = state.message().destination;
    if state.host().ext().is_destroyed(&address) {
        return Ok(());
    }
    let (o, p, n) = (s.original_value, s.present_value, s.new_value);
    let rec = (n != o) as i64 - (p != o) as i64;
    let growth = if o.is_zero() && p.is_zero() && !n.is_zero() {
        1
    } else if o.is_zero() && !p.is_zero() && n.is_zero() {
        -1
    } else {
        0
    };
    let ext = state.host().ext_mut();
    if let Some(stop) = ext.draw([rec * WRITE_RECORD_SIZE as i64, rec, growth]) {
        return stop_frame(gas, state, stop);
    }
    ext.set_slot_record((address, *key), (n != o).then(|| o.is_zero()));
    if rec > 0 {
        charge_history(gas, state, WRITE_RECORD_SIZE, false)?;
    } else if rec < 0 {
        refill_history(gas, state, WRITE_RECORD_SIZE);
    }
    Ok(())
}

/// LOG0..LOG4: the log's bytes are data size, held to the limits; then its history is charged,
/// the frame's history allowance first.
#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_log<const N: usize>(cx: _) -> Result {
    let len = stack.peek(1).ok_or(InstrStop::StackUnderflow)?.to_usize()?;
    ins::log::<MegaTypes, N>::execute(cx.pc, stack, cx.state)?;
    let bytes = 32 + 32 * N as u64 + len as u64;
    let (gas, state) = unsafe { evm2::interpreter::private::split_gas_state(cx.state) };
    if let Some(stop) = state.host().ext_mut().draw([bytes as i64, 0, 0]) {
        return stop_frame(gas, state, stop);
    }
    charge_history(gas, state, bytes, true)
}

/// SELFDESTRUCT: the base body (its beneficiary's new-account state spill funded W first), then
/// the completed-write rule: the destruction table of an account created in the transaction,
/// the records of the account it clears and of its beneficiary, its
/// transfer log and state growth, held to the limits, then the records' history.
#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_selfdestruct(cx: _) -> Result {
    // The base body's first check: nothing is loaded in a static frame.
    if cx.state.is_static() {
        return Err(InstrStop::StateChangeDuringStaticCall);
    }
    let target = stack.peek(0).ok_or(InstrStop::StackUnderflow)?.to_address();
    let me = cx.state.message().destination;
    // Either end the block beneficiary in a frame whose volatile access is off: refused.
    let beneficiary = cx.state.host().block().beneficiary;
    if (target == beneficiary || me == beneficiary) && crate::satin_ext::refused_here(cx.state) {
        return revert_with(
            cx.gas,
            cx.state,
            &crate::satin_ext::disabled_revert(crate::satin_ext::access::BENEFICIARY),
        );
    }
    let (gas, state) = unsafe { evm2::interpreter::private::split_gas_state(cx.state) };
    let looked: core::result::Result<_, evm2::DatabaseError> = (|| {
        let mut a = state.host().state_mut().account(&me)?;
        let eligible = a.is_created() && !a.is_destructed();
        let len = if eligible { a.load_code()?.original_byte_slice().len() } else { 0 };
        // Satin gives the leaf's write record back only for an account the transaction
        // found absent at its start (`Accounts::was_absent_at_start`); a pre-funded address
        // created onto keeps its record ("a part that existed at the start ... its write record
        // stands").
        let absent_at_start = a.original().is_none_or(|o| o.is_empty());
        Ok((a.balance(), eligible, len as u64, absent_at_start))
    })();
    let (balance, eligible, code_len, absent_at_start) =
        looked.map_err(|e| state.fail(evm2::HostError::from(e)))?;
    let me_changed = changed(state.host(), me);
    let moves = !balance.is_zero() && target != me;
    // The beneficiary is probed only where the opcode itself loads it: its skip-cold check sees
    // spendable + withheld, so a load the whole gas cannot pay is not made (no read).
    let cold = state.gas_params().selfdestruct_cold_cost();
    let w = state.host().ext().detention.frames.last().map_or(0, |f| f.withheld);
    let w = if off("sload_copy") { 0 } else { w };
    if gas.remaining().saturating_add(w) >= cold {
        let target_warm =
            state.host().state_mut().account(&target).map(|a| a.is_warm()).unwrap_or(false);
        let target_info = state.host().state_mut().account_info_untracked(&target).ok().flatten();
        let topup = target_info.as_ref().is_none_or(|i| i.is_empty()) && !balance.is_zero();
        let regular = state.gas_params().selfdestruct_cost(topup, !target_warm);
        if !target_warm && gas.remaining() < cold {
            // Only the whole gas pays the cold access: Satin loads the beneficiary and the charge
            // then crosses; the stock body would skip the load.
            let before = gas.remaining();
            gas.tracker_mut().record_failed_charge(before, regular);
            return Err(InstrStop::OutOfGas);
        }
        // W-first funding of the beneficiary's new-account state spill, only when the regular
        // charges fit the spendable part (otherwise the base body's regular charge crosses).
        if topup && gas.remaining() >= regular && !off("selfdestruct_lend") {
            let price = <MegaTypes as EvmTypesHost>::state_gas_price(
                state.host(),
                GasId::NewAccountState,
                StateGasSite::Account(target),
            )
            .map_err(|e| state.fail(e))?;
            lend_state(gas, state, price);
        }
    } else {
        detention::no_debit_halt(gas);
    }
    let r = ins::selfdestruct::<MegaTypes>::execute(cx.pc, stack, cx.state);
    if r != Err(InstrStop::SelfDestruct) {
        return r;
    }
    let (gas, state) = unsafe { evm2::interpreter::private::split_gas_state(cx.state) };
    beneficiary_read(gas, state, target);
    beneficiary_read(gas, state, me);
    // The beneficiary as it was, rebuilt from the balance the opcode moved (Satin
    // `CompletedWrite::SelfDestruct::balance`); the body loaded it.
    let (target_changed, target_empty) = {
        let looked: core::result::Result<_, evm2::DatabaseError> = (|| {
            let a = state.host().state_mut().account(&target)?;
            Ok((a.original().cloned().unwrap_or_default(), a.get().cloned().unwrap_or_default()))
        })();
        let (orig, cur) = looked.map_err(|e| state.fail(evm2::HostError::from(e)))?;
        let host = state.host();
        let before_balance = if moves { cur.balance.saturating_sub(balance) } else { cur.balance };
        let before_empty = before_balance.is_zero() &&
            cur.nonce == 0 &&
            cur.code_hash == alloy_primitives::KECCAK256_EMPTY ||
            (before_balance.is_zero() &&
                cur.nonce == 0 &&
                cur.code_hash == alloy_primitives::B256::ZERO);
        let skip = target == host.ext().sender || host.ext().is_destroyed(&target);
        let changed = !skip &&
            (orig.nonce, orig.balance, orig.code_hash) !=
                (cur.nonce, before_balance, cur.code_hash);
        (changed, before_empty)
    };
    // The destruction table: parts the transaction made come back to this frame.
    let mut give_back = [0i64; 3];
    let mut refill_state = 0u64;
    let mut refill_bytes = 0u64;
    if eligible {
        let ext = state.host().ext();
        let slots: Vec<_> = ext
            .slot_records
            .iter()
            .filter(|((a, _), _)| *a == me)
            .map(|((_, k), f)| (*k, *f))
            .collect();
        let price_of = |ext: &Lite, id| {
            ext.created_prices
                .iter()
                .rev()
                .find(|(a, i, _)| *a == me && *i == id)
                .map_or(0, |(_, _, p)| *p)
        };
        let leaf = price_of(ext, GasId::CreateState);
        let code = price_of(ext, GasId::CodeDepositState) * code_len;
        let fresh = slots.iter().filter(|(_, f)| *f).count() as u64;
        // A fresh slot was charged at its 0 -> x write, at the price the transaction's SALT cache
        // gives it again here.
        let slot_gas: u64 = slots
            .iter()
            .filter(|(_, f)| *f)
            .map(|(k, _)| {
                reprice(state.host(), GasId::SstoreSetState, StateGasSite::Storage(me, *k))
            })
            .sum();
        let records = slots.len() as u64 + (absent_at_start || off("sd_existing_record")) as u64;
        give_back = [
            -((records * WRITE_RECORD_SIZE + code_len) as i64),
            -(records as i64),
            -((fresh + (leaf != 0) as u64) as i64),
        ];
        refill_state = leaf + code + slot_gas;
        refill_bytes = records * WRITE_RECORD_SIZE + code_len;
    }
    // The operation's own records, rebuilt from what it moved: the account it clears (unless the
    // table gave it back) and the beneficiary, each by its change since before the opcode — a
    // move that returns an account to its original takes its record back.
    let host = state.host();
    let me_delta = if eligible || off("sd_records") {
        (!eligible && moves && !me_changed) as i64
    } else {
        changed(host, me) as i64 - me_changed as i64
    };
    let target_delta = if target == me {
        0
    } else if off("sd_records") {
        (moves && !target_changed && target != host.ext().sender) as i64
    } else {
        changed(host, target) as i64 - target_changed as i64
    };
    let growth = (moves && target_empty) as i64;
    let ds = (me_delta + target_delta) * WRITE_RECORD_SIZE as i64 +
        if moves { TRANSFER_LOG_SIZE as i64 } else { 0 };
    let delta = [ds + give_back[0], me_delta + target_delta + give_back[1], growth + give_back[2]];
    if let Some(stop) = state.host().ext_mut().draw(delta) {
        return stop_frame(gas, state, stop);
    }
    if eligible {
        gas.refill_reservoir(refill_state);
        refill_history(gas, state, refill_bytes);
        let ext = state.host().ext_mut();
        let keys: Vec<_> = ext.slot_records.keys().filter(|(a, _)| *a == me).copied().collect();
        for key in keys {
            ext.set_slot_record(key, None);
        }
        if !ext.is_destroyed(&me) {
            ext.destroyed.push(me);
        }
    }
    let net = me_delta + target_delta;
    if net > 0 {
        charge_history(gas, state, net as u64 * WRITE_RECORD_SIZE, false)?;
    } else if net < 0 {
        refill_history(gas, state, (-net) as u64 * WRITE_RECORD_SIZE);
    }
    Err(InstrStop::SelfDestruct)
}

// ---------------------------------------------------------------------------------------------
// CALL / CREATE: start records, start charge, reservoir share, allowance, detention

/// The frame-start hook's charge of the start's records, out of what the caller kept. A caller
/// that cannot pay starts no frame: the forward is handed back first and the halt's record
/// re-written to what the frame then has (Satin), so its burn is exact. Never a crossing.
fn start_charge_or_hand_back(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    bytes: u64,
    forward: u64,
) -> Result {
    match charge_history(gas, state, bytes, false) {
        Ok(()) => Ok(()),
        Err(stop) => {
            if !off("hand_back") {
                // A failed debit may leave a wrapped counter: restart from its record.
                let kept = gas.tracker().failed_charge().map_or(gas.remaining(), |(r, _)| r);
                let with_forward = kept.saturating_add(forward);
                gas.tracker_mut().set_remaining(with_forward);
                gas.tracker_mut().record_failed_charge(with_forward, u64::MAX);
            }
            Err(stop)
        }
    }
}

/// Whether a call gets as far as loading its target, as revm's `CALL` family does after
/// the static-value check and the memory expansion of its input and output ranges.
fn reaches_target_load(
    stack: &StackMut<'_>,
    gas: &Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    kind: MessageKind,
) -> bool {
    let has_value = matches!(kind, MessageKind::Call | MessageKind::CallCode);
    let base = if has_value { 3 } else { 2 };
    let Some(words) = (0..4).map(|i| stack.peek(base + i)).collect::<Option<Vec<_>>>() else {
        return false
    };
    if kind == MessageKind::Call && state.is_static() && !stack.peek(2).is_some_and(|v| v.is_zero())
    {
        return false;
    }
    let end = |off: &Word, len: &Word| -> Option<u64> {
        if len.is_zero() {
            return Some(0);
        }
        u64::try_from(*off).ok()?.checked_add(u64::try_from(*len).ok()?)
    };
    let (Some(a), Some(b)) = (end(&words[0], &words[1]), end(&words[2], &words[3])) else {
        return false
    };
    let need = a.max(b).div_ceil(32);
    let have = (state.memory().len() as u64).div_ceil(32);
    let cost = |w: u64| 3 * w + w * w / 512;
    let memory = if need <= have { 0 } else { cost(need) - cost(have) };
    // ... then the value-transfer charge, then the skip-cold check (`call_emulation.rs`'s order):
    // a frame that cannot pay a cold access loads nothing.
    let value = if has_value && !stack.peek(2).is_some_and(|v| v.is_zero()) {
        state.gas_params().get(GasId::TransferValueCost)
    } else {
        0
    };
    let total = detention::available(gas.tracker(), &state.host().ext().detention);
    memory.saturating_add(value) <= gas.remaining() &&
        total.saturating_sub(memory + value) >= state.gas_params().cold_account_additional_cost()
}

fn mega_call_impl(
    stack: StackMut<'_>,
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    kind: MessageKind,
) -> Result {
    // A call of the block beneficiary in a frame whose volatile access is off: the Host
    // refuses the target's load; the frame reverts on the gas it had after the static gas.
    if let Some(target) = stack.peek(1).map(|v| v.to_address()) &&
        crate::satin_ext::refused_here(state)
    {
        let beneficiary = state.host().block().beneficiary;
        // ... or of an account whose EIP-7702 delegate is the beneficiary (the delegate's load).
        let delegate = if target == beneficiary ||
            crate::off("q8_refuse_delegate") ||
            !reaches_target_load(&stack, gas, state, kind)
        {
            None
        } else {
            state
                .host()
                .state_mut()
                .account(&target)
                .ok()
                .and_then(|mut a| a.load_code().ok())
                .and_then(|c| c.eip7702_address())
        };
        if target == beneficiary || delegate == Some(beneficiary) {
            return revert_with(
                gas,
                state,
                &crate::satin_ext::disabled_revert(crate::satin_ext::access::BENEFICIARY),
            );
        }
    }
    let requested = stack.peek(0).map(|v| u64::try_from(v).unwrap_or(u64::MAX)).unwrap_or(0);
    call_with(
        stack,
        gas,
        state,
        kind,
        |gas, state, _, price| {
            if !off("call_lend") {
                lend_state(gas, state, price);
            }
            Ok(())
        },
        |gas, state, msg| {
            detention::repair_forward(gas, state, msg, requested);
            let mut start = StartRecords::default();
            let transfers = matches!(kind, MessageKind::Call | MessageKind::CallCode) &&
                !msg.value.is_zero() &&
                msg.caller != msg.destination;
            if transfers {
                let host = state.host();
                let caller_info =
                    host.state_mut().account_info_untracked(&msg.caller).ok().flatten();
                let can_fund = caller_info.as_ref().is_some_and(|i| i.balance >= msg.value);
                if can_fund {
                    let dest_info =
                        host.state_mut().account_info_untracked(&msg.destination).ok().flatten();
                    let (c, d) = (changed(host, msg.caller), changed(host, msg.destination));
                    start = StartRecords::two(
                        (msg.caller, c),
                        (msg.destination, d),
                        dest_info.is_none_or(|i| i.is_empty()) as i64,
                        true,
                    );
                }
            }
            let sender = state.host().ext().sender;
            let records =
                start.accounts().iter().filter(|(a, c)| !c && *a != sender).count() as u64;
            let prices = state.host().ext().prices_history;
            let start_charge = if prices { records * WRITE_RECORD_SIZE * CPHB } else { 0 };
            if start_charge > 0 {
                let stipend = if !msg.value.is_zero() &&
                    matches!(kind, MessageKind::Call | MessageKind::CallCode)
                {
                    2_300
                } else {
                    0
                };
                start_charge_or_hand_back(
                    gas,
                    state,
                    records * WRITE_RECORD_SIZE,
                    msg.gas_limit - stipend,
                )?;
            }
            let r = gas.reservoir();
            let share = reservoir_share(r);
            msg.reservoir = share;
            gas.set_reservoir(r - share);
            let start = (start.n != 0).then_some(start);
            let stipend = if !msg.value.is_zero() &&
                matches!(kind, MessageKind::Call | MessageKind::CallCode)
            {
                2_300
            } else {
                0
            };
            let allowance = if stipend != 0 && prices && !off("allowance") {
                HISTORY_ALLOWANCE_BYTES * CPHB
            } else {
                0
            };
            state.host().ext_mut().push_child(start_charge, r - share, start, allowance);
            detention::on_suspend(gas, state, stipend);
            let beneficiary = state.host().block().beneficiary;
            if (msg.call_target == beneficiary || msg.code_address == beneficiary) &&
                !off("beneficiary")
            {
                let cap = state.host().ext().detention.cap;
                let forwarded = msg.gas_limit - stipend;
                detention::commit_read(state.host().ext_mut(), gas.tracker_mut(), cap, forwarded);
            }
            let remaining = detention::available(gas.tracker(), &state.host().ext().detention);
            state.host().ext_mut().last_caller_remaining = remaining;
            Ok(())
        },
        |gas, state, _msg, result| {
            let success = result.stop.is_success();
            let entry = state.host().ext_mut().pop_child(success);
            gas.set_reservoir(gas.reservoir() + entry.kept_reservoir);
            if !success && entry.start_charge > 0 {
                refill_history(gas, state, entry.start_charge / CPHB);
            }
            if success && entry.takebacks > 0 {
                refill_history(gas, state, entry.takebacks * WRITE_RECORD_SIZE);
            }
            // Satin `hold_caller`: a failed child restored records a take-back had freed
            // while a hint it sent into that room stays: the caller resumes over a limit.
            if !success && let Some(stop) = crate::satin_ext::hold_caller(state.host().ext()) {
                return stop_frame(gas, state, stop);
            }
            Ok(())
        },
    )?;
    detention::on_resume(gas, state)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_call(cx: _) -> Result {
    mega_call_impl(stack, cx.gas, cx.state, MessageKind::Call)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_callcode(cx: _) -> Result {
    mega_call_impl(stack, cx.gas, cx.state, MessageKind::CallCode)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_delegatecall(cx: _) -> Result {
    mega_call_impl(stack, cx.gas, cx.state, MessageKind::DelegateCall)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_staticcall(cx: _) -> Result {
    mega_call_impl(stack, cx.gas, cx.state, MessageKind::StaticCall)
}

fn mega_create_impl(
    stack: StackMut<'_>,
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    is_create2: bool,
) -> Result {
    let creator = state.message().destination;
    let creator_changed = core::cell::Cell::new(false);
    create_with_state_charge(
        stack,
        gas,
        state,
        is_create2,
        |gas, state, _, price| {
            if !off("create_lend") {
                lend_state(gas, state, price);
            }
            Ok(())
        },
        |gas, state, msg| {
            detention::repair_forward(gas, state, msg, u64::MAX);
            let host = state.host();
            let dest_info =
                host.state_mut().account_info_untracked(&msg.destination).ok().flatten();
            let dest_empty = dest_info.as_ref().is_none_or(|i| i.is_empty());
            let dest_changed = changed(host, msg.destination);
            creator_changed.set(changed(host, creator));
            let start = StartRecords::one(
                (msg.destination, dest_changed),
                dest_empty as i64,
                !msg.value.is_zero(),
            );
            let prices = state.host().ext().prices_history;
            let start_records = (!dest_changed) as u64;
            let nonce_records =
                (!creator_changed.get() && creator != state.host().ext().sender) as u64;
            let start_charge = if prices { start_records * WRITE_RECORD_SIZE * CPHB } else { 0 };
            start_charge_or_hand_back(
                gas,
                state,
                (start_records + nonce_records) * WRITE_RECORD_SIZE,
                msg.gas_limit,
            )?;
            let r = gas.reservoir();
            let share = reservoir_share(r);
            msg.reservoir = share;
            gas.set_reservoir(r - share);
            state.host().ext_mut().push_child(start_charge, r - share, Some(start), 0);
            detention::on_suspend(gas, state, 0);
            Ok(())
        },
        |gas, state, _msg, result| {
            let success = result.stop.is_success() && result.created_address.is_some();
            let entry = state.host().ext_mut().pop_child(success);
            gas.set_reservoir(gas.reservoir() + entry.kept_reservoir);
            if !success && entry.start_charge > 0 {
                refill_history(gas, state, entry.start_charge / CPHB);
            }
            // `hold_creator`: the creator's nonce record is counted on the creator now.
            let host = state.host();
            let records = (changed(host, creator) as i64 - creator_changed.get() as i64).max(0);
            let records = if creator == host.ext().sender { 0 } else { records };
            if let Some(stop) =
                host.ext_mut().draw([records * WRITE_RECORD_SIZE as i64, records, 0])
            {
                return stop_frame(gas, state, stop);
            }
            if !success && let Some(stop) = crate::satin_ext::hold_caller(state.host().ext()) {
                return stop_frame(gas, state, stop);
            }
            Ok(())
        },
    )?;
    detention::on_resume(gas, state)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_create(cx: _) -> Result {
    mega_create_impl(stack, cx.gas, cx.state, false)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_create2(cx: _) -> Result {
    mega_create_impl(stack, cx.gas, cx.state, true)
}

// ---------------------------------------------------------------------------------------------
// Message hooks: start records, answers, detention, code-deposit admission

pub fn count_start(evm: &mut Evm<'_, MegaTypes>) -> Option<(u8, u64)> {
    let start = evm.ext_mut().frames.last_mut().unwrap().start.take()?;
    let mut records = 0i64;
    let mut takebacks = 0u64;
    for &(a, before) in start.accounts() {
        let after = changed(evm, a);
        records += after as i64 - before as i64;
        if before && !after {
            takebacks += 1;
        }
    }
    let ds = records * WRITE_RECORD_SIZE as i64 +
        if start.transfer_log { TRANSFER_LOG_SIZE as i64 } else { 0 };
    let ext = evm.ext_mut();
    let crossing = ext.draw([ds, records, start.growth]);
    if crossing.is_none() {
        ext.frames.last_mut().unwrap().takebacks = takebacks;
    }
    crossing
}

#[derive(Debug, Default)]
pub struct StartHooks;

fn is_precompile(evm: &mut Evm<'_, MegaTypes>, msg: &Message<MegaTypes>) -> bool {
    evm.precompiles().contains_message(msg)
}

fn is_answered(evm: &mut Evm<'_, MegaTypes>, msg: &Message<MegaTypes>) -> bool {
    matches!(
        msg.kind,
        MessageKind::Call |
            MessageKind::CallCode |
            MessageKind::StaticCall |
            MessageKind::DelegateCall
    ) && (msg.code.is_empty() || is_precompile(evm, msg))
}

impl MessageHooks<MegaTypes> for StartHooks {
    fn enter(evm: &mut Evm<'_, MegaTypes>, msg: &Message<MegaTypes>) {
        if msg.depth == 0 {
            evm.ext_mut().top_started = true;
        }
    }

    fn before(
        evm: &mut Evm<'_, MegaTypes>,
        msg: &mut Message<MegaTypes>,
    ) -> core::result::Result<
        Option<evm2::interpreter::MessageResult<MegaTypes>>,
        evm2::ExecutionError,
    > {
        // A transaction whose sender or recipient is the block beneficiary, or whose
        // recipient delegates to it, reads volatile data from its start: detained at compute 0.
        if msg.depth == 0 && !off("q8_tx_beneficiary") {
            let beneficiary = evm.block().beneficiary;
            let ext = evm.ext_mut();
            if (msg.caller == beneficiary ||
                msg.call_target == beneficiary ||
                msg.code_address == beneficiary) &&
                detention::detains(ext) &&
                ext.detention.cap != u64::MAX
            {
                let cap = ext.detention.cap;
                ext.detention.limit = Some(ext.detention.limit.map_or(cap, |l| l.min(cap)));
            }
        }
        if msg.depth == 0 &&
            !off("q8_preframe_stop") &&
            let Some(stop) = evm.ext_mut().q8.pre_frame_stop.take()
        {
            // An authorization list the KV / state-growth limit skipped: the first frame's
            // stop, before it runs and before anything moves for it, on its untouched gas.
            evm.ext_mut().tx_start_pending = None;
            evm.ext_mut().note_stop(stop);
            return Ok(Some(evm2::interpreter::MessageResultExt {
                stop: InstrStop::Revert,
                output: limit_exceeded_output(stop.0, stop.1).into(),
                gas: GasTracker::new_with_execution_gas_and_reservoir(msg.gas_limit, msg.reservoir),
                ..Default::default()
            }));
        }
        if let Some(answer) = keyless::before(evm, msg) {
            return Ok(Some(answer));
        }
        if let Some(answer) = intercept::hint_before(evm, msg) {
            return Ok(Some(answer));
        }
        if evm.ext().intercept_before &&
            let Some(answer) = intercept::answer_before(evm, msg)
        {
            return Ok(Some(answer));
        }
        if off("precompile_hold") {
            return Ok(None);
        }
        let pc = is_precompile(evm, msg);
        Ok(detention::hold_precompile(evm, msg, pc))
    }

    fn started(
        evm: &mut Evm<'_, MegaTypes>,
        msg: &Message<MegaTypes>,
    ) -> core::result::Result<Option<(InstrStop, Bytes)>, evm2::ExecutionError> {
        if is_answered(evm, msg) {
            return Ok(None);
        }
        // Every start revm decided is held to its budgets and the transaction's limits, even one
        // that draws nothing (a deposit's created caller over a growth limit of nothing).
        // The first frame: a transaction already over a limit before it (a deposit's created
        // caller) is stopped before its start is counted; then the start is drawn.
        let crossing = if msg.depth == 0 {
            let before = if off("hold_start") { None } else { evm.ext().hold_check() };
            before.or_else(|| {
                evm.ext_mut().tx_start_pending.take().and_then(|d| evm.ext_mut().draw(d))
            })
        } else {
            count_start(evm)
                .or_else(|| if off("hold_start") { None } else { evm.ext().hold_check() })
        };
        if let Some(stop) = crossing {
            evm.ext_mut().note_stop(stop);
            return Ok(Some((InstrStop::Revert, limit_exceeded_output(stop.0, stop.1).into())));
        }
        Ok(None)
    }

    fn frame_built(evm: &mut Evm<'_, MegaTypes>, msg: &Message<MegaTypes>, gas: &mut GasTracker) {
        detention::on_frame_built(evm, msg, gas);
    }

    fn before_commit(
        evm: &mut Evm<'_, MegaTypes>,
        msg: &Message<MegaTypes>,
        stop: &mut InstrStop,
        output: &mut Bytes,
        gas: &mut GasTracker,
    ) -> core::result::Result<(), evm2::ExecutionError> {
        let answered = is_answered(evm, msg);
        detention::before_commit(evm, msg, answered, stop, output, gas);
        // A frame's return switches back on what it (or a frame below it) switched off.
        if evm.ext().q8.disabled_from.is_some_and(|from| from >= msg.depth as usize) {
            evm.ext_mut().q8.disabled_from = None;
        }
        if msg.depth == 0 {
            // The first frame answered without running (a value transfer to an account with no
            // code): its start is drawn now, a success giving way to the stop.
            if let Some(d) = evm.ext_mut().tx_start_pending.take() &&
                answered &&
                stop.is_success() &&
                let Some(cross) = evm.ext_mut().draw(d)
            {
                evm.ext_mut().note_stop(cross);
                *stop = InstrStop::Revert;
                *output = limit_exceeded_output(cross.0, cross.1).into();
                *gas =
                    GasTracker::new_with_execution_gas_and_reservoir(msg.gas_limit, msg.reservoir);
            }
            return Ok(());
        }
        if answered &&
            stop.is_success() &&
            let Some(cross) = count_start(evm)
        {
            evm.ext_mut().note_stop(cross);
            *stop = InstrStop::Revert;
            *output = limit_exceeded_output(cross.0, cross.1).into();
            *gas = GasTracker::new_with_execution_gas_and_reservoir(msg.gas_limit, msg.reservoir);
        }
        Ok(())
    }

    fn before_code_deposit(
        evm: &mut Evm<'_, MegaTypes>,
        msg: &Message<MegaTypes>,
        code: &Bytes,
        gas: &mut Gas,
    ) -> core::result::Result<(), evm2::HostError> {
        evm.ext_mut().q8.empty_deposit = code.is_empty() && !off("q8_empty_deposit_price");
        if off("deposit_site") {
            return Ok(());
        }
        detention::before_code_deposit(evm, msg, code, gas)
    }

    fn admit_code_deposit(
        evm: &mut Evm<'_, MegaTypes>,
        _msg: &Message<MegaTypes>,
        code: &Bytes,
        _gas_before: &GasTracker,
        gas: &mut Gas,
    ) -> core::result::Result<Option<(InstrStop, Bytes)>, evm2::ExecutionError> {
        if keyless::refuses_empty_deposit(evm, _msg, code) {
            return Ok(Some((InstrStop::Revert, Bytes::new())));
        }
        // The deposited code's history (W first), then its bytes drawn on the creation's budget.
        let len = code.len() as u64;
        if evm.ext().prices_history && len > 0 {
            let amount = len * CPHB;
            let spill = amount.saturating_sub(gas.reservoir());
            let ext = evm.ext_mut();
            if let Some(frame) = ext.detention.frames.last_mut() &&
                !off("deposit_site")
            {
                let loan = spill.min(frame.withheld);
                frame.withheld -= loan;
                gas.erase_cost(loan);
                ext.deposit_loan += loan;
            }
            if gas.spend_state(amount).is_err() {
                evm.ext_mut().deposit_loan = 0;
                return Ok(Some((InstrStop::OutOfGas, Bytes::new())));
            }
            evm.ext_mut().book_history(amount as i64, len as i64);
        }
        let loan = core::mem::take(&mut evm.ext_mut().deposit_loan);
        if let Some(cross) = evm.ext_mut().draw([len as i64, 0, 0]) {
            // The refusal restores the pre-hook snapshot: the withheld gas lent since goes back.
            if !off("deposit_loan_back") &&
                let Some(frame) = evm.ext_mut().detention.frames.last_mut()
            {
                frame.withheld += loan;
            }
            evm.ext_mut().note_stop(cross);
            return Ok(Some((InstrStop::Revert, limit_exceeded_output(cross.0, cross.1).into())));
        }
        Ok(None)
    }
}

pub fn word_u256(w: &Word) -> U256 {
    *w
}
