//! An EIP-7702 transaction's authorities are recovered through the installed crypto provider, as
//! the ecrecover precompile's are; the y-parity and `s ≤ secp256k1n/2` checks stay outside it.
//!
//! The provider is process-wide and set once, so this test lives in its own binary.

use alloy_consensus::{TxEip7702, transaction::Recovered};
use alloy_eips::eip7702::{Authorization, SignedAuthorization, constants::SECP256K1N_HALF};
use alloy_primitives::{Address, Bytes, U256};
use evm2::{
    Crypto, Evm, ExecutionConfig, PrecompileHalt,
    bytecode::Bytecode,
    env::BlockEnv,
    ethereum::LazyTxEip7702,
    evm::{AccountInfo, Db, InMemoryDB},
    install_crypto,
    interpreter::InstrStop,
};
use mega_evm2::{MegaSpec, MegaTypes, precompiles, satin_version, tx::registry};
use op_evm2::OpTxEnvelope;

const SENDER: Address = Address::with_last_byte(0x50);
const TARGET: Address = Address::with_last_byte(0xa0);
const DELEGATE: Address = Address::with_last_byte(0xd0);
/// The authority the provider recovers from every signature.
const AUTHORITY: Address = Address::repeat_byte(0x7a);

/// Recovers [`AUTHORITY`] from any signature it is given.
#[derive(Debug)]
struct FixedRecovery;

impl Crypto for FixedRecovery {
    fn secp256k1_ecrecover(
        &self,
        _: &[u8; 64],
        _: u8,
        _: &[u8; 32],
    ) -> Result<[u8; 32], PrecompileHalt> {
        Ok(AUTHORITY.into_word().0)
    }
}

/// A list entry delegating to [`DELEGATE`] at `nonce`, with the given y-parity and `s`.
fn entry(nonce: u64, y_parity: u8, s: U256) -> SignedAuthorization {
    let inner = Authorization { chain_id: U256::from(1), address: DELEGATE, nonce };
    SignedAuthorization::new_unchecked(inner, y_parity, U256::ONE, s)
}

#[test]
fn test_authorities_are_recovered_by_the_installed_provider() {
    assert!(install_crypto(FixedRecovery), "the provider was already set");

    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default().with_balance(U256::from(10).pow(U256::from(18))),
    );
    db.insert_account_info(
        &DELEGATE,
        AccountInfo::default().with_code(Bytecode::new_legacy(Bytes::from_static(&[0x00]))),
    );
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
        Db::new(db),
        precompiles(),
    );
    evm.state_mut().set_read_absent_storage(true);
    // The provider recovers AUTHORITY from all three entries. The first applies at nonce 0; the
    // second (an `s` above secp256k1n/2) and the third (a y-parity of 2) would apply at nonce 1,
    // but both are refused before the provider is asked.
    let tx = TxEip7702 {
        chain_id: 1,
        gas_limit: 10_000_000,
        to: TARGET,
        authorization_list: vec![
            entry(0, 0, U256::ONE),
            entry(1, 0, SECP256K1N_HALF + U256::ONE),
            entry(1, 2, U256::ONE),
        ],
        ..Default::default()
    };
    // Signed entries, recovered on demand, as a decoded block transaction carries them.
    let envelope = OpTxEnvelope::new(LazyTxEip7702::from(tx), Some(Bytes::new()));
    let result = evm.transact(&Recovered::new_unchecked(envelope, SENDER)).expect("valid").commit();
    assert_eq!(result.stop, InstrStop::Stop);

    let mut authority = evm.state_mut().account(&AUTHORITY).unwrap();
    assert_eq!(authority.nonce(), 1, "only the well-formed entry applies");
    assert_eq!(authority.load_code().unwrap(), Bytecode::new_eip7702(DELEGATE));
}
