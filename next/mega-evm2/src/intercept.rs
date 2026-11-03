//! Satin's `MegaLimitControl` interceptor as a precompile provider (the thin evm2 fork's message
//! dispatch, alloy-rs/evm2#523): dispatch by
//! message (scheme guard and selector in `contains_message`, so DELEGATECALL, CALLCODE and an
//! unknown selector run the contract's bytecode), the value policy (`NonZeroTransfer()`), and the
//! answer `remainingComputeGas()` = the caller's regular gas after the forward plus the forward,
//! at most detention's allowance (Satin `system/limit_control.rs`).

use crate::{MegaTypes as P5Types, detention::regular_spent};
use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use evm2::{
    Evm, PrecompileError,
    evm::PrewarmSet,
    interpreter::{GasTracker, Message, MessageKind},
    precompile::{PrecompileOutput, PrecompileProvider},
    precompiles::{MovePrecompileError, PrecompileId, Precompiles},
};

/// Satin's `MegaLimitControl` system contract.
pub const LIMIT_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05]);

/// The selector of `remainingComputeGas()`.
pub fn remaining_compute_gas_selector() -> [u8; 4] {
    keccak256("remainingComputeGas()")[..4].try_into().unwrap()
}

pub fn non_zero_transfer() -> Bytes {
    Bytes::copy_from_slice(&keccak256("NonZeroTransfer()")[..4])
}

#[derive(Debug)]
pub struct SatinProvider {
    pub base: Precompiles<P5Types>,
}

impl SatinProvider {
    pub fn new() -> Self {
        Self { base: Precompiles::new(std::borrow::Cow::Borrowed(crate::precompile_map())) }
    }
}

impl PrecompileProvider<P5Types> for SatinProvider {
    fn addresses(&self) -> Vec<Address> {
        // The system contract is not a precompile: its address is not pre-warmed.
        self.base.addresses()
    }

    fn warm_addresses(&self, warm: &mut PrewarmSet) {
        self.base.warm_addresses(warm);
    }

    fn precompile_ids(&self) -> Vec<(Address, PrecompileId)> {
        self.base.precompile_ids()
    }

    fn contains(&self, address: &Address) -> bool {
        *address == LIMIT_CONTROL || self.base.contains(address)
    }

    fn contains_message(&self, m: &Message<P5Types>) -> bool {
        if m.call_target == crate::satin_ext::ACCESS_CONTROL && !crate::off("q8_access_control") {
            // `MegaAccessControl`, scheme guard then selector.
            return matches!(m.kind, MessageKind::Call | MessageKind::StaticCall) &&
                crate::satin_ext::access_control_method(&m.input).is_some();
        }
        if m.call_target == LIMIT_CONTROL {
            // Scheme guard, then the selector on its four bytes.
            return matches!(m.kind, MessageKind::Call | MessageKind::StaticCall) &&
                m.input.get(..4) == Some(&remaining_compute_gas_selector()[..]);
        }
        self.base.contains_message(m)
    }

    fn move_precompiles(
        &mut self,
        moves: &[(Address, Address)],
    ) -> Result<(), MovePrecompileError> {
        self.base.move_precompiles(moves)
    }

    fn execute(
        &self,
        evm: &mut Evm<'_, P5Types>,
        m: &Message<P5Types>,
        gas: &mut GasTracker,
    ) -> Option<Result<PrecompileOutput, PrecompileError>> {
        if m.call_target != LIMIT_CONTROL {
            return self.base.execute(evm, m, gas);
        }
        if !m.value.is_zero() {
            return Some(Err(PrecompileError::Revert(non_zero_transfer())));
        }
        Some(Ok(PrecompileOutput::new(remaining_compute_gas(evm, m))))
    }
}

/// The answer of `remainingComputeGas()` to the call `m`.
pub fn remaining_compute_gas(evm: &mut Evm<'_, P5Types>, m: &Message<P5Types>) -> Bytes {
    {
        let ext = evm.ext();
        let own = ext.last_caller_remaining.saturating_add(m.gas_limit);
        let det = &ext.detention;
        let allowance = det.limit.map(|limit| {
            // compute at the start of a frame at `depth` with `gas_limit`: every suspended
            // caller's, the caller's own contribution included (Satin `compute_at_start`).
            let depth = m.depth as usize;
            let contribution =
                depth.checked_sub(1).and_then(|i| det.frames.get(i)).map_or(0, |c| {
                    c.at_suspension.saturating_sub(m.gas_limit.saturating_sub(c.child_stipend))
                });
            let at_start = det
                .suspended
                .saturating_add(contribution)
                .saturating_add(det.stipends)
                .saturating_sub(det.burned);
            limit.saturating_sub(at_start)
        });
        let remaining = allowance.map_or(own, |a| own.min(a));
        let _ = regular_spent;
        Bytes::copy_from_slice(&B256::from(U256::from(remaining)).0)
    }
}

/// The interceptor run from the `before` message hook, ahead of evm2's depth check and value
/// transfer: the
/// value policy then answers before any value moves, as Satin's does.
pub fn answer_before(
    evm: &mut Evm<'_, P5Types>,
    m: &Message<P5Types>,
) -> Option<evm2::interpreter::MessageResult<P5Types>> {
    if m.call_target == crate::satin_ext::ACCESS_CONTROL && evm.precompiles().contains_message(m) {
        // `MegaAccessControl` (Satin `system/control.rs`): the value policy, then the method,
        // answered on the untouched forward.
        let method = crate::satin_ext::access_control_method(&m.input)?;
        let (stop, output) = if !m.value.is_zero() {
            (evm2::interpreter::InstrStop::Revert, non_zero_transfer())
        } else {
            let (ok, out) = crate::satin_ext::access_control_answer(
                &mut evm.ext_mut().q8,
                method,
                m.depth as usize,
            );
            (
                if ok {
                    evm2::interpreter::InstrStop::Return
                } else {
                    evm2::interpreter::InstrStop::Revert
                },
                Bytes::from(out),
            )
        };
        return Some(evm2::interpreter::MessageResultExt {
            stop,
            output,
            gas: GasTracker::new_with_execution_gas_and_reservoir(m.gas_limit, m.reservoir),
            ..Default::default()
        });
    }
    if m.call_target != LIMIT_CONTROL || !evm.precompiles().contains_message(m) {
        return None;
    }
    let (stop, output) = if m.value.is_zero() {
        (evm2::interpreter::InstrStop::Return, remaining_compute_gas(evm, m))
    } else {
        (evm2::interpreter::InstrStop::Revert, non_zero_transfer())
    };
    Some(evm2::interpreter::MessageResultExt {
        stop,
        output,
        gas: GasTracker::new_with_execution_gas_and_reservoir(m.gas_limit, m.reservoir),
        ..Default::default()
    })
}

impl Default for SatinProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// The Oracle's `sendHint` (Satin `system/oracle.rs`), from the `before` message hook: a hint
/// from a frame
/// whose access is on, carrying no value and forwarded some gas, is counted on the transaction
/// (data size, never history, no frame budget) and forwarded; one that would cross the
/// transaction's data-size limit answers its call with the stop. The bytecode runs otherwise.
pub fn hint_before(
    evm: &mut Evm<'_, P5Types>,
    m: &Message<P5Types>,
) -> Option<evm2::interpreter::MessageResult<P5Types>> {
    use crate::satin_ext::*;
    if crate::off("q8_hints") ||
        m.call_target != crate::ORACLE ||
        !matches!(m.kind, MessageKind::Call | MessageKind::StaticCall) ||
        !is_send_hint(&m.input) ||
        !m.value.is_zero() ||
        m.gas_limit == 0 ||
        (m.depth as usize).checked_sub(1).is_some_and(|c| refuses(&evm.ext().q8, c))
    {
        return None;
    }
    let bytes = m.input.len() as u64;
    let ext = evm.ext_mut();
    if !ext.exempt {
        let total: i64 = ext.tx_net[0] + ext.frames.iter().map(|f| f.net[0]).sum::<i64>();
        if total + bytes as i64 > ext.cfg.limits[0] as i64 {
            let limit = ext.cfg.limits[0];
            ext.stops += 1;
            if m.depth == 0 {
                ext.top_stop = Some((0, limit));
            }
            return Some(evm2::interpreter::MessageResultExt {
                stop: evm2::interpreter::InstrStop::Revert,
                output: crate::limit_exceeded_output(0, limit).into(),
                gas: GasTracker::new_with_execution_gas_and_reservoir(m.gas_limit, m.reservoir),
                ..Default::default()
            });
        }
    }
    ext.tx_net[0] += bytes as i64;
    ext.q8.hint_bytes += bytes;
    if let Some((topic, data)) = decode_hint(&m.input) {
        ext.q8.hints.push((m.caller, topic, data));
    }
    None
}
