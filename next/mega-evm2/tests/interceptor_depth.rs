//! A call whose frame would be deeper than the call-stack limit reaches no interceptor and counts
//! no hint: the `before` message hook leaves it to evm2, which answers it with `CallTooDeep`, its
//! forward and reservoir share returned whole. No transaction reaches the limit under the
//! execution cap (a frame at depth 1,024 has less than one gas), so the hook is called directly,
//! at the deepest depth a frame may have and one past it.

use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use evm2::{
    Evm, ExecutionConfig, MessageHooks,
    env::BlockEnv,
    evm::{Db, InMemoryDB},
    interpreter::{InstrStop, Message, MessageKind},
};
use mega_evm2::{
    MegaSpec, MegaTypes, ORACLE, StartHooks, intercept::LIMIT_CONTROL, precompiles, satin_version,
    tx::registry,
};

const SENDER: Address = Address::with_last_byte(0x50);
const CALLER: Address = Address::with_last_byte(0xa0);
/// Satin's `MegaAccessControl` system contract.
const ACCESS_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04]);

fn satin_evm() -> Evm<'static, MegaTypes> {
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(InMemoryDB::default()),
        precompiles(),
    );
    evm.ext_mut().begin_tx(SENDER, 310, true, false);
    evm
}

fn selector(signature: &str) -> [u8; 4] {
    keccak256(signature)[..4].try_into().unwrap()
}

/// A CALL of `target` with `input` and 10,000 gas, from a frame at `depth − 1`.
fn call(target: Address, input: Vec<u8>, depth: u16) -> Message<MegaTypes> {
    Message::<MegaTypes> {
        kind: MessageKind::Call,
        depth,
        gas_limit: 10_000,
        destination: target,
        call_target: target,
        code_address: target,
        caller: CALLER,
        input: Bytes::from(input),
        ..Default::default()
    }
}

/// `sendHint(bytes32 topic, bytes data)` with a one-byte `data`.
fn send_hint() -> Vec<u8> {
    let mut input = selector("sendHint(bytes32,bytes)").to_vec();
    input.extend_from_slice(&B256::repeat_byte(0x11).0);
    input.extend_from_slice(&B256::from(U256::from(0x40)).0);
    input.extend_from_slice(&B256::from(U256::from(1)).0);
    input.extend_from_slice(&[0xaa; 32]);
    input
}

#[test]
fn test_a_call_past_the_depth_limit_reaches_no_interceptor() {
    let mut evm = satin_evm();
    let deepest = evm.version().max_call_depth();
    for (target, input) in [
        (LIMIT_CONTROL, selector("remainingComputeGas()").to_vec()),
        (ACCESS_CONTROL, selector("isVolatileDataAccessDisabled()").to_vec()),
    ] {
        // Within the limit the interceptor answers the call.
        let mut within = call(target, input.clone(), deepest);
        let answer = <StartHooks as MessageHooks<MegaTypes>>::before(&mut evm, &mut within)
            .expect("no execution error");
        assert_eq!(answer.map(|a| a.stop), Some(InstrStop::Return), "{target} at {deepest}");
        // Past it nothing answers: evm2's depth check does.
        let mut past = call(target, input, deepest + 1);
        let answer = <StartHooks as MessageHooks<MegaTypes>>::before(&mut evm, &mut past)
            .expect("no execution error");
        assert!(answer.is_none(), "{target} past the limit was answered: {answer:?}");
    }
}

#[test]
fn test_a_hint_past_the_depth_limit_is_not_counted() {
    let mut evm = satin_evm();
    let deepest = evm.version().max_call_depth();
    let mut past = call(ORACLE, send_hint(), deepest + 1);
    let answer = <StartHooks as MessageHooks<MegaTypes>>::before(&mut evm, &mut past)
        .expect("no execution error");
    assert!(answer.is_none());
    assert_eq!(evm.ext().mechanisms.hint_bytes, 0, "a hint past the limit was counted");
    assert!(evm.ext().mechanisms.hints.is_empty(), "a hint past the limit was forwarded");
    // The same hint within the limit is counted and forwarded.
    let mut within = call(ORACLE, send_hint(), deepest);
    let answer = <StartHooks as MessageHooks<MegaTypes>>::before(&mut evm, &mut within)
        .expect("no execution error");
    assert!(answer.is_none(), "a hint runs the Oracle's bytecode");
    assert_eq!(evm.ext().mechanisms.hint_bytes, send_hint().len() as u64);
    assert_eq!(evm.ext().mechanisms.hints.len(), 1);
}
