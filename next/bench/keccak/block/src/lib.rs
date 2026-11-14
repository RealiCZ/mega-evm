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

/// The variant's label: `KEC_VARIANT` (the runner names the build, its ISA flags included), else
/// the Keccak features this binary was built with.
pub fn variant() -> String {
    if let Ok(v) = std::env::var("KEC_VARIANT") {
        return v;
    }
    let features: Vec<&str> = [
        (cfg!(feature = "asm"), "asm"),
        (cfg!(feature = "tiny"), "tiny"),
        (cfg!(feature = "cache"), "cache"),
    ]
    .into_iter()
    .filter_map(|(on, name)| on.then_some(name))
    .collect();
    if features.is_empty() { "baseline".into() } else { features.join("-") }
}
