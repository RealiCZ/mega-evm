//! EIP-7702 transactions under Satin's rules (satin.md §5 "Gas Schedule and Intrinsic Gas",
//! §7 "History Gas", §10 "Authorizations before the first frame").
//!
//! Validation is evm2's `eip7702::prepare_with_hooks` (EIP-2780 intrinsic, 7,816 per entry) with
//! the Satin body (101 bytes per entry) and a data-size check of the body plus one write record
//! per entry. Execution follows evm2's `eip7702::execute_prepared` step for step and adds:
//! - the list rule: a list that would keep more write records or create more accounts than the
//!   transaction's KV / state-growth limits allow is simulated, skipped whole and the first frame
//!   stopped before it runs (`MegaLimitExceeded(1|3, tx limit)`);
//! - the authorities' records: one KV record and 40 bytes per distinct applied authority other than
//!   the sender, one unit of growth per authority the application created, kept on the transaction
//!   whatever the first frame does, their history charged with the first frame's start record
//!   (which is predicted after the application: an authority recipient is changed already);
//! - an applied authority that is the block beneficiary detains the transaction from its start.
//!
//! The state gas of each authority (new account, delegation indicator, SALT-priced) and the
//! 9,000 account write are evm2's own EIP-2780 runtime charges (`RuntimeAuthCharges`).

use crate::{MegaTypes, ledger::*, tx::MegaHooks};
use alloy_primitives::{Address, TxKind, U256};
use evm2::{
    Evm, EvmTypes, TxResult,
    env::TxEnvExt,
    ethereum::{
        LazyTxEip7702, PreparedTx, effective_gas_price,
        eip7702::{
            self, AppliedAuth, AuthAccounting, Eip7702Authorization, RuntimeAuthCharges,
            apply_auth_list,
        },
        execute_initial_frame, initial_gas_and_reservoir, prepare_initial_frame,
        runtime_oog_result,
    },
    handler::{GasSettlement, TxHandlerHooks},
    interpreter::{GasTracker, MessageResult},
    registry::{HandlerResult, TxRequest},
};

/// Satin `AUTHORIZATION_SIZE`: the body bytes of one authorization entry.
pub const AUTHORIZATION_SIZE: u64 = 101;

pub fn prepare(req: &mut TxRequest<'_, '_, MegaTypes, LazyTxEip7702>) -> HandlerResult<PreparedTx> {
    let caller = req.tx.signer();
    let tx = req.tx.inner();
    let entries = tx.authorizations().len() as u64;
    let extra = crate::tx::body_extra(tx.input.len(), &tx.access_list) as u64 +
        AUTHORIZATION_SIZE * entries;
    // The first frame's start records are predicted after the authorities are applied.
    req.host.ext_mut().begin_tx(caller, TX_BODY_SIZE + extra, true, false);
    // Satin `admits_tx_body`: the body and one write record per entry, before any gas check.
    crate::tx::admits_body(req.host, entries)?;
    eip7702::prepare_with_hooks::<MegaTypes, MegaHooks>(req)
}

/// evm2's runtime charges, plus the accepted authorities and whether each existed.
struct Charges<'g> {
    inner: RuntimeAuthCharges<'g>,
    accepted: Vec<(Address, bool)>,
}

impl AuthAccounting for Charges<'_> {
    fn rejected(&mut self) {
        self.inner.rejected();
    }

    fn accepted<T: EvmTypes>(
        &mut self,
        host: &mut Evm<'_, T>,
        authority: Address,
        auth: &AppliedAuth,
    ) -> Result<(), evm2::HostError> {
        self.inner.accepted(host, authority, auth)?;
        self.accepted.push((authority, auth.existed));
        Ok(())
    }
}

/// Satin `simulate_authorization_list`: (records, created) the list would keep were it applied.
/// Loads each consulted authority once (with its code) without warming it.
fn simulate<A: Eip7702Authorization>(
    host: &mut Evm<'_, MegaTypes>,
    chain_id: u64,
    caller: Address,
    list: impl IntoIterator<Item = A>,
) -> HandlerResult<(u64, u64)> {
    struct Sim {
        address: Address,
        nonce: u64,
        applies: bool,
        absent: bool,
        accepted: bool,
    }
    let mut sims: Vec<Sim> = Vec::new();
    for a in list {
        let cid = a.chain_id();
        if (!cid.is_zero() && cid != U256::from(chain_id)) || a.nonce() == u64::MAX {
            continue;
        }
        let Some(authority) = a.authority() else { continue };
        let i = match sims.iter().position(|s| s.address == authority) {
            Some(i) => i,
            None => {
                let mut acc = host.state_mut().account(&authority)?;
                let code = acc.load_code()?;
                let applies = code.is_empty() || code.is_eip7702();
                let info = acc.get().cloned().unwrap_or_default();
                let absent = info.is_empty() && !acc.exists() && !acc.is_touched();
                sims.push(Sim {
                    address: authority,
                    nonce: info.nonce,
                    applies,
                    absent,
                    accepted: false,
                });
                sims.len() - 1
            }
        };
        let s = &mut sims[i];
        if !s.applies || a.nonce() != s.nonce {
            continue;
        }
        s.nonce += 1;
        s.accepted = true;
    }
    let records = sims.iter().filter(|s| s.accepted && s.address != caller).count() as u64;
    let created = sims.iter().filter(|s| s.accepted && s.absent).count() as u64;
    Ok((records, created))
}

pub fn execute(
    req: TxRequest<'_, '_, MegaTypes, LazyTxEip7702>,
    prepared: PreparedTx,
) -> HandlerResult<TxResult<MegaTypes>> {
    let _ = prepared;
    let caller = req.tx.signer();
    let tx = req.tx.inner();
    let envelope = req.envelope;
    let host = req.host;
    let (intrinsic, initial_state_gas, floor_gas) =
        host.ext_mut().mechanisms.prepared.take().expect("adjust_intrinsic_gas ran");
    let gas_price = effective_gas_price(
        U256::from(tx.max_fee_per_gas),
        U256::from(tx.max_priority_fee_per_gas),
        host.block().basefee,
    );
    let chain_id = host.version().chain_id;
    *host.tx_env_mut() = TxEnvExt {
        origin: caller,
        gas_price,
        chain_id: U256::from(chain_id),
        ..TxEnvExt::default()
    };
    let (execution_gas_limit, reservoir) =
        initial_gas_and_reservoir(host.version(), tx.gas_limit, intrinsic, initial_state_gas);
    let mut tx_gas =
        GasTracker::new_with_execution_gas_and_reservoir(execution_gas_limit, reservoir);
    let checkpoint = host.state_mut().checkpoint();
    let to: TxKind = tx.to.into();

    let settle = |host: &mut Evm<'_, MegaTypes>, result: MessageResult<MegaTypes>| {
        <MegaHooks as TxHandlerHooks<MegaTypes>>::settle_transaction(
            host,
            envelope,
            GasSettlement {
                caller,
                gas_price,
                gas_limit: tx.gas_limit,
                floor_gas,
                initial_state_gas,
                state_refund: 0,
                result,
            },
        )
    };
    let settle_oog = |host: &mut Evm<'_, MegaTypes>| {
        let features = host.version().features;
        host.state_mut().rollback(checkpoint, features);
        let ext = host.ext_mut();
        ext.tx_start_pending = None;
        ext.tx_start_charge = 0;
        ext.tx_start_bytes = 0;
        settle(host, runtime_oog_result(execution_gas_limit, reservoir))
    };

    // The list rule: decided before the list is applied, KV first, then state growth, against
    // the transaction's limits (no frame budget exists yet).
    let entries = tx.authorizations().len() as u64;
    let (kv_room, growth_room) = {
        let ext = host.ext();
        let room = |i: usize| {
            (ext.cfg.limits[i] as i64 - ext.tx_net[i] - if i == 2 { ext.tx_growth } else { 0 })
                .max(0) as u64
        };
        (room(1), room(2))
    };
    let mut skip = None;
    if !host.ext().exempt &&
        !crate::off("auth_list_rule") &&
        (entries > kv_room || entries > growth_room)
    {
        let (records, created) = simulate(host, chain_id, caller, tx.authorizations())?;
        let limits = host.ext().cfg.limits;
        if records > kv_room {
            skip = Some((1u8, limits[1]));
        } else if created > growth_room {
            skip = Some((3u8, limits[2]));
        }
    }

    let mut accepted = Vec::new();
    if skip.is_none() {
        let mut charges = Charges {
            inner: RuntimeAuthCharges::new(host.version(), &mut tx_gas, caller, tx.to, tx.value),
            accepted: Vec::new(),
        };
        let oog = apply_auth_list(host, chain_id, tx.authorizations(), &mut charges)?;
        accepted = charges.accepted;
        if oog {
            return settle_oog(host);
        }
    }
    let mut authorities: Vec<Address> =
        accepted.iter().map(|(a, _)| *a).filter(|a| *a != caller).collect();
    authorities.sort_unstable();
    authorities.dedup();
    let records = authorities.len() as u64;
    let created = accepted.iter().filter(|(_, existed)| !existed).count() as u64;

    // The first frame's start record, as the journal stands after the application, then the
    // history of every record made outside a frame, in one charge.
    crate::tx::first_frame_start(host, caller, to, tx.value, 0);
    if host.ext().prices_history && !crate::off("auth_history") {
        let first = host.ext().tx_start_charge;
        let auth = records * WRITE_RECORD_SIZE * CPHB;
        let before = tx_gas.reservoir();
        if tx_gas.spend_state(first + auth).is_err() {
            return settle_oog(host);
        }
        let paid = (first + auth).min(before);
        host.ext_mut().mechanisms.start_from_reservoir = Some(paid.saturating_sub(auth).min(first));
    } else if host.ext().prices_history {
        host.ext_mut().mechanisms.start_from_reservoir = Some(0);
    }
    let frame =
        prepare_initial_frame(host, caller, tx.nonce, to, &tx.input, tx.value, &mut tx_gas)?;
    // A first frame the runtime gas phase cannot pay is settled here, as Satin opens no frame for
    // it: an inspector sees none.
    if frame.runtime_oog {
        return settle_oog(host);
    }
    // The authorities' records and created accounts are the transaction's own: kept whatever the
    // first frame does; the first frame's budget is what they leave.
    {
        let ext = host.ext_mut();
        if !crate::off("auth_counts") {
            let add = [(records * WRITE_RECORD_SIZE) as i64, records as i64];
            for (i, a) in add.into_iter().enumerate() {
                ext.tx_net[i] += a;
                ext.frames[0].budget[i] = ext.frames[0].budget[i].saturating_sub(a as u64);
            }
            ext.tx_growth += created as i64;
            ext.frames[0].budget[2] = ext.frames[0].budget[2].saturating_sub(created);
        }
        ext.mechanisms.pre_frame_stop = skip;
        // The authorities' history is reported only once nothing can take the list
        // back (a pre-execution out-of-gas takes the records back with it).
        if ext.prices_history && !crate::off("auth_history") && !crate::off("auth_history_report") {
            ext.body_history += records * WRITE_RECORD_SIZE * CPHB;
            ext.body_bytes += records * WRITE_RECORD_SIZE;
        }
    }
    // An applied authority that is the block beneficiary: detained from the start.
    let beneficiary = host.block().beneficiary;
    if authorities.contains(&beneficiary) ||
        (caller == beneficiary && accepted.iter().any(|(a, _)| *a == caller))
    {
        let ext = host.ext_mut();
        if crate::detention::detains(ext) &&
            ext.detention.cap != u64::MAX &&
            !crate::off("auth_beneficiary")
        {
            let cap = ext.detention.cap;
            ext.detention.limit = Some(ext.detention.limit.map_or(cap, |l| l.min(cap)));
        }
    }
    let result = execute_initial_frame(host, frame, &mut tx_gas, execution_gas_limit, reservoir)?;
    settle(host, result)
}
