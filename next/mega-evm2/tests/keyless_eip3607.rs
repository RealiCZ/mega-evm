//! Keyless deployment's signer-code rule is EIP-3607's: unless the feature is disabled, a signer
//! with code other than an EIP-7702 delegation is refused with `SignerHasCode()`. With the feature
//! off the signer's code is neither loaded nor checked, and the deployment goes on.
//!
//! The differential gates run Satin's version, where EIP-3607 is on, and the scenario format
//! cannot turn it off, so this file runs the gate's `k16c_signer_has_code` input (the `ok` keyless
//! transaction, a gas override of 1,000,000, its signer holding the code `0x00`) with the feature
//! on and off. The expected outcomes are the Satin implementation's under review for the same
//! input, the off case with `CfgEnv::disable_eip3607` set.

use alloy_consensus::{TxLegacy, transaction::Recovered};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, address, hex, keccak256};
use evm2::{
    DatabaseError, Evm, EvmFeatures, ExecutionConfig,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Database, Db, InMemoryDB},
    interpreter::{InstrStop, Word},
    registry::HandlerError,
};
use mega_evm2::{MegaSpec, MegaTypes, precompiles, satin_version, tx::registry};
use op_evm2::OpTxEnvelope;

/// The gate scenarios' sender.
const SENDER: Address = Address::new([0x50; 20]);
const KEYLESS: Address = address!("0x6342000000000000000000000000000000000003");
/// The `ok` keyless transaction of the gate scenarios: a legacy creation without a chain id whose
/// init code `PUSH1 1 PUSH1 0 RETURN` deploys the code `0x00`, signed for the signer below.
const OK_TX: &str = "f8568085174876e80083030d4080808560016000f31ba02222222222222222222222222222222222222222222222222222222222222222a02222222222222222222222222222222222222222222222222222222222222222";
const SIGNER: Address = address!("0x25a3eF92221C997F6abE3b393C5bA020c5E7EcD3");
/// `SIGNER.create(0)`, the deploy address.
const DEPLOY: Address = address!("0x7D62ff74389eBd91F9f40b72533eaa574Cbb6073");

fn selector(sig: &str) -> [u8; 4] {
    keccak256(sig)[..4].try_into().unwrap()
}

fn word(v: u64) -> [u8; 32] {
    U256::from(v).to_be_bytes()
}

/// `keylessDeploy(bytes,uint256)` with the `ok` transaction and a gas override of 1,000,000.
fn keyless_call() -> Bytes {
    let raw = hex::decode(OK_TX).unwrap();
    let mut input = selector("keylessDeploy(bytes,uint256)").to_vec();
    input.extend_from_slice(&word(0x40));
    input.extend_from_slice(&word(1_000_000));
    input.extend_from_slice(&word(raw.len() as u64));
    let mut data = raw.clone();
    data.resize(raw.len().div_ceil(32) * 32, 0);
    input.extend_from_slice(&data);
    input.into()
}

#[derive(Debug)]
struct Unavailable;

impl core::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("signer code unavailable")
    }
}

impl std::error::Error for Unavailable {}

/// The sender (1 ether) and the signer holding `signer_code`; with `code_unavailable` the
/// signer's account comes without its code and a read of that code fails.
#[derive(Debug)]
struct Backend {
    inner: InMemoryDB,
    signer_code_hash: B256,
    code_unavailable: bool,
}

impl Backend {
    fn new(signer_code: &'static [u8], code_unavailable: bool) -> Self {
        let mut inner = InMemoryDB::default();
        inner.insert_account_info(
            &SENDER,
            AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
        );
        inner.insert_account_info(
            &SIGNER,
            AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from_static(signer_code))),
        );
        Self { inner, signer_code_hash: keccak256(signer_code), code_unavailable }
    }
}

impl Database for Backend {
    type Error = DatabaseError;

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        let mut info = self.inner.get_account(address)?;
        if self.code_unavailable &&
            *address == SIGNER &&
            let Some(info) = &mut info
        {
            info.code = None;
        }
        Ok(info)
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        if self.code_unavailable && *code_hash == self.signer_code_hash {
            return Err(DatabaseError::new(Unavailable, true));
        }
        self.inner.get_code_by_hash(code_hash)
    }

    fn get_storage(&mut self, address: &Address, key: &Word) -> Result<Word, Self::Error> {
        self.inner.get_storage(address, key)
    }

    fn get_block_hash(&mut self, number: &Word) -> Result<B256, Self::Error> {
        self.inner.get_block_hash(number)
    }
}

/// The gate's engine for the scenario: Satin's version, with EIP-3607 removed when `eip3607` is
/// false, the default limits and caps.
fn satin_evm(eip3607: bool, backend: Backend) -> Evm<'static, MegaTypes> {
    let mut version = satin_version();
    if !eip3607 {
        version.features.remove(EvmFeatures::EIP3607);
    }
    let config = ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, version);
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(10_000_000_000u64),
        slot_num: U256::from(42),
        ..BlockEnv::<MegaTypes>::default()
    };
    let mut evm = Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(backend),
        precompiles(),
    );
    evm.state_mut().set_read_absent_storage(true);
    let ext = evm.ext_mut();
    ext.cfg.limits = [13_107_200, 500_000, 1_000];
    ext.cfg.cap = 20_000_000;
    ext.cfg.oracle_cap = 20_000_000;
    evm
}

fn keyless_tx() -> Recovered<OpTxEnvelope> {
    let tx = TxLegacy {
        chain_id: None,
        nonce: 0,
        gas_price: 0,
        gas_limit: 5_000_000,
        to: TxKind::Call(KEYLESS),
        value: U256::ZERO,
        input: keyless_call(),
    };
    Recovered::new_unchecked(OpTxEnvelope::new(tx, Some(Bytes::new())), SENDER)
}

/// The deployment's ABI answer: `(gasUsed, deployedAddress, errorData)`.
fn answer(gas_used: u64, deployed: Address) -> Vec<u8> {
    let mut out = word(gas_used).to_vec();
    out.extend_from_slice(&[0; 12]);
    out.extend_from_slice(deployed.as_slice());
    out.extend_from_slice(&word(0x60));
    out.extend_from_slice(&word(0));
    out
}

/// The reference's outcome with EIP-3607 disabled: the deployment succeeds, its constructor used
/// 1,633 gas; 365,727 gas used, of which 128,941 regular and 236,786 state (185,130 of it new
/// accounts' and 51,656 history); the signer's nonce is 1 and the deploy address holds `0x00`.
#[test]
fn test_keyless_signer_code_is_not_checked_with_eip3607_disabled() {
    let mut evm = satin_evm(false, Backend::new(&[0x00], false));
    let result = evm.transact(&keyless_tx()).expect("a valid transaction").commit();
    assert_eq!(result.stop, InstrStop::Return, "{result:?}");
    assert_eq!(result.output.as_ref(), answer(0x661, DEPLOY).as_slice());
    assert_eq!(result.tx_gas_used(), 365_727);
    assert_eq!(result.total_gas_spent - result.state_gas_spent, 128_941);
    assert_eq!(result.state_gas_spent, 185_130 + 51_656);
    let signer = evm.state_mut().account(&SIGNER).unwrap().get().cloned().unwrap();
    assert_eq!(signer.nonce, 1);
    let deployed = evm.state_mut().account(&DEPLOY).unwrap().get().cloned().unwrap();
    assert_eq!((deployed.nonce, deployed.code_hash), (1, keccak256([0x00])));
}

/// The control, the gate's own configuration: with EIP-3607 on the signer's code is refused, as
/// the reference refuses it (`SignerHasCode()`, 161,452 gas used, nothing deployed).
#[test]
fn test_keyless_signer_with_code_is_refused_with_eip3607_enabled() {
    let mut evm = satin_evm(true, Backend::new(&[0x00], false));
    let result = evm.transact(&keyless_tx()).expect("a valid transaction").commit();
    assert_eq!(result.stop, InstrStop::Revert, "{result:?}");
    assert_eq!(result.output.as_ref(), selector("SignerHasCode()"));
    assert_eq!(result.tx_gas_used(), 161_452);
    assert!(evm.state_mut().account(&DEPLOY).unwrap().get().is_none());
}

/// With EIP-3607 disabled the signer's code is not even read: a database that cannot serve it does
/// not stop the deployment. With the feature on, the read is made and its error fails the
/// transaction.
#[test]
fn test_keyless_reads_no_signer_code_with_eip3607_disabled() {
    let mut evm = satin_evm(false, Backend::new(&[0xfe], true));
    let result = evm.transact(&keyless_tx()).expect("no read of the signer's code").commit();
    assert_eq!(result.stop, InstrStop::Return, "{result:?}");
    assert_eq!(result.output.as_ref(), answer(0x661, DEPLOY).as_slice());

    let mut evm = satin_evm(true, Backend::new(&[0xfe], true));
    match evm.transact(&keyless_tx()).err() {
        Some(HandlerError::Database(e)) => assert!(e.downcast_ref::<Unavailable>().is_some()),
        other => panic!("expected the code read's error, got {other:?}"),
    }
}
