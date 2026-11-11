//! A value `CALL` refused for its depth makes no record, so it is charged no start records' history
//! and gives none back into the reservoir, as a creation refused for its depth is not. No
//! transaction reaches the call-stack limit under the execution cap (a frame at depth 1,024 has
//! less than one gas), so the frame at the deepest depth is run directly.

mod common;

use alloy_primitives::{Address, Bytes, U256};
use evm2::{
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    interpreter::{Host, InstrStop, Message, MessageKind},
};
use mega_evm2::MegaTypes;

const SENDER: Address = Address::with_last_byte(0x50);
const CALLER: Address = Address::with_last_byte(0xa0);
const TARGET: Address = Address::with_last_byte(0xb0);

#[test]
fn test_a_value_call_refused_for_its_depth_is_charged_no_start_records() {
    // CALL(GAS, TARGET, 1, 0, 0, 0, 0); POP; GAS returned.
    let mut code = vec![0x5f, 0x5f, 0x5f, 0x5f, 0x60, 1, 0x73];
    code.extend_from_slice(TARGET.as_slice());
    code.extend([0x5a, 0xf1, 0x50, 0x5a, 0x5f, 0x52, 0x60, 32, 0x5f, 0xf3]);
    let code = Bytecode::new_legacy(Bytes::from(code));
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &CALLER,
        AccountInfo::default().with_nonce(1).with_balance(U256::from(10)).with_code(code.clone()),
    );
    db.insert_account_info(
        &TARGET,
        AccountInfo::default()
            .with_nonce(1)
            .with_code(Bytecode::new_legacy(Bytes::from_static(&[0x00]))),
    );
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = common::evm(block, Db::new(db));
    evm.ext_mut().begin_tx(SENDER, 310, true, false);
    let deepest = evm.initial_call_depth() + evm2::constants::CALL_DEPTH_LIMIT;
    let mut message = Message::<MegaTypes> {
        kind: MessageKind::Call,
        depth: deepest,
        gas_limit: 100_000,
        destination: CALLER,
        call_target: CALLER,
        code_address: CALLER,
        caller: SENDER,
        code,
        ..Default::default()
    };
    let data_before = evm.ext().tx_net;
    let result = Host::execute_message(&mut evm, &mut message).expect("no execution error");
    // The frame runs on: the six pushes and `GAS` (16), the `CALL`'s 12,000 (its charges less the
    // 2,300 stipend a call refused for its depth returns, the forward returned whole), `POP` and
    // `GAS`. Charging the two records' 7,040 before the refusal had the caller pay them from the
    // fiftieth it keeps of the forward, which it could not, and the frame halted out of gas.
    assert_eq!(result.stop, InstrStop::Return);
    assert_eq!(U256::from_be_slice(&result.output), U256::from(100_000 - 16 - 12_000 - 2 - 2));
    assert_eq!((result.gas.reservoir(), result.gas.state_gas_spent()), (0, 0));
    assert_eq!(evm.ext().tx_net, data_before, "the refused call counted records");
}
