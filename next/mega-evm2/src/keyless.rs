//! Native keyless deployment (satin.md §15; Satin `system/keyless/{dispatch,settle}.rs`).
//!
//! A `keylessDeploy(bytes,uint256)` CALL at depth 0 to `0x6342…0003` is dispatched in the
//! `before` message hook: with value it is answered there (the overhead, then `NoEtherTransfer()`)
//! before evm2 moves anything; without value its frame is built on a one-byte dispatcher (`0x0c`,
//! an undefined opcode that stays `OpcodeNotFound` everywhere else) whose instruction makes the
//! call's actions by hand: the 100,000 overhead, the nine rules in the legacy order, the signer's
//! account state gas, the `CREATE` opcode's charges and records, the 98/100 reservoir share and
//! the budgets, then a creation of the signer at its Nick's-Method address started through
//! `Host::execute_message` (evm2 keeps the message's destination and bumps the caller's nonce),
//! and on its return the settlement (signer account refill, nonce take-back, the signer's record
//! held on the call) and the ABI answer.

use crate::{MegaTypes, detention, ledger::*};
use alloy_consensus::{Signed, TxLegacy, transaction::RlpEcdsaDecodableTx};
use alloy_primitives::{Address, B256, Bytes, KECCAK256_EMPTY, U256, keccak256};
use evm2::{
    Evm, EvmTypesHost,
    bytecode::Bytecode,
    interpreter::{
        Gas, GasTracker, Host, InstrStop, InterpreterState, Message, MessageKind, MessageResult,
        Result,
    },
    version::{GasId, StateGasSite},
};
use evm2_macros::instruction;

pub const KEYLESS: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03]);
pub const OVERHEAD: u64 = 100_000;
pub const DISPATCHER: u8 = 0x0c;

pub fn selector() -> [u8; 4] {
    keccak256("keylessDeploy(bytes,uint256)")[..4].try_into().unwrap()
}

fn err(sig: &str, args: &[[u8; 32]]) -> Vec<u8> {
    let mut out = keccak256(sig)[..4].to_vec();
    for a in args {
        out.extend_from_slice(a);
    }
    out
}

fn w(v: u64) -> [u8; 32] {
    B256::from(U256::from(v)).0
}

fn err_bytes(sig: &str, head: &[[u8; 32]], data: &[u8]) -> Vec<u8> {
    let mut out = keccak256(sig)[..4].to_vec();
    for a in head {
        out.extend_from_slice(a);
    }
    out.extend_from_slice(&w(32 * (head.len() as u64 + 1)));
    out.extend_from_slice(&w(data.len() as u64));
    out.extend_from_slice(data);
    out.resize(out.len() + (32 - data.len() % 32) % 32, 0);
    out
}

/// The `keylessDeploy` return value `(uint64 gasUsed, address deployedAddress, bytes errorData)`.
fn returned(gas_used: u64, deployed: Address, error: &[u8]) -> Vec<u8> {
    let mut out = w(gas_used).to_vec();
    out.extend_from_slice(B256::left_padding_from(deployed.as_slice()).as_slice());
    out.extend_from_slice(&w(96));
    out.extend_from_slice(&w(error.len() as u64));
    out.extend_from_slice(error);
    out.resize(out.len() + (32 - error.len() % 32) % 32, 0);
    out
}

pub fn is_dispatched(msg: &Message<MegaTypes>) -> bool {
    msg.depth == 0 &&
        msg.call_target == KEYLESS &&
        msg.kind == MessageKind::Call &&
        msg.input.get(..4) == Some(&selector()[..])
}

/// The `before` message hook at depth 0.
pub fn before(
    evm: &mut Evm<'_, MegaTypes>,
    msg: &mut Message<MegaTypes>,
) -> Option<MessageResult<MegaTypes>> {
    if crate::off("keyless") || !is_dispatched(msg) {
        return None;
    }
    if !msg.value.is_zero() {
        // Rule 2, before the frame is built: the overhead, then the refusal. The overhead is the
        // call's first charge: under detention it can be the crossing, which stops the call with
        // nothing billed.
        if !crate::off("keyless_value_hold") &&
            let Some(answer) = crate::detention::hold_charge(evm, msg, OVERHEAD)
        {
            return Some(answer);
        }
        let mut gas =
            GasTracker::new_with_execution_gas_and_reservoir(msg.gas_limit, msg.reservoir);
        let (stop, output) = if gas.spend(OVERHEAD).is_err() {
            gas.spend_all();
            (InstrStop::OutOfGas, Bytes::new())
        } else {
            (InstrStop::Revert, Bytes::from(err("NoEtherTransfer()", &[])))
        };
        return Some(evm2::interpreter::MessageResultExt {
            stop,
            output,
            gas,
            ..Default::default()
        });
    }
    msg.code = Bytecode::new_legacy(Bytes::from_static(&[DISPATCHER]));
    evm.ext_mut().mechanisms.keyless_frame = true;
    None
}

/// Satin `decode_keyless_tx`.
fn decode_tx(raw: &[u8]) -> core::result::Result<Signed<TxLegacy>, Vec<u8>> {
    let mut buf = raw;
    let signed =
        TxLegacy::rlp_decode_signed(&mut buf).map_err(|_| err("MalformedEncoding()", &[]))?;
    if !buf.is_empty() {
        return Err(err("MalformedEncoding()", &[]));
    }
    if !signed.tx().to.is_create() {
        return Err(err("NotContractCreation()", &[]));
    }
    if signed.tx().chain_id.is_some() {
        return Err(err("NotPreEIP155()", &[]));
    }
    Ok(signed)
}

pub fn recover(signed: &Signed<TxLegacy>) -> Option<Address> {
    use secp256k1::{
        Message as SecpMessage,
        ecdsa::{RecoverableSignature, RecoveryId},
    };
    let sig = signed.signature();
    let mut compact = [0u8; 64];
    compact[..32].copy_from_slice(&sig.r().to_be_bytes::<32>());
    compact[32..].copy_from_slice(&sig.s().to_be_bytes::<32>());
    let id = RecoveryId::try_from(sig.v() as i32).ok()?;
    let rsig = RecoverableSignature::from_compact(&compact, id).ok()?;
    let hash = signed.signature_hash();
    let key = rsig.recover_ecdsa(SecpMessage::from_digest(hash.0)).ok()?;
    let uncompressed = key.serialize_uncompressed();
    Some(Address::from_slice(&keccak256(&uncompressed[1..])[12..]))
}

/// alloy's ABI decode of `(bytes, uint256)` (offsets and lengths in bounds, every range end
/// checked: an offset near the top of `usize` is malformed, not an overflow).
fn decode_call(input: &[u8]) -> Option<(Vec<u8>, U256)> {
    let body = input.get(4..)?;
    let word = |i: usize| body.get(i..i.checked_add(32)?).map(U256::from_be_slice);
    let offset: usize = word(0)?.try_into().ok()?;
    let gas = word(32)?;
    let len: usize = word(offset)?.try_into().ok()?;
    let start = offset.checked_add(32)?;
    let data = body.get(start..start.checked_add(len)?)?;
    Some((data.to_vec(), gas))
}

/// What the call needs to settle and answer once its creation returned.
struct Started {
    signer: Address,
    deploy: Address,
    signer_nonce: u64,
    signer_account_charged: bool,
    signer_changed_before: bool,
    /// Whether rule 14 charged the signer's nonce record (not when the transaction had changed
    /// the signer's account already, the sender's among them).
    nonce_record_charged: bool,
}

#[instruction(EvmTypes = MegaTypes, hooked)]
pub fn mega_keyless(cx: _) -> Result {
    let is_call = cx.state.message().depth == 0 &&
        cx.state.message().call_target == KEYLESS &&
        cx.state.host().ext().mechanisms.keyless_frame;
    if !is_call {
        return Err(InstrStop::OpcodeNotFound);
    }
    cx.state.host().ext_mut().mechanisms.keyless_frame = false;
    run(cx.gas, cx.state)
}

fn state_price(
    state: &mut InterpreterState<'_, '_, MegaTypes>,
    id: GasId,
    site: StateGasSite,
) -> Result<u64> {
    <MegaTypes as EvmTypesHost>::state_gas_price(state.host(), id, site).map_err(|e| state.fail(e))
}

fn run(gas: &mut Gas, state: &mut InterpreterState<'_, '_, MegaTypes>) -> Result {
    macro_rules! refuse {
        ($e:expr) => {
            return revert_with(gas, state, &$e)
        };
    }
    gas.spend(OVERHEAD)?;
    let input = state.message().input.clone();
    let Some((raw, override_gas)) = decode_call(&input) else {
        refuse!(err("MalformedEncoding()", &[]))
    };
    let signed = match decode_tx(&raw) {
        Ok(s) => s,
        Err(e) => refuse!(e),
    };
    let tx = signed.tx().clone();
    if tx.nonce != 0 {
        refuse!(err("NonZeroTxNonce(uint64)", &[w(tx.nonce)]));
    }
    let max = state.version().max_initcode_size;
    if tx.input.len() > max {
        refuse!(err("InitCodeTooLarge(uint64,uint64)", &[w(tx.input.len() as u64), w(max as u64)]));
    }
    let override_gas = u64::try_from(override_gas).unwrap_or(u64::MAX);
    if override_gas < tx.gas_limit {
        refuse!(err("GasLimitTooLow(uint64,uint64)", &[w(tx.gas_limit), w(override_gas)]));
    }
    let Some(signer) = recover(&signed) else { refuse!(err("InvalidSignature()", &[])) };
    let deploy = signer.create(0);
    let loaded = (|| -> core::result::Result<_, evm2::HostError> {
        let mut acc = state.host().state_mut().account(&signer).map_err(evm2::HostError::from)?;
        acc.warm();
        let code = acc.load_code().map_err(evm2::HostError::from)?;
        Ok((acc.get().cloned().unwrap_or_default(), code))
    })();
    let (info, code) = loaded.map_err(|e| state.fail(e))?;
    if signer == state.host().block().beneficiary && !crate::off("keyless_beneficiary") {
        let cap = state.host().ext().detention.cap;
        detention::commit_read(state.host().ext_mut(), gas.tracker_mut(), cap, 0);
    }
    if info.nonce > 1 {
        refuse!(err("SignerNonceTooHigh(uint64)", &[w(info.nonce)]));
    }
    if !code.is_empty() && !code.is_eip7702() {
        refuse!(err("SignerHasCode()", &[]));
    }
    let signer_account_charged = info.is_empty();
    if signer_account_charged {
        let price = state_price(state, GasId::NewAccountState, StateGasSite::Account(signer))?;
        evm2::interpreter::instructions::spend_state_reserve_first(
            gas,
            state,
            GasId::NewAccountState,
            price,
        )?;
    }
    // What a `CREATE` opcode forwards from: the spendable and the withheld gas.
    let available = |gas: &Gas, state: &mut InterpreterState<'_, '_, MegaTypes>| {
        evm2::interpreter::instructions::gas_budget(
            gas,
            state,
            evm2::interpreter::instructions::BudgetUse::Forward,
        )
    };
    let forward = override_gas.min(available(gas, state));
    if forward < tx.gas_limit {
        refuse!(err("GasLimitTooLow(uint64,uint64)", &[w(tx.gas_limit), w(forward)]));
    }
    let loaded = (|| -> core::result::Result<_, evm2::HostError> {
        let mut acc = state.host().state_mut().account(&deploy).map_err(evm2::HostError::from)?;
        acc.warm();
        Ok(acc.get().cloned())
    })();
    let deploy_info = loaded.map_err(|e| state.fail(e))?;
    if deploy_info.as_ref().is_some_and(|i| i.code_hash != KECCAK256_EMPTY) {
        refuse!(err("ContractAlreadyExists()", &[]));
    }
    if info.balance < tx.value {
        refuse!(err("InsufficientBalance()", &[]));
    }
    // The `CREATE` opcode's charges, in its order, after every rule.
    let regular = state
        .gas_params()
        .get(GasId::Create)
        .saturating_add(state.gas_params().initcode_cost(tx.input.len()));
    gas.spend(regular)?;
    let deploy_empty = deploy_info.as_ref().is_none_or(|i| i.is_empty());
    let create_price = if deploy_empty {
        state_price(state, GasId::CreateState, StateGasSite::Account(deploy))?
    } else {
        0
    };
    if create_price > 0 {
        evm2::interpreter::instructions::spend_state_reserve_first(
            gas,
            state,
            GasId::CreateState,
            create_price,
        )?;
    }
    let deploy_changed = changed(state.host(), deploy);
    let signer_changed_before = changed(state.host(), signer);
    let start_records = (!deploy_changed) as u64;
    let nonce_records = (!signer_changed_before && signer != state.host().ext().sender) as u64;
    let start_charge = if state.host().ext().prices_history {
        start_records * WRITE_RECORD_SIZE * CPHB
    } else {
        0
    };
    charge_history(gas, state, (start_records + nonce_records) * WRITE_RECORD_SIZE, false)?;
    let forward = override_gas.min(available(gas, state));
    if forward < tx.gas_limit {
        refuse!(err("GasLimitTooLow(uint64,uint64)", &[w(tx.gas_limit), w(forward)]));
    }
    // The forward draws the withheld part first, as a `CREATE` opcode's does.
    evm2::interpreter::instructions::spend_reserve_first(
        gas,
        state,
        forward,
        evm2::interpreter::instructions::ReserveDraw::Forward,
    )?;
    let r = gas.reservoir();
    let share = reservoir_share(r);
    gas.set_reservoir(r - share);
    let start =
        StartRecords::one((deploy, deploy_changed), deploy_empty as i64, !tx.value.is_zero());
    state.host().ext_mut().push_child(start_charge, r - share, Some(start), 0);
    detention::on_suspend(gas, state, 0);
    let started = Started {
        signer,
        deploy,
        signer_nonce: info.nonce,
        signer_account_charged,
        signer_changed_before,
        nonce_record_charged: nonce_records == 1,
    };
    state.host().ext_mut().mechanisms.keyless = Some(crate::satin_ext::Keyless {
        signer,
        deploy,
        signer_nonce_before: info.nonce,
        value: tx.value,
        empty_deposit_refused: false,
    });

    let mut message = state.message().clone();
    message.kind = MessageKind::Create;
    message.depth = 1;
    message.gas_limit = forward;
    message.reservoir = share;
    message.destination = deploy;
    message.call_target = deploy;
    message.caller = signer;
    message.code = Bytecode::new_legacy(tx.input.clone());
    message.input = tx.input.clone();
    message.value = tx.value;
    message.code_address = signer;
    message.caller_is_static = false;
    message.salt = B256::ZERO;
    let result = state.host().execute_message(&mut message).map_err(|e| state.fail(e))?;
    if result.stop.is_fatal() {
        return Err(result.stop);
    }
    let used = forward.saturating_sub(result.gas.remaining()) +
        share.saturating_sub(result.gas.reservoir());
    gas.merge_child_gas(result.gas, result.stop);
    let success = result.stop.is_success() && result.created_address.is_some();
    if create_price != 0 && !success {
        gas.refill_reservoir(create_price);
    }
    let entry = state.host().ext_mut().pop_child(success);
    gas.set_reservoir(gas.reservoir() + entry.kept_reservoir);
    if !success && entry.start_charge > 0 {
        refill_history(gas, state, entry.start_charge / CPHB);
    }
    let keyless = state.host().ext_mut().mechanisms.keyless.take().unwrap_or_default();
    // Satin `keyless::settle`.
    let nonce = state
        .host()
        .state_mut()
        .account_info_untracked(&started.signer)
        .ok()
        .flatten()
        .map_or(0, |i| i.nonce);
    if nonce == started.signer_nonce && started.signer_account_charged {
        let price =
            state_price(state, GasId::NewAccountState, StateGasSite::Account(started.signer))?;
        gas.refill_reservoir(price);
    }
    if started.signer_nonce > 0 &&
        nonce == started.signer_nonce + 1 &&
        !crate::off("keyless_takeback")
    {
        let reset = state
            .host()
            .state_mut()
            .account(&started.signer)
            .map(|mut acc| acc.set_nonce(started.signer_nonce))
            .map_err(evm2::HostError::from);
        reset.map_err(|e| state.fail(e))?;
        // No record stands once the nonce is set back: the history charged for it comes back,
        // and only what was charged (nothing for a signer the transaction had changed already,
        // such as its sender, whose account is never recorded again).
        let charged = started.nonce_record_charged || crate::off("keyless_sender_record");
        if charged && !changed(state.host(), started.signer) {
            refill_history(gas, state, WRITE_RECORD_SIZE);
        }
    }
    let host = state.host();
    let records =
        (changed(host, started.signer) as i64 - started.signer_changed_before as i64).max(0);
    let records = if started.signer == host.ext().sender { 0 } else { records };
    // The creation's nonce bump that created the signer is a unit of growth on the call
    // (Satin `Accounts::creator_counted`).
    let now_exists = host
        .state_mut()
        .account_info_untracked(&started.signer)
        .ok()
        .flatten()
        .is_some_and(|i| !i.is_empty());
    let growth = (started.signer_account_charged &&
        now_exists &&
        !crate::off("keyless_signer_growth")) as i64;
    if let Some(stop) = host.ext_mut().draw([records * WRITE_RECORD_SIZE as i64, records, growth]) {
        return stop_frame(gas, state, stop);
    }
    detention::on_resume(gas, state)?;
    // Satin `keyless::answer`.
    let output = if keyless.empty_deposit_refused {
        returned(used, Address::ZERO, &err("EmptyCodeDeployed(uint64)", &[w(used)]))
    } else if result.stop == InstrStop::Revert {
        returned(
            used,
            Address::ZERO,
            &err_bytes("ExecutionReverted(uint64,bytes)", &[w(used)], &result.output),
        )
    } else if !result.stop.is_success() {
        returned(used, Address::ZERO, &err("ExecutionHalted(uint64)", &[w(used)]))
    } else if result.created_address != Some(started.deploy) {
        let e = if result.created_address.is_some() {
            err("AddressMismatch()", &[])
        } else {
            err("NoContractCreated()", &[])
        };
        return revert_with(gas, state, &e);
    } else {
        let code = state
            .host()
            .state_mut()
            .account_info_untracked(&started.deploy)
            .ok()
            .flatten()
            .map_or(KECCAK256_EMPTY, |i| i.code_hash);
        if code == KECCAK256_EMPTY || state.host().ext().destroyed.contains(&started.deploy) {
            returned(used, Address::ZERO, &err("EmptyCodeDeployed(uint64)", &[w(used)]))
        } else {
            returned(used, started.deploy, &[])
        }
    };
    let _ = keyless.value;
    return_with(state, &output)
}

fn return_with(state: &mut InterpreterState<'_, '_, MegaTypes>, payload: &[u8]) -> Result {
    let offset = state.memory().len();
    let mut scratch = Gas::new(u64::MAX / 2);
    state.resize_memory(&mut scratch, offset, payload.len())?;
    state.memory().set(offset, payload);
    state.set_output(offset as u32..(offset + payload.len()) as u32);
    Err(InstrStop::Return)
}

/// Satin `refuses_empty_deposit` (the code-deposit admission): a keyless creation whose
/// constructor returns empty code for the deploy address is refused with empty revert data.
pub fn refuses_empty_deposit(
    evm: &mut Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    code: &Bytes,
) -> bool {
    let destroyed = evm.ext().destroyed.contains(&msg.destination);
    let Some(k) = evm.ext_mut().mechanisms.keyless.as_mut() else { return false };
    if code.is_empty() &&
        msg.destination == k.deploy &&
        !destroyed &&
        !crate::off("keyless_empty_deposit")
    {
        k.empty_deposit_refused = true;
        return true;
    }
    false
}
