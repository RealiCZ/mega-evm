//! The engine's typed errors: a configuration's ([`ConfigError`]), a block's ([`BlockError`]) and a
//! candidate transaction's ([`CandidateError`]).
//!
//! Each block-level error carries what failed (its kind and source), the execution phase it failed
//! in, the account, slot, bucket or transaction it failed at, and its [`Disposition`]: what the
//! failure decides about the block, by the classification of `SATIN-VAL-010` and `SATIN-PBLK-006`.
//! The display strings are diagnostics only; a caller decides by [`BlockError::disposition`].
//!
//! | Cause and phase | Disposition |
//! | --- | --- |
//! | malformed Satin key or schedule, missing or zero bootstrap value | [`ConfigError`], before execution |
//! | the block's input contradicting its parent or the schedule; a fork the engine has no rules for | [`Disposition::Internal`] |
//! | a pre-block call: a database error declared fatal | [`Disposition::Internal`] |
//! | a pre-block call: any other database error, another execution error, a result that is not a success; a missing parent beacon block root | [`Disposition::InvalidBlock`] |
//! | a read of the due-change predicate, of the L1 block info before the transactions, of the live system address | [`Disposition::Internal`], whatever the database declares |
//! | a transaction's validation or execution: a database error declared fatal, a bucket capacity read that fails or is below the minimum, another fatal error | [`Disposition::Internal`] |
//! | a transaction's validation or execution: a database error not declared fatal | [`Disposition::InvalidBlock`] |
//! | the block's bookkeeping outside a transaction's execution: the sender's account, the L1 footprint scalar | [`Disposition::Internal`] |
//! | a candidate the block's rules refuse: the activation policy, the block's gas limit, its packing budgets, its footprint room, the transaction's own validity | [`Disposition::Refused`] |
//! | a candidate whose sender changed after it ran, or that ran for another block | [`Disposition::Retry`] |
//!
//! Reverts, halts, limit stops and valid failed deposits are a transaction's outcomes, never one of
//! these errors. A database error keeps its own type and fatality through evm2's erased wrappers
//! ([`database_error`]).

use crate::{
    SaltBelowMinimum,
    chain::{Fork, JsonError},
};
use alloy_primitives::{Address, B256, U256};
use evm2::{DatabaseError, interpreter::InstrStop, registry::HandlerError};
use std::string::String;

/// What a failure decides about the block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Disposition {
    /// An internal error of the node: no verdict on the block, which cannot be executed past it.
    Internal,
    /// The block is invalid.
    InvalidBlock,
    /// The candidate is refused: a builder leaves it out, and a received block that contains it is
    /// invalid.
    Refused,
    /// The candidate went stale or ran for another block: a builder executes it again; no verdict
    /// on the transaction or the block.
    Retry,
}

/// Where in a block's execution a failure happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// The block's input checked against its parent and the configuration, before any execution.
    BlockInput,
    /// A pre-block EVM call: EIP-2935, EIP-4788, the registry's `applyPendingChanges()`.
    PreBlockCall,
    /// The registry's due-change read of the pre-block sequence.
    DueChangeRead,
    /// The pre-block read of the L1 block info.
    L1BlockInfoRead,
    /// The read of the live system address a transaction is classified against.
    SystemAddressResolution,
    /// The block's own reads for a transaction outside its execution: the sender's account before
    /// it runs, the L1 block contract's footprint gas scalar.
    Bookkeeping,
    /// The transaction's validation and execution.
    Transaction,
    /// The commit of a candidate: its sender read again.
    Commit,
}

/// The transaction a failure met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TxContext {
    /// The candidate's position: the transactions the block had committed when it ran.
    pub index: usize,
    pub sender: Address,
    /// Its EIP-2718 type.
    pub ty: u8,
}

/// Where a failure happened, as far as the engine knows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ErrorContext {
    /// The account the failing read or call was of.
    pub address: Option<Address>,
    /// The storage slot the failing read was of.
    pub slot: Option<U256>,
    /// The SALT bucket whose capacity failed.
    pub bucket: Option<u64>,
    /// The transaction the failure met.
    pub transaction: Option<TxContext>,
}

impl ErrorContext {
    pub const fn account(address: Address) -> Self {
        Self { address: Some(address), slot: None, bucket: None, transaction: None }
    }

    pub const fn with_transaction(mut self, tx: TxContext) -> Self {
        self.transaction = Some(tx);
        self
    }
}

/// A chain configuration the engine refuses, before any block executes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    /// The text is not JSON.
    Json(JsonError),
    /// A value that must be an object is not.
    NotAnObject { what: &'static str },
    /// A required key is absent.
    MissingKey { key: &'static str },
    /// A key starting with `satin`, in any ASCII case, that is not exactly `satinTime`
    /// (`SATIN-ACT-002`).
    UnknownSatinKey { key: String },
    /// A key's value is not of the kind the key takes.
    InvalidValue { key: &'static str, expected: &'static str, found: &'static str },
    /// A Mega spec below Satin on the ladder is not scheduled (`SATIN-ACT-011`).
    LadderGap { fork: Fork },
    /// A Mega spec is scheduled after the spec above it on the ladder (`SATIN-ACT-011`).
    LadderInverted { fork: Fork, time: u64, above: Fork, above_time: u64 },
    /// A base fork is not active at `satinTime` (`SATIN-ACT-003`).
    BaseForkInactive { fork: Fork, time: Option<u64>, satin_time: u64 },
    /// A registry bootstrap value is absent (`SATIN-REG-009`).
    MissingBootstrap { key: &'static str },
    /// A registry bootstrap value is zero (`SATIN-REG-009`).
    ZeroBootstrap { key: &'static str },
}

impl core::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "chain configuration is not JSON: {e}"),
            Self::NotAnObject { what } => write!(f, "{what} is not a JSON object"),
            Self::MissingKey { key } => write!(f, "chain configuration lacks `{key}`"),
            Self::UnknownSatinKey { key } => {
                write!(f, "chain configuration key `{key}` is not `satinTime`")
            }
            Self::InvalidValue { key, expected, found } => {
                write!(f, "`{key}` must be {expected}, found {found}")
            }
            Self::LadderGap { fork } => {
                write!(f, "Satin is scheduled but {fork} below it on the spec ladder is not")
            }
            Self::LadderInverted { fork, time, above, above_time } => write!(
                f,
                "{fork} at {time} is scheduled after {above} at {above_time}, the spec above it"
            ),
            Self::BaseForkInactive { fork, time, satin_time } => match time {
                Some(time) => {
                    write!(f, "base fork {fork} at {time} is not active at satinTime {satin_time}")
                }
                None => write!(f, "base fork {fork} is not scheduled, Satin is at {satin_time}"),
            },
            Self::MissingBootstrap { key } => {
                write!(f, "Satin is scheduled but the bootstrap value `{key}` is absent")
            }
            Self::ZeroBootstrap { key } => {
                write!(f, "Satin is scheduled but the bootstrap value `{key}` is zero")
            }
        }
    }
}

impl core::error::Error for ConfigError {}

/// A block input the engine cannot execute as given: it contradicts its parent context or the
/// configuration, or names a fork or header field the engine has no rules for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputError {
    /// The block's timestamp is not a Satin timestamp: the legacy engine executes it.
    NotSatin { timestamp: u64, satin_time: Option<u64> },
    /// A scheduled fork the engine has no rules for is active at the block.
    UnsupportedFork { fork: Fork, timestamp: u64 },
    /// The header has excess blob gas, which an OP chain's header never has.
    ExcessBlobGas { excess_blob_gas: u64 },
    /// The block's number does not follow its parent's (block zero has no parent; every other
    /// block has one).
    Number { number: u64, parent: Option<u64> },
    /// The header names another parent than the parent context's.
    ParentHash { parent_hash: B256, parent: B256 },
    /// The parent is later than the block.
    ParentAfterBlock { parent_timestamp: u64, timestamp: u64 },
    /// A block, or block zero's child's parent, at another timestamp than the configuration's
    /// genesis allows.
    GenesisTimestamp { timestamp: u64, genesis_timestamp: u64 },
}

impl core::fmt::Display for InputError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotSatin { timestamp, satin_time } => match satin_time {
                Some(s) => write!(f, "block at {timestamp} is before satinTime {s}"),
                None => write!(f, "block at {timestamp}: Satin is not scheduled"),
            },
            Self::UnsupportedFork { fork, timestamp } => {
                write!(f, "{fork} is active at {timestamp}; the engine has no rules for it")
            }
            Self::ExcessBlobGas { excess_blob_gas } => {
                write!(f, "header has excess blob gas {excess_blob_gas}")
            }
            Self::Number { number, parent } => match parent {
                Some(p) => write!(f, "block {number} does not follow its parent {p}"),
                None => write!(f, "block {number} has no parent"),
            },
            Self::ParentHash { parent_hash, parent } => {
                write!(f, "header's parent hash {parent_hash} is not the parent's {parent}")
            }
            Self::ParentAfterBlock { parent_timestamp, timestamp } => {
                write!(f, "parent at {parent_timestamp} is after the block at {timestamp}")
            }
            Self::GenesisTimestamp { timestamp, genesis_timestamp } => write!(
                f,
                "timestamp {timestamp} contradicts the genesis timestamp {genesis_timestamp}"
            ),
        }
    }
}

/// What failed in a block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockErrorKind {
    /// The block's input ([`InputError`]).
    Input(InputError),
    /// A database read of the block's own failed.
    Database(DatabaseError),
    /// A pre-block call to `target` failed in execution.
    SystemCall { target: Address, error: HandlerError },
    /// A pre-block call to `target` ran and did not succeed.
    SystemCallFailed { target: Address, stop: InstrStop },
    /// The block carries no parent beacon block root (EIP-4788).
    MissingParentBeaconRoot,
    /// Block zero's parent beacon block root is not zero (`SATIN-ACT-012`).
    GenesisParentBeaconRoot(B256),
    /// A transaction's execution failed for a reason that is not the transaction's: a database
    /// error, a bucket capacity, another fatal error.
    Execution(HandlerError),
}

/// A failure of a block's execution, with its phase, context and disposition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockError {
    kind: BlockErrorKind,
    phase: Phase,
    disposition: Disposition,
    context: ErrorContext,
}

impl BlockError {
    /// The block's input contradicts its context: no verdict on the block.
    pub(crate) const fn input(error: InputError) -> Self {
        Self {
            kind: BlockErrorKind::Input(error),
            phase: Phase::BlockInput,
            disposition: Disposition::Internal,
            context: ErrorContext { address: None, slot: None, bucket: None, transaction: None },
        }
    }

    /// A read of the block's own in `phase`: always an internal error, whatever the database
    /// declares (`SATIN-PBLK-006`, `SATIN-REG-007`, `SATIN-VAL-010`).
    pub(crate) fn read(phase: Phase, error: DatabaseError, context: ErrorContext) -> Self {
        Self {
            kind: BlockErrorKind::Database(error),
            phase,
            disposition: Disposition::Internal,
            context,
        }
    }

    /// A pre-block call that failed in execution: a database error declared fatal is internal;
    /// any other failure invalidates the block (`SATIN-PBLK-006`).
    pub(crate) fn system_call(target: Address, error: HandlerError) -> Self {
        let disposition = match database_error(&error) {
            Some(e) if e.is_fatal() => Disposition::Internal,
            _ => Disposition::InvalidBlock,
        };
        Self {
            kind: BlockErrorKind::SystemCall { target, error },
            phase: Phase::PreBlockCall,
            disposition,
            context: ErrorContext::account(target),
        }
    }

    /// A pre-block call that did not succeed (`SATIN-PBLK-006`).
    pub(crate) const fn system_call_failed(target: Address, stop: InstrStop) -> Self {
        Self {
            kind: BlockErrorKind::SystemCallFailed { target, stop },
            phase: Phase::PreBlockCall,
            disposition: Disposition::InvalidBlock,
            context: ErrorContext::account(target),
        }
    }

    pub(crate) const fn missing_parent_beacon_root(target: Address) -> Self {
        Self {
            kind: BlockErrorKind::MissingParentBeaconRoot,
            phase: Phase::PreBlockCall,
            disposition: Disposition::InvalidBlock,
            context: ErrorContext::account(target),
        }
    }

    pub(crate) const fn genesis_parent_beacon_root(root: B256) -> Self {
        Self {
            kind: BlockErrorKind::GenesisParentBeaconRoot(root),
            phase: Phase::BlockInput,
            disposition: Disposition::InvalidBlock,
            context: ErrorContext { address: None, slot: None, bucket: None, transaction: None },
        }
    }

    /// A transaction's execution that failed for a reason that is not the transaction's
    /// ([`CandidateError::from_execution`] decides which those are): a database error by its
    /// fatality, everything else internal (`SATIN-VAL-010`, `SATIN-STATE-007`).
    pub(crate) fn execution(error: HandlerError, tx: TxContext) -> Self {
        let disposition = match database_error(&error) {
            Some(e) if !e.is_fatal() => Disposition::InvalidBlock,
            _ => Disposition::Internal,
        };
        let bucket = salt_failure(&error).map(|s| s.bucket);
        Self {
            kind: BlockErrorKind::Execution(error),
            phase: Phase::Transaction,
            disposition,
            context: ErrorContext { bucket, transaction: Some(tx), ..ErrorContext::default() },
        }
    }

    pub const fn kind(&self) -> &BlockErrorKind {
        &self.kind
    }

    pub const fn phase(&self) -> Phase {
        self.phase
    }

    pub const fn disposition(&self) -> Disposition {
        self.disposition
    }

    pub const fn context(&self) -> &ErrorContext {
        &self.context
    }

    /// The database error behind the failure, its type and fatality kept, when there is one.
    pub fn database_error(&self) -> Option<&DatabaseError> {
        match &self.kind {
            BlockErrorKind::Database(e) => Some(e),
            BlockErrorKind::SystemCall { error, .. } | BlockErrorKind::Execution(error) => {
                database_error(error)
            }
            _ => None,
        }
    }

    pub const fn is_internal(&self) -> bool {
        matches!(self.disposition, Disposition::Internal)
    }

    pub const fn is_invalid_block(&self) -> bool {
        matches!(self.disposition, Disposition::InvalidBlock)
    }
}

impl core::fmt::Display for BlockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.kind {
            BlockErrorKind::Input(e) => write!(f, "{e}"),
            BlockErrorKind::Database(e) => match self.phase {
                Phase::DueChangeRead | Phase::SystemAddressResolution => {
                    write!(f, "registry read: {e:?}")
                }
                Phase::L1BlockInfoRead => write!(f, "L1 block info read: {e:?}"),
                _ => write!(f, "{e:?}"),
            },
            BlockErrorKind::SystemCall { target, error } => {
                write!(f, "system call {target}: {error:?}")
            }
            BlockErrorKind::SystemCallFailed { target, stop } => {
                write!(f, "system call {target} failed: {stop:?}")
            }
            BlockErrorKind::MissingParentBeaconRoot => {
                f.write_str("missing parent beacon block root (EIP-4788)")
            }
            BlockErrorKind::GenesisParentBeaconRoot(root) => {
                write!(f, "block zero's parent beacon block root {root} is not zero")
            }
            BlockErrorKind::Execution(e) => write!(f, "EVM reported invalid transaction: {e:?}"),
        }
    }
}

impl core::error::Error for BlockError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match &self.kind {
            BlockErrorKind::Database(e) => Some(e),
            BlockErrorKind::SystemCall { error, .. } | BlockErrorKind::Execution(error) => {
                Some(error)
            }
            _ => None,
        }
    }
}

/// Which of the block's packing budgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Budget {
    DataSize,
    KvUpdates,
    StateGrowth,
}

impl Budget {
    pub const ALL: [Self; 3] = [Self::DataSize, Self::KvUpdates, Self::StateGrowth];

    const fn name(self) -> &'static str {
        match self {
            Self::DataSize => "Block transactions data limit",
            Self::KvUpdates => "Block KV update limit",
            Self::StateGrowth => "Block state growth limit",
        }
    }
}

/// A rule of the block that refuses a candidate (`SATIN-BLK-*`, `SATIN-BASE-006`), or the
/// transaction's own invalidity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A non-deposit transaction in a block that admits only deposits (`SATIN-BASE-006`).
    OnlyDeposits,
    /// The transaction's gas limit is above what the block's gas limit has left.
    BlockGasLimit { gas_limit: u64, available: u64 },
    /// A packing budget the block has used up.
    Packing { budget: Budget, used: u64, limit: u64 },
    /// The transaction's data-availability footprint is above what the block's gas limit has
    /// left for footprints.
    DaFootprint { footprint: u64, available: u64 },
    /// The engine refused the transaction in its validation.
    Transaction(HandlerError),
}

impl core::fmt::Display for Refusal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::OnlyDeposits => {
                f.write_str("unexpected non-deposit transaction in an activation block")
            }
            Self::BlockGasLimit { gas_limit, available } => write!(
                f,
                "transaction gas limit {gas_limit} is more than blocks available gas {available}"
            ),
            Self::Packing { budget, used, limit } => {
                write!(f, "{} reached: block_used={used} >= limit={limit}", budget.name())
            }
            Self::DaFootprint { footprint, available } => write!(
                f,
                "transaction DA footprint {footprint} above the block's available {available}"
            ),
            Self::Transaction(e) => write!(f, "EVM reported invalid transaction: {e:?}"),
        }
    }
}

/// The identity of the block a session executes: its number and its parent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockIdentity {
    pub number: u64,
    pub parent_hash: B256,
}

/// Why a candidate transaction was not executed or not committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateError {
    /// The block's rules refuse it ([`Disposition::Refused`]).
    Refused(Refusal),
    /// Its sender's nonce or balance changed after it ran ([`Disposition::Retry`]).
    Stale { sender: Address },
    /// It ran for another block ([`Disposition::Retry`]).
    Foreign { expected: BlockIdentity, found: BlockIdentity },
    /// The block cannot be executed past it: an internal error or an invalid block. The session
    /// keeps the error and refuses everything after it.
    Block(BlockError),
}

impl CandidateError {
    /// A transaction's failure in evm2's validation or execution, routed by its cause: a database
    /// error, a bucket capacity failure or another fatal error is the block's
    /// ([`BlockError::execution`]); anything else refuses the transaction.
    pub(crate) fn from_execution(error: HandlerError, tx: TxContext) -> Self {
        let block_level = match &error {
            HandlerError::Database(_) | HandlerError::Fatal(_) => true,
            HandlerError::External(_) => database_error(&error).is_some(),
            _ => false,
        };
        if block_level {
            Self::Block(BlockError::execution(error, tx))
        } else {
            Self::Refused(Refusal::Transaction(error))
        }
    }

    pub const fn disposition(&self) -> Disposition {
        match self {
            Self::Refused(_) => Disposition::Refused,
            Self::Stale { .. } | Self::Foreign { .. } => Disposition::Retry,
            Self::Block(e) => e.disposition(),
        }
    }
}

impl core::fmt::Display for CandidateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Refused(r) => write!(f, "{r}"),
            Self::Stale { sender } => write!(f, "stale outcome: sender {sender}"),
            Self::Foreign { expected, found } => write!(
                f,
                "outcome of block {} on {} committed to block {} on {}",
                found.number, found.parent_hash, expected.number, expected.parent_hash
            ),
            Self::Block(e) => write!(f, "{e}"),
        }
    }
}

impl core::error::Error for CandidateError {}

impl From<Refusal> for CandidateError {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<BlockError> for CandidateError {
    fn from(error: BlockError) -> Self {
        Self::Block(error)
    }
}

/// The database error behind a handler error, through evm2's erased wrappers: the error itself, or
/// one a fatal or external error carries, at its top or anywhere down its source chain.
pub fn database_error(error: &HandlerError) -> Option<&DatabaseError> {
    match error {
        HandlerError::Database(e) => Some(e),
        HandlerError::Fatal(e) | HandlerError::External(e) => erased_database_error(e),
        _ => None,
    }
}

/// The database error an erased error is or wraps: evm2's own wrappers are opened by type (a
/// transparent wrapper's source skips the error it wraps), then the source chain is followed.
fn erased_database_error<'e>(
    error: &'e (dyn core::error::Error + 'static),
) -> Option<&'e DatabaseError> {
    let mut link = Some(error);
    let mut hops = 0;
    while let Some(e) = link {
        if let Some(db) = e.downcast_ref::<DatabaseError>() {
            return Some(db);
        }
        let inner: Option<&'e (dyn core::error::Error + 'static)> =
            if let Some(any) = e.downcast_ref::<evm2::AnyError>() {
                any.downcast_ref::<DatabaseError>()
                    .map(|db| db as _)
                    .or_else(|| any.downcast_ref::<evm2::ExecutionError>().map(|x| x as _))
                    .or_else(|| any.downcast_ref::<evm2::HostError>().map(|x| x as _))
                    .or_else(|| any.downcast_ref::<evm2::LoadError>().map(|x| x as _))
                    .or_else(|| any.downcast_ref::<HandlerError>().map(|x| x as _))
            } else if let Some(x) = e.downcast_ref::<evm2::ExecutionError>() {
                match x {
                    evm2::ExecutionError::Database(db) => return Some(db),
                    evm2::ExecutionError::Fatal(any) => Some(any as _),
                }
            } else if let Some(x) = e.downcast_ref::<evm2::HostError>() {
                match x {
                    evm2::HostError::Execution(x) => Some(x as _),
                    evm2::HostError::Halt(_) => None,
                }
            } else if let Some(x) = e.downcast_ref::<evm2::LoadError>() {
                match x {
                    evm2::LoadError::Database(db) => return Some(db),
                    evm2::LoadError::ColdLoadSkipped => None,
                }
            } else if let Some(x) = e.downcast_ref::<HandlerError>() {
                return database_error(x);
            } else {
                None
            };
        link = inner.or_else(|| e.source());
        // An erased error can name itself as its own source; a chain is never this long.
        hops += 1;
        if hops > 64 {
            return None;
        }
    }
    None
}

/// The bucket-capacity failure behind a handler error, when there is one.
fn salt_failure(error: &HandlerError) -> Option<&SaltBelowMinimum> {
    match error {
        HandlerError::Fatal(e) | HandlerError::External(e) => e.downcast_ref(),
        _ => None,
    }
}
