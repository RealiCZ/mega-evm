//! Transaction handlers: op-evm2's OP layer (L1 data fee, operator fee, vaults, deposits) with
//! Satin's transaction rules on top: the registry read every transaction makes first and the
//! classification of every envelope against the live system address it finds, the body's history
//! as intrinsic state gas, the first frame's start records and charge, the first-frame
//! settlement, deposits that pay no history but are charged their created
//! caller's account state (SALT-priced) and held to every limit.

use crate::{MegaTypes, ledger::*};
use alloy_consensus::{Transaction, TxEip1559, TxEip2930, TxLegacy};
use alloy_eips::eip2930::AccessList;
use alloy_primitives::{Address, B256, TxKind, U256, address, b256};
use evm2::{
    Evm, EvmTypesHost, TxResult, TxResultExt,
    env::TxEnvExt,
    ethereum::{
        self, PreparedTx, eip1559, eip2930, execute_initial_frame, finalize_gas,
        initial_gas_and_reservoir, legacy, prepare_initial_frame,
    },
    handler::{GasSettlement, TxHandlerHooks},
    interpreter::{GasTracker, Host, InstrStop},
    registry::{HandlerResult, TxRegistry, TxRequest, handler},
    version::{GasId, StateGasSite},
};
use op_evm2::{
    OpHandlerHooks, OpTxEnvelope, OpTxResultExt, PreparedDeposit, TxDeposit,
    constants::DEPOSIT_TRANSACTION_TYPE,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct MegaHooks;

/// Resets the ledger for a user transaction and parks its own frame's start records (a deposit
/// does both in `prepare_deposit_inner`, after its body check).
fn begin(
    host: &mut Evm<'_, MegaTypes>,
    caller: alloy_primitives::Address,
    to: TxKind,
    input_len: usize,
    value: U256,
) {
    host.ext_mut().begin_tx(caller, TX_BODY_SIZE + input_len as u64, true, false);
    first_frame_start(host, caller, to, value, 0);
}

/// Parks the transaction's own frame's start records and charge as the journal stands now
/// (an EIP-7702 transaction asks after its authorities are applied).
pub(crate) fn first_frame_start(
    host: &mut Evm<'_, MegaTypes>,
    caller: alloy_primitives::Address,
    to: TxKind,
    value: U256,
    mint: u128,
) {
    // No start record is needed for a zero-value call. Avoid a premature sender read before
    // transaction validation; a refused retired-system transaction must not read that account.
    if matches!(to, TxKind::Call(target) if value.is_zero() || target == caller) {
        return;
    }
    let nonce =
        host.state_mut().account_info_untracked(&caller).ok().flatten().map_or(0, |i| i.nonce);
    let (target, wants) = match to {
        TxKind::Create => (caller.create(nonce), true),
        TxKind::Call(to) => (to, !value.is_zero() && to != caller),
    };
    // A start revm refuses on its caller's account (a deposit's value the minted balance cannot
    // fund) records nothing.
    let balance = host
        .state_mut()
        .account_info_untracked(&caller)
        .ok()
        .flatten()
        .map_or(U256::ZERO, |i| i.balance);
    if !wants || balance.saturating_add(U256::from(mint)) < value {
        return;
    }
    let info = host.state_mut().account_info_untracked(&target).ok().flatten();
    let empty = info.as_ref().is_none_or(|i| i.is_empty());
    let unchanged = !changed(host, target);
    let records = unchanged as i64;
    let log = !value.is_zero();
    let ext = host.ext_mut();
    // Drawn on the first frame's pool once evm2 decided its start (`started` / `before_commit`
    // message hooks).
    ext.tx_start_pending = Some([
        records * WRITE_RECORD_SIZE as i64 + if log { TRANSFER_LOG_SIZE as i64 } else { 0 },
        records,
        empty as i64,
    ]);
    if ext.prices_history {
        ext.tx_start_charge = records as u64 * WRITE_RECORD_SIZE * CPHB;
        ext.tx_start_bytes = records as u64 * WRITE_RECORD_SIZE;
        ext.tx_start_target = Some(target);
    }
}

/// Checks the body where Satin does, after the environment's rules (the chain id, the gas limit
/// against the cap and the block, the fees, the init code): the intrinsic-gas hook runs it
/// ([`TxHandlerHooks::adjust_intrinsic_gas`]).
pub(crate) fn defer_body_check(host: &mut Evm<'_, MegaTypes>, entries: u64) -> HandlerResult<()> {
    if crate::off("body_after_env") {
        return admits_body(host, entries);
    }
    host.ext_mut().mechanisms.body_check = Some(entries);
    Ok(())
}

/// Satin `admits_tx_body`: the body, with one write record per authorization entry, against the
/// transaction's data-size limit, before any gas check (the protocol's own work is exempt).
pub(crate) fn admits_body(host: &mut Evm<'_, MegaTypes>, entries: u64) -> HandlerResult<()> {
    let ext = host.ext();
    if !crate::off("body_records") &&
        !ext.exempt &&
        ext.tx_net[0] as u64 + WRITE_RECORD_SIZE * entries > ext.cfg.limits[0]
    {
        return Err(crate::satin_ext::refuse(
            "transaction body and authorization records exceed the transaction data-size limit",
        ));
    }
    Ok(())
}

/// A host error of a load that cannot halt (no cold load is skipped) as the handler's error.
fn host_error(error: evm2::HostError) -> evm2::registry::HandlerError {
    match error {
        evm2::HostError::Execution(error) => error.into(),
        evm2::HostError::Halt(_) => evm2::registry::HandlerError::Fatal(evm2::AnyError::new(
            crate::satin_ext::Refusal("registry read halted"),
        )),
    }
}

/// Every transaction reads the live system address first (Satin `load_system_address`, the first
/// thing its `validate_env` does): the registry's account and slot 0, warm for the rest of the
/// transaction, both read whatever the account holds. A read the database cannot serve fails the
/// transaction. The address is the slot's only under the trusted code and when it is non-zero.
fn read_registry(host: &mut Evm<'_, MegaTypes>) -> HandlerResult<Option<Address>> {
    if crate::off("registry_read") {
        return Ok(None);
    }
    let slot = host.ext().cfg.registry_slot;
    Host::load_account(host, &SEQUENCER_REGISTRY, false, false).map_err(host_error)?;
    // `Host::sload` of a slot of an account the database does not hold reads nothing below the
    // journal; Satin's journal sload reads it, so the read set (witness) holds it: load it.
    host.state_mut().storage_slot(&SEQUENCER_REGISTRY, slot)?;
    let current = Host::sload(host, &SEQUENCER_REGISTRY, &slot, false).map_err(host_error)?.value;
    let code_hash =
        host.state_mut().account_info_untracked(&SEQUENCER_REGISTRY)?.map(|info| info.code_hash);
    Ok((code_hash == Some(SEQUENCER_REGISTRY_CODE_HASH) && !current.is_zero())
        .then(|| Address::from_word(current.into())))
}

/// Satin `SYSTEM_ADDRESS_WITHOUT_SYSTEM_SHAPE`: the refusal of a transaction from the live system
/// address that is not a legacy call to a whitelisted contract.
pub const SYSTEM_ADDRESS_WITHOUT_SYSTEM_SHAPE: &str =
    "a transaction from the system address must be a legacy call to a whitelisted contract";

/// Satin `classify_system_transaction`, made first for every envelope on the registry read
/// (`read_registry`): whether the transaction is a system-address transaction, a legacy (`ty` 0)
/// call to the Oracle from the live system address. Every other envelope from that address — a
/// call off the whitelist, a creation, a typed transaction, a deposit — is refused; any other
/// sender is an ordinary one.
pub(crate) fn classify(
    host: &mut Evm<'_, MegaTypes>,
    caller: Address,
    ty: u8,
    to: TxKind,
) -> HandlerResult<bool> {
    if read_registry(host)? != Some(caller) {
        return Ok(false);
    }
    if ty == 0 && to == TxKind::Call(ORACLE) {
        return Ok(!crate::off("system_tx"));
    }
    if crate::off("system_shape") {
        return Ok(false);
    }
    Err(crate::satin_ext::refuse(SYSTEM_ADDRESS_WITHOUT_SYSTEM_SHAPE))
}

/// Satin `validate_account_nonce_and_code` on the live system address, its code loaded: EIP-3607
/// first, which refuses a sender with code unless the code is an EIP-7702 delegation, then the
/// nonce. A read the database cannot serve fails the transaction.
fn check_system_sender(
    host: &mut Evm<'_, MegaTypes>,
    caller: Address,
    nonce: u64,
) -> HandlerResult<()> {
    use evm2::registry::HandlerError;
    let info = host.state_mut().account_info_untracked(&caller)?.unwrap_or_default();
    if crate::off("system_sender_code") {
        if info.nonce != nonce {
            return Err(HandlerError::InvalidNonce { expected: info.nonce, got: nonce });
        }
        if info.code_hash != alloy_primitives::KECCAK256_EMPTY {
            return Err(HandlerError::RejectCallerWithCode);
        }
        return Ok(());
    }
    if !info.is_empty_code_hash() && !host.state_mut().code_untracked(&caller)?.is_eip7702() {
        return Err(HandlerError::RejectCallerWithCode);
    }
    if nonce == u64::MAX && info.nonce == u64::MAX {
        return Err(HandlerError::NonceOverflow);
    }
    if info.nonce != nonce {
        return Err(HandlerError::InvalidNonce { expected: info.nonce, got: nonce });
    }
    Ok(())
}

impl TxHandlerHooks<MegaTypes> for MegaHooks {
    fn adjust_intrinsic_gas(
        host: &mut Evm<'_, MegaTypes>,
        _envelope: &OpTxEnvelope,
        _intrinsic: &mut u64,
        initial_state_gas: &mut u64,
        _floor_gas: &mut u64,
    ) -> HandlerResult<()> {
        if let Some(entries) = host.ext_mut().mechanisms.body_check.take() {
            admits_body(host, entries)?;
        }
        // Satin checks revm's intrinsic gas (execution and EIP-8037 state gas, without the body's
        // history), then the calldata floor, then the intrinsic gas with the history; evm2 checks
        // the floor after the intrinsic gas, which carries the history from here on. A gas limit
        // that covers the first and not the floor is refused for the floor, as in Satin: only the
        // reason of a refusal changes, never whether the transaction is refused.
        let gas_limit = _envelope.gas_limit();
        if gas_limit >= _intrinsic.saturating_add(*initial_state_gas) &&
            gas_limit < *_floor_gas &&
            !crate::off("floor_before_history")
        {
            return Err(evm2::registry::HandlerError::GasLimitBelowFloor {
                required: *_floor_gas,
                got: gas_limit,
            });
        }
        *initial_state_gas += host.ext().body_history + host.ext().tx_start_charge;
        host.ext_mut().mechanisms.prepared = Some((*_intrinsic, *initial_state_gas, *_floor_gas));
        Ok(())
    }

    fn before_execution(
        host: &mut Evm<'_, MegaTypes>,
        envelope: &OpTxEnvelope,
        caller: alloy_primitives::Address,
        upfront_fee: U256,
    ) -> HandlerResult<()> {
        // op-revm reads the L1Block account when it fetches the L1 info; op-evm2 reads only the
        // slots. Read the account so the read set matches (no gas effect).
        let number = host.block().number;
        if host.ext().l1.l2_block != Some(number) {
            host.state_mut().account_info_untracked(&op_evm2::constants::L1_BLOCK_CONTRACT)?;
        }
        <OpHandlerHooks as TxHandlerHooks<MegaTypes>>::before_execution(
            host,
            envelope,
            caller,
            upfront_fee,
        )
    }

    fn settle_transaction(
        host: &mut Evm<'_, MegaTypes>,
        envelope: &OpTxEnvelope,
        gas: GasSettlement<MegaTypes>,
    ) -> HandlerResult<TxResult<MegaTypes>> {
        // The first-frame settlement: a failed transaction frame gives its start charge back as
        // history (spill first); after a halt the regular gas the credit put back is burned.
        let mut gas = gas;
        let charge = host.ext().tx_start_charge;
        // The part of the start charge the regular gas paid: the reservoir pays the body's
        // history first, then the start (Satin charges the start on the frame, a spill there).
        let spilled = || {
            let reservoir0 = gas.gas_limit.saturating_sub(EXECUTION_CAP);
            let body = gas.initial_state_gas - charge;
            let from_reservoir = match host.ext().mechanisms.start_from_reservoir {
                // Charged after validation (EIP-7702), as recorded then.
                Some(paid) => paid,
                None => charge.min(reservoir0.saturating_sub(body)),
            };
            (charge - from_reservoir, from_reservoir)
        };
        if charge > 0 && !gas.result.stop.is_success() {
            let (spilled, from_reservoir) = spilled();
            let g = &mut gas.result.gas;
            g.erase_cost(spilled);
            g.set_reservoir(g.reservoir() + from_reservoir);
            g.add_state_gas_spent(-(charge as i64));
            if gas.result.stop.is_halt() {
                gas.result.gas.spend_all();
            }
        } else if charge > 0 &&
            host.ext().tx_start_returned &&
            !crate::off("own_creation_record_spill")
        {
            // The transaction's own creation was destroyed in its constructor (by its own frame or
            // one it delegated to) and its start record came back to the reservoir with the leaf;
            // Satin's merges give a spilled charge back to the regular gas first.
            let (spilled, _) = spilled();
            let g = &mut gas.result.gas;
            let moved = spilled.min(g.reservoir());
            g.set_reservoir(g.reservoir() - moved);
            g.erase_cost(moved);
        }
        host.ext_mut().reservoir_left = gas.result.gas.reservoir();
        settle_first_frame(host.ext_mut(), gas.result.stop.is_success());
        <OpHandlerHooks as TxHandlerHooks<MegaTypes>>::settle_transaction(host, envelope, gas)
    }
}

/// The `SequencerRegistry` code this engine trusts (v2.0.0, Satin `SEQUENCER_REGISTRY_CODE_HASH`).
pub const SEQUENCER_REGISTRY_CODE_HASH: B256 =
    b256!("abd7e8f1c8f0f9ca0346df585b65d9791a7e9ba5c431cc4eda5d2c2f1f5e9c43");
/// Satin `MEGA_SYSTEM_TX_WHITELIST`: the Oracle.
pub const ORACLE: Address = address!("0x6342000000000000000000000000000000000001");
/// Satin `MEGA_SYSTEM_TRANSACTION_SOURCE_HASH`, stamped on a promoted system-address transaction.
pub const MEGA_SYSTEM_TRANSACTION_SOURCE_HASH: B256 =
    b256!("852c082c0faff590c6300c2c34815d1f79882552fa95ba413cd5aeb1dba84957");

/// The live system address the registry names, read off the state (Satin `live_system_address`):
/// only with the trusted code and a non-zero `_currentSystemAddress`. A read the database cannot
/// serve is an error.
pub fn live_system_address(
    host: &mut Evm<'_, MegaTypes>,
) -> Result<Option<Address>, evm2::DatabaseError> {
    let Some(info) = host.state_mut().account_info_untracked(&SEQUENCER_REGISTRY)? else {
        return Ok(None);
    };
    if info.code_hash != SEQUENCER_REGISTRY_CODE_HASH {
        return Ok(None);
    }
    let slot = host.ext().cfg.registry_slot;
    let current = host.state_mut().storage_slot_untracked(&SEQUENCER_REGISTRY, &slot)?;
    Ok((!current.is_zero()).then(|| Address::from_word(current.into())))
}

/// Satin's body bytes beyond the 310-byte envelope: calldata, 20 per access-list address and
/// 32 per storage key.
pub(crate) fn body_extra(input_len: usize, al: &AccessList) -> usize {
    input_len + al.0.iter().map(|i| 20 + 32 * i.storage_keys.len()).sum::<usize>()
}

/// A legacy transaction: a user's, or a system-address transaction — a legacy call to the
/// Oracle from the live system address — which Satin validates and promotes to a deposit.
pub enum LegacyPrepared {
    User(PreparedTx),
    System(TxDeposit, PreparedDeposit),
}

fn prepare_legacy(
    req: &mut TxRequest<'_, '_, MegaTypes, TxLegacy>,
) -> HandlerResult<LegacyPrepared> {
    let caller = req.tx.signer();
    let tx = req.tx.inner();
    let (to, len, value) = (tx.to, tx.input.len(), tx.value);
    if classify(req.host, caller, 0, to)? {
        // Satin `validate_and_promote`: the chain id must be present and right.
        if tx.chain_id.is_none() && !crate::off("system_missing_chain_id") {
            return Err(evm2::registry::HandlerError::MissingChainId);
        }
        if tx.chain_id != Some(req.host.version().chain_id) {
            return Err(evm2::registry::HandlerError::InvalidChainId {
                expected: req.host.version().chain_id,
                got: tx.chain_id.unwrap_or(0),
            });
        }
        // EIP-3607 and the nonce, which the deposit path does not check (Satin checks both).
        if !crate::off("system_nonce") {
            check_system_sender(req.host, caller, tx.nonce)?;
        }
        let deposit = TxDeposit {
            source_hash: MEGA_SYSTEM_TRANSACTION_SOURCE_HASH,
            from: caller,
            to,
            mint: 0,
            value,
            gas_limit: tx.gas_limit,
            is_system_transaction: false,
            input: tx.input.clone(),
        };
        let prepared = prepare_deposit_inner(req.host, &deposit, true)?;
        return Ok(LegacyPrepared::System(deposit, prepared));
    }
    begin(req.host, caller, to, len, value);
    defer_body_check(req.host, 0)?;
    legacy::prepare_with_hooks::<MegaTypes, MegaHooks>(req).map(LegacyPrepared::User)
}

fn execute_legacy(
    req: TxRequest<'_, '_, MegaTypes, TxLegacy>,
    prepared: LegacyPrepared,
) -> HandlerResult<TxResult<MegaTypes>> {
    match prepared {
        LegacyPrepared::User(p) => legacy::execute_prepared::<MegaTypes, MegaHooks>(req, p),
        LegacyPrepared::System(deposit, p) => execute_deposit_inner(req.host, &deposit, p),
    }
}

fn prepare_2930(req: &mut TxRequest<'_, '_, MegaTypes, TxEip2930>) -> HandlerResult<PreparedTx> {
    let caller = req.tx.signer();
    let tx = req.tx.inner();
    let (to, len, value) = (tx.to, body_extra(tx.input.len(), &tx.access_list), tx.value);
    classify(req.host, caller, 1, to)?;
    begin(req.host, caller, to, len, value);
    defer_body_check(req.host, 0)?;
    eip2930::prepare_with_hooks::<MegaTypes, MegaHooks>(req)
}

fn prepare_1559(req: &mut TxRequest<'_, '_, MegaTypes, TxEip1559>) -> HandlerResult<PreparedTx> {
    let caller = req.tx.signer();
    let tx = req.tx.inner();
    let (to, len, value) = (tx.to, body_extra(tx.input.len(), &tx.access_list), tx.value);
    classify(req.host, caller, 2, to)?;
    begin(req.host, caller, to, len, value);
    defer_body_check(req.host, 0)?;
    eip1559::prepare_with_hooks::<MegaTypes, MegaHooks>(req)
}

fn prepare_deposit(
    req: &mut TxRequest<'_, '_, MegaTypes, TxDeposit>,
) -> HandlerResult<PreparedDeposit> {
    let tx = *req.tx.inner();
    match classify(req.host, tx.from, DEPOSIT_TRANSACTION_TYPE, tx.to) {
        Ok(_) => prepare_deposit_inner(req.host, tx, false),
        Err(
            error @ (evm2::registry::HandlerError::Database(_) |
            evm2::registry::HandlerError::Fatal(_)),
        ) => Err(error),
        // A deposit from the live system address: refused before anything of it runs, and
        // included as a failed deposit (op-revm). No frame is started, so its start ledger holds
        // nothing; its body is counted.
        Err(refusal) => {
            // The failed deposit still bumps the caller's nonce: an absent caller is created, a
            // unit of state growth outside any frame (Satin counts a failed deposit's created
            // caller).
            let created =
                req.host.state_mut().account_info_untracked(&tx.from)?.is_none_or(|i| i.is_empty());
            req.host.ext_mut().begin_tx(tx.from, TX_BODY_SIZE + tx.input.len() as u64, false, true);
            req.host.ext_mut().frames.clear();
            if created && !crate::off("refused_deposit_caller") {
                req.host.ext_mut().tx_growth += 1;
            }
            Ok(PreparedDeposit::Failed(refusal))
        }
    }
}

fn prepare_deposit_inner(
    host: &mut Evm<'_, MegaTypes>,
    tx: &TxDeposit,
    system: bool,
) -> HandlerResult<PreparedDeposit> {
    let info = host.state_mut().account_info_untracked(&tx.from).ok().flatten();
    let created = info.as_ref().is_none_or(|i| i.is_empty());
    // A deposit whose body is over the data-size limit is a failed deposit (Satin), refused
    // before anything of it runs: its caller is not validated, its first frame's start is not
    // even predicted (no read of its recipient), and no start settles.
    host.ext_mut().begin_tx(tx.from, TX_BODY_SIZE + tx.input.len() as u64, false, true);
    if system {
        host.ext_mut().set_system_originated();
    }
    let body_refused = admits_body(host, 0).is_err();
    let runs = !body_refused || crate::off("body_refused_start");
    // op-revm loads the caller with its code (no EIP-3607 check for a deposit): its code is read.
    if runs && info.is_some_and(|i| !i.is_empty_code_hash()) && !crate::off("deposit_caller_code") {
        host.state_mut().code_untracked(&tx.from)?;
    }
    if runs {
        first_frame_start(host, tx.from, tx.to, tx.value, tx.mint);
    }
    if created {
        let ext = host.ext_mut();
        if !crate::off("deposit_caller") {
            ext.tx_growth += 1;
        }
        // The created caller is counted outside the frames; a frame of the protocol's own work
        // keeps unlimited budgets (Satin), so nothing is taken from them.
        if !ext.exempt || crate::off("exempt_caller_budget") {
            ext.frames[0].budget[2] = ext.frames[0].budget[2].saturating_sub(1);
        }
    }
    host.ext_mut().deposit_created_caller = created;
    host.ext_mut().mechanisms.body_refused = body_refused;
    op_evm2::prepare_deposit_tx(host, tx)
}

fn execute_deposit(
    req: TxRequest<'_, '_, MegaTypes, TxDeposit>,
    prepared: PreparedDeposit,
) -> HandlerResult<TxResult<MegaTypes>> {
    let tx = *req.tx.inner();
    execute_deposit_inner(req.host, tx, prepared)
}

/// op-evm2's `execute_deposit_tx` with the created caller's account state gas as initial state
/// gas (Satin: "the account a deposit-like transaction creates for its caller is charged the
/// account-creation state gas once"), SALT-priced at the caller's bucket.
fn execute_deposit_inner(
    host: &mut Evm<'_, MegaTypes>,
    tx: &TxDeposit,
    prepared: PreparedDeposit,
) -> HandlerResult<TxResult<MegaTypes>> {
    let (intrinsic, floor_gas, nonce) = match prepared {
        PreparedDeposit::Ready { intrinsic, floor_gas, nonce } => (intrinsic, floor_gas, nonce),
        PreparedDeposit::Failed(_) => return failed_deposit(host, tx, InstrStop::OutOfGas),
    };
    if host.ext().mechanisms.body_refused {
        // No frame starts, so its start ledger holds nothing.
        if !crate::off("body_refused_start") {
            host.ext_mut().frames.clear();
        }
        return failed_deposit(host, tx, InstrStop::OutOfGas);
    }
    let created_state = if host.ext().deposit_created_caller && !crate::off("deposit_caller") {
        MegaTypes::state_gas_price(host, GasId::NewAccountState, StateGasSite::Account(tx.from))?
    } else {
        0
    };
    // A created caller the deposit's gas cannot pay is a failed deposit (Satin), not a refusal.
    if created_state > tx.gas_limit.saturating_sub(intrinsic) {
        return failed_deposit(host, tx, InstrStop::OutOfGas);
    }
    let (execution_gas_limit, reservoir) =
        initial_gas_and_reservoir(host.version(), tx.gas_limit, intrinsic, created_state);
    *host.tx_env_mut() = TxEnvExt {
        origin: tx.from,
        gas_price: U256::ZERO,
        chain_id: U256::from(host.version().chain_id),
        ..TxEnvExt::default()
    };
    let mut tx_gas =
        GasTracker::new_with_execution_gas_and_reservoir(execution_gas_limit, reservoir);
    let frame =
        prepare_initial_frame(host, tx.from, nonce, tx.to, &tx.input, tx.value, &mut tx_gas)?;
    let result = execute_initial_frame(host, frame, &mut tx_gas, execution_gas_limit, reservoir)?;
    // op-revm maps a first frame whose value the caller cannot fund (`OutOfFunds`) to a halt, so
    // the deposit fails (nonce and mint kept, gas limit used); evm2 reports it as a revert-class
    // stop, which op-evm2's `execute_deposit_tx` does not turn into a failed deposit.
    if result.stop.is_halt() || (result.stop == InstrStop::OutOfFunds && !crate::off("deposit_oof"))
    {
        return failed_deposit(host, tx, result.stop);
    }
    host.ext_mut().reservoir_left = result.gas.reservoir();
    settle_first_frame(host.ext_mut(), result.stop.is_success());
    finalize_gas(
        host,
        GasSettlement {
            caller: tx.from,
            gas_price: U256::ZERO,
            gas_limit: tx.gas_limit,
            floor_gas,
            initial_state_gas: created_state,
            state_refund: 0,
            result,
        },
    )
}

/// The transaction's own start settles on the start ledger (Satin's depth-0 settlement), as the
/// comparison reports it (`compare` feature; nothing else reads it).
#[cfg(not(feature = "compare"))]
#[inline(always)]
fn settle_first_frame(_ext: &mut Lite, _success: bool) {}

#[cfg(feature = "compare")]
fn settle_first_frame(ext: &mut Lite, success: bool) {
    if !ext.top_started {
        // No first frame was built (pre-execution out of gas): Satin settles the start at
        // unlimited budgets, nothing counted, nothing refunded.
        ext.settlements.push(Settlement {
            depth: 0,
            frame_remaining: [u64::MAX; 3],
            frame_net: [0; 3],
            caller_remaining: [u64::MAX; 3],
            caller_net: [0; 3],
            refund: 0,
        });
        return;
    }
    let f = ext.frames[0];
    let frame_remaining = core::array::from_fn(|i| f.reported_remaining(i));
    let (caller_remaining, caller_net) =
        if success { (frame_remaining, f.net) } else { (f.budget, [0; 3]) };
    let refund = if success { 0 } else { ext.tx_start_charge };
    ext.settlements.push(Settlement {
        depth: 0,
        frame_remaining,
        frame_net: f.net,
        caller_remaining,
        caller_net,
        refund,
    });
}

fn failed_deposit(
    host: &mut Evm<'_, MegaTypes>,
    tx: &TxDeposit,
    stop: InstrStop,
) -> HandlerResult<TxResult<MegaTypes>> {
    #[cfg(feature = "compare")]
    if host.ext().settlements.iter().all(|s| s.depth != 0) && !host.ext().frames.is_empty() {
        settle_first_frame(host.ext_mut(), false);
    }
    host.state_mut().clear_transaction_state();
    {
        let mut caller = host.state_mut().account(&tx.from)?;
        caller.bump_nonce();
        caller.add_balance(U256::from(tx.mint));
    }
    host.ext_mut().reservoir_left = 0;
    Ok(TxResultExt {
        status: false,
        total_gas_spent: tx.gas_limit,
        stop,
        ext: OpTxResultExt { failed_deposit: true, ..Default::default() },
        ..TxResultExt::default()
    })
}

pub fn registry() -> TxRegistry<MegaTypes, TxResult<MegaTypes>> {
    TxRegistry::new()
        .with_handler(0, OpTxEnvelope::as_legacy, handler(prepare_legacy, execute_legacy))
        .with_handler(
            1,
            OpTxEnvelope::as_eip2930,
            handler(prepare_2930, eip2930::execute_prepared::<MegaTypes, MegaHooks>),
        )
        .with_handler(
            2,
            OpTxEnvelope::as_eip1559,
            handler(prepare_1559, eip1559::execute_prepared::<MegaTypes, MegaHooks>),
        )
        .with_handler(
            4,
            OpTxEnvelope::as_eip7702,
            handler(crate::eip7702::prepare, crate::eip7702::execute),
        )
        .with_handler(
            DEPOSIT_TRANSACTION_TYPE,
            OpTxEnvelope::as_deposit,
            handler(prepare_deposit, execute_deposit),
        )
}

pub fn _unused(e: &OpTxEnvelope) -> u64 {
    let _ = ethereum::default_settle_gas::<MegaTypes>;
    e.gas_limit()
}
