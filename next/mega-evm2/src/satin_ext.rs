//! Per-transaction state of EIP-7702 authorities, keyless deployment, `MegaAccessControl` and
//! Oracle hints, kept apart from `ledger.rs` so a rewrite of the ledger's internals merges with
//! one field (`Lite::mechanisms`).

use alloy_primitives::{Address, Bytes, U256};

/// A string error for refusals evm2's `HandlerError` has no variant for (Satin's
/// `InvalidTransaction::Str`).
#[derive(Debug)]
pub struct Refusal(pub &'static str);

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for Refusal {}

pub fn refuse(reason: &'static str) -> evm2::registry::HandlerError {
    evm2::registry::HandlerError::External(evm2::AnyError::new(Refusal(reason)))
}

#[derive(Clone, Debug, Default)]
pub struct MechanismState {
    /// What `adjust_intrinsic_gas` settled (intrinsic, initial state gas, floor), for handlers
    /// that execute a `PreparedTx` they cannot read.
    pub prepared: Option<(u64, u64, u64)>,
    /// How the first frame's start charge was paid when it was charged on the transaction's gas
    /// after validation (types 0, 1 and 2 in `before_initial_frame`, EIP-7702 in its handler): the
    /// part the reservoir paid.
    pub start_from_reservoir: Option<u64>,
    /// The stop the transaction's own frame returns before it runs (an authorization list the KV
    /// or state-growth limit skips).
    pub pre_frame_stop: Option<(u8, u64)>,
    /// `MegaAccessControl`: the shallowest frame depth whose volatile-data access is off.
    pub disabled_from: Option<usize>,
    /// Oracle hints forwarded to the service (payload bytes counted on the transaction).
    pub hints: Vec<(Address, alloy_primitives::B256, Bytes)>,
    pub hint_bytes: u64,
    /// Keyless deployment in flight: the call's bookkeeping between its start and its answer.
    pub keyless: Option<Keyless>,
    /// The depth-0 frame `before` built on the keyless dispatcher.
    pub keyless_frame: bool,
    /// A deposit whose body is over the data-size limit (a failed deposit).
    pub body_refused: bool,
    /// A deposit's first-frame start target, parked once the deposit paid its created caller.
    pub deposit_start: Option<Address>,
    /// The first frame's start records parked before the frame (`tx::park_start`): the target,
    /// its record count and whether the start logs a transfer.
    pub start_records: Option<(Address, i64, bool)>,
    /// The running execution is a pre-block system call (`BlockExecutor::system_call`), not a
    /// transaction: the Oracle's storage reads the chain and asks the service nothing.
    pub system_call: bool,
    /// The body check of a user transaction, waiting for the intrinsic-gas hook (Satin checks
    /// the body after the environment's rules, which evm2 checks before the hook): its number of
    /// authorization entries.
    pub body_check: Option<u64>,
    /// The running opcode's load of the block beneficiary was refused
    /// (`MegaTypes::before_account_load`), waiting for the opcode's wrapper to revert the frame.
    pub load_refused: bool,
}

impl MechanismState {
    pub fn reset(&mut self) {
        let mut hints = core::mem::take(&mut self.hints);
        hints.clear();
        *self = Self { hints, ..Self::default() };
    }
}

#[derive(Clone, Debug, Default)]
pub struct Keyless {
    pub signer: Address,
    pub deploy: Address,
    pub signer_nonce_before: u64,
    pub value: U256,
    pub empty_deposit_refused: bool,
}

pub const ACCESS_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04]);

/// Satin `VolatileDataAccess::as_u8` for each read site.
pub mod access {
    pub const BLOCK_NUMBER: u8 = 0;
    pub const TIMESTAMP: u8 = 1;
    pub const COINBASE: u8 = 2;
    pub const GAS_LIMIT: u8 = 4;
    pub const BASE_FEE: u8 = 5;
    pub const PREV_RANDAO: u8 = 6;
    pub const BLOCK_HASH: u8 = 7;
    pub const BLOB_BASE_FEE: u8 = 8;
    pub const BENEFICIARY: u8 = 10;
    pub const ORACLE: u8 = 11;
    pub const SLOT_NUM: u8 = 12;
}

fn sel(sig: &str) -> [u8; 4] {
    alloy_primitives::keccak256(sig)[..4].try_into().unwrap()
}

/// Whether a read in a frame at `depth` is refused (`MegaAccessControl` switched access off at
/// or above it).
pub fn refuses(mechanisms: &MechanismState, depth: usize) -> bool {
    !crate::off("access_control") && mechanisms.disabled_from.is_some_and(|from| depth >= from)
}

/// `VolatileDataAccessDisabled(uint8 accessType)`.
pub fn disabled_revert(access: u8) -> Vec<u8> {
    let mut out = sel("VolatileDataAccessDisabled(uint8)").to_vec();
    out.extend_from_slice(&alloy_primitives::B256::from(U256::from(access)).0);
    out
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AcMethod {
    Disable,
    Enable,
    IsDisabled,
}

pub fn access_control_method(input: &[u8]) -> Option<AcMethod> {
    let s = input.get(..4)?;
    if s == sel("disableVolatileDataAccess()") {
        Some(AcMethod::Disable)
    } else if s == sel("enableVolatileDataAccess()") {
        Some(AcMethod::Enable)
    } else if s == sel("isVolatileDataAccessDisabled()") {
        Some(AcMethod::IsDisabled)
    } else {
        None
    }
}

/// The `MegaAccessControl` answer to a call at `depth` (its caller is `depth − 1`).
pub fn access_control_answer(
    mechanisms: &mut MechanismState,
    method: AcMethod,
    depth: usize,
) -> (bool, Vec<u8>) {
    let caller = depth.checked_sub(1);
    match method {
        AcMethod::Disable => {
            if let Some(c) = caller {
                mechanisms.disabled_from = Some(mechanisms.disabled_from.map_or(c, |f| f.min(c)));
            }
            (true, Vec::new())
        }
        AcMethod::Enable => match (caller, mechanisms.disabled_from) {
            (Some(c), Some(from)) if c > from => (false, sel("DisabledByParent()").to_vec()),
            _ => {
                mechanisms.disabled_from = None;
                (true, Vec::new())
            }
        },
        AcMethod::IsDisabled => {
            let disabled =
                caller.is_some_and(|c| mechanisms.disabled_from.is_some_and(|from| c >= from));
            (true, alloy_primitives::B256::from(U256::from(disabled as u8)).0.to_vec())
        }
    }
}

/// `sendHint(bytes32,bytes)`.
pub fn is_send_hint(input: &[u8]) -> bool {
    input.get(..4) == Some(&sel("sendHint(bytes32,bytes)")[..])
}

/// alloy's ABI decode of `sendHint(bytes32 topic, bytes data)`.
pub fn decode_hint(input: &[u8]) -> Option<(alloy_primitives::B256, Bytes)> {
    let body = input.get(4..)?;
    let topic = alloy_primitives::B256::from_slice(body.get(..32)?);
    let offset: usize = U256::from_be_slice(body.get(32..64)?).try_into().ok()?;
    let len: usize =
        U256::from_be_slice(body.get(offset..offset.checked_add(32)?)?).try_into().ok()?;
    let start = offset.checked_add(32)?;
    Some((topic, Bytes::copy_from_slice(body.get(start..start.checked_add(len)?)?)))
}

/// `refuses` for the running frame.
pub fn refused_here(
    state: &mut evm2::interpreter::InterpreterState<'_, '_, crate::MegaTypes>,
) -> bool {
    let depth = state.message().depth as usize;
    refuses(&state.host().ext().mechanisms, depth)
}

/// Satin `AdditionalLimit::hold_caller`: the transaction's limits (no frame budget) checked as a
/// caller resumes from a failed child. Only an Oracle hint, which stays on the transaction when
/// the frame that sent it fails, can leave the transaction over a limit there.
pub fn hold_caller(ext: &crate::Lite) -> Option<(u8, u64)> {
    if ext.exempt || ext.mechanisms.hint_bytes == 0 || crate::off("hold_caller") {
        return None;
    }
    (0..3).find_map(|i| {
        let total = ext.tx_net[i] +
            if i == 2 { ext.tx_growth } else { 0 } +
            ext.frames.iter().map(|f| f.net[i]).sum::<i64>();
        crate::over(total, ext.cfg.limits[i]).then_some((crate::KIND[i], ext.cfg.limits[i]))
    })
}
