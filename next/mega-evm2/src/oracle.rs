//! The engine's Oracle seam: the node's oracle service, which answers the Oracle's reads and is
//! handed its hints, and an observer of each execution's Oracle reads and standing writes, from
//! which a block builder persists the values the service gave (`SATIN-ORCL-012`) and leaves out a
//! transaction no single value can replay (`SATIN-ORCL-013`).
//!
//! Both are optional and separate: a node that validates or replays a block connects neither, and
//! every read then pushes the slot's value on the chain; a builder may observe without a service.
//! The service is synchronous and infallible: a service that cannot answer answers `None`, and the
//! read pushes the slot's value. The observer changes nothing the engine executes, and nothing it
//! sees travels with an outcome, a receipt or a block.

use crate::{MegaTypes, ledger::Lite};
use alloy_primitives::{Address, B256, Bytes, U256, map::HashMap};
use evm2::{TxResult, registry::HandlerResult};
use std::{sync::Arc, vec::Vec};

/// The node's oracle service.
pub trait OracleService {
    /// The service's value for the Oracle's `slot`, asked at every Oracle read that asks it, each
    /// time it is made (`SATIN-ORCL-002`), so two reads of one slot may be answered two values.
    /// `None` is no value, a failure of the service's included: the read pushes the slot's value.
    /// No read of a system call, a deposit or a Mega System Transaction asks (`SATIN-ORCL-005`).
    fn answer(&self, slot: U256) -> Option<U256>;

    /// An admitted `sendHint` (`SATIN-ORCL-006`, `SATIN-ORCL-007`), handed over synchronously,
    /// before the call returns: its `topic` and `data`, and `from`, the address of the frame that
    /// made the call (the transaction's sender for the transaction's own call). A hint made before
    /// a read reaches the service before that read asks. An input that does not decode as
    /// `sendHint(bytes32,bytes)` is counted and reaches no service (`SATIN-ORCL-009`).
    fn hint(&self, from: Address, topic: B256, data: &[u8]) {
        let _ = (from, topic, data);
    }
}

/// An observer of the Oracle accesses of each execution: each attempt of a block session's
/// transactions, each of its pre-block system calls, and each transaction a single transaction's
/// EVM executes to an outcome.
pub trait OracleObserver {
    /// One execution's accesses, in execution order, handed over once it has ended, so that every
    /// write a revert took back is already gone. A block session hands over every attempt, a
    /// candidate the builder then drops or executes again included: the attempt it commits is the
    /// last one handed over at that index before the commit.
    fn observe(&self, execution: OracleExecution<'_>);
}

/// What an [`OracleObserver`] is handed for one execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OracleExecution<'a> {
    pub boundary: OracleBoundary,
    /// The Oracle's reads and its standing writes, in the order they were made.
    pub accesses: &'a [OracleAccess],
}

/// Which execution an [`OracleExecution`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OracleBoundary {
    /// A block's pre-block system call to `target`.
    SystemCall { target: Address },
    /// A transaction. `index` is its index in the block, the number of transactions the session
    /// committed before it (a single transaction's EVM numbers from zero the transactions it
    /// executed to an outcome). `executed` is whether it has an outcome — the block session's
    /// candidate, which the block includes once it is committed — or none: refused, or failed with
    /// an error, nothing of it included and none of its writes reported (the reads it made before
    /// the error are kept). Only a block session hands over a transaction without an outcome; a
    /// single transaction's EVM hands over none: its caller has the error.
    Transaction { index: usize, executed: bool },
}

/// An access to the Oracle's storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OracleAccess {
    /// An `SLOAD` in the Oracle's own frame (`SATIN-ORCL-002`): the value it pushes, whether the
    /// service gave that value, and whether it was pushed. A read whose frame cannot pay its cold
    /// access, or is stopped at gas detention's crossing, is made and pushes nothing
    /// (`SATIN-ORCL-004`): `pushed` is `false`, and `value` the value the read would have pushed.
    /// A read is kept whatever its frame does afterwards.
    Read { slot: U256, value: U256, served: bool, pushed: bool },
    /// An `SSTORE` to the Oracle's storage that completed and stands, one that writes the value the
    /// slot holds included: one whose frame, or any frame above it, the transaction included,
    /// failed is not reported. A block's pre-block deployment of the Oracle is no execution and is
    /// not reported.
    Write { slot: U256, value: U256 },
}

/// The oracle service and observer an EVM is connected to; both absent by default.
#[derive(Clone, Default)]
pub struct OracleServices {
    pub service: Option<Arc<dyn OracleService>>,
    pub observer: Option<Arc<dyn OracleObserver>>,
}

impl OracleServices {
    /// A service and no observer.
    pub fn service(service: Arc<dyn OracleService>) -> Self {
        Self { service: Some(service), observer: None }
    }
}

impl core::fmt::Debug for OracleServices {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OracleServices")
            .field("service", &self.service.is_some())
            .field("observer", &self.observer.is_some())
            .finish()
    }
}

/// A service answering from fixed values by slot, receiving hints without acting on them: for
/// tests and tools (the differential scenarios' answers).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OracleAnswers(HashMap<U256, U256>);

impl OracleAnswers {
    pub fn insert(&mut self, slot: U256, value: U256) {
        self.0.insert(slot, value);
    }
}

impl FromIterator<(U256, U256)> for OracleAnswers {
    fn from_iter<I: IntoIterator<Item = (U256, U256)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl OracleService for OracleAnswers {
    fn answer(&self, slot: U256) -> Option<U256> {
        self.0.get(&slot).copied()
    }
}

/// A call a [`RecordingOracle`] passed on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OracleCall {
    Answer { slot: U256, answer: Option<U256> },
    Hint { from: Address, topic: B256, data: Bytes },
}

/// A service that passes every call on to `S` and records it, in order: for tests and tools that
/// check what the engine asked and handed over.
#[derive(Debug, Default)]
pub struct RecordingOracle<S = OracleAnswers> {
    inner: S,
    calls: std::sync::Mutex<Vec<OracleCall>>,
}

impl<S> RecordingOracle<S> {
    pub fn new(inner: S) -> Self {
        Self { inner, calls: Default::default() }
    }

    pub const fn inner(&self) -> &S {
        &self.inner
    }

    /// Every call so far, in order.
    pub fn calls(&self) -> Vec<OracleCall> {
        self.lock().clone()
    }

    /// The hints so far, in order: `(from, topic, data)`.
    pub fn hints(&self) -> Vec<(Address, B256, Bytes)> {
        self.lock()
            .iter()
            .filter_map(|c| match c {
                OracleCall::Hint { from, topic, data } => Some((*from, *topic, data.clone())),
                OracleCall::Answer { .. } => None,
            })
            .collect()
    }

    /// Forgets the calls so far.
    pub fn clear(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<OracleCall>> {
        self.calls.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl<S: OracleService> OracleService for RecordingOracle<S> {
    fn answer(&self, slot: U256) -> Option<U256> {
        let answer = self.inner.answer(slot);
        self.lock().push(OracleCall::Answer { slot, answer });
        answer
    }

    fn hint(&self, from: Address, topic: B256, data: &[u8]) {
        self.lock().push(OracleCall::Hint { from, topic, data: Bytes::copy_from_slice(data) });
        self.inner.hint(from, topic, data);
    }
}

/// An EVM's Oracle state (`Lite::oracle`): the services it is connected to, kept across
/// transactions, and the running execution's accesses, recorded only while an observer is
/// connected (nothing is recorded, or allocated, without one).
#[derive(Clone, Debug, Default)]
pub struct OracleState {
    services: OracleServices,
    accesses: Vec<OracleAccess>,
    /// The index the next transaction's boundary carries.
    index: usize,
    /// A block session's attempt is running whose boundary is not delivered yet.
    attempt: bool,
}

impl OracleState {
    pub fn new(services: OracleServices) -> Self {
        Self { services, ..Self::default() }
    }

    pub const fn services(&self) -> &OracleServices {
        &self.services
    }

    /// The service's answer for `slot`, `None` without a service.
    #[inline]
    pub(crate) fn answer(&self, slot: U256) -> Option<U256> {
        self.services.service.as_ref().and_then(|s| s.answer(slot))
    }

    /// Whether a service receives hints (`decode_hint` is skipped without one).
    #[inline]
    pub(crate) fn takes_hints(&self) -> bool {
        self.services.service.is_some()
    }

    pub(crate) fn hint(&self, from: Address, topic: B256, data: &[u8]) {
        if let Some(s) = &self.services.service {
            s.hint(from, topic, data);
        }
    }

    #[inline]
    pub(crate) fn observing(&self) -> bool {
        self.services.observer.is_some()
    }

    #[inline]
    pub(crate) fn read(&mut self, slot: U256, value: U256, served: bool, pushed: bool) {
        if self.observing() {
            self.accesses.push(OracleAccess::Read { slot, value, served, pushed });
        }
    }

    /// Records a write that stands as long as no frame it is in fails ([`Self::undo_writes`]).
    #[inline]
    pub(crate) fn write(&mut self, slot: U256, value: U256) {
        if self.observing() {
            self.accesses.push(OracleAccess::Write { slot, value });
        }
    }

    /// Where a frame starting now begins in the accesses.
    #[inline]
    pub(crate) fn mark(&self) -> usize {
        self.accesses.len()
    }

    /// A failed frame that began at `mark`: its writes, and its children's, are taken back; its
    /// reads stay, in their order.
    pub(crate) fn undo_writes(&mut self, mark: usize) {
        if self.accesses.len() <= mark {
            return;
        }
        let mut kept = mark;
        for i in mark..self.accesses.len() {
            if matches!(self.accesses[i], OracleAccess::Read { .. }) {
                self.accesses.swap(kept, i);
                kept += 1;
            }
        }
        self.accesses.truncate(kept);
    }

    /// A transaction or a system call begins: nothing of an earlier one is kept.
    pub(crate) fn begin(&mut self) {
        self.accesses.clear();
    }

    /// A block session's attempt of the transaction at `index` begins.
    pub(crate) fn begin_attempt(&mut self, index: usize) {
        self.accesses.clear();
        self.index = index;
        self.attempt = true;
    }

    /// A block session's attempt has ended: an attempt whose transaction reached no outcome is
    /// delivered as not executed, none of its writes standing (an executed one was delivered at its
    /// end).
    pub(crate) fn end_attempt(&mut self) {
        if self.attempt {
            self.undo_writes(0);
            let index = self.index;
            self.deliver(OracleBoundary::Transaction { index, executed: false });
        }
    }

    /// A transaction's outcome is decided: a failed transaction's writes are all taken back, and
    /// its accesses are delivered as executed.
    pub(crate) fn end_transaction(&mut self, success: bool) {
        if !success {
            self.undo_writes(0);
        }
        let index = self.index;
        self.deliver(OracleBoundary::Transaction { index, executed: true });
        self.index += 1;
    }

    /// A pre-block system call to `target` has ended (`success`: its frame succeeded).
    pub(crate) fn end_system_call(&mut self, target: Address, success: bool) {
        if !success {
            self.undo_writes(0);
        }
        self.deliver(OracleBoundary::SystemCall { target });
    }

    fn deliver(&mut self, boundary: OracleBoundary) {
        self.attempt = false;
        if let Some(observer) = &self.services.observer {
            observer.observe(OracleExecution { boundary, accesses: &self.accesses });
        }
        self.accesses.clear();
    }
}

/// A transaction handler's result, its outcome decided: delivered to the observer as executed. A
/// result without an outcome (an error) delivers nothing here; a block session delivers its
/// attempt as not executed.
pub(crate) fn end_transaction(ext: &mut Lite, result: &HandlerResult<TxResult<MegaTypes>>) {
    if let Ok(r) = result {
        ext.oracle.end_transaction(r.status);
    }
}
