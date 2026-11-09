//! The regular-gas invariant of Satin's give-back rule and the transaction's gas conservation,
//! checked in comparison builds (`SATIN-HIST-012`, `SATIN-POOL-012`, `SATIN-POOL-014`).
//!
//! Regular gas: every give-back of state or history gas credits a reservoir and never regular gas,
//! so within a frame that runs, regular gas (`remaining` and the part gas detention withholds)
//! never rises, except when a child returns, and then by at most the unused regular gas the child
//! returned.
//!
//! Conservation, per transaction: a frame's whole gas — regular gas, its reservoir and the state
//! and history gas it holds charged (`state_gas_spent`, signed: a give-back for a charge another
//! frame made is a transfer between frames and makes it negative) — never rises either, except by
//! what a returning child hands back; a returning message hands back at most the forward and the
//! reservoir share it received (a halt at most the share: it consumes its regular gas and keeps the
//! reservoir it received); and the transaction's give-backs never exceed the state and history
//! charges it made, counting the charges made before its first frame.
//!
//! The check observes a frame when it is built, after every instruction the engine defines (each
//! state-gas site, call, creation and volatile read among them), once a call, creation or keyless
//! deployment has taken its forward and the child's reservoir share, once a returning child's gas
//! has merged back into its caller, when a code deposit is admitted, and when the frame's message
//! ends, after its settlement (the `after` message hook: a halt consumes the regular gas). Every
//! message's end is held to what it received, whether a frame ran for it or it was answered.
//! Between two of those points only evm2's stock instructions run, none of which gives gas back;
//! their net effect is still seen at the next point. A run under EIP-8037's rule (a comparison's
//! rerun, [`crate::ReferenceRule::Eip8037GiveBack`]) gives state gas back into regular gas by
//! design and is not checked. The observations run on the plain and the inspected path alike, and
//! a violation is recorded in [`RegularGasCheck::violations`]; the comparison's harness turns it
//! into a difference of the case.

use crate::{MegaTypes, detention};
use core::marker::PhantomData;
use evm2::interpreter::{
    GasTracker, InterpreterState, Message, MessageResult, Pc, Result, StackMut,
    private::{Instruction, InstructionFlags, split_gas_state},
};

/// A frame's gas at an observation: regular gas (both parts), its reservoir and the state and
/// history gas it holds charged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pools {
    regular: u64,
    reservoir: u64,
    charged: i64,
}

impl Pools {
    fn of(regular: u64, gas: &GasTracker) -> Self {
        Self { regular, reservoir: gas.reservoir(), charged: gas.state_gas_spent() }
    }

    /// The frame's whole gas: regular gas, the reservoir and the charges it holds.
    fn total(&self) -> i128 {
        i128::from(self.regular) + i128::from(self.reservoir) + i128::from(self.charged)
    }
}

/// What a message that ended handed back to its caller: its unused regular gas and its whole gas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Handed {
    regular: u64,
    total: i128,
}

/// A frame the check follows: the message it runs (by address, which stays put while the frame
/// runs), its gas last observed, and what its children handed back since.
#[derive(Clone, Copy, Debug)]
struct Followed {
    message: usize,
    last: Pools,
    returned: Handed,
}

/// The per-transaction state of the check ([`crate::Lite::regular_check`]).
#[derive(Clone, Debug, Default)]
pub struct RegularGasCheck {
    frames: Vec<Followed>,
    /// The last message that ended, by address, and what it handed back: its caller's merge reads
    /// it, so what reaches the caller is what the end was checked with.
    ended: Option<(usize, Handed)>,
    /// Whether the transaction's first frame was built: the state charges priced before it are
    /// the charges made before the first frame.
    started: bool,
    /// The state and history gas charged before the first frame, which its frames may give back.
    pre_frame: u64,
    /// Every observation that found more gas than the frame had and its children handed back, or a
    /// message that handed back more than it received, or give-backs beyond the charges made, as
    /// `<where>: <what>`.
    pub violations: Vec<String>,
    /// Tests only: check whatever give-back rule the version runs (a defect of the reference's
    /// rule reproduced on purpose). Kept across transactions.
    pub force: bool,
}

impl RegularGasCheck {
    /// Clears the state for a new transaction.
    pub fn reset(&mut self) {
        self.frames.clear();
        self.ended = None;
        self.started = false;
        self.pre_frame = 0;
        self.violations.clear();
    }

    /// A state charge priced at `price` before the first frame was built.
    pub fn priced(&mut self, price: u64) {
        if !self.started {
            self.pre_frame = self.pre_frame.saturating_add(price);
        }
    }

    /// A frame was built for the message `message` with `pools`: followed from there. The
    /// transaction's first frame also adds `start_charge`, its start record's history charged
    /// before it, to the charges made before it.
    fn enter(&mut self, message: usize, pools: Pools, start_charge: u64) {
        if !self.started {
            self.started = true;
            self.pre_frame = self.pre_frame.saturating_add(start_charge);
        }
        self.frames.push(Followed { message, last: pools, returned: Handed::default() });
    }

    /// The running frame has `pools` at `at`: no more regular gas, and no more gas in all, than
    /// it had at the last observation and its children handed back since; and the transaction's
    /// charges still at least its give-backs.
    fn observe(&mut self, pools: Pools, at: &str) {
        let Some(f) = self.frames.last_mut() else { return };
        if pools.regular > f.last.regular.saturating_add(f.returned.regular) {
            self.violations.push(format!(
                "{at}: regular {} after {} + {} returned",
                pools.regular, f.last.regular, f.returned.regular
            ));
        }
        if pools.total() > f.last.total() + f.returned.total {
            self.violations.push(format!(
                "{at}: gas {} after {} + {} handed back",
                pools.total(),
                f.last.total(),
                f.returned.total
            ));
        }
        f.last = pools;
        f.returned = Handed::default();
        let charged: i128 = self.frames.iter().map(|f| i128::from(f.last.charged)).sum();
        if charged + i128::from(self.pre_frame) < 0 {
            self.violations.push(format!(
                "{at}: give-backs exceed the charges made by {}",
                -(charged + i128::from(self.pre_frame))
            ));
        }
    }

    /// The message `message`, which received `forward` and the reservoir share `share`, ended
    /// after its settlement with `pools`, `halted` or not, `success` or not: it hands back at most
    /// what it received (a halt at most the share), and a followed frame that did not halt is
    /// observed one last time; then it is no longer followed.
    fn end(
        &mut self,
        message: usize,
        (forward, share): (u64, u64),
        halted: bool,
        success: bool,
        pools: Pools,
    ) {
        // A failed message's charges are settled (given back or consumed); a successful one's go
        // to its caller with it.
        let charged = if success { i128::from(pools.charged) } else { 0 };
        let handed = Handed {
            regular: pools.regular,
            total: i128::from(pools.regular) + i128::from(pools.reservoir) + charged,
        };
        let received =
            if halted { i128::from(share) } else { i128::from(forward) + i128::from(share) };
        if handed.total > received || (halted && pools.regular != 0) {
            self.violations.push(format!(
                "message end: handed back {} ({} regular) of {} received",
                handed.total, handed.regular, received
            ));
        }
        self.ended = Some((message, handed));
        if self.frames.last().is_none_or(|f| f.message != message) {
            return;
        }
        if !halted {
            self.observe(pools, "frame end");
        }
        self.frames.pop();
    }

    /// The child `child` merged into the running frame, which now has `pools`: what it handed back
    /// is what its end was checked with, or `fallback` for a message whose end was not seen (an
    /// inspector's override).
    fn merged(&mut self, child: usize, fallback: Handed, pools: Pools) {
        let handed = match self.ended.take() {
            Some((message, handed)) if message == child => handed,
            _ => fallback,
        };
        if let Some(f) = self.frames.last_mut() {
            f.returned.regular = f.returned.regular.saturating_add(handed.regular);
            f.returned.total += handed.total;
        }
        self.observe(pools, "child merge");
    }
}

fn message_id(msg: &Message<MegaTypes>) -> usize {
    core::ptr::from_ref(msg) as usize
}

/// Whether the running version is checked: Satin's give-back rule, or a test's `force`.
fn checked(evm: &evm2::Evm<'_, MegaTypes>) -> bool {
    crate::reservoir_refills(evm) || evm.ext().regular_check.force
}

/// The running frame's gas, `gas` with the withheld part of its regular gas.
fn running(evm: &evm2::Evm<'_, MegaTypes>, gas: &GasTracker) -> Pools {
    Pools::of(detention::available(gas, &evm.ext().detention), gas)
}

/// A state charge priced at `price` (the state-gas price hook): counted while no frame of the
/// transaction was built yet.
pub fn on_state_priced(evm: &mut evm2::Evm<'_, MegaTypes>, price: u64) {
    if checked(evm) {
        evm.ext_mut().regular_check.priced(price);
    }
}

/// A frame was built for `msg` (the `frame_built` hook, after detention held it): followed from
/// its gas, both parts of its regular gas.
pub fn on_frame_built(
    evm: &mut evm2::Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    gas: &GasTracker,
) {
    if !checked(evm) {
        return;
    }
    let pools = running(evm, gas);
    let ext = evm.ext_mut();
    let start_charge = ext.tx_start_charge;
    ext.regular_check.enter(message_id(msg), pools, start_charge);
}

/// The running frame's gas at `at`.
pub fn observe(evm: &mut evm2::Evm<'_, MegaTypes>, gas: &GasTracker, at: &str) {
    if !checked(evm) {
        return;
    }
    let pools = running(evm, gas);
    evm.ext_mut().regular_check.observe(pools, at);
}

/// `msg`'s creation admits its code deposit with `gas`: observed when a frame was built for it (a
/// creation whose init code is empty runs no frame, and its caller's frame is not its own).
pub fn on_code_deposit(
    evm: &mut evm2::Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    gas: &GasTracker,
) {
    if !checked(evm) ||
        evm.ext().regular_check.frames.last().is_none_or(|f| f.message != message_id(msg))
    {
        return;
    }
    observe(evm, gas, "code deposit charges");
}

/// The running frame took a child's forward and reservoir share (a call's, a creation's or a
/// keyless deployment's), right before the child runs.
pub fn on_forward(evm: &mut evm2::Evm<'_, MegaTypes>, gas: &GasTracker) {
    observe(evm, gas, "forward");
}

/// The child `child`, whose result is `result`, merged back into the running frame, whose gas is
/// now `gas` (its caller's kept reservoir restored).
pub fn on_merged(
    evm: &mut evm2::Evm<'_, MegaTypes>,
    child: &Message<MegaTypes>,
    result: &MessageResult<MegaTypes>,
    gas: &GasTracker,
) {
    if !checked(evm) {
        return;
    }
    let charged =
        if result.stop.is_success() { i128::from(result.gas.state_gas_spent()) } else { 0 };
    let fallback = Handed {
        regular: result.gas.remaining(),
        total: i128::from(result.gas.remaining()) + i128::from(result.gas.reservoir()) + charged,
    };
    let pools = running(evm, gas);
    evm.ext_mut().regular_check.merged(message_id(child), fallback, pools);
}

/// `msg` ended with `result`, after its settlement (the `after` message hook): held to what it
/// received, and, when its frame was followed and did not halt, observed one last time (detention
/// folded the withheld part back into its gas when the frame ended).
pub fn on_message_end(
    evm: &mut evm2::Evm<'_, MegaTypes>,
    msg: &Message<MegaTypes>,
    result: &MessageResult<MegaTypes>,
) {
    if !checked(evm) {
        return;
    }
    let pools = Pools::of(result.gas.remaining(), &result.gas);
    evm.ext_mut().regular_check.end(
        message_id(msg),
        (msg.gas_limit, msg.reservoir),
        result.stop.is_halt(),
        result.stop.is_success(),
        pools,
    );
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
    use super::{Handed, Pools, RegularGasCheck};

    const FRAME: usize = 1;
    const CHILD: usize = 2;

    fn regular(regular: u64) -> Pools {
        Pools { regular, ..Pools::default() }
    }

    #[test]
    fn test_regular_gas_may_fall_but_not_rise() {
        let mut c = RegularGasCheck::default();
        c.enter(FRAME, regular(1_000), 0);
        c.observe(regular(900), "a");
        c.observe(regular(900), "b");
        assert!(c.violations.is_empty());
        c.observe(regular(901), "c");
        assert_eq!(c.violations[0], "c: regular 901 after 900 + 0 returned");
    }

    #[test]
    fn test_a_returning_child_allows_what_it_handed_back_until_the_next_observation() {
        let mut c = RegularGasCheck::default();
        c.enter(FRAME, regular(1_000), 0);
        c.observe(regular(400), "forward");
        c.enter(CHILD, regular(600), 0);
        c.end(CHILD, (600, 0), false, true, regular(500));
        c.merged(CHILD, Handed::default(), regular(900));
        assert!(c.violations.is_empty(), "{:?}", c.violations);
        c.observe(regular(901), "next");
        assert_eq!(c.violations[0], "next: regular 901 after 900 + 0 returned");
    }

    #[test]
    fn test_a_frame_end_is_observed_unless_it_halted() {
        let mut c = RegularGasCheck::default();
        c.enter(FRAME, regular(1_000), 0);
        c.observe(regular(400), "a");
        c.enter(CHILD, regular(300), 0);
        // A halt consumes the frame's regular gas: it hands back none.
        c.end(CHILD, (300, 0), true, false, regular(0));
        assert!(c.violations.is_empty());
        // A revert that hands back more than the frame had is a violation, and the frame is left.
        c.end(FRAME, (1_000, 0), false, false, regular(401));
        assert_eq!(
            c.violations,
            [
                "frame end: regular 401 after 400 + 0 returned",
                "frame end: gas 401 after 400 + 0 handed back"
            ]
        );
        c.observe(regular(10_000), "no frame");
        assert_eq!(c.violations.len(), 2);
    }

    #[test]
    fn test_a_message_without_a_frame_ends_nothing_but_is_held_to_what_it_received() {
        let mut c = RegularGasCheck::default();
        c.enter(FRAME, regular(1_000), 0);
        // An answered message at the next depth: the running frame stays followed.
        c.end(CHILD, (50_000, 0), false, true, regular(50_000));
        assert!(c.violations.is_empty());
        c.observe(regular(1_001), "still the frame");
        assert_eq!(c.violations[0], "still the frame: regular 1001 after 1000 + 0 returned");
        c.reset();
        c.end(CHILD, (50_000, 7), false, true, Pools { regular: 50_000, reservoir: 8, charged: 0 });
        assert_eq!(
            c.violations,
            ["message end: handed back 50008 (50000 regular) of 50007 received"]
        );
        c.reset();
        c.observe(regular(5), "after reset");
        assert!(c.violations.is_empty());
    }

    /// A give-back is a transfer: the reservoir rises with the charges falling, and a child that
    /// gives back a charge its caller made hands back more reservoir than it received, its
    /// charges negative by as much.
    #[test]
    fn test_a_give_back_moves_gas_between_frames_without_creating_it() {
        let mut c = RegularGasCheck::default();
        c.enter(FRAME, Pools { regular: 1_000, reservoir: 500, charged: 0 }, 0);
        c.observe(Pools { regular: 1_000, reservoir: 300, charged: 200 }, "charge");
        c.observe(Pools { regular: 400, reservoir: 300, charged: 200 }, "forward");
        c.enter(CHILD, Pools { regular: 600, reservoir: 0, charged: 0 }, 0);
        c.observe(Pools { regular: 590, reservoir: 200, charged: -200 }, "give-back");
        c.end(CHILD, (600, 0), false, true, Pools { regular: 590, reservoir: 200, charged: -200 });
        c.merged(CHILD, Handed::default(), Pools { regular: 990, reservoir: 500, charged: 0 });
        assert!(c.violations.is_empty(), "{:?}", c.violations);
    }
}

/// The invariant's catches on real runs: each defect the check exists for, made on purpose, is
/// reported (comparison builds' lib tests).
#[cfg(test)]
mod catches {
    use crate::{
        MegaSpec, MegaTypes, ReferenceRule, precompiles, satin_version, satin_version_under,
        tx::registry,
    };
    use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
    use alloy_primitives::{Address, Bytes, Signature, TxKind, U256};
    use evm2::{
        Evm, ExecutionConfig, Version,
        bytecode::Bytecode,
        env::BlockEnv,
        evm::{AccountInfo, Db, InMemoryDB},
        inspector::Inspector,
        interpreter::{Interpreter, Message, MessageResult},
    };
    use op_evm2::OpTxEnvelope;

    const SENDER: Address = Address::with_last_byte(0x50);
    const TOP: Address = Address::with_last_byte(0xa0);
    const C: Address = Address::with_last_byte(0xc0);

    const PUSH0: u8 = 0x5f;
    const PUSH1: u8 = 0x60;
    const PUSH3: u8 = 0x62;
    const PUSH20: u8 = 0x73;
    const SSTORE: u8 = 0x55;
    const CALL: u8 = 0xf1;
    const DELEGATECALL: u8 = 0xf4;
    const REVERT: u8 = 0xfd;
    const STOP: u8 = 0x00;
    const POP: u8 = 0x50;

    /// `CALL` (or `DELEGATECALL`) of C with 200,000 of gas, no value, no input or output.
    fn call_c(delegate: bool) -> Vec<u8> {
        let mut code = vec![PUSH0, PUSH0, PUSH0, PUSH0];
        if !delegate {
            code.push(PUSH0);
        }
        code.push(PUSH20);
        code.extend_from_slice(C.as_slice());
        code.extend([PUSH3, 0x03, 0x0d, 0x40, if delegate { DELEGATECALL } else { CALL }, POP]);
        code
    }

    /// Alters a returning child's settled result before its caller merges it (the window between
    /// the child's end and the caller's merge).
    struct AlterChild(fn(&mut MessageResult<MegaTypes>));

    impl Inspector<MegaTypes> for AlterChild {
        fn call_end(
            &mut self,
            _: &mut Interpreter<'_, '_, MegaTypes>,
            m: &Message<MegaTypes>,
            r: &mut MessageResult<MegaTypes>,
        ) {
            if m.depth == 1 {
                (self.0)(r);
            }
        }
    }

    /// Runs a call of TOP with a gas limit below the execution cap (no reservoir) under `version`,
    /// with the check forced on when `force`, and returns the check's violations.
    fn violations(
        top: &[u8],
        c: &[u8],
        version: Version,
        force: bool,
        alter: Option<fn(&mut MessageResult<MegaTypes>)>,
    ) -> Vec<String> {
        let config = ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, version);
        let block = BlockEnv::<MegaTypes> {
            number: U256::from(1),
            timestamp: U256::from(1_000),
            gas_limit: U256::from(10_000_000_000u64),
            ..BlockEnv::<MegaTypes>::default()
        };
        let mut db = InMemoryDB::default();
        let code = |c: &[u8]| {
            AccountInfo::default()
                .with_nonce(1)
                .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(c)))
        };
        db.insert_account_info(&TOP, code(top));
        db.insert_account_info(&C, code(c));
        let mut evm = Evm::<MegaTypes>::new_with_execution_config(
            config,
            MegaSpec::Satin,
            block,
            registry(),
            Db::new(db),
            precompiles(),
        );
        evm.state_mut().set_read_absent_storage(true);
        evm.ext_mut().regular_check.force = force;
        if let Some(alter) = alter {
            evm.set_inspector(AlterChild(alter));
        }
        let tx = TxLegacy {
            chain_id: Some(1),
            nonce: 0,
            gas_price: 0,
            gas_limit: 5_000_000,
            to: TxKind::Call(TOP),
            value: U256::ZERO,
            input: Bytes::new(),
        };
        let signature = Signature::new(U256::from(1), U256::from(1), false);
        let tx =
            Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER);
        let executed = evm.transact(&tx).expect("the transaction runs");
        let _ = executed.commit();
        evm.ext().regular_check.violations.clone()
    }

    /// The reviewer's merge injection, on a real run: under EIP-8037's rule a returned reservoir
    /// unwinds its caller's spill into regular gas. TOP writes a fresh slot from regular gas (no
    /// reservoir, so the slot's state and record spill whole), then delegates to C with 200,000,
    /// which writes the slot back to zero: the give-backs go to C's reservoir, which, merged,
    /// unwinds TOP's spill — 101,440 of regular gas no child returned, under the 200,000 the
    /// caller forwarded. Seen only once the caller is observed after its forward and at the merge.
    #[test]
    fn invariant_catches_merge_give_back() {
        let mut top = vec![PUSH1, 1, PUSH0, SSTORE];
        top.extend(call_c(true));
        top.push(STOP);
        let c = [PUSH0, PUSH0, SSTORE, STOP];
        let clean = violations(&top, &c, satin_version(), false, None);
        assert!(clean.is_empty(), "{clean:?}");
        let found = violations(
            &top,
            &c,
            satin_version_under(&[ReferenceRule::Eip8037GiveBack]),
            true,
            None,
        );
        assert!(found.iter().any(|v| v.starts_with("child merge: regular")), "{found:?}");
    }

    /// The settlement injection: under EIP-8037's rule a reverted frame's spill returns to its
    /// regular gas when the frame settles, after its last instruction. C writes a fresh slot from
    /// regular gas and reverts; its settlement gives the 101,440 back. Seen only once the frame's
    /// end is checked after its settlement.
    #[test]
    fn invariant_catches_settlement_give_back() {
        let mut top = call_c(false);
        top.push(STOP);
        let c = [PUSH1, 1, PUSH0, SSTORE, PUSH0, PUSH0, REVERT];
        let clean = violations(&top, &c, satin_version(), false, None);
        assert!(clean.is_empty(), "{clean:?}");
        let found = violations(
            &top,
            &c,
            satin_version_under(&[ReferenceRule::Eip8037GiveBack]),
            true,
            None,
        );
        assert!(found.iter().any(|v| v.starts_with("frame end: regular")), "{found:?}");
    }

    /// A conservation violation: a returning child's reservoir credited again after its end (a
    /// kept reservoir added twice, a give-back with no charge), which no regular gas shows.
    #[test]
    fn invariant_catches_reservoir_created() {
        let mut top = call_c(false);
        top.push(STOP);
        let c = [PUSH1, 1, POP, STOP];
        let clean = violations(&top, &c, satin_version(), false, None);
        assert!(clean.is_empty(), "{clean:?}");
        let found = violations(
            &top,
            &c,
            satin_version(),
            false,
            Some(|r| r.gas.set_reservoir(r.gas.reservoir() + 200)),
        );
        assert!(found.iter().any(|v| v.starts_with("child merge: gas")), "{found:?}");
    }

    /// A conservation violation of the transaction's: a give-back credited with its charge taken
    /// back (the reservoir up, the charges down), for a charge no frame made.
    #[test]
    fn invariant_catches_give_back_beyond_charges() {
        let mut top = call_c(false);
        top.push(STOP);
        let c = [PUSH1, 1, POP, STOP];
        let found = violations(
            &top,
            &c,
            satin_version(),
            false,
            Some(|r| {
                r.gas.set_reservoir(r.gas.reservoir() + 200);
                r.gas.add_state_gas_spent(-200);
            }),
        );
        assert!(
            found.iter().any(|v| v == "child merge: give-backs exceed the charges made by 200"),
            "{found:?}"
        );
    }
}
