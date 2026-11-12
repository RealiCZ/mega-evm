//! The block hashes `BLOCKHASH` is served, as the engine records them (`SATIN-WIT-014`).
//!
//! - **The record** ([`BlockHashReads`], `Lite::block_hashes`): every hash the running attempt (one
//!   execution of a transaction, or one pre-block system call) was served, with the number it was
//!   served for. It is written by the engine's own `BLOCKHASH` ([`mega_blockhash`]) once the host
//!   has answered, so it sits above every cache: a hash the EVM's overlay, or a node's state cache
//!   below it, answers without reaching the backing database is recorded as one the database
//!   answers. It is emptied when the next attempt begins: by the ledger when a transaction or a
//!   system call begins, and by a block session before a candidate's checks, so that a candidate
//!   refused before it runs leaves an empty record. A single transaction's EVM keeps the record of
//!   its last transaction that began; a caller that reads it after a refusal empties it first
//!   ([`BlockHashReads::begin_attempt`]).
//! - **What is not a read** (`SATIN-WIT-012`): a `BLOCKHASH` of the current block, of a future
//!   block or of a block outside the last [`BLOCK_HASH_HISTORY`] blocks pushes zero and asks the
//!   host nothing; one refused in a frame whose volatile-data access is off never reaches the
//!   instruction; one the host fails to answer has no hash. None of them is recorded.
//! - **The exports**: an attempt's own ([`BlockHashReads::served`], and a candidate's), and the
//!   block's union over every attempt of a block session — its pre-block calls, and the candidates
//!   it executed and dropped, included — which a session that executed for another one hands over
//!   as a [`BlockHashExport`].

use crate::{MegaTypes, error::BlockIdentity};
use alloy_primitives::{B256, map::HashMap};
use evm2::{
    constants::BLOCK_HASH_HISTORY,
    interpreter::{Host, Word},
};
use evm2_macros::instruction;
use std::{collections::BTreeMap, vec::Vec};

/// The block hashes the running attempt's `BLOCKHASH` was served, by block number.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockHashReads {
    served: HashMap<Word, B256>,
}

impl BlockHashReads {
    /// Begins an attempt: the previous attempt's reads are forgotten.
    pub fn begin_attempt(&mut self) {
        self.served.clear();
    }

    /// Records that `BLOCKHASH` was served `hash` for block `number`.
    pub fn record(&mut self, number: Word, hash: B256) {
        self.served.insert(number, hash);
    }

    /// Whether the running attempt was served no hash.
    pub fn is_empty(&self) -> bool {
        self.served.is_empty()
    }

    /// The hashes the running attempt was served, ascending by block number: its export.
    pub fn served(&self) -> Vec<(Word, B256)> {
        let mut served: Vec<(Word, B256)> = self.served.iter().map(|(n, h)| (*n, *h)).collect();
        served.sort_unstable_by_key(|(number, _)| *number);
        served
    }
}

/// `BLOCKHASH`: evm2's rule — the hash of one of the last [`BLOCK_HASH_HISTORY`] blocks, zero for
/// the current block, a future one or an older one, none of which asks the host — and the record
/// of every hash the host served ([`BlockHashReads`]). The record is made here, where the hash is
/// served, and not at the database: the host's answer comes through the EVM's overlay, which
/// answers a number it has answered before without reaching the database under it.
///
/// It stays a volatile read (the `Volatile` wrapper of `detention::wrap_table`, which charges its
/// static gas and refuses it in a frame whose access is off, before this body runs).
#[instruction(EvmTypes = MegaTypes)]
pub fn mega_blockhash(cx: _, [number]: [Word]) -> Result<out> {
    *out = match cx.state.host().block().number.checked_sub(*number) {
        Some(distance) if distance != 0 && distance <= BLOCK_HASH_HISTORY => {
            let hash =
                Host::block_hash(cx.state.host(), number).map_err(|error| cx.state.fail(error))?;
            cx.state.host().ext_mut().block_hashes.record(*number, hash);
            hash.into()
        }
        _ => Word::ZERO,
    };
}

/// The block hashes a block session was served, drained to be merged into the session that builds
/// the block (`BlockSession::drain_block_hash_reads`, `BlockSession::merge_block_hash_reads`):
/// bound to the block and to the rules the session ran under (consensus or a simulation's), as a
/// candidate is.
#[must_use = "the hashes are evidence the block's witness needs: merge them"]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockHashExport {
    pub(crate) block: BlockIdentity,
    pub(crate) simulated: bool,
    pub(crate) hashes: BTreeMap<u64, B256>,
}

impl BlockHashExport {
    pub const fn block(&self) -> BlockIdentity {
        self.block
    }

    /// Whether the session ran under a simulation's switches.
    pub const fn is_simulated(&self) -> bool {
        self.simulated
    }

    /// The hashes served, by block number.
    pub const fn hashes(&self) -> &BTreeMap<u64, B256> {
        &self.hashes
    }
}

/// A [`BlockHashExport`] a merge refused, handed back with the reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedBlockHashExport {
    pub reason: BlockHashMergeRefusal,
    pub export: BlockHashExport,
}

/// Why a merge refused a [`BlockHashExport`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockHashMergeRefusal {
    /// The export is of another block or of other rules than the merging session's, whose block
    /// and rules these are.
    Foreign { block: BlockIdentity, simulated: bool },
    /// The two sessions were served different hashes for block `number`: they did not execute on
    /// the same chain, and neither hash is taken for the other.
    Conflict { number: u64, held: B256, merged: B256 },
}

impl core::fmt::Display for RefusedBlockHashExport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let rules = |simulated| if simulated { "a simulation" } else { "consensus rules" };
        let e = &self.export;
        match self.reason {
            BlockHashMergeRefusal::Foreign { block, simulated } => write!(
                f,
                "block hashes served to block {} on {} ({}) merged into block {} on {} ({})",
                e.block.number,
                e.block.parent_hash,
                rules(e.simulated),
                block.number,
                block.parent_hash,
                rules(simulated)
            ),
            BlockHashMergeRefusal::Conflict { number, held, merged } => write!(
                f,
                "two sessions of block {} on {} were served different hashes of block {number}: \
                 {held} and {merged}",
                e.block.number, e.block.parent_hash
            ),
        }
    }
}

impl core::error::Error for RefusedBlockHashExport {}

#[cfg(test)]
mod tests {
    use super::*;

    /// The record keeps one hash a number, exports them ascending, and is emptied when the next
    /// attempt begins.
    #[test]
    fn blockhash_witness_record_is_ascending_and_emptied_by_the_next_attempt() {
        let (a, b) = (B256::repeat_byte(0xa1), B256::repeat_byte(0xb2));
        let mut reads = BlockHashReads::default();
        assert!(reads.is_empty());
        reads.record(Word::from(9), b);
        reads.record(Word::from(7), a);
        reads.record(Word::from(9), b);
        assert_eq!(reads.served(), [(Word::from(7), a), (Word::from(9), b)]);
        reads.begin_attempt();
        assert!(reads.is_empty());
        assert!(reads.served().is_empty());
    }
}
