//! The regular-gas invariant of Satin's give-back rule, checked in comparison builds
//! (`SATIN-HIST-012`): every give-back of state or history gas credits a reservoir and never
//! regular gas, so within a frame that runs, regular gas (`remaining` and the part gas detention
//! withholds) never rises, except when a child returns, and then by at most the unused regular gas
//! the child returned.
//!
//! The check observes a frame's regular gas when the frame is built, after every instruction the
//! engine defines (each state-gas site, each call and creation, each volatile read among them),
//! when its code deposit is admitted, and when the frame ends without a halt (a halt consumes its
//! regular gas). Between two of those points only evm2's stock instructions run, none of which
//! gives gas back; their net effect is still seen at the next point. A run under EIP-8037's rule
//! (a comparison's rerun, [`crate::ReferenceRule::Eip8037GiveBack`]) gives state gas back into
//! regular gas by design and is not checked. The observations run on the plain and the inspected
//! path alike, and a violation is recorded in [`RegularGasCheck::violations`]; the comparison's
//! harness turns it into a difference of the case.

use crate::{MegaTypes, detention};
use core::marker::PhantomData;
use evm2::interpreter::{
    GasTracker, InterpreterState, Message, Pc, Result, StackMut,
    private::{Instruction, InstructionFlags, split_gas_state},
};

/// A frame the check follows: the message it runs (by address, which stays put while the frame
/// runs), the regular gas last observed, and the unused regular gas its children returned since.
#[derive(Clone, Copy, Debug)]
struct Followed {
    message: usize,
    last: u64,
    returned: u64,
}

/// The per-transaction state of the check ([`crate::Lite::regular_check`]).
#[derive(Clone, Debug, Default)]
pub struct RegularGasCheck {
    frames: Vec<Followed>,
    /// Every observation that found regular gas above what the frame had and its children
    /// returned, as `<where>: <regular> after <last> + <returned> returned`.
    pub violations: Vec<String>,
}

impl RegularGasCheck {
    /// Clears the state for a new transaction.
    pub fn reset(&mut self) {
        self.frames.clear();
        self.violations.clear();
    }

    /// A frame was built for the message `message` with `regular` gas: followed from there.
    fn enter(&mut self, message: usize, regular: u64) {
        self.frames.push(Followed { message, last: regular, returned: 0 });
    }

    /// The running frame has `regular` gas at `at`: no more than it had at the last observation
    /// and its children returned since.
    fn observe(&mut self, regular: u64, at: &str) {
        if let Some(f) = self.frames.last_mut() {
            if regular > f.last.saturating_add(f.returned) {
                self.violations
                    .push(format!("{at}: {regular} after {} + {} returned", f.last, f.returned));
            }
            f.last = regular;
            f.returned = 0;
        }
    }

    /// A child of the running frame returned `unused` regular gas.
    fn child_returned(&mut self, unused: u64) {
        if let Some(f) = self.frames.last_mut() {
            f.returned = f.returned.saturating_add(unused);
        }
    }

    /// The message `message` ended with `regular` gas: observed unless it halted, then no longer
    /// followed. A message the check does not follow (no frame was built for it) changes nothing.
    fn end(&mut self, message: usize, halted: bool, regular: u64) {
        if self.frames.last().is_none_or(|f| f.message != message) {
            return;
        }
        if !halted {
            self.observe(regular, "frame end");
        }
        self.frames.pop();
    }
}

fn message_id(msg: &Message<MegaTypes>) -> usize {
    core::ptr::from_ref(msg) as usize
}

/// A frame was built for `msg` (the `frame_built` hook, after detention held it): followed from
/// its regular gas, both parts.
pub fn on_frame_built(
    evm: &mut evm2::Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    gas: &GasTracker,
) {
    if !crate::reservoir_refills(evm) {
        return;
    }
    let ext = evm.ext_mut();
    let regular = detention::available(gas, &ext.detention);
    ext.regular_check.enter(message_id(msg), regular);
}

/// The running frame's regular gas, both parts, at `at`.
pub fn observe(evm: &mut evm2::Evm<'_, MegaTypes>, gas: &GasTracker, at: &str) {
    if !crate::reservoir_refills(evm) {
        return;
    }
    let ext = evm.ext_mut();
    let regular = detention::available(gas, &ext.detention);
    ext.regular_check.observe(regular, at);
}

/// A child of the running frame returned `unused` regular gas.
pub fn child_returned(evm: &mut evm2::Evm<'_, MegaTypes>, unused: u64) {
    evm.ext_mut().regular_check.child_returned(unused);
}

/// `msg`'s frame ends (the `before_commit` hook, once detention folded the withheld part back into
/// `gas`): observed unless it halted, then no longer followed. A message whose frame was never
/// built (an answer, a start refused before its frame) is not followed and changes nothing.
pub fn on_frame_end(
    evm: &mut evm2::Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    halted: bool,
    gas: &GasTracker,
) {
    evm.ext_mut().regular_check.end(message_id(msg), halted, gas.remaining());
}

/// An instruction the engine defines, observed once it completed (comparison builds only).
pub struct Observed<I>(PhantomData<I>);

impl<I: Instruction<MegaTypes>> Instruction<MegaTypes> for Observed<I> {
    const FLAGS: InstructionFlags = I::FLAGS;

    fn execute(
        pc: &mut Pc,
        stack: StackMut<'_>,
        state: &mut InterpreterState<'_, '_, MegaTypes>,
    ) -> Result {
        I::execute(pc, stack, state)?;
        // SAFETY: `gas` is not reached through `state` while both are used.
        let (gas, state) = unsafe { split_gas_state(state) };
        observe(state.host(), gas.tracker(), core::any::type_name::<I>());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::RegularGasCheck;

    const FRAME: usize = 1;
    const CHILD: usize = 2;

    #[test]
    fn test_regular_gas_may_fall_but_not_rise() {
        let mut c = RegularGasCheck::default();
        c.enter(FRAME, 1_000);
        c.observe(900, "a");
        c.observe(900, "b");
        assert!(c.violations.is_empty());
        c.observe(901, "c");
        assert_eq!(c.violations, ["c: 901 after 900 + 0 returned"]);
    }

    #[test]
    fn test_a_returning_child_allows_its_unused_regular_gas_until_the_next_observation() {
        let mut c = RegularGasCheck::default();
        c.enter(FRAME, 1_000);
        c.enter(CHILD, 600);
        c.end(CHILD, false, 500);
        c.child_returned(500);
        c.child_returned(100);
        c.observe(1_600, "after the calls");
        assert!(c.violations.is_empty(), "{:?}", c.violations);
        c.observe(1_601, "next");
        assert_eq!(c.violations, ["next: 1601 after 1600 + 0 returned"]);
    }

    #[test]
    fn test_a_frame_end_is_observed_unless_it_halted() {
        let mut c = RegularGasCheck::default();
        c.enter(FRAME, 1_000);
        c.observe(400, "a");
        c.enter(CHILD, 300);
        // A halt consumes the frame's regular gas: whatever its tracker reads, nothing is checked.
        c.end(CHILD, true, 5_000);
        assert!(c.violations.is_empty());
        // A revert that hands back more than the frame had is a violation, and the frame is left.
        c.end(FRAME, false, 401);
        assert_eq!(c.violations, ["frame end: 401 after 400 + 0 returned"]);
        c.observe(10_000, "no frame");
        assert_eq!(c.violations.len(), 1);
    }

    #[test]
    fn test_a_message_without_a_frame_ends_nothing() {
        let mut c = RegularGasCheck::default();
        c.enter(FRAME, 1_000);
        // An answered or refused message at the next depth: the running frame stays followed.
        c.end(CHILD, false, 50_000);
        assert!(c.violations.is_empty());
        c.observe(1_001, "still the frame");
        assert_eq!(c.violations, ["still the frame: 1001 after 1000 + 0 returned"]);
        c.reset();
        assert!(c.violations.is_empty());
        c.observe(5, "after reset");
        assert!(c.violations.is_empty());
    }
}
