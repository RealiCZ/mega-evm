//! Satin's `MegaLimitControl` interceptor, answered from the `before` message hook: dispatch by
//! message (scheme guard and selector in [`is_intercepted`], so DELEGATECALL, CALLCODE and an
//! unknown selector run the contract's bytecode), the value policy (`NonZeroTransfer()`), and the
//! answer `remainingComputeGas()` = the caller's regular gas after the forward plus the forward,
//! at most detention's allowance (Satin `system/limit_control.rs`).

use crate::{MegaTypes, detention::regular_spent};
use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use evm2::{
    Evm,
    interpreter::{GasTracker, Message, MessageKind},
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

/// Whether the `before` hook answers `m` for a system contract: `MegaAccessControl` or
/// `MegaLimitControl`, called with CALL or STATICCALL and a selector the contract answers.
pub fn is_intercepted(m: &Message<MegaTypes>) -> bool {
    if m.call_target == crate::satin_ext::ACCESS_CONTROL && !crate::off("access_control") {
        // `MegaAccessControl`, scheme guard then selector.
        return matches!(m.kind, MessageKind::Call | MessageKind::StaticCall) &&
            crate::satin_ext::access_control_method(&m.input).is_some();
    }
    if m.call_target == LIMIT_CONTROL {
        // Scheme guard, then the selector on its four bytes.
        return matches!(m.kind, MessageKind::Call | MessageKind::StaticCall) &&
            m.input.get(..4) == Some(&remaining_compute_gas_selector()[..]);
    }
    false
}

/// The answer of `remainingComputeGas()` to the call `m`.
pub fn remaining_compute_gas(evm: &mut Evm<'_, MegaTypes>, m: &Message<MegaTypes>) -> Bytes {
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
    evm: &mut Evm<'_, MegaTypes>,
    m: &Message<MegaTypes>,
) -> Option<evm2::interpreter::MessageResult<MegaTypes>> {
    if m.call_target == crate::satin_ext::ACCESS_CONTROL && is_intercepted(m) {
        // `MegaAccessControl` (Satin `system/control.rs`): the value policy, then the method,
        // answered on the untouched forward.
        let method = crate::satin_ext::access_control_method(&m.input)?;
        let (stop, output) = if !m.value.is_zero() {
            (evm2::interpreter::InstrStop::Revert, non_zero_transfer())
        } else {
            let (ok, out) = crate::satin_ext::access_control_answer(
                &mut evm.ext_mut().mechanisms,
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
    if m.call_target != LIMIT_CONTROL || !is_intercepted(m) {
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

/// The Oracle's `sendHint` (Satin `system/oracle.rs`), from the `before` message hook: a hint
/// from a frame whose access is on, forwarded some gas, is counted on the transaction (data size,
/// never history, no frame budget) and forwarded, whether the call carries value or not
/// (`SATIN-ORCL-006`); one that would cross the transaction's data-size limit answers its call
/// with the stop. The bytecode runs otherwise, and after an admitted hint too (a call with value
/// then reverts in the bytecode's non-payable method, its hint counted all the same).
pub fn hint_before(
    evm: &mut Evm<'_, MegaTypes>,
    m: &Message<MegaTypes>,
) -> Option<evm2::interpreter::MessageResult<MegaTypes>> {
    use crate::satin_ext::*;
    // The gas the call forwards: a value call's stipend is not forwarded gas (`SATIN-DET-010`).
    let forwarded = if m.kind == MessageKind::Call && !m.value.is_zero() {
        m.gas_limit.saturating_sub(evm.version().gas_params.get(evm2::version::GasId::CallStipend))
    } else {
        m.gas_limit
    };
    if crate::off("oracle_hints") ||
        m.call_target != crate::ORACLE ||
        !matches!(m.kind, MessageKind::Call | MessageKind::StaticCall) ||
        !is_send_hint(&m.input) ||
        (!m.value.is_zero() &&
            crate::under_reference_rule(evm.ext(), crate::ReferenceRule::HintValueRefused)) ||
        forwarded == 0 ||
        (m.depth as usize).checked_sub(1).is_some_and(|c| refuses(&evm.ext().mechanisms, c))
    {
        return None;
    }
    let bytes = m.input.len() as u64;
    let ext = evm.ext_mut();
    if !ext.exempt {
        let total: i64 = ext.tx_net[0] + ext.frames.iter().map(|f| f.net[0]).sum::<i64>();
        if crate::over(total + bytes as i64, ext.limits.tx[0]) {
            let limit = ext.limits.tx[0];
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
    ext.mechanisms.hint_bytes += bytes;
    if let Some((topic, data)) = decode_hint(&m.input) {
        ext.mechanisms.hints.push((m.caller, topic, data));
    }
    None
}
