//! The regular-gas invariant of Satin's give-back rule and the transaction's gas conservation on
//! every gate case (`mega_evm2::regular_check`): within a frame, regular gas never rises except by
//! the unused regular gas a returning child hands back, a frame's whole gas never rises but by what
//! a child hands back, a message hands back at most what it received, and the give-backs never
//! exceed the charges made.
//!
//! Each candidate run under Satin's rule, plain and inspected, is checked as it runs; a run under
//! a reference's rule (a class's rerun) is not. A run whose check found a violation carries the
//! violations in its record under [`KEY`], a field no reference record has: the case differs
//! whatever its class, and the candidate checks made before any rerun
//! (`gate::candidate_differences`) never waive it. Every gate prints its count as
//! `INVARIANT regular-gas gate=<name> checked=<cases> violations=<cases>` ([`Tally::line`]), and
//! `gate.sh` sums them into one line per scope.

use mega_evm2::Lite;
use serde_json::{Map, Value, json};
use std::cell::Cell;

/// The record field of a run's violations.
pub const KEY: &str = "regular_gas_invariant";

thread_local! {
    static RUNS: Cell<u64> = const { Cell::new(0) };
}

/// The violations the transaction just run found, `ext` being its engine's state.
pub fn violations(ext: &Lite) -> Vec<String> {
    ext.regular_check.violations.clone()
}

/// Counts one checked run.
pub fn ran() {
    RUNS.with(|r| r.set(r.get() + 1));
}

/// The runs checked since the last call.
pub fn take_runs() -> u64 {
    RUNS.with(|r| r.replace(0))
}

/// Adds `violations` to `record` under [`KEY`], when there are any.
pub fn note(record: &mut Map<String, Value>, violations: Vec<String>) {
    if !violations.is_empty() {
        record.insert(KEY.into(), json!(violations));
    }
}

/// A gate's count of the cases checked and of those whose runs found a violation.
#[derive(Debug, Default)]
pub struct Tally {
    pub checked: u64,
    pub violations: u64,
}

impl Tally {
    /// One case `case`: checked when both its runs, plain and inspected, were; violated when
    /// either record carries [`KEY`], which is printed as `INVARIANT-VIOLATION <case> <run>
    /// <violations>`.
    pub fn case(&mut self, case: &str, both_runs: bool, records: [&Value; 2]) {
        if both_runs {
            self.checked += 1;
        }
        let mut violated = false;
        for (run, r) in ["plain", "inspected"].into_iter().zip(records) {
            if let Some(v) = r.get(KEY) {
                println!("INVARIANT-VIOLATION {case} {run} {v}");
                violated = true;
            }
        }
        self.violations += violated as u64;
    }

    pub fn line(&self, gate: &str) -> String {
        format!(
            "INVARIANT regular-gas gate={gate} checked={} violations={}",
            self.checked, self.violations
        )
    }
}
