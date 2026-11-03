//! Shared scenarios and strict differential gates for the Satin engine.
//!
//! The harness modules run the Satin engine (`mega-evm2`) on the shared scenarios and on recorded
//! blocks and render every outcome in the JSON shape the reference harness (`next/reference`)
//! writes, so the gates in `tests/` can compare the two field by field.

pub mod controls;
pub mod driver;
pub mod replay;
pub mod satin_applied;

#[path = "../shared/gate.rs"]
pub mod gate;
