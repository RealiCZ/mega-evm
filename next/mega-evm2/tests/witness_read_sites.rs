//! Every read site, replayed on exactly its witness (`SATIN-WIT-016`): the destruction table's
//! rows, a keyless deployment refused at each of its steps and one whose constructor fails, the
//! Oracle read through `multiCall` and on a delegating contract's storage, with its own write and
//! in a batch that reverts, the pre-block steps of the activation block, of block zero and of a
//! registry with a due change or a wiped account, a type-1 transaction's access list, and the reads
//! gas or a limit skips (`SATIN-WIT-012`). Each block replays with the same receipts, states,
//! per-transaction ledgers and counters, and each piece of evidence it reads, removed, fails it
//! with a typed missing-evidence error ([`assert_exact_replay`]); a read that was skipped is in no
//! state and no witness, so the replay needs none.
//!
//! What this checks is what execution demands: the evidence a replay reads. That the witness also
//! holds what the protocol requires and execution may not read — the code of an account loaded
//! without it, the exports of dropped candidates — is a check of the witness against its keys
//! (`witness_replay.rs`), not of the replay.

mod witness_fixture;

use alloy_consensus::{SignableTransaction, TxEip2930};
use alloy_eips::eip2930::{AccessList, AccessListItem};
use alloy_primitives::{Address, B256, Bytes, TxKind, U256, keccak256};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, Db, InMemoryDB},
};
use mega_evm2::{
    SEQUENCER_REGISTRY,
    block::{BEACON_ROOTS, HISTORY_STORAGE},
    chain::{BootstrapInput, ChainConfig, ChainConfigInput, Fork, ForkSchedule},
    factory::{BlockHeader, BlockInput, ParentContext, SatinFactory},
    oracle::{OracleAnswers, OracleServices},
    system_contracts::{CREATE2_FACTORY, address_word},
    tx::ORACLE,
    witness::Evidence,
};
use op_evm2::OpTxEnvelope;
use std::sync::Arc;
use witness_fixture::*;

/// Creates, by `CREATE2` with one wei, the account its constructor destroys.
const FACTORY: Address = Address::with_last_byte(0xf0);
/// Calls [`FACTORY`], then reverts.
const WRAPPER: Address = Address::with_last_byte(0xf1);
/// A contract the chain holds, with a balance, that destroys itself.
const OLD: Address = Address::with_last_byte(0xf2);
/// The beneficiary of every `SELFDESTRUCT`.
const HEIR: Address = Address::with_last_byte(0xbd);
/// The block beneficiary of the blocks whose frames turn their volatile-data access off.
const BENEFICIARY: Address = Address::with_last_byte(0xbe);
const SALT: u64 = 0x2a;

fn ether() -> U256 {
    U256::from(10).pow(U256::from(18))
}

/// The block [`BLOCK`] built over `parent` with `oracle`, every transaction of `txs` included.
fn produce_with(parent: &InMemoryDB, oracle: OracleServices, txs: &[Tx]) -> Produced {
    produce_over(parent, Db::new(parent.clone()), oracle, &included(txs))
}

/// The block [`BLOCK`] paid to [`BENEFICIARY`], built over `parent`, every transaction of `txs`
/// included.
fn produce_paid(parent: &InMemoryDB, txs: &[Tx]) -> Produced {
    let mut block = input();
    block.header.beneficiary = BENEFICIARY;
    let database = Db::new(parent.clone());
    let steps = included(txs);
    produce_block(common::factory(), block, parent, database, OracleServices::default(), &steps)
}

// ---------------------------------------------------------------------------------------------
// The destruction table

/// The constructor: `SSTORE(5, 9)`, then `SELFDESTRUCT(HEIR)`.
fn self_destroying() -> Vec<u8> {
    [asm::sstore(5, 9), asm::push_address(HEIR), vec![0xff]].concat()
}

/// Stores `init` at memory 0 byte by byte, then `CREATE2(1, 0, len, SALT)`, and stops.
fn factory_code(init: &[u8]) -> Vec<u8> {
    let mut code = Vec::new();
    for (i, byte) in init.iter().enumerate() {
        code.extend(asm::push(*byte as u64));
        code.extend(asm::push(i as u64));
        code.push(0x53);
    }
    code.extend(asm::push(SALT));
    code.extend(asm::push(init.len() as u64));
    code.extend([0x5f, 0x60, 0x01, 0xf5, 0x50, 0x00]);
    code
}

/// The address [`FACTORY`] creates.
fn destroyed() -> Address {
    FACTORY.create2(B256::left_padding_from(&[SALT as u8]), keccak256(self_destroying()))
}

/// The fixture's chain with [`FACTORY`] (holding ten wei), [`WRAPPER`] and [`OLD`] (holding
/// five), and, at the address the factory creates, `created`; at [`HEIR`], `heir`.
fn destruction_chain(created: Option<AccountInfo>, heir: Option<AccountInfo>) -> InMemoryDB {
    let mut parent = chain(L1::Present);
    let mut factory = contract(factory_code(&self_destroying()));
    factory.balance = U256::from(10);
    parent.insert_account_info(&FACTORY, factory);
    let wrapper = [asm::call(FACTORY, None, 0), vec![0x5f, 0x5f, 0xfd]].concat();
    parent.insert_account_info(&WRAPPER, contract(wrapper));
    let mut old = contract([asm::push_address(HEIR), vec![0xff]].concat());
    old.balance = U256::from(5);
    parent.insert_account_info(&OLD, old);
    if let Some(info) = created {
        parent.insert_account_info(&destroyed(), info);
        parent.insert_account_storage(&destroyed(), &U256::from(5), &U256::from(7));
    }
    if let Some(info) = heir {
        parent.insert_account_info(&HEIR, info);
    }
    parent
}

/// A constructor destroying the account it creates, on each row of the destruction table's
/// leaf, replays exactly: an absent address, an address holding a leaf that is empty under
/// EIP-161 and storage (a present leaf: the destruction refills nothing of it, `SATIN-SD-017`),
/// and an address holding a balance before the transaction (`SATIN-SD-006`); its heir absent or an
/// existing empty account. The witness tells an absent account from an empty leaf
/// (`SATIN-WIT-017`), and the replay prices from that: the same transaction on the absent address
/// and on the empty leaf spends differently, and the empty leaf's witness with the leaf replaced by
/// an absence replays another ledger.
#[test]
fn witness_replay_of_the_destruction_table_rows() {
    let empty_leaf = AccountInfo::default();
    let prefunded = AccountInfo::default().with_balance(U256::from(3));
    let tx = call(SENDER, 0, FACTORY);
    let mut rows = Vec::new();
    for (created, heir) in [
        (None, None),
        (None, Some(AccountInfo::default())),
        (Some(empty_leaf.clone()), None),
        (Some(prefunded), None),
    ] {
        let parent = destruction_chain(created.clone(), heir.clone());
        let built = produce(&parent, &included(std::slice::from_ref(&tx)));
        assert!(built.output.receipts[0].status);
        let state = &built.output.tx_states[0];
        let gone = state.account_changes_for(&destroyed()).expect("the created address");
        assert!(gone.current.is_none(), "created and destroyed in the transaction");
        let heir_now = state.account_changes_for(&HEIR).and_then(|h| h.current.cloned());
        let left = created.as_ref().map_or(U256::ZERO, |c| c.balance);
        assert_eq!(heir_now.map(|h| h.balance), Some(U256::from(1) + left), "the heir's balance");
        assert_eq!(built.witness.account(&destroyed()), Some(created.as_ref()));
        assert_eq!(built.witness.account(&HEIR), Some(heir.as_ref()));
        let read = assert_exact_replay(&built);
        assert!(read.contains(&Evidence::Account(destroyed())));
        assert!(read.contains(&Evidence::Account(HEIR)));
        rows.push(built);
    }
    // An absent heir and an empty one are both empty under EIP-161: the same charges
    // (`SATIN-SD-018`).
    assert_eq!(rows[0].ledgers, rows[1].ledgers);
    // The present leaf is not refilled: the transaction keeps the new-account charge.
    assert_ne!(rows[0].ledgers[0].gas, rows[2].ledgers[0].gas);
    let built = &rows[2];
    let mut witness = built.witness.clone();
    witness.insert_account(destroyed(), None);
    let (replay, _) = replay(witness, &built.included);
    let replay = replay.expect("an absent address replays");
    assert_ne!(replay.ledgers, built.ledgers, "the leaf decides the price");
}

/// A destruction inside a frame that then reverts takes back its effects, not its reads: the
/// created address and the heir are in the transaction's state and in the witness, and the replay,
/// which reverts the same way, needs both. A contract the chain holds that destroys itself is not
/// deleted (EIP-6780): its balance moves to the heir, and both reads are required.
#[test]
fn witness_rollback_of_a_destruction_and_a_destruction_of_an_old_account() {
    let parent = destruction_chain(None, None);
    let built = produce(&parent, &included(&[call(SENDER, 0, WRAPPER)]));
    assert!(!built.output.receipts[0].status, "the wrapper reverts");
    for address in [destroyed(), HEIR] {
        assert!(built.output.tx_states[0].account_changes_for(&address).is_some(), "{address}");
    }
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Account(destroyed())));
    assert!(read.contains(&Evidence::Account(HEIR)));
    let built = produce(&parent, &included(&[call(SENDER, 0, OLD)]));
    assert!(built.output.receipts[0].status);
    let old = built.output.changes.accounts().find(|(a, _)| *a == OLD).expect("its balance moved");
    assert_eq!(old.1.current.as_ref().map(|a| a.balance), Some(U256::ZERO), "not deleted");
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Account(OLD)) && read.contains(&Evidence::Account(HEIR)));
}

// ---------------------------------------------------------------------------------------------
// Keyless deployment

/// The keyless call of `raw` with a gas override of `gas`, from [`SENDER`].
fn keyless_call(raw: &[u8], gas: u64) -> Tx {
    let input = common::keyless_deploy(raw, gas).to_vec();
    legacy(SENDER, 0, TxKind::Call(common::KEYLESS), 0, input)
}

/// A keyless deployment refused at each step reads only what the steps before the refusal read
/// (`SATIN-WIT-012`): refused before its signer is loaded (a gas override below the signed gas
/// limit), the witness names neither the signer nor the deploy address; refused at the signer (a
/// nonce above one, code), it names the signer and not the deploy address; refused at the deploy
/// address (code there), it names both. Each replays exactly, every read it makes required.
#[test]
fn witness_replay_of_a_keyless_deployment_refused_at_each_step() {
    let (raw, signer) = common::keyless_tx(&[0x60, 0x01, 0x5f, 0xf3]);
    let deploy = signer.create(0);
    let funded = AccountInfo::default().with_balance(ether());
    let with_code =
        |info: AccountInfo| info.with_code(Bytecode::new_legacy(Bytes::from_static(&[0x00])));
    // (the signer, the deploy address, the override; the refusal; whether the signer and the
    // deploy address are named)
    let cases = [
        (funded.clone(), None, 100_000, "GasLimitTooLow(uint64,uint64)", false, false),
        (funded.clone().with_nonce(2), None, 1_000_000, "SignerNonceTooHigh(uint64)", true, false),
        (with_code(funded.clone()), None, 1_000_000, "SignerHasCode()", true, false),
        (
            funded.clone(),
            Some(with_code(AccountInfo::default().with_nonce(1))),
            1_000_000,
            "ContractAlreadyExists()",
            true,
            true,
        ),
    ];
    for (signer_info, deploy_info, gas, refusal, signer_named, deploy_named) in cases {
        let mut parent = chain(L1::Present);
        parent.insert_account_info(&signer, signer_info);
        if let Some(info) = deploy_info {
            parent.insert_account_info(&deploy, info);
        }
        let built = produce(&parent, &included(&[keyless_call(&raw, gas)]));
        assert_eq!(built.ledgers[0].result.output[..4], selector(refusal)[..], "{refusal}");
        let state = &built.output.tx_states[0];
        assert_eq!(state.account_changes_for(&signer).is_some(), signer_named, "{gas}");
        assert_eq!(state.account_changes_for(&deploy).is_some(), deploy_named, "{gas}");
        assert_eq!(built.witness.account(&deploy).is_some(), deploy_named);
        assert!(!state.account_changes_for(&deploy).is_some_and(|d| d.is_changed()), "refused");
        let read = assert_exact_replay(&built);
        assert_eq!(read.contains(&Evidence::Account(signer)), signer_named);
    }
}

/// A keyless deployment whose constructor reverts or halts keeps its reads, the signer (here with
/// nonce one) and the deploy address both named and required. A deploy address the chain does not
/// hold is created against its SALT bucket, which the block exports and the replay needs
/// (`SATIN-WIT-013`); one holding a balance without code is not empty, so no creation charge
/// prices it and no bucket is read.
#[test]
fn witness_replay_of_a_keyless_deployment_whose_constructor_fails() {
    for (init, prefunded) in [
        (vec![0x5f, 0x5f, 0xfd], false),
        (vec![0xfe], false),
        (vec![0x5f, 0x5f, 0xfd], true),
        (vec![0xfe], true),
    ] {
        let (raw, signer) = common::keyless_tx(&init);
        let deploy = signer.create(0);
        let mut parent = chain(L1::Present);
        let signer_info = AccountInfo::default().with_nonce(1).with_balance(ether());
        parent.insert_account_info(&signer, signer_info);
        if prefunded {
            parent.insert_account_info(&deploy, AccountInfo::default().with_balance(U256::from(5)));
        }
        let built = produce(&parent, &included(&[keyless_call(&raw, 1_000_000)]));
        let state = &built.output.tx_states[0];
        assert!(state.account_changes_for(&signer).is_some());
        let deployed = state.account_changes_for(&deploy).expect("the deploy address is loaded");
        let code = deployed.current.map(|d| d.code_hash);
        assert!(code.is_none_or(|c| c == alloy_primitives::KECCAK256_EMPTY), "nothing deployed");
        let read = assert_exact_replay(&built);
        assert!(read.contains(&Evidence::Account(signer)));
        assert!(read.contains(&Evidence::Account(deploy)));
        let bucket = mega_evm2::salt::bucket_id(deploy.as_slice());
        assert_eq!(built.output.bucket_reads.contains(&bucket), !prefunded, "{init:?}");
        assert_eq!(read.contains(&Evidence::Capacity(bucket)), !prefunded, "{init:?}");
    }
}

// ---------------------------------------------------------------------------------------------
// The Oracle

fn selector(signature: &str) -> Vec<u8> {
    keccak256(signature)[..4].to_vec()
}

fn word(v: u64) -> [u8; 32] {
    U256::from(v).to_be_bytes()
}

fn get_slot(slot: u64) -> Vec<u8> {
    [selector("getSlot(uint256)"), word(slot).to_vec()].concat()
}

fn set_slot(slot: u64, value: u64) -> Vec<u8> {
    [selector("setSlot(uint256,bytes32)"), word(slot).to_vec(), word(value).to_vec()].concat()
}

fn padded(data: &[u8]) -> Vec<u8> {
    let mut out = data.to_vec();
    out.resize(data.len().div_ceil(32) * 32, 0);
    out
}

/// `multiCall(bytes[])` of `payloads`.
fn multi_call(payloads: &[Vec<u8>]) -> Vec<u8> {
    let mut input = selector("multiCall(bytes[])");
    input.extend_from_slice(&word(0x20));
    input.extend_from_slice(&word(payloads.len() as u64));
    let mut offset = 32 * payloads.len() as u64;
    for p in payloads {
        input.extend_from_slice(&word(offset));
        offset += 32 + padded(p).len() as u64;
    }
    for p in payloads {
        input.extend_from_slice(&word(p.len() as u64));
        input.extend(padded(p));
    }
    input
}

/// A service answering slot 5 with the value the chain holds there, as a sequencer that persisted
/// it.
fn persisted() -> OracleServices {
    let answers = OracleAnswers::from_iter([(U256::from(ORACLE_SLOT), U256::from(ORACLE_HELD))]);
    OracleServices::service(Arc::new(answers))
}

/// `multiCall`'s payloads run by `DELEGATECALL` of the Oracle into itself, on the Oracle's
/// storage: both slots its two `getSlot`s read, one of them empty, proved zero, are in the
/// witness, and a replay with no service pushes what the building node's service answered, the
/// persisted value (`SATIN-ORCL-002`, `SATIN-ORCL-012`). A batch whose second payload fails
/// reverts, and keeps the read of its first.
#[test]
fn witness_replay_of_oracle_reads_through_multicall() {
    let parent = chain(L1::Present);
    let batch = multi_call(&[get_slot(ORACLE_SLOT), get_slot(6)]);
    let built =
        produce_with(&parent, persisted(), &[legacy(SENDER, 0, TxKind::Call(ORACLE), 0, batch)]);
    assert!(built.output.receipts[0].status);
    assert_eq!(built.witness.slot(&ORACLE, &U256::from(6)), Some(U256::ZERO));
    let read = assert_exact_replay(&built);
    for slot in [ORACLE_SLOT, 6] {
        assert!(read.contains(&Evidence::Slot(ORACLE, U256::from(slot))), "{slot}");
    }
    let failing = multi_call(&[get_slot(ORACLE_SLOT), set_slot(ORACLE_SLOT, 1)]);
    let built =
        produce_with(&parent, persisted(), &[legacy(SENDER, 0, TxKind::Call(ORACLE), 0, failing)]);
    assert!(!built.output.receipts[0].status, "NotSystemAddress(): the batch reverts");
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Slot(ORACLE, U256::from(ORACLE_SLOT))));
}

/// A Mega System Transaction's `multiCall([setSlot(5, 8), getSlot(5)])` reads its own write, and a
/// later user's read of slot 5 is answered the value it persisted: the replay, with no service,
/// pushes the same values, the slot's value before the block required.
#[test]
fn witness_replay_of_an_oracle_read_of_its_own_write() {
    let parent = chain(L1::Present);
    let own = multi_call(&[set_slot(ORACLE_SLOT, 8), get_slot(ORACLE_SLOT)]);
    let system = legacy(SYSTEM, 0, TxKind::Call(ORACLE), 0, own);
    let user = legacy(SENDER, 0, TxKind::Call(ORACLE), 0, get_slot(ORACLE_SLOT));
    let answers = OracleAnswers::from_iter([(U256::from(ORACLE_SLOT), U256::from(8))]);
    let built = produce_with(&parent, OracleServices::service(Arc::new(answers)), &[system, user]);
    assert!(built.output.receipts.iter().all(|r| r.status));
    assert!(built.ledgers[0].is_system_transaction);
    assert_eq!(built.ledgers[1].result.output[..], word(8));
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Slot(ORACLE, U256::from(ORACLE_SLOT))));
}

/// A contract that runs the Oracle's `getSlot` by `DELEGATECALL` reads its own storage, not the
/// Oracle's, and asks no service (`SATIN-ORCL-002`): its slot is in the witness and the Oracle's is
/// not, and the replay pushes the same value.
#[test]
fn witness_replay_of_an_oracle_read_on_a_delegating_contracts_storage() {
    let delegator = Address::with_last_byte(0xd5);
    let mut parent = chain(L1::Present);
    let code =
        [get_slot_in_memory(ORACLE_SLOT), asm::delegatecall(ORACLE, None, 36), vec![0x00]].concat();
    parent.insert_account_info(&delegator, contract(code));
    parent.insert_account_storage(&delegator, &U256::from(ORACLE_SLOT), &U256::from(77));
    let answers = OracleAnswers::from_iter([(U256::from(ORACLE_SLOT), U256::from(42))]);
    let built = produce_with(
        &parent,
        OracleServices::service(Arc::new(answers)),
        &[call(SENDER, 0, delegator)],
    );
    assert!(built.output.receipts[0].status);
    assert_eq!(built.witness.slot(&delegator, &U256::from(ORACLE_SLOT)), Some(U256::from(77)));
    assert_eq!(built.witness.slot(&ORACLE, &U256::from(ORACLE_SLOT)), None);
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Slot(delegator, U256::from(ORACLE_SLOT))));
}

// ---------------------------------------------------------------------------------------------
// The pre-block steps

/// The EIP-7997 factory's runtime code, which the block in which Satin activates requires.
const CREATE2_FACTORY_CODE: &[u8] = &alloy_primitives::hex!(
    "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe03601600081602082378035828234f58015156039578182fd5b8082525050506014600cf3"
);

/// A factory on the tests' chain with Satin at `satin`.
fn satin_at(satin: u64) -> SatinFactory {
    let bootstrap = BootstrapInput {
        initial_sequencer: Some(Address::repeat_byte(0x5e)),
        initial_admin: Some(Address::repeat_byte(0xad)),
        min_rotation_delay: Some(1),
    };
    let schedule = ForkSchedule::satin_from(0).with(Fork::Satin, Some(satin));
    let input =
        ChainConfigInput { chain_id: common::CHAIN_ID, genesis_timestamp: 0, schedule, bootstrap };
    SatinFactory::new(ChainConfig::new(input).expect("a valid configuration"))
}

/// The block in which Satin activates: the factory check reads the EIP-7997 factory's account, the
/// deployments install the system contracts, and only deposits run. The replay reads the factory's
/// account from the witness, required.
#[test]
fn witness_replay_of_the_activation_block() {
    let mut parent = chain(L1::Present);
    parent.insert_account_info(
        &CREATE2_FACTORY,
        AccountInfo::default()
            .with_code(Bytecode::new_legacy(Bytes::from_static(CREATE2_FACTORY_CODE))),
    );
    let factory = satin_at(input().header.timestamp);
    let built = produce_block(
        factory,
        input(),
        &parent,
        Db::new(parent.clone()),
        OracleServices::default(),
        &included(&[deposit(NOOP), deposit(READER)]),
    );
    assert!(
        built
            .output
            .pre_block_states
            .iter()
            .any(|s| s.account_changes_for(&CREATE2_FACTORY).is_some())
    );
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Account(CREATE2_FACTORY)));
    // The check compares the factory's code hash, which its account carries: the replay reads no
    // code, and the witness holds the code all the same (`SATIN-WIT-001`).
    let hash = keccak256(CREATE2_FACTORY_CODE);
    assert!(!read.contains(&Evidence::Code(hash)));
    assert!(built.witness.code(&hash).is_some());
}

/// Block zero makes neither the EIP-2935 nor the EIP-4788 call: no state names either contract,
/// the witness holds neither, and the replay, which skips them too, needs neither.
#[test]
fn witness_replay_of_block_zero() {
    let mut parent = chain(L1::Present);
    for target in [HISTORY_STORAGE, BEACON_ROOTS] {
        parent.insert_account_info(&target, contract(vec![0x5f, 0x5f, 0xfd]));
    }
    let zero = BlockInput {
        header: BlockHeader {
            number: 0,
            timestamp: 0,
            gas_limit: 30_000_000,
            parent_hash: B256::ZERO,
            parent_beacon_block_root: Some(B256::ZERO),
            ..BlockHeader::default()
        },
        parent: ParentContext::BlockZero,
    };
    let built = produce_block(
        common::factory(),
        zero,
        &parent,
        Db::new(parent.clone()),
        OracleServices::default(),
        &included(&[deposit(NOOP), call(SENDER, 0, READER)]),
    );
    for target in [HISTORY_STORAGE, BEACON_ROOTS] {
        assert_eq!(built.witness.account(&target), None, "{target}");
    }
    assert_exact_replay(&built);
}

/// A registry change due in the block: the due-change read loads the registry's pending slots,
/// `applyPendingChanges()` its own, and the resolution reads the new system address; not due, the
/// call is not made. Both replay exactly, every slot read required.
#[test]
fn witness_replay_of_a_due_registry_change() {
    let pending = Address::with_last_byte(0x77);
    for activation in [BLOCK, BLOCK + 1] {
        let mut parent = chain(L1::Present);
        parent.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(7), &address_word(pending));
        parent.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(8), &U256::from(activation));
        let built = produce(&parent, &included(&[deposit(NOOP), call(SENDER, 0, READER)]));
        let due = activation == BLOCK;
        assert_eq!(built.output.system_address, Some(if due { pending } else { SYSTEM }));
        let read = assert_exact_replay(&built);
        for slot in [0, 7, 8] {
            assert!(read.contains(&Evidence::Slot(SEQUENCER_REGISTRY, U256::from(slot))), "{slot}");
        }
    }
}

/// A registry deployment over an account without code seeds the registry and wipes the account's
/// earlier storage, and over an absent address seeds it: the witness holds the account as the
/// chain held it, a leaf or an absence, and both blocks replay exactly.
#[test]
fn witness_replay_of_a_registry_seeded_over_a_wiped_or_absent_account() {
    for leaf in [true, false] {
        let mut parent = chain_with(L1::Present, false);
        if leaf {
            parent.insert_account_info(&SEQUENCER_REGISTRY, AccountInfo::default().with_nonce(1));
            parent.insert_account_storage(&SEQUENCER_REGISTRY, &U256::from(3), &U256::from(5));
        }
        let built = produce(&parent, &included(&[deposit(NOOP), call(SENDER, 0, READER)]));
        assert_eq!(built.witness.account(&SEQUENCER_REGISTRY).map(|a| a.is_some()), Some(leaf));
        let read = assert_exact_replay(&built);
        assert!(read.contains(&Evidence::Account(SEQUENCER_REGISTRY)));
    }
}

// ---------------------------------------------------------------------------------------------
// Access lists and the reads gas or a limit skips

/// A type-1 transaction from `from` with nonce `nonce` calling `to`, its access list `list`.
fn call_2930(from: Address, nonce: u64, to: Address, list: Vec<(Address, Vec<u64>)>) -> Tx {
    let access_list = AccessList(
        list.into_iter()
            .map(|(address, keys)| AccessListItem {
                address,
                storage_keys: keys.into_iter().map(|k| B256::from(U256::from(k))).collect(),
            })
            .collect(),
    );
    let tx = TxEip2930 {
        chain_id: common::CHAIN_ID,
        nonce,
        gas_limit: 2_000_000,
        to: TxKind::Call(to),
        access_list,
        ..Default::default()
    };
    let signature = alloy_primitives::Signature::new(U256::from(1), U256::from(1), false);
    let envelope = OpTxEnvelope::from_signed(tx.into_signed(signature));
    let encoded = envelope.enveloped_tx.clone().unwrap().to_vec();
    Tx { tx: alloy_consensus::transaction::Recovered::new_unchecked(envelope, from), encoded }
}

/// A type-1 transaction replays exactly: what its frames read is required, and its access list
/// warms the accounts and slots it names without loading them, so an entry no frame reads is in no
/// state and no witness.
#[test]
fn witness_replay_of_an_access_list_transaction() {
    let parent = chain(L1::Present);
    let tx = call_2930(SENDER, 0, READER, vec![(READER, vec![1]), (HOLDER, vec![7])]);
    let built = produce(&parent, &included(&[tx]));
    assert!(built.output.receipts[0].status);
    assert_eq!(built.witness.slot(&HOLDER, &U256::from(7)), None, "warmed, not loaded");
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Slot(READER, U256::from(1))));
}

/// Reads gas skips load nothing and are not in the witness (`SATIN-WIT-012`): a cold `SLOAD` and a
/// cold `BALANCE` with less gas than their cold surcharge, and `BLOCKHASH` of the current block, a
/// future one and one outside the window. The replay skips them the same way.
#[test]
fn witness_replay_skips_the_reads_gas_and_the_window_skip() {
    let sloader = Address::with_last_byte(0xd7);
    let short = Address::with_last_byte(0xd6);
    let hashes = Address::with_last_byte(0xd9);
    let mut parent = chain(L1::Present);
    parent.insert_account_info(&sloader, contract([asm::sload(1), vec![0x00]].concat()));
    parent.insert_account_storage(&sloader, &U256::from(1), &U256::from(5));
    let calls = [asm::call(sloader, Some(2_000), 0), asm::call(PROBE, Some(2_000), 0), vec![0x00]];
    parent.insert_account_info(&short, contract(calls.concat()));
    let blockhashes = [
        asm::store_blockhash(BLOCK),
        asm::store_blockhash(BLOCK + 5),
        asm::store_blockhash(BLOCK - 300),
        vec![0x00],
    ];
    parent.insert_account_info(&hashes, contract(blockhashes.concat()));
    // The control: with all the gas, the same calls load both.
    let full = [asm::call(sloader, None, 0), asm::call(PROBE, None, 0), vec![0x00]];
    let control = Address::with_last_byte(0xd8);
    parent.insert_account_info(&control, contract(full.concat()));
    let loaded = produce(&parent, &included(&[call(SENDER, 0, control)]));
    assert_eq!(loaded.witness.slot(&sloader, &U256::from(1)), Some(U256::from(5)));
    assert!(loaded.witness.account(&HOLDER).is_some());
    let built = produce(&parent, &included(&[call(SENDER, 0, short), call(SENDER, 1, hashes)]));
    assert!(built.output.receipts.iter().all(|r| r.status));
    let state = &built.output.tx_states[0];
    let sloaded = state.account_changes_for(&sloader).unwrap();
    assert!(!sloaded.storage.slots.contains_key(&U256::from(1)), "the cold SLOAD was skipped");
    assert!(state.account_changes_for(&HOLDER).is_none(), "the cold BALANCE was skipped");
    assert_eq!(built.witness.slot(&sloader, &U256::from(1)), None);
    assert_eq!(built.witness.account(&HOLDER), None);
    assert!(built.output.block_hash_reads.is_empty());
    assert!(built.witness.block_hashes().is_empty());
    assert_exact_replay(&built);
}

/// `disableVolatileDataAccess()`, then a call of `child` with `gas`.
fn access_off_then_call(child: Address, gas: u64) -> Vec<u8> {
    let mut word = [0u8; 32];
    word[..4].copy_from_slice(&keccak256("disableVolatileDataAccess()")[..4]);
    let access_control =
        Address::new([0x63, 0x42, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x04]);
    [
        asm::mstore32(0, word),
        asm::call(access_control, Some(100_000), 4),
        asm::call(child, Some(gas), 0),
        vec![0x00],
    ]
    .concat()
}

/// In a frame whose volatile-data access is off, an Oracle read is refused before it loads
/// (`SATIN-MAC-003`), so the slot is in no state and no witness; and a call whose code source
/// delegates to the block beneficiary is refused after its completed read of the source's account
/// and code, which stays in the witness and is required (`SATIN-WIT-012`, `SATIN-MAC-007`).
#[test]
fn witness_replay_of_reads_refused_with_volatile_access_off() {
    let (caller, reader, refuser, source) = (
        Address::with_last_byte(0xda),
        Address::with_last_byte(0xdb),
        Address::with_last_byte(0xdc),
        Address::with_last_byte(0xdd),
    );
    let mut parent = chain(L1::Present);
    parent.insert_account_info(&caller, contract(access_off_then_call(reader, 200_000)));
    let read_oracle =
        [get_slot_in_memory(ORACLE_SLOT), asm::call(ORACLE, None, 36), vec![0x00]].concat();
    parent.insert_account_info(&reader, contract(read_oracle));
    let other_caller = Address::with_last_byte(0xde);
    parent.insert_account_info(&other_caller, contract(access_off_then_call(refuser, 1_000)));
    // CALLCODE(GAS, source, 0, 0, 1 MiB, 0, 0).
    let mut callcode = vec![0x5f, 0x5f, 0x62, 0x10, 0x00, 0x00, 0x5f, 0x5f, 0x73];
    callcode.extend_from_slice(source.as_slice());
    callcode.extend_from_slice(&[0x5a, 0xf2, 0x00]);
    parent.insert_account_info(&refuser, contract(callcode));
    let delegation = Bytecode::new_eip7702(BENEFICIARY);
    parent.insert_account_info(
        &source,
        AccountInfo::default().with_nonce(1).with_code(delegation.clone()),
    );
    // The control: with its access on, the same read loads the slot.
    let control = produce_paid(&parent, &[call(SENDER, 0, reader)]);
    let loaded = Some(U256::from(ORACLE_HELD));
    assert_eq!(control.witness.slot(&ORACLE, &U256::from(ORACLE_SLOT)), loaded);
    let built = produce_paid(&parent, &[call(SENDER, 0, caller), call(SENDER, 1, other_caller)]);
    assert!(built.output.receipts.iter().all(|r| r.status));
    assert_eq!(built.witness.slot(&ORACLE, &U256::from(ORACLE_SLOT)), None, "refused, not loaded");
    let state = &built.output.tx_states[1];
    assert!(state.account_changes_for(&source).is_some(), "the source was read");
    let read = assert_exact_replay(&built);
    assert!(read.contains(&Evidence::Account(source)));
    assert!(read.contains(&Evidence::Code(delegation.hash_slow())));
}

/// A Mega System Transaction's state charge — a fresh Oracle slot — prices at `m = 1` and reads no
/// bucket (`SATIN-WIT-012`, `SATIN-WIT-013`): the block exports none, and the replay reads none.
#[test]
fn witness_replay_of_a_system_transactions_state_charge_reads_no_bucket() {
    let parent = chain(L1::Present);
    // The control: a user's fresh slot is priced against its bucket.
    assert!(
        !produce(&parent, &included(&[call(SENDER, 0, HASHER)])).output.bucket_reads.is_empty()
    );
    let built = produce(&parent, &included(&[deposit(NOOP), system_tx(0, 9, 1)]));
    assert!(built.output.receipts.iter().all(|r| r.status));
    assert!(built.output.bucket_reads.is_empty());
    let read = assert_exact_replay(&built);
    assert!(!read.iter().any(|e| matches!(e, Evidence::Capacity(_))));
}
