//! Gas detention using consumer-owned withheld gas, the thin evm2 fork's failed-charge records,
//! static message hooks and CALL/CREATE hooks. No evm2 gas-core split.
//!
//! The rule is Satin's (`access/detention.rs`):
//!
//! ```text
//! compute = Σ suspended callers (spent at suspension − gas forwarded)
//!         + regular_spent(running frame) + Σ stipends of returned value calls − burned
//! regular_spent(gas, self) = limit − (spendable + withheld) − state/history gas spilled
//! ```
//!
//! A read sets `limit = compute + cap` (lowest binds); every frame is held at
//! `spendable = limit − compute` at its start, at every resume, after the read and after SSTORE;
//! a regular charge the spendable part cannot pay but the whole gas can is the crossing, and the
//! frame's out-of-gas end is classified as the stop `MegaLimitExceeded(2, limit)` with its gas put
//! back to the record. Ported: block-environment reads through TIMESTAMP and NUMBER only;
//! precompile prices, interceptor answers, the Oracle and the access switch are not.

use crate::{MegaTypes, ledger::*};
use alloy_primitives::{Bytes, U256};
use core::marker::PhantomData;
use evm2::{
    Evm, OpcodeConfig,
    interpreter::{
        Gas, GasTracker, InstrStop, InterpreterState, Message, Pc, Result, StackMut, instructions,
        op,
        private::{Instruction, split_gas_state},
    },
};

/// Detention's per-transaction state (Satin `access/detention.rs`, `FrameFigures`).
#[derive(Clone, Debug, Default)]
pub struct DetentionState {
    pub limit: Option<u64>,
    pub cap: u64,
    pub oracle_cap: u64,
    pub frames: Vec<DFrame>,
    pub suspended: u64,
    pub stipends: u64,
    pub burned: u64,
    pub crossings: u64,
    /// Precompile calls decided from their input-only price (held, crossing) for the report.
    #[cfg(feature = "compare")]
    pub priced: Vec<(u64, u64, bool)>,
}

impl DetentionState {
    /// Per-transaction reset in place (the frame stack keeps its capacity).
    pub fn reset(&mut self, cap: u64, oracle_cap: u64) {
        self.limit = None;
        self.cap = cap;
        self.oracle_cap = oracle_cap;
        self.frames.clear();
        self.suspended = 0;
        self.stipends = 0;
        self.burned = 0;
        self.crossings = 0;
        #[cfg(feature = "compare")]
        self.priced.clear();
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DFrame {
    /// Withheld regular gas, stored outside evm2's gas core.
    pub withheld: u64,
    pub at_suspension: u64,
    pub child_stipend: u64,
    pub contribution: u64,
}

/// The regular gas a frame spent: its limit, less what it has left (both parts), less the state
/// and history gas that spilled onto its regular gas.
pub fn available(gas: &GasTracker, det: &DetentionState) -> u64 {
    gas.remaining() + det.frames.last().map_or(0, |f| f.withheld)
}

pub fn regular_spent(gas: &GasTracker, det: &DetentionState) -> u64 {
    gas.limit().saturating_sub(available(gas, det)).saturating_sub(gas.state_gas_spilled())
}

impl DetentionState {
    pub fn compute(&self, gas: &GasTracker) -> u64 {
        self.suspended
            .saturating_add(regular_spent(gas, self))
            .saturating_add(self.stipends)
            .saturating_sub(self.burned)
    }

    pub fn hold(&mut self, gas: &mut GasTracker) {
        if crate::off("hold") {
            return;
        }
        if let Some(limit) = self.limit {
            let total = available(gas, self);
            let spendable = total.min(limit.saturating_sub(self.compute(gas)));
            if let Some(frame) = self.frames.last_mut() {
                frame.withheld = total - spendable;
            }
            gas.set_remaining(spendable);
        }
    }

    pub fn caller_contribution(&self, depth: usize, gas_limit: u64) -> u64 {
        depth.checked_sub(1).and_then(|i| self.frames.get(i)).map_or(0, |c| {
            c.at_suspension.saturating_sub(gas_limit.saturating_sub(c.child_stipend))
        })
    }

    /// Satin's `FrameFigures::on_frame_run`: a first run adds the caller's contribution, a resume
    /// takes it back out and counts the returned stipend.
    pub fn on_frame_run(&mut self, gas_limit: u64, depth: usize) {
        if crate::debug() {
            eprintln!(
                "RUN depth={depth} gas_limit={gas_limit} frames={} suspended={} stipends={} burned={}",
                self.frames.len(),
                self.suspended,
                self.stipends,
                self.burned
            );
        }
        if self.frames.len() == depth {
            let contribution = self.caller_contribution(depth, gas_limit);
            if let Some(caller) = depth.checked_sub(1).and_then(|i| self.frames.get_mut(i)) {
                caller.contribution = contribution;
            }
            self.suspended = self.suspended.saturating_add(contribution);
            self.frames.push(DFrame::default());
        } else if let Some(frame) = self.frames.get_mut(depth) {
            self.suspended = self.suspended.saturating_sub(frame.contribution);
            self.stipends = self.stipends.saturating_add(frame.child_stipend);
            frame.contribution = 0;
            frame.child_stipend = 0;
        }
    }

    pub fn on_frame_suspend(&mut self, gas: &GasTracker, depth: usize, stipend: u64) {
        let spent = regular_spent(gas, self);
        if let Some(frame) = self.frames.get_mut(depth) {
            frame.at_suspension = spent;
            frame.child_stipend = stipend;
        }
    }
}

pub fn detains(ext: &Lite) -> bool {
    ext.detention.cap != u64::MAX || ext.detention.oracle_cap != u64::MAX
}

/// Commits a read of volatile data once its opcode completed (Satin `commit_reads`): the limit is
/// the compute at the read plus `cap` (lowest binds), less a forward the opcode already took for
/// a child it will start; the frame is held.
pub fn commit_read(ext: &mut Lite, gas: &mut GasTracker, cap: u64, forwarded: u64) {
    if !detains(ext) || cap == u64::MAX {
        return;
    }
    let det = &mut ext.detention;
    let limit = det.compute(gas).saturating_sub(forwarded).saturating_add(cap);
    det.limit = Some(det.limit.map_or(limit, |l| l.min(limit)));
    if forwarded == 0 {
        det.hold(gas);
    }
}

// ---------------------------------------------------------------------------------------------
// Opcode side

/// A volatile read: the base opcode, then the read is committed (the limit set) and the frame
/// held. Built on evm2's `#[doc(hidden)]` `Instruction` trait.
pub struct Volatile<I, const A: u8>(PhantomData<I>);

impl<I: Instruction<MegaTypes>, const A: u8> Instruction<MegaTypes> for Volatile<I, A> {
    const DYNAMIC_GAS: bool = true;

    fn execute(
        pc: &mut Pc,
        stack: StackMut<'_>,
        state: &mut InterpreterState<'_, '_, MegaTypes>,
    ) -> Result {
        // `MegaAccessControl` switched access off at or above this frame: the read is refused
        // and the frame reverts on the gas it had after the opcode's static gas.
        if crate::satin_ext::refused_here(state) {
            let (gas, state) = unsafe { split_gas_state(state) };
            return crate::ledger::revert_with(gas, state, &crate::satin_ext::disabled_revert(A));
        }
        I::execute(pc, stack, state)?;
        // SAFETY: `gas` is not reached through `state` while both are used.
        let (gas, state) = unsafe { split_gas_state(state) };
        let ext = state.host().ext_mut();
        if detains(ext) {
            let det = &mut ext.detention;
            if crate::debug() {
                eprintln!(
                    "READ compute={} suspended={} regular_spent={} stipends={} burned={} limit_before={:?} gas.limit={} rem={} spill={}",
                    det.compute(gas.tracker()),
                    det.suspended,
                    regular_spent(gas.tracker(), det),
                    det.stipends,
                    det.burned,
                    det.limit,
                    gas.tracker().limit(),
                    gas.tracker().remaining(),
                    gas.tracker().state_gas_spilled()
                );
            }
            let limit = det.compute(gas.tracker()).saturating_add(det.cap);
            det.limit = Some(det.limit.map_or(limit, |l| l.min(limit)));
            det.hold(gas.tracker_mut());
        }
        Ok(())
    }
}

pub const fn wrap_table(mut c: OpcodeConfig<MegaTypes>) -> OpcodeConfig<MegaTypes> {
    c.set_instruction::<total_gas>(op::GAS, c.static_gas(op::GAS));
    c.set_instruction::<Volatile<instructions::coinbase<MegaTypes>, { crate::satin_ext::access::COINBASE }>>(op::COINBASE, c.static_gas(op::COINBASE));
    c.set_instruction::<Volatile<instructions::difficulty<MegaTypes>, { crate::satin_ext::access::PREV_RANDAO }>>(op::DIFFICULTY, c.static_gas(op::DIFFICULTY));
    c.set_instruction::<Volatile<instructions::gaslimit<MegaTypes>, { crate::satin_ext::access::GAS_LIMIT }>>(op::GASLIMIT, c.static_gas(op::GASLIMIT));
    c.set_instruction::<Volatile<instructions::basefee<MegaTypes>, { crate::satin_ext::access::BASE_FEE }>>(op::BASEFEE, c.static_gas(op::BASEFEE));
    c.set_instruction::<Volatile<instructions::blobbasefee<MegaTypes>, { crate::satin_ext::access::BLOB_BASE_FEE }>>(op::BLOBBASEFEE, c.static_gas(op::BLOBBASEFEE));
    c.set_instruction::<Volatile<instructions::slotnum<MegaTypes>, { crate::satin_ext::access::SLOT_NUM }>>(op::SLOTNUM, c.static_gas(op::SLOTNUM));
    c.set_instruction::<Volatile<instructions::blockhash<MegaTypes>, { crate::satin_ext::access::BLOCK_HASH }>>(op::BLOCKHASH, c.static_gas(op::BLOCKHASH));
    c.set_instruction::<Volatile<instructions::timestamp<MegaTypes>, { crate::satin_ext::access::TIMESTAMP }>>(op::TIMESTAMP, c.static_gas(op::TIMESTAMP));
    c.set_instruction::<Volatile<instructions::block_number<MegaTypes>, { crate::satin_ext::access::BLOCK_NUMBER }>>(op::NUMBER, c.static_gas(op::NUMBER));
    c
}

/// After SSTORE: a restore can refill regular gas, so the frame is held again.
pub fn after_sstore(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>) -> Result {
    let ext = state.host().ext_mut();
    if detains(ext) {
        ext.detention.hold(gas.tracker_mut());
    }
    Ok(())
}

/// The caller suspends on a child (the CALL/CREATE `before` hook, after the forward is taken).
pub fn on_suspend(gas: &Gas, state: &mut InterpreterState<'_, '_, MegaTypes>, stipend: u64) {
    let depth = state.message().depth as usize;
    let ext = state.host().ext_mut();
    if detains(ext) {
        ext.detention.on_frame_suspend(gas.tracker(), depth, stipend);
    }
}

/// The caller resumes on what its child handed back: held again.
pub fn on_resume(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>) -> Result {
    let depth = state.message().depth as usize;
    let ext = state.host().ext_mut();
    if detains(ext) {
        ext.detention.on_frame_run(gas.limit(), depth);
        ext.detention.hold(gas.tracker_mut());
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Frame side (message hooks)

/// A frame was built (`frame_built` hook): its first run, held before its first instruction.
pub fn on_frame_built(
    evm: &mut Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    gas: &mut GasTracker,
) {
    let ext = evm.ext_mut();
    if detains(ext) {
        ext.detention.on_frame_run(msg.gas_limit, msg.depth as usize);
        ext.detention.hold(gas);
    }
}

pub fn on_start(_evm: &mut Evm<'_, MegaTypes>, _msg: &Message<MegaTypes>) {}

/// A frame that ran ends (`before_commit` hook, before evm2's commit-or-rollback decision): an
/// out-of-gas whose gas carries a crossing record is the stop; any other halt burns.
pub fn before_commit(
    evm: &mut Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    answered: bool,
    stop: &mut InstrStop,
    output: &mut Bytes,
    gas: &mut GasTracker,
) {
    let ext = evm.ext_mut();
    if !detains(ext) {
        return;
    }
    // A result for a message whose frame was never built (an answer, or a start evm2 refused:
    // `OutOfFunds`, depth) has no side-ledger entry: the top entry is its caller's.
    let built = det_built(&ext.detention, msg) || crate::off("built_check");
    if answered || !built {
        // A precompile (or interceptor) answer that halts burns its whole forward, stipend
        // included: none of it is compute.
        if stop.is_halt() {
            ext.detention.burned = ext.detention.burned.saturating_add(msg.gas_limit);
        }
        return;
    }
    let det = &mut ext.detention;
    let withheld = det.frames.pop().map_or(0, |f| f.withheld);
    if !stop.is_halt() {
        gas.erase_cost(withheld);
        return;
    }
    if stop.is_out_of_gas() &&
        !crate::off("classify") &&
        let Some((remaining, cost)) = gas.failed_charge() &&
        cost <= remaining.saturating_add(withheld) &&
        let Some(limit) = det.limit
    {
        gas.clear_failed_charge();
        gas.set_remaining(remaining + withheld);
        det.crossings += 1;
        *stop = InstrStop::Revert;
        *output = limit_exceeded_output(2, limit).into();
        if msg.depth == 0 {
            ext.top_stop = Some((2, limit));
        }
        return;
    }
    let left = gas
        .failed_charge()
        .map_or(gas.remaining(), |(remaining, _)| remaining)
        .saturating_add(withheld);
    if crate::debug() {
        eprintln!(
            "BURN depth={} stop={:?} failed={:?} rem={} withheld={} spilled={} limit={} burned_before={}",
            msg.depth,
            stop,
            gas.failed_charge(),
            gas.remaining(),
            withheld,
            gas.state_gas_spilled(),
            gas.limit(),
            det.burned
        );
    }
    det.burned = det.burned.saturating_add(left).saturating_add(gas.state_gas_spilled());
}

#[evm2_macros::instruction(EvmTypes = MegaTypes, dynamic_gas)]
pub fn total_gas(cx: _) -> out {
    let total = available(cx.gas.tracker(), &cx.state.host().ext().detention);
    *out = evm2::interpreter::Word::from(if crate::off("gas_total") {
        cx.gas.remaining()
    } else {
        total
    });
}

/// Recomputes the forward from spendable + withheld and debits withheld first.
pub fn repair_forward(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    msg: &mut Message<MegaTypes>,
    requested: u64,
) {
    let stipend = if !msg.value.is_zero() &&
        matches!(
            msg.kind,
            evm2::interpreter::MessageKind::Call | evm2::interpreter::MessageKind::CallCode
        ) {
        2300
    } else {
        0
    };
    let naive = msg.gas_limit.saturating_sub(stipend);
    let before = gas.remaining() + naive;
    let w = state.host().ext().detention.frames.last().map_or(0, |f| f.withheld);
    if w == 0 || crate::off("forward") {
        return;
    }
    let total = before + w;
    let forward = state.gas_params().call_stipend_reduction(total).min(requested);
    let drawn = forward.min(w);
    state.host().ext_mut().detention.frames.last_mut().unwrap().withheld -= drawn;
    gas.set_remaining(before - (forward - drawn));
    msg.gas_limit = forward + stipend;
}

/// SSTORE's sentry/load checks see the full budget; regular charges still draw spendable gas.
pub fn sstore_body(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    key: &evm2::interpreter::Word,
    value: &evm2::interpreter::Word,
) -> Result {
    use evm2::{EvmFeatures, interpreter::Host, version::GasId};
    if state.is_static() {
        return Err(InstrStop::StateChangeDuringStaticCall);
    }
    let eip2200 = state.feature(EvmFeatures::EIP2200);
    if eip2200 &&
        available(gas.tracker(), &state.host().ext().detention) <=
            state.gas_params().get(GasId::CallStipend)
    {
        no_debit_halt(gas);
        return Err(InstrStop::ReentrancySentryOOG);
    }
    gas.spend(state.gas_params().get(GasId::SstoreStatic))?;
    let cold = state.gas_params().get(GasId::ColdStorageAdditionalCost);
    let skip = state.feature(EvmFeatures::EIP2929) &&
        available(gas.tracker(), &state.host().ext().detention) < cold;
    let dest = state.message().destination;
    let load = state.host().sstore(&dest, key, value, skip).map_err(|e| state.fail(e))?;
    gas.spend(state.gas_params().sstore_dynamic_gas(eip2200, &load))?;
    if state.feature(EvmFeatures::EIP8037) {
        // The slot's SALT price, captured for the 0 -> x -> 0 refill too.
        let site = evm2::version::StateGasSite::Storage(dest, *key);
        if state.gas_params().sstore_state_gas(&load) != 0 {
            let price = <MegaTypes as evm2::EvmTypesHost>::state_gas_price(
                state.host(),
                GasId::SstoreSetState,
                site,
            )
            .map_err(|e| state.fail(e))?;
            spend_state_withheld(gas, state, price)?;
        }
        if state.gas_params().sstore_state_gas_refill(&load) != 0 {
            let price = <MegaTypes as evm2::EvmTypesHost>::state_gas_price(
                state.host(),
                GasId::SstoreSetState,
                site,
            )
            .map_err(|e| state.fail(e))?;
            gas.refill_reservoir(price);
        }
    }
    gas.record_refund(state.gas_params().sstore_refund(eip2200, &load));
    crate::sstore_completed(gas, state, key, load)
}

// ---------------------------------------------------------------------------------------------
// Code deposits and precompiles

/// The code deposit (code-deposit pre-charge hook, before evm2's deposit precheck and charges):
/// the hash
/// charge is a regular charge on the spendable part (a crossing if S < cost <= S + W, recorded
/// as a failed charge so `before_commit` classifies it); the deposit's state spill draws W first.
pub fn before_code_deposit(
    evm: &mut Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    code: &Bytes,
    gas: &mut Gas,
) -> core::result::Result<(), evm2::HostError> {
    if !detains(evm.ext()) || evm.ext().detention.frames.last().is_none_or(|f| f.withheld == 0) {
        return Ok(());
    }
    let params = &evm.version().gas_params;
    let regular = (code.len() as u64)
        .saturating_mul(params.get(evm2::version::GasId::CodeDepositCost))
        .saturating_add(params.keccak256_word_cost(code.len()));
    if gas.remaining() < regular {
        let before = gas.remaining();
        gas.tracker_mut().record_failed_charge(before, regular);
        return Err(InstrStop::OutOfGas.into());
    }
    // Empty code prices nothing and reads no bucket (Satin prices no empty deposit).
    let price = if code.is_empty() && !crate::off("empty_deposit_price") {
        0
    } else {
        <MegaTypes as evm2::EvmTypesHost>::state_gas_price(
            evm,
            evm2::version::GasId::CodeDepositState,
            evm2::version::StateGasSite::Account(msg.destination),
        )
        .map_err(|_| evm2::HostError::from(InstrStop::OutOfGas))?
    };
    let state_cost = price.saturating_mul(code.len() as u64);
    let spill = state_cost.saturating_sub(gas.reservoir());
    let ext = evm.ext_mut();
    if let Some(frame) = ext.detention.frames.last_mut() {
        let loan = spill.min(frame.withheld);
        frame.withheld -= loan;
        gas.erase_cost(loan);
        // An admission refusal restores evm2's pre-hook snapshot of the frame's gas, which does
        // not hold this loan: the side ledger takes it back then (`admit_code_deposit`).
        ext.deposit_loan = loan;
    }
    Ok(())
}

fn det_built(det: &DetentionState, msg: &Message<MegaTypes>) -> bool {
    det.frames.len() == msg.depth as usize + 1
}

/// Input-only price catalog of the Satin set that the scenarios reach (Satin
/// `PricedPrecompiles`).
pub fn precompile_price(address: &alloy_primitives::Address, input: &[u8]) -> Option<u64> {
    let words = (input.len() as u64).div_ceil(32);
    let last = address.0[19];
    if address.0[..18] != [0u8; 18] || (address.0[18] != 0 && !(address.0[18] == 1 && last == 0)) {
        return None;
    }
    if address.0[18] == 1 {
        return Some(6_900); // P256VERIFY (Osaka)
    }
    match last {
        1 => Some(3_000),
        2 => Some(60 + 12 * words),
        3 => Some(600 + 120 * words),
        4 => Some(15 + 3 * words),
        6 => Some(150),
        7 => Some(6_000),
        8 => Some(45_000 + 34_000 * (input.len() as u64 / 192)),
        9 => input.get(..4).map(|r| u32::from_be_bytes(r.try_into().unwrap()) as u64),
        10 => Some(100_000),
        _ => None,
    }
}

/// Satin's `hold_precompile` (`before` message hook, before evm2 runs the precompile): priced past
/// the
/// allowance and within the forward, the call is answered with the compute stop without running;
/// the caller gets its whole forward back. A value call whose caller cannot fund it is not held.
pub fn hold_precompile(
    evm: &mut Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    is_precompile: bool,
) -> Option<evm2::interpreter::MessageResult<MegaTypes>> {
    use evm2::interpreter::MessageKind;
    if !is_precompile || !detains(evm.ext()) || msg.depth == 0 {
        return None;
    }
    if !matches!(
        msg.kind,
        MessageKind::Call |
            MessageKind::CallCode |
            MessageKind::StaticCall |
            MessageKind::DelegateCall
    ) {
        return None;
    }
    let limit = evm.ext().detention.limit?;
    let price = precompile_price_of(evm, msg)?;
    if !msg.value.is_zero() && matches!(msg.kind, MessageKind::Call | MessageKind::CallCode) {
        let balance = evm
            .state_mut()
            .account_info_untracked(&msg.caller)
            .ok()
            .flatten()
            .map_or(U256::ZERO, |i| i.balance);
        if balance < msg.value {
            return None;
        }
    }
    let det = &evm.ext().detention;
    let depth = msg.depth as usize;
    let contribution = det.caller_contribution(depth, msg.gas_limit);
    let at_start = det
        .suspended
        .saturating_add(contribution)
        .saturating_add(det.stipends)
        .saturating_sub(det.burned);
    let allowance = limit.saturating_sub(at_start).min(msg.gas_limit);
    let crosses = price > allowance && price <= msg.gas_limit;
    #[cfg(feature = "compare")]
    evm.ext_mut().detention.priced.push((price, allowance, crosses));
    if !crosses {
        return None;
    }
    evm.ext_mut().detention.crossings += 1;
    Some(evm2::interpreter::MessageResultExt {
        stop: InstrStop::Revert,
        output: limit_exceeded_output(2, limit).into(),
        gas: GasTracker::new_with_execution_gas_and_reservoir(msg.gas_limit, msg.reservoir),
        ..Default::default()
    })
}

/// A halt that attempts no debit (the SSTORE sentry, a skipped cold load) leaves no failed-charge
/// record, and
/// evm2's inspected path zeroes `remaining` on every out-of-gas-class halt (`inspect_step_end`),
/// so what the frame had is lost there. Record it as a failed charge no gas could pay: never a
/// crossing, and the burn reads it on both paths.
pub fn no_debit_halt(gas: &mut Gas) {
    if crate::off("no_debit_record") {
        return;
    }
    let before = gas.remaining();
    gas.tracker_mut().record_failed_charge(before, u64::MAX);
}

/// The full price catalogue. An entry with an input-only quote is priced by it; every other entry
/// of
/// the Satin set (ModExp on the Osaka schedule, the BN254 pairing and the BLS12-381 MSMs and
/// pairing behind op-evm2's Karst size caps, BLAKE2F, the other BLS12-381 entries, P256VERIFY)
/// is priced by running it on a tracker with no gas: its first charge fails and the failed-charge
/// record
/// holds the charge (`GasTracker::failed_charge`), before any of its work; an input it refuses
/// before any gas check (over a size cap, a malformed length) is priced at nothing, as Satin's
/// op-revm fork prices it.
pub fn precompile_price_of(evm: &mut Evm<'_, MegaTypes>, msg: &Message<MegaTypes>) -> Option<u64> {
    let p = crate::precompile_map().get(&msg.code_address)?;
    if let Some(price) = p.required_gas(&msg.input) {
        return Some(price);
    }
    if crate::off("precompile_catalog") {
        return None;
    }
    let mut probe = GasTracker::new_with_execution_gas_and_reservoir(0, 0);
    let _ = (p.run())(evm, msg, &mut probe);
    let first = probe.failed_charge().map_or(0, |(_, cost)| cost);
    // evm2's ModExp charges its 500 minimum, then the rest (`modexp.rs:240`); every
    // other entry charges once. A second probe funded with the first charge reads the rest.
    if msg.code_address == alloy_primitives::Address::with_last_byte(5) &&
        first > 0 &&
        !crate::off("modexp_two_step")
    {
        let mut probe = GasTracker::new_with_execution_gas_and_reservoir(first, 0);
        let _ = (p.run())(evm, msg, &mut probe);
        return Some(first + probe.failed_charge().map_or(0, |(_, cost)| cost));
    }
    Some(first)
}
