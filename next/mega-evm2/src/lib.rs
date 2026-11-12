//! The Satin execution engine on evm2: Satin's frame, stop and start-ledger rules, history and
//! SALT pricing, destruction, and gas detention on a side ledger, composed on the thin evm2 fork's
//! hooks and op-evm2's OP layer, run through `Evm::transact`.
//!
//! Where each mechanism lives:
//! - type family, Satin `Version`, opcode table, SALT pricing (state-gas price hook): this file;
//! - start ledger, pools, stops, history ledger: `ledger.rs`;
//! - SALT's mapping, the node's capacity snapshot, the per-attempt multiplier cache and the lookup
//!   exports: `salt.rs`;
//! - `BLOCKHASH`, and the record and exports of the hashes it is served: `block_hashes.rs`;
//! - withheld-gas detention (side ledger, read and drawn through the fork's gas reserve),
//!   code-deposit and precompile sites: `detention.rs`;
//! - interceptors (`MegaLimitControl`, `MegaAccessControl`, the Oracle's hints): `intercept.rs`;
//! - transaction handlers (body history, first frame, settlement, deposits, L1 fee): `tx.rs`;
//! - EIP-7702 transactions: `eip7702.rs`; keyless deployment: `keyless.rs`;
//! - per-transaction state of volatile-access control, hints and authorities: `satin_ext.rs`;
//! - block execution (the block session; its pre-block sequence in `block/pre_block.rs`):
//!   `block.rs`; the system contracts it deploys: `system_contracts.rs`;
//! - the chain configuration (schedule, activation, bootstrap, the fixed protocol values):
//!   `chain.rs`; the factory that makes every EVM and block session: `factory.rs`; typed errors:
//!   `error.rs`.

#![allow(missing_docs)]

pub mod block;
pub mod block_hashes;
pub mod chain;
pub mod detention;
pub mod eip7702;
pub mod error;
pub mod factory;
pub mod intercept;
pub mod keyless;
pub mod ledger;
#[cfg(feature = "compare")]
pub mod regular_check;
pub mod salt;
#[cfg(not(feature = "salt-test-hasher"))]
pub mod salt_hash;
pub mod satin_ext;
mod system_code;
pub mod system_contracts;
pub mod tx;

pub use crate::{block_hashes::*, ledger::*, salt::*};

use alloy_primitives::{Address, Bytes, U256};
use evm2::{
    Evm, EvmConfig, EvmConfigSelector, EvmFeatures, EvmTypes, EvmTypesHost, ExecutionConfig,
    MessageHooks, OpcodeConfig, SpecId, Version,
    interpreter::{
        Gas, GasTracker, InstrStop, InterpreterState, Message, MessageKind, Result, StackMut, Word,
        instructions::{
            self as ins, AccountLoadSite, BudgetUse, ReserveDraw, call_with, create_with,
        },
        op,
    },
    precompiles::{PrecompileMap, Precompiles},
    utils::WordExt,
    version::{GasId, StateGasSite},
};
use evm2_macros::instruction;
use op_evm2::{OpSpecId, OpTxEnvelope, OpTxResultExt, OpTypes};
use std::sync::OnceLock;

/// Negative controls: `MEGA_EVM2_DISABLE=a,b` switches named fixes off (tests only; `compare`
/// feature, otherwise a constant `false` the compiler folds away).
#[cfg(feature = "compare")]
pub fn off(name: &str) -> bool {
    static OFF: OnceLock<Vec<String>> = OnceLock::new();
    OFF.get_or_init(|| {
        std::env::var("MEGA_EVM2_DISABLE")
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

/// `MEGA_EVM2_DEBUG` traces, read once (a per-frame `getenv` would distort measurements; `compare`
/// feature only).
#[cfg(feature = "compare")]
pub fn debug() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var("MEGA_EVM2_DEBUG").is_ok())
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
        let price = reprice(host, id, site)?;
        #[cfg(feature = "compare")]
        regular_check::on_state_priced(host, price);
        if matches!(id, GasId::CreateState | GasId::CodeDepositState) {
            host.ext_mut().created_prices.push((address, id, price));
        }
        Ok(price)
    }

    /// Detention's withheld gas: every budget decision of evm2's instructions sees it.
    #[inline(always)]
    fn gas_reserve(host: &mut Evm<'_, Self>, use_: BudgetUse) -> u64 {
        detention::reserve_for(host.ext(), use_)
    }

    /// The forward and the state spills draw the withheld gas first.
    #[inline(always)]
    fn draw_gas_reserve(host: &mut Evm<'_, Self>, amount: u64, draw: ReserveDraw) -> u64 {
        detention::draw_reserve(host.ext_mut(), amount, draw)
    }

    /// Satin refuses a read of the block beneficiary's account in a frame whose volatile access
    /// is off where the load happens (its Host refuses the load): evm2 offers every load of its
    /// instructions here, after the charges the instruction makes before it and before its
    /// skip-cold check. The opcode's wrapper then reverts the frame ([`settle_refusal`]).
    #[inline(always)]
    fn before_account_load(
        state: &mut InterpreterState<'_, '_, Self>,
        address: &Address,
        site: AccountLoadSite,
    ) -> Result {
        if crate::satin_ext::refused_here(state) && refuses_load(state, address, site) {
            state.host().ext_mut().mechanisms.load_refused = true;
            return Err(InstrStop::Revert);
        }
        Ok(())
    }
}

/// The SALT price of a state charge at `site`, as `state_gas_price` makes it for an account the
/// destruction table has not given back: the schedule's entry times the bucket's multiplier, read
/// through the attempt's cache (so the same price for every charge and give-back in the bucket in
/// a transaction, `SATIN-STATE-006`). The protocol's own work — a system-originated transaction, a
/// system call — prices at `m = 1` and reads no capacity (`SATIN-STATE-009`). A lookup that fails,
/// or answers a capacity below the minimum, fails the transaction as an internal error of the node
/// (`SATIN-STATE-007`).
pub fn reprice(
    host: &mut Evm<'_, MegaTypes>,
    id: GasId,
    site: StateGasSite,
) -> core::result::Result<u64, evm2::ExecutionError> {
    let m = if host.ext().is_system || off("salt") {
        1
    } else {
        host.ext_mut()
            .salt
            .multiplier(bucket_for(site))
            .map_err(|e| evm2::ExecutionError::Fatal(evm2::AnyError::new(e)))?
    };
    Ok(host.version().gas_params.get(id).saturating_mul(m))
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

/// An instruction the engine defines, observed by the regular-gas check in comparison builds
/// ([`regular_check::Observed`]); the instruction itself otherwise.
#[cfg(feature = "compare")]
pub(crate) type Obs<I> = regular_check::Observed<I>;
#[cfg(not(feature = "compare"))]
pub(crate) type Obs<I> = I;

/// The opcodes a frame whose volatile-data access is off may have refused (`SATIN-MAC-003`): each
/// charges its static gas itself, once it is not refused, so a refused read costs nothing, not
/// even its static gas (`SATIN-MAC-004`). Their entries in [`satin_table`] carry no static gas;
/// [`static_gas`] is what they charge.
pub(crate) const REFUSABLE: [u8; 20] = [
    op::COINBASE,
    op::DIFFICULTY,
    op::GASLIMIT,
    op::BASEFEE,
    op::BLOBBASEFEE,
    op::SLOTNUM,
    op::BLOCKHASH,
    op::TIMESTAMP,
    op::NUMBER,
    op::BALANCE,
    op::EXTCODESIZE,
    op::EXTCODEHASH,
    op::EXTCODECOPY,
    op::SELFBALANCE,
    op::CALL,
    op::CALLCODE,
    op::DELEGATECALL,
    op::STATICCALL,
    op::SELFDESTRUCT,
    op::SLOAD,
];

/// Osaka's table + Amsterdam opcodes and prices + Satin's wrappers; a refusable opcode
/// ([`REFUSABLE`]) charges its static gas itself, so its entry carries none.
pub const fn satin_table() -> OpcodeConfig<MegaTypes> {
    let mut c = priced_table();
    let mut i = 0;
    while i < REFUSABLE.len() {
        c.set_static_gas(REFUSABLE[i], 0);
        i += 1;
    }
    c
}

/// The static gas of `opcode` in Satin's schedule, which a refusable opcode ([`REFUSABLE`]) charges
/// itself once it is not refused (read at compile time by its instruction).
pub(crate) const fn static_gas(opcode: u8) -> u64 {
    priced_table().static_gas(opcode) as u64
}

/// [`satin_table`] with every opcode at its static gas.
const fn priced_table() -> OpcodeConfig<MegaTypes> {
    let mut c = OpcodeConfig::<MegaTypes>::base::<evm2::BaseEvmConfig<{ SpecId::OSAKA as u32 }>>();
    c.set_instruction::<Obs<ins::dupn<MegaTypes>>>(op::DUPN, 3);
    c.set_instruction::<Obs<ins::swapn<MegaTypes>>>(op::SWAPN, 3);
    c.set_instruction::<Obs<ins::exchange<MegaTypes>>>(op::EXCHANGE, 3);
    c.set_instruction::<Obs<mega_slotnum>>(op::SLOTNUM, 2);
    c.set_static_gas(op::EXTCODESIZE, 200);
    c.set_static_gas(op::EXTCODECOPY, 200);
    c.set_instruction::<Obs<mega_sstore>>(op::SSTORE, c.static_gas(op::SSTORE));
    c.set_instruction::<Obs<mega_sload>>(op::SLOAD, c.static_gas(op::SLOAD));
    c.set_instruction::<Obs<mega_balance>>(op::BALANCE, c.static_gas(op::BALANCE));
    c.set_instruction::<Obs<mega_extcodesize>>(op::EXTCODESIZE, c.static_gas(op::EXTCODESIZE));
    c.set_instruction::<Obs<mega_extcodehash>>(op::EXTCODEHASH, c.static_gas(op::EXTCODEHASH));
    c.set_instruction::<Obs<mega_extcodecopy>>(op::EXTCODECOPY, c.static_gas(op::EXTCODECOPY));
    c.set_instruction::<Obs<mega_selfbalance>>(op::SELFBALANCE, c.static_gas(op::SELFBALANCE));
    c.set_instruction::<Obs<mega_call>>(op::CALL, c.static_gas(op::CALL));
    c.set_instruction::<Obs<mega_callcode>>(op::CALLCODE, c.static_gas(op::CALLCODE));
    c.set_instruction::<Obs<mega_delegatecall>>(op::DELEGATECALL, c.static_gas(op::DELEGATECALL));
    c.set_instruction::<Obs<mega_staticcall>>(op::STATICCALL, c.static_gas(op::STATICCALL));
    c.set_instruction::<Obs<mega_create>>(op::CREATE, c.static_gas(op::CREATE));
    c.set_instruction::<Obs<mega_create2>>(op::CREATE2, c.static_gas(op::CREATE2));
    c.set_instruction::<Obs<mega_log<0>>>(op::LOG0, c.static_gas(op::LOG0));
    c.set_instruction::<Obs<mega_log<1>>>(op::LOG1, c.static_gas(op::LOG1));
    c.set_instruction::<Obs<mega_log<2>>>(op::LOG2, c.static_gas(op::LOG2));
    c.set_instruction::<Obs<mega_log<3>>>(op::LOG3, c.static_gas(op::LOG3));
    c.set_instruction::<Obs<mega_log<4>>>(op::LOG4, c.static_gas(op::LOG4));
    c.set_instruction::<Obs<mega_selfdestruct>>(op::SELFDESTRUCT, c.static_gas(op::SELFDESTRUCT));
    c.set_instruction::<Obs<keyless::mega_keyless>>(keyless::DISPATCHER, 0);
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
    satin_version_at(CPSB)
}

/// [`satin_version`] with every EIP-8037 state-gas entry rebuilt at `cost_per_state_byte`: a fresh
/// slot's 64 bytes, a new account's and a created contract's 120, deployed code's 1 per byte and
/// an EIP-7702 delegation indicator's 23 (Amsterdam's entries are built at its own cost per state
/// byte).
pub fn satin_version_at(cost_per_state_byte: u64) -> Version {
    let mut v = Version::new(SpecId::OSAKA);
    v.features.insert(EvmFeatures::EIP8037 | EvmFeatures::EIP2780 | EvmFeatures::EIP7708);
    v.gas_params = Version::new(SpecId::AMSTERDAM).gas_params;
    let entry = |bytes: u64| {
        bytes
            .checked_mul(cost_per_state_byte)
            .and_then(|gas| u32::try_from(gas).ok())
            .expect("a state-gas entry fits the schedule's 32 bits")
    };
    let p = &mut v.gas_params;
    p.set(GasId::SstoreSetState, entry(64));
    p.set(GasId::NewAccountState, entry(120));
    p.set(GasId::CreateState, entry(120));
    p.set(GasId::CodeDepositState, entry(1));
    p.set(GasId::TxEip7702PerAuthState, entry(23));
    // Every give-back of state and history gas credits a reservoir, never regular gas
    // (`SATIN-HIST-012`), and the state an opcode creates takes its legacy share from regular gas
    // first (`SATIN-STATE-012`): its pre-Satin price less the regular cost the schedule gives the
    // component, flat, at every multiplier.
    v.features.insert(EvmFeatures::EIP8037_RESERVOIR_REFILLS);
    let share = |pre_fork: u64, execution: u64| {
        u32::try_from(pre_fork.saturating_sub(execution)).expect("a legacy share fits 32 bits")
    };
    p.set(
        GasId::SstoreSetStateLegacyShare,
        share(PRE_FORK_SSTORE_SET_COST, p.get(GasId::SstoreSetWithoutLoadCost)),
    );
    p.set(
        GasId::CallNewAccountStateLegacyShare,
        share(PRE_FORK_NEW_ACCOUNT_COST, p.get(GasId::NewAccountCost)),
    );
    p.set(
        GasId::SelfdestructNewAccountStateLegacyShare,
        share(PRE_FORK_NEW_ACCOUNT_COST, p.get(GasId::NewAccountCostForSelfdestruct)),
    );
    p.set(GasId::CreateStateLegacyShare, share(PRE_FORK_CREATE_COST, p.get(GasId::Create)));
    p.set(GasId::CallStipendReduction, 50);
    v.tx_gas_limit_cap = EXECUTION_CAP;
    v.tx_total_gas_cap = u64::MAX;
    v.max_code_size = 512 * 1024;
    v.max_initcode_size = 1024 * 1024;
    v.chain_id = 1;
    v
}

/// [`satin_version`] as the comparison runs a case under the reference's `rules`
/// (`Lite::reference_rules`): with [`ReferenceRule::Eip8037GiveBack`] among them without the
/// reservoir-only give-back rule and with every legacy share zero; otherwise Satin's.
#[cfg(feature = "compare")]
pub fn satin_version_under(rules: &[ReferenceRule]) -> Version {
    let mut v = satin_version();
    if rules.contains(&ReferenceRule::Eip8037GiveBack) {
        v.features.remove(EvmFeatures::EIP8037_RESERVOIR_REFILLS);
        for id in [
            GasId::SstoreSetStateLegacyShare,
            GasId::CallNewAccountStateLegacyShare,
            GasId::SelfdestructNewAccountStateLegacyShare,
            GasId::CreateStateLegacyShare,
        ] {
            v.gas_params.set(id, 0);
        }
    }
    v
}

/// Whether the running version gives state and history gas back into reservoirs only
/// (`SATIN-HIST-012`): Satin's rule, which the comparison turns off to run a case under the
/// reference's ([`ReferenceRule::Eip8037GiveBack`]).
#[inline(always)]
pub(crate) fn reservoir_refills(evm: &Evm<'_, MegaTypes>) -> bool {
    evm.feature(EvmFeatures::EIP8037_RESERVOIR_REFILLS)
}

/// The Satin precompile set: op-evm2's Karst map, KZG repriced to 100,000.
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
            #[cfg(feature = "compare")]
            let p = if a == Address::with_last_byte(0x05) {
                MODEXP.get_or_init(|| p.run());
                evm2::precompiles::Precompile::new(
                    a,
                    evm2::precompiles::PrecompileId::ModExp,
                    modexp_compare,
                )
            } else {
                p
            };
            out.insert(p);
        }
        out
    })
}

/// The Satin precompile provider: evm2's `Precompiles` over [`precompile_map`]. The system
/// contracts are not in it; the `before` message hook answers them.
pub fn precompiles() -> Precompiles<MegaTypes> {
    Precompiles::new(std::borrow::Cow::Borrowed(precompile_map()))
}

/// evm2's ModExp, which the comparison's [`modexp_compare`] runs.
#[cfg(feature = "compare")]
static MODEXP: OnceLock<evm2::precompiles::PrecompileFn<MegaTypes>> = OnceLock::new();

/// ModExp under the comparison: evm2's own, or, under the reference's rule
/// ([`ReferenceRule::ModexpBoundFirst`]), the order of the implementation under review: an input
/// over EIP-7823's bound costs its 500 minimum and fails before its formula price (so the price
/// probe reads 500 for it).
#[cfg(feature = "compare")]
fn modexp_compare(
    evm: &mut Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    gas: &mut GasTracker,
) -> evm2::precompiles::PrecompileResult {
    use evm2::{PrecompileError, precompiles::PrecompileHalt};
    if under_reference_rule(evm.ext(), ReferenceRule::ModexpBoundFirst) &&
        modexp_over_bound(evm.message_input(msg))
    {
        gas.spend(500).map_err(|_| PrecompileError::Halt(PrecompileHalt::OutOfGas))?;
        return Err(PrecompileError::Halt(PrecompileHalt::ModexpEip7823LimitSize));
    }
    (MODEXP.get().expect("the map holds evm2's ModExp"))(evm, msg, gas)
}

/// Whether `input`, read as ModExp's header (three 32-byte lengths, right-padded with zeros), has
/// a base, exponent or modulus length over EIP-7823's 1,024 bytes (comparison only).
#[cfg(feature = "compare")]
fn modexp_over_bound(input: &[u8]) -> bool {
    (0..3).any(|i| {
        let mut word = [0u8; 32];
        let part = input.get(32 * i..).unwrap_or_default();
        let n = part.len().min(32);
        word[..n].copy_from_slice(&part[..n]);
        U256::from_be_bytes(word) > U256::from(1_024)
    })
}

fn kzg100k(
    evm: &mut Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    gas: &mut GasTracker,
) -> evm2::precompiles::PrecompileResult {
    gas.spend(100_000)
        .map_err(|_| evm2::PrecompileError::Halt(evm2::precompiles::PrecompileHalt::OutOfGas))?;
    let mut free = GasTracker::new(u64::MAX / 2);
    evm2::precompiles::kzg_point_evaluation::run(evm.message_input(msg), &mut free)
}

/// SLOTNUM (EIP-7843) pushes zero in every block: MegaETH has no slot, so the block environment's
/// slot number is not read. It stays a volatile read (the `Volatile` wrapper of `wrap_table`).
#[instruction(EvmTypes = MegaTypes)]
pub fn mega_slotnum(cx: _) -> Result<out> {
    *out = if under_reference_rule(cx.state.host().ext(), ReferenceRule::SlotNumber) {
        cx.state.host().block().slot_num
    } else {
        Word::ZERO
    };
}

/// Whether the engine runs under the implementation under review's rule `rule` instead of its own
/// (`Lite::reference_rules`, comparison only); always `false` without the `compare` feature.
#[cfg(feature = "compare")]
pub(crate) fn under_reference_rule(ext: &Lite, rule: ReferenceRule) -> bool {
    ext.reference_rules.contains(&rule)
}

#[cfg(not(feature = "compare"))]
#[inline(always)]
pub(crate) fn under_reference_rule(_: &Lite, _: ReferenceRule) -> bool {
    false
}

// ---------------------------------------------------------------------------------------------
// Completed writes: SSTORE, LOG, SELFDESTRUCT

/// SLOAD: the Oracle's slots are Satin's `oracle_sload`; every other slot is evm2's body, whose
/// skip-cold check sees the withheld gas through the gas reserve.
#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_sload(cx: _) -> Result {
    let destination = cx.state.message().destination;
    let reference =
        under_reference_rule(cx.state.host().ext(), ReferenceRule::RefusalAfterStaticGas);
    // The Oracle's storage in a frame whose access is off: refused once the key is on the stack,
    // before any gas is charged (`SATIN-MAC-003`, `SATIN-MAC-004`); an empty stack halts on the
    // underflow as with access on.
    if destination == ORACLE &&
        !reference &&
        !stack.is_empty() &&
        crate::satin_ext::refused_here(cx.state)
    {
        return revert_with(
            cx.gas,
            cx.state,
            &crate::satin_ext::disabled_revert(crate::satin_ext::access::ORACLE),
        );
    }
    if destination != ORACLE {
        cx.gas.spend(const { static_gas(op::SLOAD) })?;
        return ins::sload::<MegaTypes>::execute(cx.pc, stack, cx.state);
    }
    // The reference's orders (comparison only), each its own decision's old rule. Its refusal
    // comes after the opcode's static gas (the warm base) and the key, so a refused read pays the
    // warm base first (`RefusalAfterStaticGas`); a read that is not refused takes nothing from
    // that rule. Its read charges the warm base first, before the key and the read
    // (`OracleColdSkip`).
    let cold_skip = under_reference_rule(cx.state.host().ext(), ReferenceRule::OracleColdSkip);
    let refused_late = reference && crate::satin_ext::refused_here(cx.state);
    let warm_first = refused_late || cold_skip;
    if warm_first {
        cx.gas.spend(const { static_gas(op::SLOAD) })?;
    }
    let key = stack.pop()?;
    if refused_late {
        return revert_with(
            cx.gas,
            cx.state,
            &crate::satin_ext::disabled_revert(crate::satin_ext::access::ORACLE),
        );
    }
    let (gas, state) = unsafe { evm2::interpreter::private::split_gas_state(cx.state) };
    // Satin `oracle_sload`: the slot is loaded through the journal, the service asked and the read
    // marked as a volatile read under the Oracle's cap, at the compute before the cold access,
    // and only then is the read priced cold whatever its warmth, `COLD_SLOAD_COST` with its warm
    // base in one charge (`SATIN-ORCL-004`, `SATIN-DET-003`): a frame whose gas cannot pay it,
    // even one short of the warm base, still loads, asks and marks, then runs out of gas, the
    // mark standing (or is stopped, when the charge is detention's crossing). No read of a system
    // call, a deposit or a Mega System Transaction asks the service (`SATIN-ORCL-005`): it pushes
    // the slot's value on the chain. Under the reference's order (comparison only) a frame that
    // cannot pay the cold access once the warm base is paid runs out of gas before the read, and
    // a read is marked once its charges are paid.
    let additional_cold_cost = state.gas_params().get(GasId::ColdStorageAdditionalCost);
    if cold_skip && state.feature(EvmFeatures::EIP2929) {
        let budget = ins::gas_budget(gas, state, BudgetUse::ColdStorage);
        if budget < additional_cold_cost {
            return Err(InstrStop::OutOfGas);
        }
    }
    let load = evm2::interpreter::Host::sload(state.host(), &destination, &key, false)
        .map_err(|error| state.fail(error))?;
    let ext = state.host().ext();
    let unserved = ext.mechanisms.system_call ||
        (ext.is_deposit && !under_reference_rule(ext, ReferenceRule::OracleAsksDepositsAndMsts));
    let answer = if unserved { None } else { ext.cfg.oracle.get(&key).copied() };
    if !unserved {
        state.host().ext_mut().oracle_reads += 1;
    }
    let cap = state.host().ext().detention.oracle_cap;
    if cold_skip {
        gas.spend(additional_cold_cost)?;
        detention::commit_read(state.host().ext_mut(), gas.tracker_mut(), cap, 0);
    } else {
        detention::commit_read(state.host().ext_mut(), gas.tracker_mut(), cap, 0);
        gas.spend(const { static_gas(op::SLOAD) } + additional_cold_cost)?;
    }
    stack.push(answer.unwrap_or(load.value))
}

/// Whether Satin refuses the load of `address`'s account at `site` in a frame whose volatile
/// access is off: a read of the block beneficiary's account. A creation's loads of its creator and
/// of the created address are no reads; a call's load of its target's EIP-7702 delegate is.
#[inline(never)]
fn refuses_load(
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    address: &Address,
    site: AccountLoadSite,
) -> bool {
    let read = match site {
        AccountLoadSite::CreateCaller | AccountLoadSite::CreateDestination => false,
        AccountLoadSite::CallDelegate => !off("delegate_refusal"),
        _ => true,
    };
    read && *address == state.host().block().beneficiary
}

/// The gas a refused load hands back, kept as a refusable opcode's body starts: what the frame
/// had after the opcode's static gas. Kept only in a frame whose reads are refused.
#[inline(always)]
fn refusal_gas(gas: &Gas, state: &mut InterpreterState<'_, '_, MegaTypes>) -> Option<Gas> {
    // Every refusable load runs inside a wrapper that settles it: none is left pending here.
    debug_assert!(!state.host().ext().mechanisms.load_refused, "a refused load was not settled");
    crate::satin_ext::refused_here(state).then_some(*gas)
}

/// Settles a refusable opcode's body: a load [`MegaTypes::before_account_load`] refused reverts
/// the frame with `VolatileDataAccessDisabled` on `kept`, the gas the frame had after the opcode's
/// static gas (Satin `read_volatile`): none of the dynamic gas charged before the load is owed for
/// a read that did not happen. Any other result stands.
#[inline(always)]
fn settle_refusal(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    kept: Option<Gas>,
    result: Result,
) -> Result {
    match kept {
        Some(kept) if core::mem::take(&mut state.host().ext_mut().mechanisms.load_refused) => {
            refuse_on(gas, state, kept)
        }
        _ => result,
    }
}

#[cold]
#[inline(never)]
fn refuse_on(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>, kept: Gas) -> Result {
    if !off("refusal_gas") {
        *gas = kept;
    }
    revert_with(
        gas,
        state,
        &crate::satin_ext::disabled_revert(crate::satin_ext::access::BENEFICIARY),
    )
}

/// Whether the block beneficiary is refused before an opcode's body runs once its static gas is
/// charged, where the reference's order refuses it at its load: only with the switch
/// `refuse_at_load` off.
fn refused_before_body(
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    operand: Option<Word>,
) -> bool {
    off("refuse_at_load") &&
        operand.is_some_and(|w| w.to_address() == state.host().block().beneficiary) &&
        crate::satin_ext::refused_here(state)
}

/// Whether Satin refuses a conditionally volatile opcode before it charges or checks anything
/// (`SATIN-MAC-003`, `SATIN-MAC-004`): in a frame whose volatile-data access is off, once its
/// `operands` are on the stack, when one of `reads` is the block beneficiary. An opcode short of
/// its operands is not refused: it fails on them as with access on. Under the reference's rule
/// (comparison only) nothing is refused here, and the opcode is refused where evm2 loads the
/// beneficiary's account, once its static gas and its body's earlier charges are made.
#[inline(always)]
fn refused_up_front(
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    stack_len: usize,
    operands: usize,
    reads: &[Address],
) -> bool {
    stack_len >= operands &&
        crate::satin_ext::refused_here(state) &&
        !under_reference_rule(state.host().ext(), ReferenceRule::RefusalAfterStaticGas) &&
        reads.contains(&state.host().block().beneficiary)
}

/// The refusal [`refused_up_front`] decided: the frame reverts with `VolatileDataAccessDisabled`
/// on the gas it held when the opcode was reached.
#[cold]
#[inline(never)]
fn refuse_up_front(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>) -> Result {
    revert_with(
        gas,
        state,
        &crate::satin_ext::disabled_revert(crate::satin_ext::access::BENEFICIARY),
    )
}

/// Commits a read of the beneficiary's account once the opcode completed (`read_volatile`).
fn beneficiary_read(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>, addr: Address) {
    if addr == state.host().block().beneficiary && !off("beneficiary") {
        let cap = state.host().ext().detention.cap;
        detention::commit_read(state.host().ext_mut(), gas.tracker_mut(), cap, 0);
    }
}

/// An account opcode `$op` of `$operands` operands, its target the first: in a frame whose
/// volatile access is off, refused before anything when the target is the block beneficiary
/// ([`refused_up_front`]); then its static gas and evm2's body (its skip-cold check sees the
/// withheld gas through the gas reserve; under the reference's rule its load of the beneficiary is
/// refused through `before_account_load`), then its volatile read once it completed.
macro_rules! account_opcode {
    ($name:ident, $base:ident, $op:expr, $operands:expr) => {
        #[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
        pub fn $name(cx: _) -> Result {
            let target = stack.peek(0).map(|w| w.to_address()).unwrap_or_default();
            if refused_up_front(cx.state, stack.len(), $operands, &[target]) {
                return refuse_up_front(cx.gas, cx.state);
            }
            cx.gas.spend(const { static_gas($op) })?;
            let a = stack.peek(0).ok_or(InstrStop::StackUnderflow)?.to_address();
            if refused_before_body(cx.state, stack.peek(0)) {
                return revert_with(
                    cx.gas,
                    cx.state,
                    &crate::satin_ext::disabled_revert(crate::satin_ext::access::BENEFICIARY),
                );
            }
            let kept = refusal_gas(cx.gas, cx.state);
            let result = ins::$base::<MegaTypes>::execute(cx.pc, stack, cx.state);
            // SAFETY: `gas` is not reached through `state` while both are used.
            let (gas, state) = unsafe { evm2::interpreter::private::split_gas_state(cx.state) };
            settle_refusal(gas, state, kept, result)?;
            beneficiary_read(gas, state, a);
            Ok(())
        }
    };
}
account_opcode!(mega_balance, balance, op::BALANCE, 1);
account_opcode!(mega_extcodesize, extcodesize, op::EXTCODESIZE, 1);
account_opcode!(mega_extcodehash, extcodehash, op::EXTCODEHASH, 1);
account_opcode!(mega_extcodecopy, extcodecopy, op::EXTCODECOPY, 4);

/// SELFBALANCE: the frame's own account, loaded as revm loads it, before the push's stack check:
/// a refused load comes before a full stack's overflow. evm2's body (and its account-load site) is
/// not used, since its stack preamble checks the push before the load.
#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_selfbalance(cx: _) -> Result {
    let a = cx.state.message().destination;
    let refused = |state: &mut InterpreterState<'_, '_, MegaTypes>| {
        crate::satin_ext::refused_here(state) &&
            refuses_load(state, &a, AccountLoadSite::SelfBalance)
    };
    let overflows = stack.len() >= evm2::constants::STACK_LIMIT;
    if overflows && off("selfbalance_stack_order") {
        return Err(InstrStop::StackOverflow);
    }
    // Refused before any gas is charged (`SATIN-MAC-004`); under the reference's rule
    // (comparison only) after the static gas.
    let reference =
        under_reference_rule(cx.state.host().ext(), ReferenceRule::RefusalAfterStaticGas);
    if !reference && refused(cx.state) {
        return refuse_up_front(cx.gas, cx.state);
    }
    cx.gas.spend(const { static_gas(op::SELFBALANCE) })?;
    if reference && refused(cx.state) {
        return revert_with(
            cx.gas,
            cx.state,
            &crate::satin_ext::disabled_revert(crate::satin_ext::access::BENEFICIARY),
        );
    }
    let balance = evm2::interpreter::Host::load_account(cx.state.host(), &a, false, false)
        .map_err(|e| cx.state.fail(e))?
        .balance;
    stack.push(balance)?;
    beneficiary_read(cx.gas, cx.state, a);
    Ok(())
}

/// SSTORE: evm2's body (its sentry, skip-cold check and state spill see the withheld gas through
/// the gas reserve), then Satin's completed write once every charge is made.
#[instruction(EvmTypes = MegaTypes, dynamic_gas)]
pub fn mega_sstore(cx: _, [key, value]: [Word]) -> Result {
    let mut written = None;
    ins::sstore_with(cx.gas, cx.state, key, value, |_, _, _, s| {
        written = Some(*s);
        Ok(())
    })?;
    if let Some(s) = written {
        sstore_completed(cx.gas, cx.state, key, s)?;
    }
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

/// SELFDESTRUCT: evm2's body (its skip-cold check sees the withheld gas, its beneficiary's
/// new-account state spill draws it first, both through the gas reserve), then
/// the completed-write rule: the destruction table of an account created in the transaction,
/// the records of the account it clears and of its beneficiary, its
/// transfer log and state growth, held to the limits, then the records' history; last, the
/// volatile reads of both ends, once nothing can fail the opcode any more.
#[instruction(EvmTypes = MegaTypes, no_stack_preamble, dynamic_gas)]
pub fn mega_selfdestruct(cx: _) -> Result {
    // In a frame whose access is off, refused before anything, its static-context check included,
    // when its target or the frame's own account is the beneficiary (`SATIN-MAC-003`).
    let me = cx.state.message().destination;
    let target = stack.peek(0).map(|w| w.to_address()).unwrap_or_default();
    if refused_up_front(cx.state, stack.len(), 1, &[target, me]) {
        return refuse_up_front(cx.gas, cx.state);
    }
    cx.gas.spend(const { static_gas(op::SELFDESTRUCT) })?;
    // The base body's first check: nothing is loaded in a static frame.
    if cx.state.is_static() {
        return Err(InstrStop::StateChangeDuringStaticCall);
    }
    let target = stack.peek(0).ok_or(InstrStop::StackUnderflow)?.to_address();
    let me = cx.state.message().destination;
    let state = &mut *cx.state;
    let looked: core::result::Result<_, evm2::DatabaseError> = (|| {
        let mut a = state.host().state_mut().account(&me)?;
        let eligible = a.is_created() && !a.is_destructed();
        let len = if eligible { a.load_code()?.original_byte_slice().len() } else { 0 };
        // Satin gives the leaf back only for an account the transaction found absent at its
        // start (`Accounts::was_absent_at_start`); a pre-funded address created onto, or an
        // account that existed empty, keeps its leaf ("a part that existed at the start ... its
        // write record stands").
        let absent_at_start = if off("sd_absent_leaf") {
            a.original().is_none_or(|o| o.is_empty())
        } else {
            a.original().is_none()
        };
        Ok((a.balance(), eligible, len as u64, absent_at_start))
    })();
    let (balance, eligible, code_len, absent_at_start) =
        looked.map_err(|e| state.fail(evm2::HostError::from(e)))?;
    let me_changed = changed(state.host(), me);
    let moves = !balance.is_zero() && target != me;
    // In a frame whose volatile access is off, evm2's body offers the loads of both ends to
    // `before_account_load`, which refuses the block beneficiary's.
    let kept = refusal_gas(cx.gas, cx.state);
    let r = ins::selfdestruct::<MegaTypes>::execute(cx.pc, stack, cx.state);
    // SAFETY: `gas` is not reached through `state` while both are used.
    let (gas, state) = unsafe { evm2::interpreter::private::split_gas_state(cx.state) };
    let r = settle_refusal(gas, state, kept, r);
    if r != Err(InstrStop::SelfDestruct) {
        return r;
    }
    // A read counts only once the opcode completed: after its records were held to the limits and
    // their history was paid (below). A write that then fails reads nothing.
    let reads_last = !off("sd_read_after_write");
    if !reads_last {
        beneficiary_read(gas, state, target);
        beneficiary_read(gas, state, me);
    }
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
    // The records the deletion makes: a loaded slot whose original is not zero and that still
    // holds it.
    let mut deleted = 0i64;
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
        let created_leaf = price_of(ext, GasId::CreateState);
        let code = price_of(ext, GasId::CodeDepositState) * code_len;
        // The leaf comes back when the transaction found the address absent at its start (Satin
        // `DestroyedParts::leaf`), at the new-account price, whichever charge materialized it: a
        // transfer, a destruction's balance or the creation itself.
        let (leaf_back, leaf) = if off("sd_absent_leaf") {
            (created_leaf != 0, created_leaf)
        } else if absent_at_start {
            let price = reprice(state.host(), GasId::NewAccountState, StateGasSite::Account(me));
            (true, price.map_err(|e| state.fail(evm2::HostError::from(e)))?)
        } else {
            (false, 0)
        };
        let fresh = slots.iter().filter(|(_, f)| *f).count() as u64;
        // A fresh slot was charged at its 0 -> x write, at the price the transaction's SALT cache
        // gives it again here.
        let slot_gas: core::result::Result<u64, _> = slots
            .iter()
            .filter(|(_, f)| *f)
            .map(|(k, _)| {
                reprice(state.host(), GasId::SstoreSetState, StateGasSite::Storage(me, *k))
            })
            .sum();
        let slot_gas = slot_gas.map_err(|e| state.fail(evm2::HostError::from(e)))?;
        // The records the table takes back: each slot made non-zero from zero, and the account's
        // when its leaf was absent at the start. A slot whose original is not zero keeps the
        // record its write made ("non-zero / any": the record stands).
        let records = fresh + (absent_at_start || off("sd_existing_record")) as u64;
        deleted = unchanged_original_slots(state, me)
            .map_err(|e| state.fail(evm2::HostError::from(e)))? as i64;
        give_back = [
            -((records * WRITE_RECORD_SIZE + code_len) as i64),
            -(records as i64),
            -((fresh + leaf_back as u64) as i64),
        ];
        refill_state = leaf + code + slot_gas;
        refill_bytes = records * WRITE_RECORD_SIZE + code_len;
        // A keyless deployment's creation destroying the account it creates: the give-back of
        // the account's own leaf and record, which the call charged at its step 14.
        let ext = state.host().ext_mut();
        let record =
            if ext.prices_history { (records - fresh) * WRITE_RECORD_SIZE * CPHB } else { 0 };
        if let Some(k) = ext.mechanisms.keyless.as_mut().filter(|k| k.deploy == me) {
            k.table_gave_back = leaf + record;
        }
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
    let made = me_delta + target_delta + deleted;
    let ds = made * WRITE_RECORD_SIZE as i64 + if moves { TRANSFER_LOG_SIZE as i64 } else { 0 };
    let delta = [ds + give_back[0], made + give_back[1], growth + give_back[2]];
    if let Some(stop) = state.host().ext_mut().draw(delta) {
        return stop_frame(gas, state, stop);
    }
    if eligible {
        ins::give_back_state_gas(gas, state, refill_state);
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
    let net = made;
    if net > 0 {
        charge_history(gas, state, net as u64 * WRITE_RECORD_SIZE, false)?;
    } else if net < 0 {
        refill_history(gas, state, (-net) as u64 * WRITE_RECORD_SIZE);
    }
    if reads_last {
        beneficiary_read(gas, state, target);
        beneficiary_read(gas, state, me);
    }
    Err(InstrStop::SelfDestruct)
}

/// The slots of `address`, an account created in the transaction, that the transaction loaded
/// while they held a value other than zero and that still hold it: the destruction deletes each
/// with the account, which makes its write record. Only a slot loaded before the creation can have
/// such an original (one first loaded after it reads zero), which no instruction reaches: the
/// account then has no code to run in, and an access list only pre-warms.
fn unchanged_original_slots(
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    address: Address,
) -> core::result::Result<u64, evm2::DatabaseError> {
    let s = state.host().state_mut();
    let keys: Vec<U256> = s
        .loaded_accounts()
        .find(|(a, _)| **a == address)
        .map(|(_, keys)| keys.copied().collect())
        .unwrap_or_default();
    let mut n = 0;
    for key in keys {
        let slot = s.storage_slot(&address, key)?;
        n += (!slot.original().is_zero() && slot.current() == slot.original()) as u64;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------------------------
// CALL / CREATE: start records, start charge, reservoir share, allowance, detention

/// A limit a frame meets as it resumes from a child (a creator's nonce record, a caller held
/// again by `hold_caller`): Satin counts it once the child's result merged into the frame, so the
/// frame resumes on the detention ledger first, then stops.
fn stop_on_resume(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    stop: (u8, u64),
) -> Result {
    if !off("resume_before_stop") {
        detention::on_resume(gas, state)?;
    }
    stop_frame(gas, state, stop)
}

/// The frame-start hook's charge of the start's records, out of what the caller kept. A caller
/// that cannot pay starts no frame: the forward is handed back first and the halt is recorded as
/// one that made no debit, on what the frame then has (Satin), so its burn is exact. Never a
/// crossing.
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
                gas.tracker_mut().record_no_debit_halt(with_forward);
            }
            Err(stop)
        }
    }
}

/// Whether a call opcode of `operands` operands is refused before it charges or checks anything
/// (`SATIN-MAC-003`, `SATIN-MAC-007`): in a frame whose volatile access is off, once its operands
/// are on the stack, when its target is the block beneficiary or delegates to it by EIP-7702, one
/// hop. The delegate is found by reading the target's account and code into the transaction's
/// state, so the read lands in its read set, which warms neither; the read is required, a failure
/// of it the transaction's (`SATIN-VAL-010`).
fn call_refused_up_front(
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    stack: &StackMut<'_>,
    operands: usize,
) -> Result<bool> {
    let target = stack.peek(1).map(|w| w.to_address()).unwrap_or_default();
    if refused_up_front(state, stack.len(), operands, &[target]) {
        return Ok(true);
    }
    if stack.len() < operands ||
        !crate::satin_ext::refused_here(state) ||
        under_reference_rule(state.host().ext(), ReferenceRule::RefusalAfterStaticGas) ||
        off("delegate_refusal")
    {
        return Ok(false);
    }
    let read: core::result::Result<_, evm2::DatabaseError> = (|| {
        let mut account = state.host().state_mut().account(&target)?;
        Ok(account.load_code()?.eip7702_address())
    })();
    let delegate = read.map_err(|e| state.fail(evm2::HostError::from(e)))?;
    Ok(delegate == Some(state.host().block().beneficiary))
}

fn mega_call_impl(
    stack: StackMut<'_>,
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    kind: MessageKind,
) -> Result {
    let (operands, static_gas) = match kind {
        MessageKind::Call => (7, const { static_gas(op::CALL) }),
        MessageKind::CallCode => (7, const { static_gas(op::CALLCODE) }),
        MessageKind::DelegateCall => (6, const { static_gas(op::DELEGATECALL) }),
        _ => (6, const { static_gas(op::STATICCALL) }),
    };
    if call_refused_up_front(state, &stack, operands)? {
        return refuse_up_front(gas, state);
    }
    gas.spend(static_gas)?;
    if refused_before_body(state, stack.peek(1)) {
        return revert_with(
            gas,
            state,
            &crate::satin_ext::disabled_revert(crate::satin_ext::access::BENEFICIARY),
        );
    }
    // Under the reference's rule (comparison only), evm2's body offers the load of the target, and
    // of its EIP-7702 delegate, to `before_account_load`, which refuses the block beneficiary's in
    // a frame whose volatile access is off.
    let kept = refusal_gas(gas, state);
    let result = call_with(
        stack,
        gas,
        state,
        kind,
        |gas, state, msg| {
            let mut start = StartRecords::default();
            // A call past the call-stack limit is refused for its depth before it moves value, so
            // it makes no record and is charged none, as a creation refused for its depth is not.
            let too_deep = msg.depth > max_call_depth(state.host());
            let transfers = !too_deep &&
                matches!(kind, MessageKind::Call | MessageKind::CallCode) &&
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
            // An account the transaction created and destroyed counts nothing more (Satin): a
            // value moved to it makes no record, so its start charge holds none
            // (`count_start` counts none for it either).
            let ext = state.host().ext();
            let records = start
                .accounts()
                .iter()
                .filter(|(a, c)| {
                    !c && *a != sender && (!ext.is_destroyed(a) || off("destroyed_start_charge"))
                })
                .count() as u64;
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
            #[cfg(feature = "compare")]
            regular_check::on_forward(state.host(), gas.tracker());
            Ok(())
        },
        |gas, state, msg, result| {
            let success = result.stop.is_success();
            let entry = state.host().ext_mut().pop_child(success);
            gas.set_reservoir(gas.reservoir() + entry.kept_reservoir);
            #[cfg(feature = "compare")]
            regular_check::on_merged(state.host(), msg, result, gas.tracker());
            #[cfg(not(feature = "compare"))]
            let _ = msg;
            if !success && entry.start_charge > 0 {
                refill_history(gas, state, entry.start_charge / CPHB);
            }
            if success && entry.takebacks > 0 {
                refill_history(gas, state, entry.takebacks * WRITE_RECORD_SIZE);
            }
            // Satin `hold_caller`: a failed child restored records a take-back had freed
            // while a hint it sent into that room stays: the caller resumes over a limit.
            if !success && let Some(stop) = crate::satin_ext::hold_caller(state.host().ext()) {
                return stop_on_resume(gas, state, stop);
            }
            Ok(())
        },
    );
    settle_refusal(gas, state, kept, result)?;
    detention::on_resume(gas, state)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, hooked)]
pub fn mega_call(cx: _) -> Result {
    mega_call_impl(stack, cx.gas, cx.state, MessageKind::Call)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, hooked)]
pub fn mega_callcode(cx: _) -> Result {
    mega_call_impl(stack, cx.gas, cx.state, MessageKind::CallCode)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, hooked)]
pub fn mega_delegatecall(cx: _) -> Result {
    mega_call_impl(stack, cx.gas, cx.state, MessageKind::DelegateCall)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, hooked)]
pub fn mega_staticcall(cx: _) -> Result {
    mega_call_impl(stack, cx.gas, cx.state, MessageKind::StaticCall)
}

fn mega_create_impl(
    stack: StackMut<'_>,
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    is_create2: bool,
) -> Result {
    if is_create2 && stack.len() == 3 && !off("create2_salt_order") {
        return create2_short_of_salt(stack, gas, state);
    }
    let creator = state.message().destination;
    let creator_changed = core::cell::Cell::new(false);
    create_with(
        stack,
        gas,
        state,
        is_create2,
        |gas, state, msg| {
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
            // A creation onto an account the transaction created and destroyed makes no record of
            // it (Satin: the account counts nothing more).
            let start_records = (!dest_changed &&
                (!state.host().ext().is_destroyed(&msg.destination) ||
                    off("destroyed_start_charge"))) as u64;
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
            #[cfg(feature = "compare")]
            regular_check::on_forward(state.host(), gas.tracker());
            Ok(())
        },
        |gas, state, msg, result| {
            let success = result.stop.is_success() && result.created_address.is_some();
            let entry = state.host().ext_mut().pop_child(success);
            gas.set_reservoir(gas.reservoir() + entry.kept_reservoir);
            #[cfg(feature = "compare")]
            regular_check::on_merged(state.host(), msg, result, gas.tracker());
            #[cfg(not(feature = "compare"))]
            let _ = msg;
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
                return stop_on_resume(gas, state, stop);
            }
            if !success && let Some(stop) = crate::satin_ext::hold_caller(state.host().ext()) {
                return stop_on_resume(gas, state, stop);
            }
            Ok(())
        },
    )?;
    detention::on_resume(gas, state)
}

/// A CREATE2 on three stack items, short of its salt, in Satin's order (`SATIN-SIZE-005`): the
/// static context, the value, offset and size operands and the size's range, the initcode size
/// against `MAX_INITCODE_SIZE`, EIP-3860's initcode cost, and the offset's range and the memory
/// (neither for empty initcode) come before the salt operand, so the frame pays the initcode cost
/// and the memory, or fails on the first it cannot pay, before it fails on the missing salt
/// (`SATIN-SIZE-006`). evm2's `create_with` pops the salt before those charges. The halt consumes
/// the frame's gas either way, but under gas detention one of the charges can be the crossing,
/// which stops the frame with a revert where the stack underflow would halt it.
///
/// The steps are `create_with`'s own, up to its salt; with a salt on the stack, `create_with` runs.
fn create2_short_of_salt(
    mut stack: StackMut<'_>,
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
) -> Result {
    if state.is_static() {
        return Err(InstrStop::StateChangeDuringStaticCall);
    }
    let [_value, offset, len] = stack.popn::<3>()?;
    let len = len.to_usize()?;
    if state.feature(EvmFeatures::EIP3860) {
        if len > state.version().max_initcode_size {
            return Err(InstrStop::CreateInitCodeSizeLimit);
        }
        gas.spend(state.gas_params().initcode_cost(len))?;
    }
    if len != 0 {
        state.resize_memory(gas, offset.to_usize()?, len)?;
    }
    Err(InstrStop::StackUnderflow)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, hooked)]
pub fn mega_create(cx: _) -> Result {
    mega_create_impl(stack, cx.gas, cx.state, false)
}

#[instruction(EvmTypes = MegaTypes, no_stack_preamble, hooked)]
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

pub(crate) fn is_precompile(evm: &mut Evm<'_, MegaTypes>, msg: &Message<MegaTypes>) -> bool {
    evm.precompiles().contains(&msg.call_target)
}

/// The deepest call depth at which evm2 runs a message: its call-stack limit below the initial
/// depth, as evm2 measures it (its own `max_call_depth` is private).
fn max_call_depth(evm: &Evm<'_, MegaTypes>) -> u16 {
    evm.initial_call_depth() + evm2::constants::CALL_DEPTH_LIMIT
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
        // A message past the call-stack limit reaches no interceptor, counts no hint and is held
        // by no limit: evm2 answers it with `CallTooDeep`, its forward and reservoir share
        // returned whole. The depth gate comes before every other decision about the message.
        if msg.depth > max_call_depth(evm) {
            return Ok(None);
        }
        // A transaction whose sender or recipient is the block beneficiary, or whose
        // recipient delegates to it, reads volatile data from its start: detained at compute 0.
        // A creation has no recipient: its created address is no recipient even when it is the
        // beneficiary, and its loads of it are the creation's own checks, not volatile reads.
        if msg.depth == 0 && !off("tx_beneficiary") {
            let beneficiary = evm.block().beneficiary;
            let recipient = !msg.kind.is_create() || off("creation_no_recipient");
            let ext = evm.ext_mut();
            if (msg.caller == beneficiary ||
                recipient && (msg.call_target == beneficiary || msg.code_address == beneficiary)) &&
                detention::detains(ext) &&
                ext.detention.cap != u64::MAX
            {
                let cap = ext.detention.cap;
                ext.detention.limit = Some(ext.detention.limit.map_or(cap, |l| l.min(cap)));
            }
        }
        if msg.depth == 0 &&
            !off("pre_frame_stop") &&
            let Some(stop) = evm.ext_mut().mechanisms.pre_frame_stop.take()
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
        if let Some(answer) = intercept::answer_before(evm, msg) {
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
        // that draws nothing (a deposit's created caller over a growth limit of nothing). The
        // first frame's start is drawn and held in one step, dimension by dimension
        // (`draw_first_frame`), as an answered first frame's is in `before_commit`.
        let crossing = if msg.depth == 0 && !off("first_frame_hold") {
            let start = evm.ext_mut().tx_start_pending.take().unwrap_or_default();
            evm.ext_mut().draw_first_frame(start)
        } else if msg.depth == 0 {
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
        #[cfg(feature = "compare")]
        regular_check::on_frame_built(evm, msg, gas);
    }

    fn before_commit(
        evm: &mut Evm<'_, MegaTypes>,
        msg: &Message<MegaTypes>,
        stop: &mut InstrStop,
        output: &mut Bytes,
        gas: &mut GasTracker,
    ) -> core::result::Result<(), evm2::ExecutionError> {
        // A creation's deposit is over once its frame ends: a loan its admission did not settle
        // (the deposit failed before admission ran) is dropped, so it never reaches the next
        // deposit's refusal.
        if !off("deposit_loan_reset") {
            evm.ext_mut().deposit_loan.drop_unsettled();
        }
        let answered = is_answered(evm, msg);
        detention::before_commit(evm, msg, answered, stop, output, gas);
        // A frame's return switches back on what it (or a frame below it) switched off.
        if evm.ext().mechanisms.disabled_from.is_some_and(|from| from >= msg.depth as usize) {
            evm.ext_mut().mechanisms.disabled_from = None;
        }
        if msg.depth == 0 {
            // The first frame answered without running (a call to an account with no code, a
            // precompile that answers): its start is drawn now and held as a built frame's is, a
            // success giving way to the stop; a failure kept nothing and is not stopped.
            let start = evm.ext_mut().tx_start_pending.take();
            let held = |evm: &mut Evm<'_, MegaTypes>| {
                if off("first_frame_hold") {
                    start.and_then(|d| evm.ext_mut().draw(d))
                } else {
                    evm.ext_mut().draw_first_frame(start.unwrap_or_default())
                }
            };
            if answered &&
                stop.is_success() &&
                let Some(cross) = held(evm)
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

    /// Every message's end once its gas is settled, before its caller merges it: the regular-gas
    /// invariant and the conservation check hold it to what it received and observe a followed
    /// frame one last time (comparison builds only).
    fn after(
        evm: &mut Evm<'_, MegaTypes>,
        msg: &Message<MegaTypes>,
        result: &mut evm2::interpreter::MessageResult<MegaTypes>,
    ) -> core::result::Result<(), evm2::ExecutionError> {
        #[cfg(feature = "compare")]
        regular_check::on_message_end(evm, msg, result);
        let _ = (evm, msg, result);
        Ok(())
    }

    fn admit_code_deposit(
        evm: &mut Evm<'_, MegaTypes>,
        _msg: &Message<MegaTypes>,
        code: &Bytes,
        _gas_before: &GasTracker,
        gas: &mut Gas,
    ) -> core::result::Result<Option<(InstrStop, Bytes)>, evm2::ExecutionError> {
        #[cfg(feature = "compare")]
        regular_check::on_code_deposit(evm, _msg, gas.tracker());
        if keyless::refuses_empty_deposit(evm, _msg, code) {
            return Ok(Some((InstrStop::Revert, Bytes::new())));
        }
        // The deposited code's history (W first, as its state spill was), then its bytes drawn on
        // the creation's budget.
        let len = code.len() as u64;
        if evm.ext().prices_history && len > 0 {
            let amount = len * CPHB;
            let spill = amount.saturating_sub(gas.reservoir());
            if !off("deposit_site") {
                let ext = evm.ext_mut();
                let drawn = detention::draw_withheld(ext, spill);
                gas.erase_cost(drawn);
                ext.deposit_loan.lend(drawn);
            }
            if gas.spend_state(amount).is_err() {
                // The refusal keeps the charged gas: what the deposit drew is spent.
                evm.ext_mut().deposit_loan.settle();
                return Ok(Some((InstrStop::OutOfGas, Bytes::new())));
            }
            evm.ext_mut().book_history(amount as i64, len as i64);
        }
        let loan = evm.ext_mut().deposit_loan.settle();
        if let Some(cross) = evm.ext_mut().draw([len as i64, 0, 0]) {
            // Under EIP-8037's rule the refusal restores the frame's gas from before the deposit,
            // which does not hold what the deposit drew: the withheld gas takes it back. Under
            // Satin's the charges stay and the revert gives the state and history into the
            // reservoir, the drawn gas among them, so the withheld gas does not get it back.
            if !off("deposit_loan_back") &&
                !reservoir_refills(evm) &&
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every EIP-8037 state-gas entry is rebuilt at the cost per state byte, the delegation
    /// indicator's 23 bytes included: Amsterdam builds that entry at its own 1,530 per byte, which
    /// only equals Satin's while the two costs per state byte are equal.
    #[test]
    fn test_every_state_gas_entry_is_rebuilt_at_the_cost_per_state_byte() {
        let p = satin_version_at(1_000).gas_params;
        assert_eq!(p.get(GasId::SstoreSetState), 64_000);
        assert_eq!(p.get(GasId::NewAccountState), 120_000);
        assert_eq!(p.get(GasId::CreateState), 120_000);
        assert_eq!(p.get(GasId::CodeDepositState), 1_000);
        assert_eq!(p.get(GasId::TxEip7702PerAuthState), 23_000);
        // Satin's own: 23 × 1,530.
        assert_eq!(satin_version().gas_params.get(GasId::TxEip7702PerAuthState), 35_190);
    }

    /// `SATIN-STATE-012`'s shares at the schedule's current prices: a fresh slot 20,000 − 10,000,
    /// a value call's or a keyless signer's new account 25,000 − 0, a `SELFDESTRUCT`'s new
    /// beneficiary 25,000 − 9,000 and a created account 32,000 − 12,000, flat; give-backs credit
    /// reservoirs only (`SATIN-HIST-012`).
    #[test]
    fn test_satin_takes_the_legacy_shares_and_gives_back_into_reservoirs() {
        let v = satin_version();
        assert!(v.feature(EvmFeatures::EIP8037_RESERVOIR_REFILLS));
        let p = v.gas_params;
        assert_eq!(p.get(GasId::SstoreSetStateLegacyShare), 10_000);
        assert_eq!(p.get(GasId::CallNewAccountStateLegacyShare), 25_000);
        assert_eq!(p.get(GasId::SelfdestructNewAccountStateLegacyShare), 16_000);
        assert_eq!(p.get(GasId::CreateStateLegacyShare), 20_000);
        // Flat: a cost per state byte that moves the state charges leaves the shares.
        let q = satin_version_at(1_000).gas_params;
        assert_eq!(q.get(GasId::SstoreSetStateLegacyShare), 10_000);
        assert_eq!(q.get(GasId::CreateStateLegacyShare), 20_000);
    }
}
