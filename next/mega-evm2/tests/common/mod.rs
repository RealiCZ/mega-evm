//! The harness of the engine's give-back tests: a Satin EVM over an in-memory database at `m = 1`
//! everywhere, a frame recorder, and a keyless deployment's transaction and call.
#![allow(dead_code)]

use alloy_consensus::{SignableTransaction, TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256, keccak256};
use evm2::{
    Evm,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, DynDatabase, InMemoryDB},
    inspector::Inspector,
    interpreter::{InstrStop, Interpreter, Message, MessageResult},
};
use mega_evm2::{
    MegaTypes, TxLimits,
    block::BlockSession,
    chain::{BootstrapInput, ChainConfig},
    factory::{BlockHeader, BlockInput, BlockServices, ParentContext, SatinFactory, Simulation},
    keyless,
};
use op_evm2::OpTxEnvelope;

pub const SENDER: Address = Address::with_last_byte(0x50);
pub const TOP: Address = Address::with_last_byte(0xa0);
pub const P: Address = Address::with_last_byte(0xb0);
pub const C: Address = Address::with_last_byte(0xc0);
/// Satin's `KeylessDeploy` and `MegaLimitControl` system contracts.
pub const KEYLESS: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x03]);
pub const LIMIT_CONTROL: Address =
    Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x05]);

/// The history gas of one write record (`RECORD_HISTORY_GAS`), of an empty `LOG0` (32 bytes),
/// and a fresh slot's state gas at `m = 1` (`SLOT_STATE_GAS`) and a created account's
/// (`ACCOUNT_STATE_GAS`).
pub const RECORD: u64 = 3_520;
pub const LOG0: u64 = 2_816;
pub const SLOT_STATE: u64 = 97_920;
pub const ACCOUNT_STATE: u64 = 183_600;

pub mod op {
    pub const STOP: u8 = 0x00;
    pub const SUB: u8 = 0x03;
    pub const ORIGIN: u8 = 0x32;
    pub const TIMESTAMP: u8 = 0x42;
    pub const POP: u8 = 0x50;
    pub const MSTORE: u8 = 0x52;
    pub const SSTORE: u8 = 0x55;
    pub const GAS: u8 = 0x5a;
    pub const PUSH0: u8 = 0x5f;
    pub const PUSH1: u8 = 0x60;
    pub const PUSH2: u8 = 0x61;
    pub const PUSH20: u8 = 0x73;
    pub const PUSH32: u8 = 0x7f;
    pub const SWAP1: u8 = 0x90;
    pub const LOG0: u8 = 0xa0;
    pub const CREATE: u8 = 0xf0;
    pub const CALL: u8 = 0xf1;
    pub const RETURN: u8 = 0xf3;
    pub const DELEGATECALL: u8 = 0xf4;
    pub const STATICCALL: u8 = 0xfa;
    pub const REVERT: u8 = 0xfd;
    pub const SELFDESTRUCT: u8 = 0xff;
}

/// A frame as the inspector saw it open and close.
#[derive(Clone, Copy, Debug, Default)]
pub struct Frame {
    pub depth: u16,
    pub gas_limit: u64,
    pub reservoir: u64,
    pub stop: Option<InstrStop>,
    pub gas_left: u64,
    pub reservoir_left: u64,
}

#[derive(Debug, Default)]
pub struct Frames {
    pub frames: Vec<Frame>,
    open: Vec<usize>,
}

impl Frames {
    fn open(&mut self, m: &Message<MegaTypes>) {
        self.open.push(self.frames.len());
        self.frames.push(Frame {
            depth: m.depth,
            gas_limit: m.gas_limit,
            reservoir: m.reservoir,
            ..Frame::default()
        });
    }

    fn close(&mut self, r: &MessageResult<MegaTypes>) {
        let i = self.open.pop().expect("an open frame");
        let f = &mut self.frames[i];
        f.stop = Some(r.stop);
        f.gas_left = r.gas.remaining();
        f.reservoir_left = r.gas.reservoir();
    }
}

impl Inspector<MegaTypes> for Frames {
    fn call(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        m: &mut Message<MegaTypes>,
    ) -> Option<MessageResult<MegaTypes>> {
        self.open(m);
        None
    }
    fn call_end(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        _: &Message<MegaTypes>,
        r: &mut MessageResult<MegaTypes>,
    ) {
        self.close(r);
    }
    fn create(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        m: &mut Message<MegaTypes>,
    ) -> Option<MessageResult<MegaTypes>> {
        self.open(m);
        None
    }
    fn create_end(
        &mut self,
        _: &mut Interpreter<'_, '_, MegaTypes>,
        _: &Message<MegaTypes>,
        r: &mut MessageResult<MegaTypes>,
    ) {
        self.close(r);
    }
}

/// What a transaction left: its output and status, the frames below the transaction's own, and
/// the reservoir it returned to the sender.
pub struct Run {
    pub success: bool,
    pub stop: InstrStop,
    pub output: Bytes,
    pub frames: Vec<Frame>,
    pub reservoir_left: u64,
    /// What the transaction spent from both pools, and of it the state and history gas kept.
    pub total_gas_spent: u64,
    pub state_gas_spent: u64,
}

impl Run {
    pub fn frame(&self, depth: u16) -> Frame {
        *self.frames.iter().find(|f| f.depth == depth).expect("a frame at that depth")
    }

    pub fn word(&self, i: usize) -> u64 {
        U256::from_be_slice(&self.output[32 * i..32 * (i + 1)]).to::<u64>()
    }
}

pub fn contract(code: &[u8]) -> AccountInfo {
    AccountInfo::default()
        .with_nonce(1)
        .with_code(Bytecode::new_legacy(Bytes::copy_from_slice(code)))
}

/// Runs a legacy call from [`SENDER`] to `to` with `input` and gas limit `gas_limit`, gas price 0,
/// on `db` in a block at `m = 1` everywhere.
pub fn run(db: InMemoryDB, to: Address, input: Bytes, gas_limit: u64) -> Run {
    run_with(db, TxKind::Call(to), U256::ZERO, input, gas_limit)
}

/// [`run`] of a legacy transaction of kind `to` (a call or a creation) moving `value`.
pub fn run_with(db: InMemoryDB, to: TxKind, value: U256, input: Bytes, gas_limit: u64) -> Run {
    run_detained(db, to, value, input, gas_limit, None)
}

/// [`run_with`], with gas detention's block-environment cap at `cap` when given (a counterfactual
/// limit a test sets, `Lite::limits`).
pub fn run_detained(
    db: InMemoryDB,
    to: TxKind,
    value: U256,
    input: Bytes,
    gas_limit: u64,
    cap: Option<u64>,
) -> Run {
    run_limited(db, to, value, input, gas_limit, |limits| {
        if let Some(cap) = cap {
            limits.cap = cap;
        }
    })
}

/// [`run_with`], with Satin's transaction limits changed by `limits` first (a counterfactual limit
/// a test sets, `Lite::limits`).
pub fn run_limited(
    db: InMemoryDB,
    to: TxKind,
    value: U256,
    input: Bytes,
    gas_limit: u64,
    limits: impl FnOnce(&mut TxLimits),
) -> Run {
    let mut tx_limits = TxLimits::SATIN;
    limits(&mut tx_limits);
    let simulation = Simulation { limits: Some(tx_limits), ..Simulation::default() };
    let mut evm = simulated_evm(block(), Db::new(db), &simulation);
    run_on(&mut evm, to, value, input, gas_limit)
}

/// The chain id of the tests' chain.
pub const CHAIN_ID: u64 = 1;

/// The tests' chain: Satin and every fork below it active from genesis, at timestamp zero, chain
/// id [`CHAIN_ID`], the registry's bootstrap values non-zero.
pub fn chain_config() -> ChainConfig {
    let bootstrap = BootstrapInput {
        initial_sequencer: Some(Address::repeat_byte(0x5e)),
        initial_admin: Some(Address::repeat_byte(0xad)),
        min_rotation_delay: Some(1),
    };
    ChainConfig::satin_from_genesis(CHAIN_ID, bootstrap).expect("a valid configuration")
}

/// The engine's factory on the tests' chain ([`chain_config`]).
pub fn factory() -> SatinFactory {
    SatinFactory::new(chain_config())
}

/// The block environment of block 1 at timestamp 1,000 with a gas limit of 10,000,000,000.
pub fn block() -> BlockEnv<MegaTypes> {
    BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(10_000_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    }
}

/// A single transaction's Satin EVM over `database` in `block`, from the factory.
pub fn evm<'a>(block: BlockEnv<MegaTypes>, database: impl DynDatabase + 'a) -> Evm<'a, MegaTypes> {
    factory().evm(block, database).expect("a Satin timestamp")
}

/// [`evm`] under a simulation's switches.
pub fn simulated_evm<'a>(
    block: BlockEnv<MegaTypes>,
    database: impl DynDatabase + 'a,
    simulation: &Simulation,
) -> Evm<'a, MegaTypes> {
    factory().simulated_evm(block, database, simulation).expect("a Satin timestamp")
}

/// Block `number` (at least 1) at `timestamp` with gas limit `gas_limit`, on a parent one block
/// and one second before it, with a zero parent hash (the parent's hash) and `root` as its parent
/// beacon block root.
pub fn block_input(number: u64, timestamp: u64, gas_limit: u64, root: Option<B256>) -> BlockInput {
    BlockInput {
        header: BlockHeader {
            number,
            timestamp,
            gas_limit,
            parent_beacon_block_root: root,
            ..BlockHeader::default()
        },
        parent: ParentContext::Parent {
            number: number - 1,
            hash: B256::ZERO,
            timestamp: if number == 1 { 0 } else { timestamp - 1 },
        },
    }
}

/// The value the block's change set holds for `address`'s slot `key`, when a committed
/// transaction or pre-block step changed it.
pub fn committed_slot(block: &BlockSession<'_>, address: Address, key: U256) -> Option<U256> {
    block
        .changes()
        .storage()
        .find(|(k, _)| k.address() == address && k.key() == key)
        .map(|(_, t)| t.current)
}

/// A consensus block session for `input` over `database`, its pre-block sequence run.
pub fn start_block<'a>(input: &BlockInput, database: impl DynDatabase + 'a) -> BlockSession<'a> {
    factory().start_block(input, database, BlockServices::default()).expect("the block starts")
}

/// A Satin EVM over `db` in a block at `m = 1` everywhere.
pub fn satin_evm(db: InMemoryDB) -> Evm<'static, MegaTypes> {
    evm(block(), Db::new(db))
}

/// Runs a legacy transaction from [`SENDER`] of kind `to` moving `value` with `input` and gas
/// limit `gas_limit`, gas price 0, on `evm`, and commits it; `evm`'s state stays readable.
pub fn run_on(
    evm: &mut Evm<'static, MegaTypes>,
    to: TxKind,
    value: U256,
    input: Bytes,
    gas_limit: u64,
) -> Run {
    evm.set_inspector(Frames::default());
    let tx = TxLegacy { chain_id: Some(1), nonce: 0, gas_price: 0, gas_limit, to, value, input };
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let tx = Recovered::new_unchecked(OpTxEnvelope::from_signed(tx.into_signed(signature)), SENDER);
    let result = match evm.transact(&tx) {
        Ok(executed) => executed.commit(),
        Err(error) => panic!("transaction refused: {error:?}"),
    };
    let reservoir_left = evm.ext().reservoir_left;
    let frames = evm.clear_inspector_as::<Frames>().expect("the inspector").frames;
    Run {
        success: result.status,
        stop: result.stop,
        total_gas_spent: result.total_gas_spent,
        state_gas_spent: result.state_gas_spent,
        output: result.output.clone(),
        frames: frames.into_iter().filter(|f| f.depth > 0).collect(),
        reservoir_left,
    }
}

/// A pre-EIP-155 creation of `init` with the fixed signature the keyless fixtures use, and the
/// account that signed it.
pub fn keyless_tx(init: &[u8]) -> (Vec<u8>, Address) {
    let tx = TxLegacy {
        chain_id: None,
        nonce: 0,
        gas_price: 100_000_000_000,
        gas_limit: 200_000,
        to: TxKind::Create,
        value: U256::ZERO,
        input: Bytes::copy_from_slice(init),
    };
    let r = U256::from_be_bytes([0x22; 32]);
    let signed = tx.into_signed(Signature::new(r, r, false));
    let signer = keyless::recover(&signed).expect("a signer");
    let mut raw = Vec::new();
    alloy_eips::eip2718::Encodable2718::encode_2718(&signed, &mut raw);
    (raw, signer)
}

/// ABI-encoded `keylessDeploy(bytes signedTx, uint256 gasLimitOverride)`.
pub fn keyless_deploy(raw: &[u8], gas_limit_override: u64) -> Bytes {
    let word = |v: u64| U256::from(v).to_be_bytes::<32>();
    let mut input = keccak256("keylessDeploy(bytes,uint256)")[..4].to_vec();
    input.extend_from_slice(&word(0x40));
    input.extend_from_slice(&word(gas_limit_override));
    input.extend_from_slice(&word(raw.len() as u64));
    let mut padded = raw.to_vec();
    padded.resize(raw.len().div_ceil(32) * 32, 0);
    input.extend_from_slice(&padded);
    input.into()
}

/// The `errorData` of a `keylessDeploy` answer `(uint64 gasUsed, address, bytes errorData)`.
pub fn error_data(output: &[u8]) -> &[u8] {
    let len = U256::from_be_slice(&output[96..128]).to::<usize>();
    &output[128..128 + len]
}

/// A custom error with one `uint64` argument.
pub fn error_u64(signature: &str, v: u64) -> Vec<u8> {
    let mut e = keccak256(signature)[..4].to_vec();
    e.extend_from_slice(&U256::from(v).to_be_bytes::<32>());
    e
}
