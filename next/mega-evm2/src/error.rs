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
//! | the block's input contradicting its parent or the schedule; a SALT snapshot of another parent; a fork the engine has no rules for | [`Disposition::Internal`] |
//! | a pre-block call: a database error declared fatal | [`Disposition::Internal`] |
//! | a pre-block call: any other database error, another execution error, a result that is not a success; a missing parent beacon block root | [`Disposition::InvalidBlock`] |
//! | the EIP-7997 factory absent, without code or with other code in the block in which Satin activates; a SequencerRegistry with code the deployment cannot migrate; a registry the resolution finds absent, with other code or naming no system address | [`Disposition::InvalidBlock`] |
//! | a read of the factory check, of a system contract's deployment, of the due-change predicate, of the L1 block info before the transactions, of the live system address | [`Disposition::Internal`], whatever the database declares |
//! | a transaction's validation or execution: a database error declared fatal, a bucket capacity read that fails or is below the minimum (whatever the snapshot's backend declares), another fatal error | [`Disposition::Internal`] |
//! | a transaction's validation or execution: a database error not declared fatal | [`Disposition::InvalidBlock`] |
//! | the block's bookkeeping outside a transaction's execution: the sender's account, the L1 footprint scalar | [`Disposition::Internal`] |
//! | a candidate the block's rules refuse: the activation policy, the block's gas limit, its packing budgets, its footprint room, the transaction's own validity | [`Disposition::Refused`] |
//! | a candidate whose sender changed after it ran, or that ran for another block | [`Disposition::Retry`] |
//!
//! Reverts, halts, limit stops and valid failed deposits are a transaction's outcomes, never one of
//! these errors. A database error keeps its own type and fatality through evm2's erased wrappers
//! ([`database_error`]).

use crate::{
    chain::{Fork, JsonError},
    salt::{BucketId, SaltLookupError, SnapshotId},
};
use alloy_primitives::{Address, B256, U256};
use evm2::{DatabaseError, interpreter::InstrStop, registry::HandlerError};
use std::{boxed::Box, string::String};

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
    /// The check of the EIP-7997 factory in the block in which Satin activates.
    FactoryCheck,
    /// A system contract's deployment of the pre-block sequence, the SequencerRegistry's included.
    Deployment,
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
    pub bucket: Option<BucketId>,
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
    /// The block's timestamp does not fit 64 bits.
    TimestampTooLarge,
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
    /// The SALT snapshot is of the state after another block than the block's parent.
    SaltSnapshotParent { parent_hash: B256, snapshot: SnapshotId },
}

impl core::fmt::Display for InputError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TimestampTooLarge => f.write_str("block timestamp does not fit 64 bits"),
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
            Self::SaltSnapshotParent { parent_hash, snapshot } => write!(
                f,
                "SALT snapshot of the state after {} (root {}), the block's parent is {parent_hash}",
                snapshot.parent_hash, snapshot.state_root
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
    /// In the block in which Satin activates, the EIP-7997 factory's address holds no account
    /// (`None`) or an account whose code hash is not the factory's (`SATIN-ACT-005`).
    Create2Factory { code_hash: Option<B256> },
    /// The SequencerRegistry's address holds code the deployment cannot migrate
    /// (`SATIN-REG-008`).
    UnmigratableRegistry { code_hash: B256 },
    /// The live system address cannot be resolved after the other pre-block steps
    /// (`SATIN-REG-004`).
    SystemAddress(SystemAddressFault),
    /// A transaction's execution failed for a reason that is not the transaction's: a database
    /// error, a bucket capacity, another fatal error.
    Execution(HandlerError),
}

/// Why the block's live system address cannot be resolved (`SATIN-REG-004`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemAddressFault {
    /// No account at the registry's address.
    RegistryAbsent,
    /// The registry's account holds other code than version 2.0.0.
    RegistryCode(B256),
    /// The registry's slot 0 (`_currentSystemAddress`) is zero.
    ZeroSystemAddress,
}

/// A failure of a block's execution, with its phase, context and disposition (boxed: an error is
/// rare, and a result that may hold one stays small).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockError(Box<Failure>);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Failure {
    kind: BlockErrorKind,
    phase: Phase,
    disposition: Disposition,
    context: ErrorContext,
}

impl BlockError {
    /// The block's input contradicts its context: no verdict on the block.
    pub(crate) fn input(error: InputError) -> Self {
        Self(Box::new(Failure {
            kind: BlockErrorKind::Input(error),
            phase: Phase::BlockInput,
            disposition: Disposition::Internal,
            context: ErrorContext { address: None, slot: None, bucket: None, transaction: None },
        }))
    }

    /// A read of the block's own in `phase`: always an internal error, whatever the database
    /// declares (`SATIN-PBLK-006`, `SATIN-REG-007`, `SATIN-VAL-010`).
    pub(crate) fn read(phase: Phase, error: DatabaseError, context: ErrorContext) -> Self {
        Self(Box::new(Failure {
            kind: BlockErrorKind::Database(error),
            phase,
            disposition: Disposition::Internal,
            context,
        }))
    }

    /// A pre-block call that failed in execution: a database error declared fatal is internal;
    /// any other failure invalidates the block (`SATIN-PBLK-006`).
    pub(crate) fn system_call(target: Address, error: HandlerError) -> Self {
        let disposition = match database_error(&error) {
            Some(e) if e.is_fatal() => Disposition::Internal,
            _ => Disposition::InvalidBlock,
        };
        Self(Box::new(Failure {
            kind: BlockErrorKind::SystemCall { target, error },
            phase: Phase::PreBlockCall,
            disposition,
            context: ErrorContext::account(target),
        }))
    }

    /// A pre-block call that did not succeed (`SATIN-PBLK-006`).
    pub(crate) fn system_call_failed(target: Address, stop: InstrStop) -> Self {
        Self(Box::new(Failure {
            kind: BlockErrorKind::SystemCallFailed { target, stop },
            phase: Phase::PreBlockCall,
            disposition: Disposition::InvalidBlock,
            context: ErrorContext::account(target),
        }))
    }

    pub(crate) fn missing_parent_beacon_root(target: Address) -> Self {
        Self(Box::new(Failure {
            kind: BlockErrorKind::MissingParentBeaconRoot,
            phase: Phase::PreBlockCall,
            disposition: Disposition::InvalidBlock,
            context: ErrorContext::account(target),
        }))
    }

    pub(crate) fn genesis_parent_beacon_root(root: B256) -> Self {
        Self(Box::new(Failure {
            kind: BlockErrorKind::GenesisParentBeaconRoot(root),
            phase: Phase::BlockInput,
            disposition: Disposition::InvalidBlock,
            context: ErrorContext { address: None, slot: None, bucket: None, transaction: None },
        }))
    }

    /// The EIP-7997 factory the block in which Satin activates requires is not at its address
    /// (`SATIN-ACT-005`): an invalid block.
    pub(crate) fn create2_factory(address: Address, code_hash: Option<B256>) -> Self {
        Self(Box::new(Failure {
            kind: BlockErrorKind::Create2Factory { code_hash },
            phase: Phase::FactoryCheck,
            disposition: Disposition::InvalidBlock,
            context: ErrorContext::account(address),
        }))
    }

    /// The SequencerRegistry holds code its deployment cannot migrate (`SATIN-REG-008`): an
    /// invalid block.
    pub(crate) fn unmigratable_registry(address: Address, code_hash: B256) -> Self {
        Self(Box::new(Failure {
            kind: BlockErrorKind::UnmigratableRegistry { code_hash },
            phase: Phase::Deployment,
            disposition: Disposition::InvalidBlock,
            context: ErrorContext::account(address),
        }))
    }

    /// The live system address cannot be resolved (`SATIN-REG-004`): an invalid block.
    pub(crate) fn system_address(address: Address, fault: SystemAddressFault) -> Self {
        let slot = matches!(fault, SystemAddressFault::ZeroSystemAddress).then_some(U256::ZERO);
        Self(Box::new(Failure {
            kind: BlockErrorKind::SystemAddress(fault),
            phase: Phase::SystemAddressResolution,
            disposition: Disposition::InvalidBlock,
            context: ErrorContext { slot, ..ErrorContext::account(address) },
        }))
    }

    /// A transaction's execution that failed for a reason that is not the transaction's
    /// ([`CandidateError::from_execution`] decides which those are): a SALT lookup internal,
    /// whatever its snapshot's backend wraps; a database error by its fatality; everything else
    /// internal (`SATIN-VAL-010`, `SATIN-STATE-007`).
    pub(crate) fn execution(error: HandlerError, tx: TxContext) -> Self {
        // The read of the live system address the transaction is classified against: internal,
        // whatever the database declares (`SATIN-REG-007`).
        if system_address_read_failure(&error).is_some() {
            let context = ErrorContext::account(crate::SEQUENCER_REGISTRY).with_transaction(tx);
            return Self(Box::new(Failure {
                kind: BlockErrorKind::Execution(error),
                phase: Phase::SystemAddressResolution,
                disposition: Disposition::Internal,
                context,
            }));
        }
        let bucket = salt_failure(&error).map(|s| s.bucket);
        let disposition = match database_error(&error) {
            Some(e) if !e.is_fatal() && bucket.is_none() => Disposition::InvalidBlock,
            _ => Disposition::Internal,
        };
        Self(Box::new(Failure {
            kind: BlockErrorKind::Execution(error),
            phase: Phase::Transaction,
            disposition,
            context: ErrorContext { bucket, transaction: Some(tx), ..ErrorContext::default() },
        }))
    }

    pub fn kind(&self) -> &BlockErrorKind {
        &self.0.kind
    }

    pub fn phase(&self) -> Phase {
        self.0.phase
    }

    pub fn disposition(&self) -> Disposition {
        self.0.disposition
    }

    pub fn context(&self) -> &ErrorContext {
        &self.0.context
    }

    /// The database error behind the failure, its type and fatality kept, when there is one.
    pub fn database_error(&self) -> Option<&DatabaseError> {
        match &self.0.kind {
            BlockErrorKind::Database(e) => Some(e),
            BlockErrorKind::SystemCall { error, .. } | BlockErrorKind::Execution(error) => {
                database_error(error)
            }
            _ => None,
        }
    }

    pub fn is_internal(&self) -> bool {
        matches!(self.0.disposition, Disposition::Internal)
    }

    pub fn is_invalid_block(&self) -> bool {
        matches!(self.0.disposition, Disposition::InvalidBlock)
    }
}

impl core::fmt::Display for BlockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match &self.0.kind {
            BlockErrorKind::Input(e) => write!(f, "{e}"),
            BlockErrorKind::Database(e) => match self.0.phase {
                Phase::DueChangeRead | Phase::SystemAddressResolution => {
                    write!(f, "registry read: {e:?}")
                }
                Phase::L1BlockInfoRead => write!(f, "L1 block info read: {e:?}"),
                Phase::FactoryCheck => write!(f, "factory read: {e:?}"),
                Phase::Deployment => write!(f, "system contract read: {e:?}"),
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
            BlockErrorKind::Create2Factory { code_hash: None } => {
                f.write_str("the EIP-7997 factory is absent in the block in which Satin activates")
            }
            BlockErrorKind::Create2Factory { code_hash: Some(h) } => write!(
                f,
                "the EIP-7997 factory's code hash is {h} in the block in which Satin activates"
            ),
            BlockErrorKind::UnmigratableRegistry { code_hash } => {
                write!(f, "the SequencerRegistry holds code {code_hash} no deployment migrates")
            }
            BlockErrorKind::SystemAddress(SystemAddressFault::RegistryAbsent) => {
                f.write_str("no live system address: the SequencerRegistry is absent")
            }
            BlockErrorKind::SystemAddress(SystemAddressFault::RegistryCode(h)) => {
                write!(f, "no live system address: the SequencerRegistry holds code {h}")
            }
            BlockErrorKind::SystemAddress(SystemAddressFault::ZeroSystemAddress) => {
                f.write_str("no live system address: the SequencerRegistry's slot 0 is zero")
            }
            BlockErrorKind::Execution(e) => write!(f, "EVM reported invalid transaction: {e:?}"),
        }
    }
}

impl core::error::Error for BlockError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match &self.0.kind {
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
    /// It ran under other rules than the session's: a simulation's in a consensus session, or
    /// consensus rules in a simulation ([`Disposition::Retry`]).
    OtherRules { simulated: bool },
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
            HandlerError::External(_) => {
                database_error(&error).is_some() || salt_failure(&error).is_some()
            }
            _ => false,
        };
        if block_level {
            Self::Block(BlockError::execution(error, tx))
        } else {
            Self::Refused(Refusal::Transaction(error))
        }
    }

    pub fn disposition(&self) -> Disposition {
        match self {
            Self::Refused(_) => Disposition::Refused,
            Self::Stale { .. } | Self::Foreign { .. } | Self::OtherRules { .. } => {
                Disposition::Retry
            }
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
            Self::OtherRules { simulated: true } => {
                f.write_str("outcome of a simulation committed to a consensus block")
            }
            Self::OtherRules { simulated: false } => {
                f.write_str("outcome under consensus rules committed to a simulated block")
            }
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
        HandlerError::Fatal(e) | HandlerError::External(e) => find_erased(e),
        _ => None,
    }
}

/// The first error of type `T` an erased error is or wraps: evm2's own wrappers are opened by
/// type (a transparent wrapper's source skips the error it wraps, and an `AnyError` hides its
/// inner error behind its type), then the source chain is followed.
fn find_erased<'e, T: core::error::Error + 'static>(
    error: &'e (dyn core::error::Error + 'static),
) -> Option<&'e T> {
    let mut link = Some(error);
    let mut hops = 0;
    while let Some(e) = link {
        if let Some(found) = e.downcast_ref::<T>() {
            return Some(found);
        }
        let inner: Option<&'e (dyn core::error::Error + 'static)> =
            if let Some(any) = e.downcast_ref::<evm2::AnyError>() {
                any.downcast_ref::<T>()
                    .map(|x| x as _)
                    .or_else(|| any.downcast_ref::<DatabaseError>().map(|x| x as _))
                    .or_else(|| any.downcast_ref::<SaltLookupError>().map(|x| x as _))
                    .or_else(|| any.downcast_ref::<evm2::ExecutionError>().map(|x| x as _))
                    .or_else(|| any.downcast_ref::<evm2::HostError>().map(|x| x as _))
                    .or_else(|| any.downcast_ref::<evm2::LoadError>().map(|x| x as _))
                    .or_else(|| any.downcast_ref::<HandlerError>().map(|x| x as _))
            } else if let Some(x) = e.downcast_ref::<evm2::ExecutionError>() {
                match x {
                    evm2::ExecutionError::Database(db) => Some(db as _),
                    evm2::ExecutionError::Fatal(any) => Some(any as _),
                }
            } else if let Some(x) = e.downcast_ref::<evm2::HostError>() {
                match x {
                    evm2::HostError::Execution(x) => Some(x as _),
                    evm2::HostError::Halt(_) => None,
                }
            } else if let Some(x) = e.downcast_ref::<evm2::LoadError>() {
                match x {
                    evm2::LoadError::Database(db) => Some(db as _),
                    evm2::LoadError::ColdLoadSkipped => None,
                }
            } else if let Some(x) = e.downcast_ref::<HandlerError>() {
                match x {
                    HandlerError::Database(db) => Some(db as _),
                    HandlerError::Fatal(e) | HandlerError::External(e) => Some(e as _),
                    _ => None,
                }
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

/// The failed read of the live system address behind a handler error, when there is one.
pub fn system_address_read_failure(
    error: &HandlerError,
) -> Option<&crate::tx::SystemAddressReadFailed> {
    match error {
        HandlerError::Fatal(e) => e.downcast_ref(),
        _ => None,
    }
}

/// The failed SALT lookup behind a handler error, through evm2's erased wrappers as a database
/// error is found ([`database_error`]): whichever charge site failed, and however it carried the
/// failure out.
fn salt_failure(error: &HandlerError) -> Option<&SaltLookupError> {
    match error {
        HandlerError::Fatal(e) | HandlerError::External(e) => find_erased(e),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evm2::{AnyError, ExecutionError, HostError, LoadError};

    #[derive(Debug)]
    struct Unavailable;

    impl core::fmt::Display for Unavailable {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("unavailable")
        }
    }

    impl core::error::Error for Unavailable {}

    fn db(fatal: bool) -> DatabaseError {
        DatabaseError::new(Unavailable, fatal)
    }

    fn tx() -> TxContext {
        TxContext { index: 3, sender: Address::with_last_byte(0x50), ty: 2 }
    }

    /// Every way evm2 can carry a database error, for a database error of `fatal`.
    fn carriers(fatal: bool) -> [HandlerError; 7] {
        [
            HandlerError::Database(db(fatal)),
            HandlerError::Fatal(AnyError::new(db(fatal))),
            HandlerError::Fatal(AnyError::new(ExecutionError::Database(db(fatal)))),
            HandlerError::Fatal(AnyError::new(ExecutionError::Fatal(AnyError::new(db(fatal))))),
            HandlerError::External(AnyError::new(HostError::Execution(ExecutionError::Database(
                db(fatal),
            )))),
            HandlerError::Fatal(AnyError::new(LoadError::Database(db(fatal)))),
            HandlerError::External(AnyError::new(HandlerError::Database(db(fatal)))),
        ]
    }

    #[test]
    fn test_a_database_error_is_found_through_every_wrapper_with_its_fatality() {
        for fatal in [true, false] {
            for error in carriers(fatal) {
                let found = database_error(&error).unwrap_or_else(|| panic!("{error:?}"));
                assert!(found.downcast_ref::<Unavailable>().is_some(), "{error:?}");
                assert_eq!(found.is_fatal(), fatal, "{error:?}");
            }
        }
        for error in [
            HandlerError::Fatal(AnyError::from("not a database error")),
            HandlerError::External(AnyError::new(crate::satin_ext::Refusal("refused"))),
            HandlerError::Fatal(AnyError::new(HostError::Halt(InstrStop::OutOfGas))),
            HandlerError::Fatal(AnyError::new(LoadError::ColdLoadSkipped)),
            HandlerError::MissingChainId,
        ] {
            assert!(database_error(&error).is_none(), "{error:?}");
        }
    }

    #[test]
    fn test_a_pre_block_call_is_routed_by_the_databases_fatality_through_every_wrapper() {
        let target = Address::with_last_byte(0x35);
        for fatal in [true, false] {
            for error in carriers(fatal) {
                let routed = BlockError::system_call(target, error);
                let expected =
                    if fatal { Disposition::Internal } else { Disposition::InvalidBlock };
                assert_eq!(routed.disposition(), expected, "{routed:?}");
                assert_eq!(routed.phase(), Phase::PreBlockCall);
                assert_eq!(routed.context().address, Some(target));
                assert_eq!(routed.database_error().map(|e| e.is_fatal()), Some(fatal));
            }
        }
        // Any other failure of the call, and a call that does not succeed, invalidate the block.
        let other = BlockError::system_call(target, HandlerError::Fatal(AnyError::from("x")));
        assert_eq!(other.disposition(), Disposition::InvalidBlock);
        assert_eq!(
            BlockError::system_call_failed(target, InstrStop::Revert).disposition(),
            Disposition::InvalidBlock
        );
        assert_eq!(
            BlockError::missing_parent_beacon_root(target).disposition(),
            Disposition::InvalidBlock
        );
    }

    #[test]
    fn test_the_blocks_own_reads_are_internal_whatever_the_fatality() {
        for phase in [
            Phase::DueChangeRead,
            Phase::L1BlockInfoRead,
            Phase::SystemAddressResolution,
            Phase::Bookkeeping,
            Phase::Commit,
        ] {
            for fatal in [true, false] {
                let routed = BlockError::read(phase, db(fatal), ErrorContext::default());
                assert_eq!(routed.disposition(), Disposition::Internal, "{phase:?} {fatal}");
                assert_eq!(routed.phase(), phase);
                assert_eq!(routed.database_error().map(|e| e.is_fatal()), Some(fatal));
            }
        }
    }

    #[test]
    fn test_a_transactions_failure_is_routed_by_its_cause() {
        for fatal in [true, false] {
            for error in carriers(fatal) {
                let routed = CandidateError::from_execution(error.clone(), tx());
                let CandidateError::Block(e) = &routed else { panic!("{error:?}: {routed:?}") };
                let expected =
                    if fatal { Disposition::Internal } else { Disposition::InvalidBlock };
                assert_eq!(e.disposition(), expected, "{error:?}");
                assert_eq!(e.phase(), Phase::Transaction);
                assert_eq!(e.context().transaction, Some(tx()));
                assert_eq!(routed.disposition(), expected);
            }
        }
        // A bucket capacity below the minimum, another fatal error: internal.
        let bucket = BucketId::new(9).unwrap();
        let below = crate::salt::SaltFailure::BelowMinimum { capacity: 255 };
        let salt = SaltLookupError { bucket, failure: below };
        let routed =
            CandidateError::from_execution(HandlerError::Fatal(AnyError::new(salt.clone())), tx());
        let CandidateError::Block(e) = &routed else { panic!("{routed:?}") };
        assert_eq!(e.disposition(), Disposition::Internal);
        assert_eq!(e.context().bucket, Some(bucket));
        assert!(e.database_error().is_none());
        let fatal = CandidateError::from_execution(HandlerError::Fatal(AnyError::from("x")), tx());
        assert_eq!(fatal.disposition(), Disposition::Internal);
        // Carried as an external error, a bucket capacity failure is still the node's.
        let external = HandlerError::External(AnyError::new(salt));
        let routed = CandidateError::from_execution(external, tx());
        let CandidateError::Block(e) = &routed else { panic!("{routed:?}") };
        assert_eq!(e.disposition(), Disposition::Internal);
        assert_eq!(e.context().bucket, Some(bucket));
        // A failed lookup is found through every wrapper a charge site may carry it out in, and
        // stays internal when its snapshot's backend wraps a database error not declared fatal.
        let backend = crate::salt::SaltReadError::Backend(AnyError::new(db(false)));
        let failed = SaltLookupError { bucket, failure: crate::salt::SaltFailure::Read(backend) };
        let any = || AnyError::new(failed.clone());
        for carried in [
            HandlerError::Fatal(any()),
            HandlerError::External(any()),
            HandlerError::Fatal(AnyError::new(ExecutionError::Fatal(any()))),
            HandlerError::External(AnyError::new(HostError::Execution(ExecutionError::Fatal(
                any(),
            )))),
            HandlerError::Fatal(AnyError::new(HandlerError::External(any()))),
        ] {
            let routed = CandidateError::from_execution(carried.clone(), tx());
            let CandidateError::Block(e) = &routed else { panic!("{carried:?}: {routed:?}") };
            assert_eq!(e.disposition(), Disposition::Internal, "{carried:?}");
            assert_eq!(e.context().bucket, Some(bucket), "{carried:?}");
            assert_eq!(e.database_error().map(|d| d.is_fatal()), Some(false), "{carried:?}");
        }
        // The classification read of the live system address: internal whatever the database
        // declares, in the resolution's phase.
        for fatal in [true, false] {
            let read = crate::tx::SystemAddressReadFailed(db(fatal));
            let routed =
                CandidateError::from_execution(HandlerError::Fatal(AnyError::new(read)), tx());
            let CandidateError::Block(e) = &routed else { panic!("{routed:?}") };
            assert_eq!(e.disposition(), Disposition::Internal);
            assert_eq!(e.phase(), Phase::SystemAddressResolution);
            assert_eq!(e.database_error().map(|d| d.is_fatal()), Some(fatal));
        }
        // The transaction's own invalidity refuses it.
        for error in [
            HandlerError::InvalidNonce { expected: 1, got: 2 },
            HandlerError::MissingChainId,
            HandlerError::External(AnyError::new(crate::satin_ext::Refusal("refused"))),
        ] {
            let routed = CandidateError::from_execution(error.clone(), tx());
            assert_eq!(routed, CandidateError::Refused(Refusal::Transaction(error)));
            assert_eq!(routed.disposition(), Disposition::Refused);
        }
    }

    /// The differential gates classify a refusal by its text: the texts stay the engine's.
    #[test]
    fn test_refusal_texts() {
        let cases = [
            (
                Refusal::BlockGasLimit { gas_limit: 7, available: 5 },
                "transaction gas limit 7 is more than blocks available gas 5",
            ),
            (
                Refusal::Packing { budget: Budget::DataSize, used: 9, limit: 8 },
                "Block transactions data limit reached: block_used=9 >= limit=8",
            ),
            (
                Refusal::Packing { budget: Budget::KvUpdates, used: 9, limit: 8 },
                "Block KV update limit reached: block_used=9 >= limit=8",
            ),
            (
                Refusal::Packing { budget: Budget::StateGrowth, used: 9, limit: 8 },
                "Block state growth limit reached: block_used=9 >= limit=8",
            ),
            (
                Refusal::DaFootprint { footprint: 4, available: 3 },
                "transaction DA footprint 4 above the block's available 3",
            ),
            (Refusal::OnlyDeposits, "unexpected non-deposit transaction in an activation block"),
            (
                Refusal::Transaction(HandlerError::InvalidNonce { expected: 1, got: 2 }),
                "EVM reported invalid transaction: InvalidNonce { expected: 1, got: 2 }",
            ),
        ];
        for (refusal, text) in cases {
            assert_eq!(refusal.to_string(), text);
            assert_eq!(CandidateError::Refused(refusal).to_string(), text);
        }
        let sender = Address::with_last_byte(0x50);
        assert_eq!(
            CandidateError::Stale { sender }.to_string(),
            format!("stale outcome: sender {sender}")
        );
        let read = BlockError::read(Phase::DueChangeRead, db(true), ErrorContext::default());
        assert!(read.to_string().starts_with("registry read: "), "{read}");
        let read = BlockError::read(Phase::L1BlockInfoRead, db(true), ErrorContext::default());
        assert!(read.to_string().starts_with("L1 block info read: "), "{read}");
    }
}
