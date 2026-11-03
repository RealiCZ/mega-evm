//! Consumer CALL fallback: skip-cold decisions must read spendable + withheld.
//! Copied from the thin evm2 fork's `system.rs`; all other calls use its public CALL/CREATE hook
//! helpers.

use alloy_primitives::{Address, B256, Bytes};
use core::{cmp::min, ops::Range};
use evm2::{
    EvmFeatures,
    bytecode::Bytecode,
    interpreter::{
        Gas, Host, InstrStop, InterpreterState, Message, MessageExt, MessageKind, MessageResult,
        Result, StackMut, Word,
    },
    utils::WordExt,
    version::{GasId, StateGasSite},
};

fn resize_memory_range<T: crate::LiteTypes>(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, T>,
    offset: Word,
    len: Word,
) -> Result<Range<usize>> {
    let len = len.to_usize()?;
    let offset = if len != 0 {
        let offset = offset.to_usize()?;
        state.resize_memory(gas, offset, len)?;
        offset
    } else {
        usize::MAX
    };
    Ok(offset..offset + len)
}

fn get_memory_input_and_out_ranges<T: crate::LiteTypes>(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, T>,
    input_offset: Word,
    input_len: Word,
    return_offset: Word,
    return_len: Word,
) -> Result<(Range<usize>, Range<usize>)> {
    let input = resize_memory_range(gas, state, input_offset, input_len)?;
    let output = resize_memory_range(gas, state, return_offset, return_len)?;
    Ok((input, output))
}

fn memory_range_bytes<T: crate::LiteTypes>(
    state: &mut InterpreterState<'_, '_, T>,
    range: Range<usize>,
) -> Result<Bytes> {
    if range.is_empty() {
        return Ok(Bytes::new());
    }
    Ok(Bytes::copy_from_slice(state.memory().slice(range.start, range.len())))
}

fn load_acc_and_calc_gas<T: crate::LiteTypes>(
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, T>,
    to: Address,
    transfers_value: bool,
    create_empty_account: bool,
    stack_gas_limit: u64,
    before_state_charge: impl FnOnce(&mut Gas, &mut InterpreterState<'_, '_, T>, Address, u64) -> Result,
) -> Result<(u64, u64, Bytecode, Address)> {
    if transfers_value {
        gas.spend(state.gas_params().get(GasId::TransferValueCost))?;
    }

    let additional_cold_cost = state.gas_params().cold_account_additional_cost();
    let remaining_gas =
        gas.remaining() + state.host().ext().detention.frames.last().map_or(0, |f| f.withheld);
    let skip_cold_load = remaining_gas < additional_cold_cost;
    let account = match state.host().load_account(&to, true, skip_cold_load) {
        Ok(account) => account,
        Err(error) => {
            if skip_cold_load {
                crate::detention::no_debit_halt(gas);
            }
            return Err(state.fail(error));
        }
    };

    let mut cost = 0;
    if account.is_cold {
        cost += additional_cold_cost;
    }
    let mut code = account.code;
    let mut code_address = to;
    if state.feature(EvmFeatures::EIP7702) &&
        let Some(delegated_address) = code.eip7702_address()
    {
        cost += state.gas_params().get(GasId::WarmStorageReadCost);
        // Both halts below make no debit; without a failed-charge record evm2's inspected path
        // zeroes the frame's gas (as for the target's load).
        if cost > remaining_gas {
            if !crate::off("delegate_no_debit") {
                crate::detention::no_debit_halt(gas);
            }
            return Err(InstrStop::OutOfGas);
        }
        let skip_cold_load = remaining_gas < cost.saturating_add(additional_cold_cost);
        let delegated_account =
            match state.host().load_account(&delegated_address, true, skip_cold_load) {
                Ok(account) => account,
                Err(error) => {
                    if skip_cold_load && !crate::off("delegate_no_debit") {
                        crate::detention::no_debit_halt(gas);
                    }
                    return Err(state.fail(error));
                }
            };
        if delegated_account.is_cold {
            cost += additional_cold_cost;
        }
        code = delegated_account.code;
        code_address = delegated_address;
    }
    let features = state.version().features;
    let mut new_account_state_gas = 0;
    if create_empty_account &&
        (!features.contains(EvmFeatures::EIP161) || transfers_value) &&
        account.is_empty_for_new_account_gas
    {
        cost += state.gas_params().get(GasId::NewAccountCost);
        // EIP-8037: value transfer to a new account also charges state gas from
        // the reservoir. Charged before the 63/64 child gas split below.
        if features.contains(EvmFeatures::EIP8037) && transfers_value {
            new_account_state_gas =
                state_price(state, GasId::NewAccountState, StateGasSite::Account(to))?;
        }
    }
    gas.spend(cost)?;
    before_state_charge(gas, state, to, new_account_state_gas)?;
    gas.spend_state(new_account_state_gas)?;

    let mut gas_limit = if state.feature(EvmFeatures::EIP150) {
        min(state.gas_params().call_stipend_reduction(gas.remaining()), stack_gas_limit)
    } else {
        stack_gas_limit
    };
    gas.spend(gas_limit)?;

    if transfers_value {
        gas_limit = gas_limit.saturating_add(state.gas_params().get(GasId::CallStipend));
    }

    Ok((gas_limit, new_account_state_gas, code, code_address))
}

#[inline(never)]
fn prepare_call<T: crate::LiteTypes>(
    stack: &mut StackMut<'_>,
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, T>,
    kind: MessageKind,
    message: &mut Message<T>,
    return_memory_range: &mut Range<usize>,
    before_state_charge: impl FnOnce(&mut Gas, &mut InterpreterState<'_, '_, T>, Address, u64) -> Result,
) -> Result<u64> {
    let has_value = match kind {
        MessageKind::Call | MessageKind::CallCode => true,
        MessageKind::DelegateCall | MessageKind::StaticCall => false,
        _ => unreachable!("invalid call message kind"),
    };
    let [local_gas_limit, to] = stack.popn::<2>()?;
    let value = if has_value { stack.pop()? } else { Word::ZERO };
    let [input_offset, input_len, return_offset, return_len] = stack.popn::<4>()?;
    let to = to.to_address();
    let has_transfer = !value.is_zero();
    if state.is_static() && kind == MessageKind::Call && has_transfer {
        return Err(InstrStop::CallNotAllowedInsideStatic);
    }

    let local_gas_limit = u64::try_from(local_gas_limit).unwrap_or(u64::MAX);
    let (input_range, prepared_return_memory_range) = get_memory_input_and_out_ranges(
        gas,
        state,
        input_offset,
        input_len,
        return_offset,
        return_len,
    )?;
    let (gas_limit, new_account_state_gas, loaded_code, resolved_code_address) =
        load_acc_and_calc_gas(
            gas,
            state,
            to,
            has_transfer,
            kind == MessageKind::Call,
            local_gas_limit,
            before_state_charge,
        )?;
    let input = memory_range_bytes(state, input_range)?;

    let current = state.message();
    let (destination, caller, call_value, code_address) = match kind {
        MessageKind::Call => (to, current.destination, value, resolved_code_address),
        MessageKind::CallCode => {
            (current.destination, current.destination, value, resolved_code_address)
        }
        MessageKind::DelegateCall => {
            (current.destination, current.caller, current.value, resolved_code_address)
        }
        MessageKind::StaticCall => (to, current.destination, Word::ZERO, resolved_code_address),
        _ => unreachable!("invalid call message kind"),
    };
    *message = MessageExt {
        kind,
        depth: current.depth.saturating_add(1),
        gas_limit,
        reservoir: gas.reservoir(),
        destination,
        call_target: to,
        caller,
        input,
        value: call_value,
        code: loaded_code,
        code_address,
        caller_is_static: state.is_static(),
        salt: B256::ZERO,
        ext: T::MessageExt::default(),
        _non_exhaustive: (),
    };
    *return_memory_range = prepared_return_memory_range;

    Ok(new_account_state_gas)
}

/// Runs a call opcode of `kind` with hooks around its state charge and child message.
///
/// `before_state_charge` runs after regular opcode charges, immediately before the opcode's
/// own state-gas debit and before forwarding. It receives the target address and captured
/// price (possibly zero), and may adjust funding or stop the caller. It must not pay the debit
/// itself. The captured price is also used for the failed-child refund.
///
/// `before` runs after every charge the opcode makes and after the forward is taken from `gas`,
/// before the message executes; it may charge or credit `gas`, edit the message (its
/// `gas_limit`, `reservoir`, `ext`), or fail the opcode. `after` runs once the child's result is
/// merged into `gas` and the opcode's upfront state gas is refunded for a failed child, before
/// the return data is copied and the success word pushed; returning an error ends the calling
/// frame with that error. Fatal child errors bypass `after`. These callbacks run only in the
/// interpreter, like [`sstore_with`](super::sstore_with).
///
/// This is the shape of [`sstore_with`](super::sstore_with) for `CALL`, `CALLCODE`,
/// `DELEGATECALL` and `STATICCALL`.
#[inline]
fn emulated_call_with<T: crate::LiteTypes>(
    mut stack: StackMut<'_>,
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, T>,
    kind: MessageKind,
    before_state_charge: impl FnOnce(&mut Gas, &mut InterpreterState<'_, '_, T>, Address, u64) -> Result,
    before: impl FnOnce(&mut Gas, &mut InterpreterState<'_, '_, T>, &mut Message<T>) -> Result,
    after: impl FnOnce(
        &mut Gas,
        &mut InterpreterState<'_, '_, T>,
        &Message<T>,
        &MessageResult<T>,
    ) -> Result,
) -> Result {
    let mut message = Message::<T>::default();
    let mut return_memory_range = 0..0;
    let new_account_state_gas = prepare_call(
        &mut stack,
        gas,
        state,
        kind,
        &mut message,
        &mut return_memory_range,
        before_state_charge,
    )?;
    before(gas, state, &mut message)?;

    let result = state.host().execute_message(&mut message).map_err(|error| state.fail(error))?;
    if result.stop.is_fatal() {
        return Err(result.stop);
    }
    gas.merge_child_gas(result.gas, result.stop);
    // EIP-8037: a value-bearing CALL that creates the target charges NEW_ACCOUNT state
    // gas upfront on this frame. If the call does not succeed (depth/balance failure,
    // child revert or halt) the target is not created, so refund the upfront charge to
    // the reservoir (execution-specs `credit_state_gas_refund`), mirroring CREATE.
    if new_account_state_gas != 0 && !result.stop.is_success() {
        gas.refill_reservoir(new_account_state_gas);
    }
    after(gas, state, &message, &result)?;
    let copy_len = min(return_memory_range.len(), result.output.len());
    unsafe {
        let output = result.output.get_unchecked(..copy_len);
        state.memory().set(return_memory_range.start, output);
    }
    *state.return_data_mut() = result.output;
    stack.push_u64(result.stop.is_success() as u64)
}

fn state_price<T: crate::LiteTypes>(
    state: &mut InterpreterState<'_, '_, T>,
    id: GasId,
    site: StateGasSite,
) -> Result<u64> {
    T::state_gas_price(state.host(), id, site).map_err(|error| state.fail(error))
}

pub fn call_with<T: crate::LiteTypes>(
    stack: StackMut<'_>,
    gas: &mut Gas,
    state: &mut InterpreterState<'_, '_, T>,
    kind: MessageKind,
    pre: impl FnOnce(&mut Gas, &mut InterpreterState<'_, '_, T>, Address, u64) -> Result,
    before: impl FnOnce(&mut Gas, &mut InterpreterState<'_, '_, T>, &mut Message<T>) -> Result,
    after: impl FnOnce(
        &mut Gas,
        &mut InterpreterState<'_, '_, T>,
        &Message<T>,
        &MessageResult<T>,
    ) -> Result,
) -> Result {
    let ext = state.host().ext();
    // The copy is used whenever detention keeps figures (the default caps are finite): its
    // skip-cold check sees S + W, and a skipped cold load leaves a failed-charge record, which the
    // stock body does not — a halt's burn enters compute even before the first read.
    if ext.detention.frames.last().is_some_and(|f| f.withheld != 0) ||
        (crate::detention::detains(ext) && !crate::off("emulate_detained_calls"))
    {
        emulated_call_with(stack, gas, state, kind, pre, before, after)
    } else {
        evm2::interpreter::instructions::call_with_state_charge(
            stack, gas, state, kind, pre, before, after,
        )
    }
}
