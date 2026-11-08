//! A `keylessDeploy(bytes,uint256)` call whose `bytes` offset word is the largest `usize` is a
//! malformed encoding, answered with `MalformedEncoding()` after the call's overhead, in every
//! build: the decoder checks the end of every range it reads, so an offset near the top of `usize`
//! cannot overflow (this test runs in the default test profile, which checks overflow).
//!
//! The differential gate runs the same input (`edge_keyless_offset_u64_max`) in release builds,
//! where an unchecked addition would wrap rather than panic.

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, Bytes, TxKind, U256, keccak256};
use evm2::{
    Evm, ExecutionConfig,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    interpreter::InstrStop,
};
use mega_evm2::{MegaSpec, MegaTypes, precompiles, satin_version, tx::registry};
use op_evm2::OpTxEnvelope;

const SENDER: Address = Address::with_last_byte(0x50);
const KEYLESS: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03]);

fn selector(sig: &str) -> [u8; 4] {
    keccak256(sig)[..4].try_into().unwrap()
}

/// The call's input: the selector, the `bytes` offset word and the gas override word, nothing
/// else.
fn call_with_offset(offset: U256) -> Bytes {
    let mut input = selector("keylessDeploy(bytes,uint256)").to_vec();
    input.extend_from_slice(&offset.to_be_bytes::<32>());
    input.extend_from_slice(&U256::from(1_000_000u64).to_be_bytes::<32>());
    input.into()
}

fn run(input: Bytes) -> evm2::TxResult<MegaTypes> {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
    );
    let config =
        ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, satin_version());
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(db),
        precompiles(),
    );
    evm.state_mut().set_read_absent_storage(true);
    let tx = TxLegacy {
        chain_id: None,
        nonce: 0,
        gas_price: 0,
        gas_limit: 5_000_000,
        to: TxKind::Call(KEYLESS),
        value: U256::ZERO,
        input,
    };
    let tx = Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), SENDER);
    evm.transact(&tx).expect("the call is a valid transaction").commit()
}

fn assert_malformed(result: &evm2::TxResult<MegaTypes>) {
    assert_eq!(result.stop, InstrStop::Revert, "{result:?}");
    assert_eq!(result.output.as_ref(), selector("MalformedEncoding()"), "{result:?}");
}

#[test]
fn test_offset_at_the_top_of_usize_is_malformed() {
    assert_malformed(&run(call_with_offset(U256::from(usize::MAX))));
    assert_malformed(&run(call_with_offset(U256::from(usize::MAX - 31))));
}

/// The control: an offset that only points past the input is malformed the same way.
#[test]
fn test_offset_past_the_input_is_malformed() {
    assert_malformed(&run(call_with_offset(U256::from(0x40))));
}
