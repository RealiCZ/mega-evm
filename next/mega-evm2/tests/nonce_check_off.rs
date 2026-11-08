//! A creation transaction's address comes from its sender's state nonce, the nonce validation
//! returns before bumping it, as evm2's `execute_prepared` takes it: with the nonce check off (a
//! simulation that does not hold the envelope's nonce to the account's), a creation lands at the
//! state nonce's address, and an account already there is a collision, not bypassed by deploying
//! at the envelope nonce's address. A creation from a sender whose nonce cannot be bumped
//! (`2^64 − 1`) creates nothing and succeeds, as revm answers it.

use alloy_consensus::{
    SignableTransaction, TxEip1559, TxEip2930, TxLegacy, transaction::Recovered,
};
use alloy_primitives::{Address, Bytes, Signature, TxKind, U256};
use evm2::{
    Evm, EvmFeatures, ExecutionConfig, TxResult,
    bytecode::Bytecode,
    env::BlockEnv,
    evm::{AccountInfo, Db, InMemoryDB},
    interpreter::InstrStop,
};
use mega_evm2::{MegaSpec, MegaTypes, precompiles, satin_version, tx::registry};
use op_evm2::OpTxEnvelope;

const SENDER: Address = Address::with_last_byte(0x50);
/// The sender's nonce in the state; every envelope below carries nonce 0.
const STATE_NONCE: u64 = 7;
/// Init code returning one zero byte: the deployed code is `STOP`.
const INIT: [u8; 5] = [0x60, 0x01, 0x60, 0x00, 0xf3];

fn db(occupied: bool) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    db.insert_account_info(
        &SENDER,
        AccountInfo::default()
            .with_nonce(STATE_NONCE)
            .with_balance(U256::from(10).pow(U256::from(18))),
    );
    if occupied {
        db.insert_account_info(
            &SENDER.create(STATE_NONCE),
            AccountInfo::default()
                .with_nonce(1)
                .with_code(Bytecode::new_legacy(Bytes::from_static(&[0x00]))),
        );
    }
    db
}

/// A Satin EVM without the nonce check.
fn evm(db: InMemoryDB) -> Evm<'static, MegaTypes> {
    let mut version = satin_version();
    version.features.remove(EvmFeatures::NONCE_CHECK);
    let config = ExecutionConfig::<MegaTypes>::for_spec_and_version(MegaSpec::Satin, version);
    let block = BlockEnv::<MegaTypes> {
        number: U256::from(1),
        timestamp: U256::from(1_000),
        gas_limit: U256::from(30_000_000u64),
        ..BlockEnv::<MegaTypes>::default()
    };
    Evm::<MegaTypes>::new_with_execution_config(
        config,
        MegaSpec::Satin,
        block,
        registry(),
        Db::new(db),
        precompiles(),
    )
}

/// A creation of envelope type `ty` (0, 1 or 2) with nonce 0.
fn creation(ty: u8) -> Recovered<OpTxEnvelope> {
    let signature = Signature::new(U256::from(1), U256::from(1), false);
    let (gas_limit, to, input) = (1_000_000, TxKind::Create, Bytes::from_static(&INIT));
    let envelope = match ty {
        0 => OpTxEnvelope::from_signed(
            TxLegacy { chain_id: Some(1), gas_limit, to, input, ..Default::default() }
                .into_signed(signature),
        ),
        1 => OpTxEnvelope::from_signed(
            TxEip2930 { chain_id: 1, gas_limit, to, input, ..Default::default() }
                .into_signed(signature),
        ),
        _ => OpTxEnvelope::from_signed(
            TxEip1559 { chain_id: 1, gas_limit, to, input, ..Default::default() }
                .into_signed(signature),
        ),
    };
    Recovered::new_unchecked(envelope, SENDER)
}

fn run(occupied: bool, ty: u8) -> (TxResult<MegaTypes>, Evm<'static, MegaTypes>) {
    let mut evm = evm(db(occupied));
    let result = match evm.transact(&creation(ty)) {
        Ok(executed) => executed.commit(),
        Err(error) => panic!("type {ty} refused: {error:?}"),
    };
    (result, evm)
}

fn info(evm: &mut Evm<'_, MegaTypes>, address: Address) -> Option<AccountInfo> {
    evm.state_mut().account_info_untracked(&address).unwrap()
}

#[test]
fn test_nonce_check_off_creation_lands_at_the_state_nonce_address() {
    for ty in 0..=2 {
        let (result, mut evm) = run(false, ty);
        let at_state_nonce = SENDER.create(STATE_NONCE);
        assert!(result.status, "type {ty}: {result:?}");
        assert_eq!(result.created_address, Some(at_state_nonce), "type {ty}");
        let created = info(&mut evm, at_state_nonce).expect("created at the state nonce");
        assert_eq!(
            created.code_hash,
            Bytecode::new_legacy(Bytes::from_static(&[0x00])).hash_slow()
        );
        assert!(info(&mut evm, SENDER.create(0)).is_none(), "type {ty}: nothing at nonce 0");
        assert_eq!(info(&mut evm, SENDER).unwrap().nonce, STATE_NONCE + 1, "type {ty}");
    }
}

#[test]
fn test_nonce_check_off_collision_is_not_bypassed() {
    for ty in 0..=2 {
        let (result, mut evm) = run(true, ty);
        assert!(!result.status, "type {ty}: {result:?}");
        assert_eq!(result.stop, InstrStop::CreateCollision, "type {ty}");
        assert_eq!(result.created_address, None, "type {ty}");
        assert!(info(&mut evm, SENDER.create(0)).is_none(), "type {ty}: nothing at nonce 0");
        let occupant = info(&mut evm, SENDER.create(STATE_NONCE)).unwrap();
        assert_eq!(occupant.nonce, 1, "type {ty}: the occupant is untouched");
    }
}

/// What Satin's implementation answers for a creation of `PUSH1 1 PUSH0 RETURN` from a sender
/// whose nonce is `2^64 − 1`, with the nonce check off and 1,000,000 gas, for each of the three
/// envelopes: a success that creates nothing, gas used 51,698 (the intrinsic 24,066 and the 314
/// body bytes' history, 27,632: no start record and no created account's state gas is kept), and
/// one `Create` frame at level 0 started on 761,182 gas (after the created account's 183,600 of
/// state gas and the start record's 3,520) that returns all of it.
#[test]
fn test_a_creation_whose_nonce_cannot_be_bumped_creates_nothing_and_succeeds() {
    use evm2::{
        evm::inspector::Inspector,
        interpreter::{Interpreter, Message, MessageResult},
    };
    #[derive(Default)]
    struct Frames(Vec<(u16, u64, InstrStop, u64)>);
    impl Inspector<MegaTypes> for Frames {
        fn create_end(
            &mut self,
            _: &mut Interpreter<'_, '_, MegaTypes>,
            m: &Message<MegaTypes>,
            r: &mut MessageResult<MegaTypes>,
        ) {
            self.0.push((m.depth, m.gas_limit, r.stop, r.gas.remaining()));
        }
    }
    for ty in 0..=2u8 {
        let mut db = InMemoryDB::default();
        db.insert_account_info(
            &SENDER,
            AccountInfo::default()
                .with_nonce(u64::MAX)
                .with_balance(U256::from(10).pow(U256::from(18))),
        );
        let mut evm = evm(db);
        evm.set_inspector(Frames::default());
        let signature = Signature::new(U256::from(1), U256::from(1), false);
        let (gas_limit, to, input) =
            (1_000_000, TxKind::Create, Bytes::from_static(&[0x60, 0x01, 0x5f, 0xf3]));
        let envelope = match ty {
            0 => OpTxEnvelope::from_signed(
                TxLegacy { chain_id: Some(1), gas_limit, to, input, ..Default::default() }
                    .into_signed(signature),
            ),
            1 => OpTxEnvelope::from_signed(
                TxEip2930 { chain_id: 1, gas_limit, to, input, ..Default::default() }
                    .into_signed(signature),
            ),
            _ => OpTxEnvelope::from_signed(
                TxEip1559 { chain_id: 1, gas_limit, to, input, ..Default::default() }
                    .into_signed(signature),
            ),
        };
        let executed = evm.transact(&Recovered::new_unchecked(envelope, SENDER)).unwrap();
        let result = executed.commit();
        assert!(result.status, "type {ty}: {result:?}");
        assert_eq!(result.stop, InstrStop::Return, "type {ty}");
        assert_eq!(result.created_address, None, "type {ty}");
        assert_eq!(result.tx_gas_used(), 51_698, "type {ty}");
        assert_eq!(evm.ext().history(true), (27_632, 314), "type {ty}");
        assert_eq!(evm.ext().usage(true), [314, 0, 0], "type {ty}");
        let frames = evm.clear_inspector_as::<Frames>().unwrap().0;
        assert_eq!(frames, vec![(0, 761_182, InstrStop::Return, 761_182)], "type {ty}");
        assert!(info(&mut evm, SENDER.create(u64::MAX)).is_none(), "type {ty}: nothing created");
        assert_eq!(info(&mut evm, SENDER).unwrap().nonce, u64::MAX, "type {ty}");
    }
}
