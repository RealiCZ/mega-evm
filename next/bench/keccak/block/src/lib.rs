//! Shared by the block timing binary (`kec-block`) and the validating trace capture
//! (`kec-trace`): the recorded corpus as the differential gates read it, and the replay helpers
//! they run it with.

#![allow(missing_docs)]

/// One recorded block and its sparse prestate, the gates' own reader (included, unchanged).
#[path = "../../../../diff/shared/blockdb.rs"]
pub mod blockdb;

/// In-memory inputs and repeated measurements of the gates' block timing (included, unchanged).
#[path = "../../../../diff/shared/src/bench_blocks.rs"]
pub mod bench_blocks;

pub mod replay;

/// The variant this binary was built as, from its Cargo features.
pub const VARIANT: &str = match (cfg!(feature = "asm"), cfg!(feature = "cache")) {
    (false, false) => "baseline",
    (true, false) => "asm",
    (false, true) => "cache",
    (true, true) => "asm-cache",
};
