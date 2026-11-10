//! A frame starts with its caller's EIP-8037 reservoir less the two per cent the caller keeps,
//! rounded down. Satin bounds a transaction's gas limit by the block gas limit alone, so a
//! reservoir can exceed 2^63, where twice the reservoir no longer fits in a `u64`.
//!
//! Each frame test runs a transaction of gas limit 2^63 + 10^9 in a block of gas limit
//! `u64::MAX` and records, for the first frame below the transaction's, the reservoir its caller
//! kept and the reservoir it started with.

mod common;

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, Bytes, Signature, TxKind, U256, hex, keccak256};
use evm2::{
    Evm,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    inspector::Inspector,
    interpreter::{Interpreter, Message, MessageResult},
};
use mega_evm2::MegaTypes;
use op_evm2::OpTxEnvelope;

/// 2^63 + 10^9: the reservoir is above 2^63 once the 200M execution gas is taken out.
const GAS_LIMIT: u64 = (1 << 63) + 1_000_000_000;
const CHAIN_ID: u64 = 1;

const SENDER: Address = Address::with_last_byte(0x50);
/// Runs the frame under test.
const CALLER: Address = Address::with_last_byte(0xa0);
/// Stores 1 in slot 0.
const CALLEE: Address = Address::with_last_byte(0xb0);
/// Satin's `KeylessDeploy` system contract.
const KEYLESS: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03]);

/// A pre-EIP-155 creation signed with key 0x42..42: gas price 100 gwei, gas limit 200,000, init
/// code storing ORIGIN@1, GASPRICE@2, CALLER@3 and returning one byte.
const KEYLESS_SIGNED_TX: &str = "f8628085174876e80083030d40808091326001553a6002553360035560016000f31ca02327bc65dfaefd3c65ae4f03ed8045b517d7a99e2b80026e1703dfc1131baf39a00750cc742e3f22b826b0f71e060d1181bf0e0d29d292f6f93e91803001cdb5d8";

/// `PUSH1 1 PUSH1 0 SSTORE STOP`.
const STORE_ONE: [u8; 6] = [0x60, 0x01, 0x60, 0x00, 0x55, 0x00];
/// `CREATE(0, 0, 0) STOP`: creates an account with empty code.
const CREATE_EMPTY: [u8; 8] = [0x60, 0x00, 0x60, 0x00, 0x60, 0x00, 0xf0, 0x00];

/// The reservoirs at every frame start below the transaction's frame, in start order.
#[derive(Debug, Default)]
struct FrameStarts {
    /// The transaction frame's reservoir.
    top: Option<u64>,
    /// `(kept by the caller, given to the frame)`.
    children: Vec<(u64, u64)>,
}

impl FrameStarts {
    fn record(&mut self, caller: &Interpreter<'_, '_, MegaTypes>, message: &Message<MegaTypes>) {
        if message.depth == 0 {
            self.top = Some(message.reservoir);
        } else {
            self.children.push((caller.gas().reservoir(), message.reservoir));
        }
    }
}

impl Inspector<MegaTypes> for FrameStarts {
    fn call(
        &mut self,
        interp: &mut Interpreter<'_, '_, MegaTypes>,
        message: &mut Message<MegaTypes>,
    ) -> Option<MessageResult<MegaTypes>> {
        self.record(interp, message);
        None
    }

    fn create(
        &mut self,
        interp: &mut Interpreter<'_, '_, MegaTypes>,
        message: &mut Message<MegaTypes>,
    ) -> Option<MessageResult<MegaTypes>> {
        self.record(interp, message);
        None
    }
}

/// `CALL(GAS, CALLEE, 0, 0, 0, 0, 0) STOP`.
fn call_callee() -> Vec<u8> {
    let mut code = vec![0x60, 0, 0x60, 0, 0x60, 0, 0x60, 0, 0x60, 0, 0x73];
    code.extend_from_slice(CALLEE.as_slice());
    code.extend_from_slice(&[0x5a, 0xf1, 0x00]);
    code
}

fn contract(code: &[u8]) -> AccountInfo {
    AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code)))
}

/// A Satin EVM over `db` in a block whose gas limit is `u64::MAX`.
fn satin_evm(db: InMemoryDB) -> Evm<'static, MegaTypes> {
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(u64::MAX),
        ..BlockEnv::<MegaTypes>::default()
    };
    common::evm(block, Db::new(db))
}

/// A legacy transaction from [`SENDER`] to `to` with gas limit [`GAS_LIMIT`] and gas price 0.
fn legacy(to: Address, input: Bytes) -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy {
        chain_id: Some(CHAIN_ID),
        nonce: 0,
        gas_price: 0,
        gas_limit: GAS_LIMIT,
        to: TxKind::Call(to),
        value: U256::ZERO,
        input,
    };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER)
}

/// ABI-encoded `keylessDeploy(bytes signedTx, uint256 gasLimitOverride)`.
fn keyless_deploy(gas_limit_override: u64) -> Bytes {
    let signed = hex::decode(KEYLESS_SIGNED_TX).unwrap();
    let word = |v: u64| U256::from(v).to_be_bytes::<32>();
    let mut input = keccak256("keylessDeploy(bytes,uint256)")[..4].to_vec();
    input.extend_from_slice(&word(0x40));
    input.extend_from_slice(&word(gas_limit_override));
    input.extend_from_slice(&word(signed.len() as u64));
    let mut padded = signed;
    padded.resize(padded.len().div_ceil(32) * 32, 0);
    input.extend_from_slice(&padded);
    input.into()
}

/// Runs `tx` over `db` and returns the frame starts it recorded.
fn run(db: InMemoryDB, tx: &Recovered<OpTxEnvelope>) -> FrameStarts {
    let mut evm = satin_evm(db);
    evm.set_inspector(FrameStarts::default());
    let result = match evm.transact(tx) {
        Ok(executed) => executed.commit(),
        Err(error) => panic!("transaction refused: {error:?}"),
    };
    assert!(result.status, "{result:?}");
    *evm.clear_inspector_as::<FrameStarts>().expect("the inspector")
}

/// Satin's frame share of a caller reservoir `reservoir`: `reservoir − ⌊reservoir × 2 / 100⌋`.
fn spec_share(reservoir: u64) -> u64 {
    reservoir - (u128::from(reservoir) * 2 / 100) as u64
}

/// The first child frame started with the spec's share of its caller's reservoir, and that
/// reservoir was above 2^63.
fn assert_first_child_share(starts: &FrameStarts) -> (u64, u64) {
    let &(kept, share) = starts.children.first().expect("a child frame");
    let reservoir = kept + share;
    assert!(reservoir > 1 << 63, "the caller's reservoir {reservoir} is above 2^63");
    assert_eq!(share, spec_share(reservoir), "share of the caller's reservoir {reservoir}");
    (reservoir, share)
}

#[test]
fn test_call_child_reservoir_above_2_63_is_the_98_percent_share() {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&CALLER, contract(&call_callee()));
    db.insert_account_info(&CALLEE, contract(&STORE_ONE));
    let starts = run(db, &legacy(CALLER, Bytes::new()));
    let (reservoir, share) = assert_first_child_share(&starts);
    // The reference Satin implementation's figures for this transaction.
    assert_eq!(starts.top, Some(9_223_372_037_654_748_528));
    assert_eq!(reservoir, 9_223_372_037_654_748_528, "the CALL is the caller's first state use");
    assert_eq!(share, 9_038_904_596_901_653_558);
}

#[test]
fn test_create_child_reservoir_above_2_63_is_the_98_percent_share() {
    let mut db = InMemoryDB::default();
    db.insert_account_info(&CALLER, contract(&CREATE_EMPTY));
    let starts = run(db, &legacy(CALLER, Bytes::new()));
    assert_first_child_share(&starts);
}

#[test]
fn test_keyless_child_reservoir_above_2_63_is_the_98_percent_share() {
    let starts = run(InMemoryDB::default(), &legacy(KEYLESS, keyless_deploy(300_000)));
    assert_first_child_share(&starts);
}

/// The share table of the reference Satin implementation's own test, including `u64::MAX`.
#[test]
fn test_reservoir_share_matches_the_reference_table() {
    for (reservoir, share) in [
        (0, 0),
        (1, 1),
        (49, 49),
        (50, 49),
        (99, 98),
        (100, 98),
        (149, 147),
        (1_000, 980),
        (1_000_000, 980_000),
        (9_223_372_037_654_748_528, 9_038_904_596_901_653_558),
        (u64::MAX, u64::MAX - u64::MAX / 50),
    ] {
        assert_eq!(mega_evm2::ledger::reservoir_share(reservoir), share, "{reservoir}");
    }
}
