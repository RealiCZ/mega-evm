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
///
/// What a block builder's service must do, which the engine does not do for it:
/// - **Interior mutability behind `&self`.** A hint may change the answers that follow it, and both
///   calls take `&self`.
/// - **One handle per block session.** The engine tells the service nothing about which execution
///   asks or hints: `answer` is called at every read of every attempt (dropped and repeated ones
///   included), and `hint` at every admitted hint of every execution. A handle that stays on its
///   session's thread needs only a `Cell` or a `RefCell`; a `Mutex` or atomics are needed only for
///   node state that several handles share behind it.
/// - **The builder marks the attempts.** The builder makes each `execute_candidate` call, so it
///   tells its handle where an attempt begins and ends.
/// - **Answers pinned for a re-execution.** A candidate executed again after the Mega System
///   Transaction that persists its values (see [`OracleObserver`]) must be answered the values it
///   persisted: a service that answers, say, the time of each ask would answer a new value, ask for
///   another Mega System Transaction, and never let the transaction in.
/// - **Hints outside any attempt.** Hints also arrive from the pre-block system calls (during
///   `start_block`, before the builder holds the session), from deposits and Mega System
///   Transactions (whose reads are never answered), and again from every re-execution; the service
///   must accept them.
///
/// The trait does not require `Send` or `Sync`, as [`crate::salt::SaltSnapshot`] does not: a
/// session and its services are built on the thread that runs them, and a builder whose service is
/// thread-safe clones its concrete `Arc` into each thread and turns it into an
/// `Arc<dyn OracleService>` there. Bounds would shut out a single-threaded guest.
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
///
/// **Pairing a record with a candidate.** A block session hands over an attempt's record during the
/// [`execute_candidate`](crate::block::BlockSession::execute_candidate) call that ran it, once the
/// attempt has ended and before the call returns: the record of a candidate is the one handed over
/// during the call that returned it, and an attempt that returned an error is handed over as not
/// executed. The index in the boundary does not identify a candidate: a candidate executed again,
/// or another one executed before it is committed, carries the same index, and a session that
/// executes for another one numbers by its own commits.
///
/// **One observer per session.** Connect each block session to its own observer (another
/// `Arc`). The boundary carries no tag naming the session: an observer object is already one, and
/// it can hold whatever the caller needs to know about its session; a tag would add a field to
/// every boundary for the case an observer per session covers.
///
/// **Execute again after a Mega System Transaction.** A candidate executed before a Mega System
/// Transaction that writes a slot it read must be executed again after it before it is committed:
/// the commit checks the block, the room and the sender, not the Oracle slots the candidate read,
/// and the candidate's reads, gas and state were computed over the slot's old value.
///
/// The trait does not require `Send` or `Sync` (see [`OracleService`]).
pub trait OracleObserver {
    /// One execution's accesses, in execution order, handed over once it has ended, so that every
    /// write a revert took back is already gone. A block session hands over every attempt, a
    /// candidate the builder then drops or executes again included.
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
    /// A transaction. In a block session `index` is the number of transactions the executing
    /// session had committed when the attempt began, the index the transaction takes if it is
    /// committed next (a single transaction's EVM numbers from zero the transactions it executed
    /// to an outcome). `executed` is whether the attempt returned a candidate — an outcome, which
    /// the block includes once the builder commits it — or not: refused, or failed with an error,
    /// nothing of it included and none of its writes reported (the reads it made before the error
    /// are kept). Only a block session hands over a transaction without an outcome; a single
    /// transaction's EVM hands over none: its caller has the error.
    Transaction { index: usize, executed: bool },
}

/// An access to the Oracle's storage.
///
/// Each read carries what a builder needs to decide it on its own (`SATIN-ORCL-012`,
/// `SATIN-ORCL-013`), with no look at the writes:
/// - a pushed read with `own`: `value` must equal `held`, or the transaction is left out; nothing
///   is persisted for it (a node replaying the block pushes the transaction's own write there);
/// - a pushed read without `own`: the slot must hold `value` before the transaction, so a served
///   value that differs from `held` is persisted before it; all such reads of one slot must agree,
///   or the transaction is left out;
/// - a read that pushed nothing asks nothing of the slot's value.
///
/// The writes that stand are reported as well, in their place, for a builder that wants them; the
/// reads do not depend on them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OracleAccess {
    /// An `SLOAD` in the Oracle's own frame (`SATIN-ORCL-002`). A read whose frame cannot pay its
    /// cold access, or is stopped at gas detention's crossing, is made and pushes nothing
    /// (`SATIN-ORCL-004`). A read is kept whatever its frame does afterwards, with what it saw at
    /// the moment it was made.
    Read {
        slot: U256,
        /// The value the read pushed: the service's answer when it gave one, else `held`; for a
        /// read that pushed nothing, the value it would have pushed.
        value: U256,
        /// The value the slot held at the read, through the journal: the value a node with no
        /// service pushes there, a write of the execution's own in effect included.
        held: U256,
        /// Whether `held` is a write of this execution still in effect at the read: one whose
        /// frame, and every frame above it, had not failed by then. Without one, `held` is the
        /// value the slot held before the execution.
        own: bool,
        /// Whether the service gave `value` (also when it equals `held`).
        served: bool,
        /// Whether the read pushed a value: its cold access was paid.
        pushed: bool,
    },
    /// An `SSTORE` to the Oracle's storage that completed and stands, one that writes the value the
    /// slot holds included: one whose frame, or any frame above it, the transaction included,
    /// failed is not reported. A block's pre-block deployment of the Oracle is no execution and is
    /// not reported.
    Write { slot: U256, value: U256 },
}

/// The oracle service and observer an EVM is connected to; both absent by default. Neither `Send`
/// nor `Sync`, as the traits do not require it (see [`OracleService`]).
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
    /// The running execution's writes in effect, counted by slot (a slot without one has no
    /// entry): exactly the writes in `accesses`, which a failed frame's undo takes out of both.
    live: HashMap<U256, u32>,
    /// The index the next transaction's boundary carries in a single transaction's EVM.
    index: usize,
    /// A block session hands its executions over itself, once each has ended
    /// (`BlockSession::execute_candidate`); a single transaction's EVM hands each transaction over
    /// at its outcome.
    session: bool,
}

impl OracleState {
    pub fn new(services: OracleServices) -> Self {
        Self { services, ..Self::default() }
    }

    pub const fn services(&self) -> &OracleServices {
        &self.services
    }

    /// The EVM is a block session's, which hands its executions over itself.
    pub(crate) const fn hand_over_by_session(&mut self) {
        self.session = true;
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

    /// Records a read of `slot` that pushed (or would have pushed) `value` over the slot's `held`.
    #[inline]
    pub(crate) fn read(&mut self, slot: U256, value: U256, held: U256, served: bool, pushed: bool) {
        if self.observing() {
            let own = self.live.contains_key(&slot);
            self.accesses.push(OracleAccess::Read { slot, value, held, own, served, pushed });
        }
    }

    /// Records a write that stands as long as no frame it is in fails ([`Self::undo_writes`]).
    #[inline]
    pub(crate) fn write(&mut self, slot: U256, value: U256) {
        if self.observing() {
            *self.live.entry(slot).or_default() += 1;
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
            match self.accesses[i] {
                OracleAccess::Read { .. } => {
                    self.accesses.swap(kept, i);
                    kept += 1;
                }
                OracleAccess::Write { slot, .. } => {
                    if let Some(n) = self.live.get_mut(&slot) {
                        *n -= 1;
                        if *n == 0 {
                            self.live.remove(&slot);
                        }
                    }
                }
            }
        }
        self.accesses.truncate(kept);
    }

    /// A transaction, a system call or a block session's attempt begins: nothing of an earlier one
    /// is kept.
    pub(crate) fn begin(&mut self) {
        self.accesses.clear();
        self.live.clear();
    }

    /// A block session's attempt of the transaction at `index` has ended, `executed` when it
    /// returned a candidate: handed over, an attempt without one with none of its writes.
    pub(crate) fn end_attempt(&mut self, index: usize, executed: bool) {
        if !executed {
            self.undo_writes(0);
        }
        self.deliver(OracleBoundary::Transaction { index, executed });
    }

    /// A transaction's outcome is decided: a failed transaction's writes are all taken back. A
    /// single transaction's EVM hands its accesses over now, as executed; a block session hands
    /// them over once the attempt has ended ([`Self::end_attempt`]).
    pub(crate) fn end_transaction(&mut self, success: bool) {
        if !success {
            self.undo_writes(0);
        }
        if !self.session {
            let index = self.index;
            self.deliver(OracleBoundary::Transaction { index, executed: true });
            self.index += 1;
        }
    }

    /// A pre-block system call to `target` has ended (`success`: its frame succeeded).
    pub(crate) fn end_system_call(&mut self, target: Address, success: bool) {
        if !success {
            self.undo_writes(0);
        }
        self.deliver(OracleBoundary::SystemCall { target });
    }

    fn deliver(&mut self, boundary: OracleBoundary) {
        if let Some(observer) = &self.services.observer {
            observer.observe(OracleExecution { boundary, accesses: &self.accesses });
        }
        self.begin();
    }
}

/// A transaction handler's result, its outcome decided: a single transaction's EVM hands it over
/// as executed. A result without an outcome (an error) is handed over by nothing here; a block
/// session hands its attempt over as not executed.
pub(crate) fn end_transaction(ext: &mut Lite, result: &HandlerResult<TxResult<MegaTypes>>) {
    if let Ok(r) = result {
        ext.oracle.end_transaction(r.status);
    }
}
