//! Satin under every tracer and inspector of evm2's inspector crate (`evm2-inspectors`).
//!
//! A node serves its tracing endpoints from that crate: the Geth tracers through
//! `DebugInspector::get_result` (the struct logger, the call, prestate, flat-call, four-byte,
//! mux, no-op, state-gas, ERC-7562, keccak-preimage and JavaScript tracers), the Parity trace,
//! state diff and VM trace through the Parity builder, and the access-list, transfer and
//! Otterscan inspectors on their own. These tests run Satin transactions through the engine's
//! factory with each of them attached and hold the result two ways:
//!
//! - **Inspected versus plain.** Every traced run is checked against the run with no inspector: the
//!   receipt, the state changes, every gas figure and the engine's ledgers are identical
//!   ([`harness::Case::traced`]). Observing a transaction never changes what it does.
//! - **Golden output.** Each scenario's JSON, for every tracer, is pinned in
//!   `golden/<scenario>.json` and read back, so a change in a trace's shape or figures fails the
//!   scenario's test. `MEGA_EVM2_BLESS_TRACES=1` rewrites the files instead of comparing them;
//!   review the diff before committing it.
//!
//! The scenarios are of two kinds: the fifteen the tracer tests of the Satin implementation under
//! review pin (`a_scenarios.rs`, built the same way: the same accounts, code, values and limits,
//! so their output can be compared), and the frames that are hard to trace under Satin
//! (`hard_frames.rs`): a native keyless deployment's creation, intercepted system-contract
//! calls, a first frame that runs out of gas before it starts, failed deposits, caught limit
//! stops, reverted logs, EIP-7702 delegation, precompiles, `SELFDESTRUCT`, gas detention and the
//! reservoir. `tracers.rs` holds what each tracer must show of them, by name.

#[path = "../common/mod.rs"]
mod common;

mod golden;
mod hard_frames;
mod harness;
mod reference_scenarios;
mod tracers;

/// The call tracer's frames, depth first.
pub fn tracers_frames(call: &serde_json::Value) -> Vec<&serde_json::Value> {
    fn walk<'a>(f: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
        out.push(f);
        for c in f["calls"].as_array().into_iter().flatten() {
            walk(c, out);
        }
    }
    let mut out = Vec::new();
    walk(call, &mut out);
    out
}
