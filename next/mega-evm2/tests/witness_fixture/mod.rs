//! The fixture of the witness tests: a chain before block [`BLOCK`], the transactions the tests
//! run on it, a building node that executes a block and derives its witness from its states and
//! exports (`mega_evm2::witness`), and a validator that replays the block on exactly that witness,
//! recording every piece of evidence it reads.
#![allow(dead_code)]

#[path = "../common/mod.rs"]
pub mod common;

use alloy_consensus::{
    SignableTransaction, TxEip1559, TxEip7702, TxLegacy, transaction::Recovered,
};
use alloy_eips::eip7702::{Authorization, SignedAuthorization};
use alloy_primitives::{Address, B256, Bytes, Signature, TxKind, U256, keccak256};
use evm2::{
    DatabaseError,
    bytecode::Bytecode,
    ethereum::LazyTxEip7702,
    evm::{AccountInfo, Database, Db, DynDatabase, InMemoryDB, PendingState},
};
use mega_evm2::{
    BucketId, SEQUENCER_REGISTRY, SaltReadError, SaltSnapshot, SnapshotId,
    block::{BlockOutput, BlockSession, encode_receipt},
    error::{BlockError, CandidateError, Disposition},
    factory::{BlockInput, BlockServices},
    oracle::OracleServices,
    system_contracts::{ORACLE_CODE, SEQUENCER_REGISTRY_CODE, address_word},
    tx::ORACLE,
    witness::{Evidence, MissingEvidence, Witness, WitnessDb, WitnessKeys},
};
use op_evm2::{OpTxEnvelope, TxDeposit, constants::L1_BLOCK_CONTRACT};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    rc::Rc,
    sync::{Arc, Mutex},
};

/// The block the tests execute.
pub const BLOCK: u64 = 1_000;
/// Funded senders of user transactions, and a depositor.
pub const SENDER: Address = Address::with_last_byte(0x50);
pub const OTHER: Address = Address::with_last_byte(0x51);
pub const DEPOSITOR: Address = Address::with_last_byte(0x52);
/// The live system address the registry names.
pub const SYSTEM: Address = Address::with_last_byte(0x5a);
/// Reads its slots 1 and 2 (2 is empty), calls [`REVERTER`] and [`HALTER`], stores
/// `BLOCKHASH(BLOCK - 1)` in its slot 9, and stops.
pub const READER: Address = Address::with_last_byte(0xa0);
/// Reads its slot 1, then reverts.
pub const REVERTER: Address = Address::with_last_byte(0xb0);
/// Reads its slot 1, then halts on an invalid opcode.
pub const HALTER: Address = Address::with_last_byte(0xb1);
/// Takes the balance of [`HOLDER`], which loads its account and not its code, and stops.
pub const PROBE: Address = Address::with_last_byte(0xc0);
/// A contract whose code no transaction runs.
pub const HOLDER: Address = Address::with_last_byte(0xc1);
/// Stops.
pub const NOOP: Address = Address::with_last_byte(0xe0);
/// Calls the Oracle's `getSlot(ORACLE_SLOT)` with 1,250 of gas, which reaches the `SLOAD` short of
/// its cold access: the read is made and runs out of gas (`SATIN-ORCL-004`).
pub const ORACLE_OOG: Address = Address::with_last_byte(0xd0);
/// Reads its slot 1, then `TIMESTAMP`, volatile data, then expands its memory past what gas
/// detention leaves it: the frame is stopped at detention's crossing.
pub const STOPPED: Address = Address::with_last_byte(0xd2);
/// Stores `BLOCKHASH(900)` in its slot 900, a new one.
pub const HASHER: Address = Address::with_last_byte(0xd1);
/// The Oracle slot the tests read, and what the chain holds in it.
pub const ORACLE_SLOT: u64 = 5;
pub const ORACLE_HELD: u64 = 3;

/// The L1 block contract's fixture slots: base fee 1,000, both Ecotone scalars 1 (or both empty),
/// blob base fee 10, and slot 8 holding a footprint scalar of 1 and no operator fee.
pub fn l1_slots(empty_scalars: bool) -> [(u64, U256); 5] {
    let scalars =
        if empty_scalars { U256::ZERO } else { (U256::from(1) << 96) | (U256::from(1) << 64) };
    [
        (1, U256::from(1_000)),
        (3, scalars),
        (5, U256::from(500)),
        (7, U256::from(10)),
        (8, U256::from(1) << 96),
    ]
}

/// What the chain holds at the L1 block contract's address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum L1 {
    /// The contract, its Ecotone scalars set.
    Present,
    /// The contract, its Ecotone scalars empty: the fee computation reads the overhead, slot 5.
    EmptyScalars,
    /// No account.
    Absent,
}

pub mod asm {
    //! Small pieces of bytecode.
    use alloy_primitives::Address;

    pub fn push(v: u64) -> Vec<u8> {
        if v == 0 {
            return vec![0x5f];
        }
        let bytes = v.to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        let mut out = vec![0x5f + (8 - skip) as u8];
        out.extend_from_slice(&bytes[skip..]);
        out
    }

    pub fn push_address(a: Address) -> Vec<u8> {
        let mut out = vec![0x73];
        out.extend_from_slice(a.as_slice());
        out
    }

    /// `SLOAD(slot)`, its value dropped.
    pub fn sload(slot: u64) -> Vec<u8> {
        [push(slot), vec![0x54, 0x50]].concat()
    }

    /// `SSTORE(slot, value)`.
    pub fn sstore(slot: u64, value: u64) -> Vec<u8> {
        [push(value), push(slot), vec![0x55]].concat()
    }

    /// `SSTORE(number, BLOCKHASH(number))`.
    pub fn store_blockhash(number: u64) -> Vec<u8> {
        [push(number), vec![0x40], push(number), vec![0x55]].concat()
    }

    /// `CALL(gas or GAS, to, 0, 0, args, 0, 0)`, its success dropped.
    pub fn call(to: Address, gas: Option<u64>, args: u64) -> Vec<u8> {
        let mut out = vec![0x5f, 0x5f];
        out.extend(push(args));
        out.extend([0x5f, 0x5f]);
        out.extend(push_address(to));
        match gas {
            Some(gas) => out.extend(push(gas)),
            None => out.push(0x5a),
        }
        out.extend([0xf1, 0x50]);
        out
    }

    /// `BALANCE(a)`, dropped.
    pub fn balance(a: Address) -> Vec<u8> {
        [push_address(a), vec![0x31, 0x50]].concat()
    }

    /// `MSTORE(offset, word)` of a 32-byte word.
    pub fn mstore32(offset: u64, word: [u8; 32]) -> Vec<u8> {
        let mut out = vec![0x7f];
        out.extend_from_slice(&word);
        out.extend(push(offset));
        out.push(0x52);
        out
    }
}

pub fn contract(code: Vec<u8>) -> AccountInfo {
    AccountInfo::default().with_nonce(1).with_code(Bytecode::new_legacy(code.into()))
}

/// The Oracle's `getSlot(uint256)` calldata for `slot`, in memory at 0 (36 bytes).
fn get_slot_in_memory(slot: u64) -> Vec<u8> {
    let mut first = [0u8; 32];
    first[..4].copy_from_slice(&keccak256("getSlot(uint256)")[..4]);
    let mut second = [0u8; 32];
    second[..4].copy_from_slice(&(slot as u32).to_be_bytes());
    [asm::mstore32(0, first), asm::mstore32(32, second)].concat()
}

/// The chain's hash of block `number`.
pub fn hash_of(number: u64) -> B256 {
    keccak256(format!("block {number}"))
}

/// The chain before block [`BLOCK`].
pub fn chain(l1: L1) -> InMemoryDB {
    let mut db = InMemoryDB::default();
    let rich = U256::from(10).pow(U256::from(18));
    for sender in [SENDER, OTHER, DEPOSITOR] {
        db.insert_account_info(&sender, AccountInfo::default().with_balance(rich));
    }
    // The registry, version 2.0.0, naming the live system address.
    db.insert_account_info(
        &SEQUENCER_REGISTRY,
        AccountInfo::default().with_code(Bytecode::new_legacy(SEQUENCER_REGISTRY_CODE)),
    );
    db.insert_account_storage(&SEQUENCER_REGISTRY, &U256::ZERO, &address_word(SYSTEM));
    // The Oracle, already deployed, its slot holding a value.
    db.insert_account_info(
        &ORACLE,
        AccountInfo::default().with_code(Bytecode::new_legacy(ORACLE_CODE)),
    );
    db.insert_account_storage(&ORACLE, &U256::from(ORACLE_SLOT), &U256::from(ORACLE_HELD));
    match l1 {
        L1::Absent => {}
        L1::Present | L1::EmptyScalars => {
            db.insert_account_info(&L1_BLOCK_CONTRACT, AccountInfo::default().with_nonce(1));
            for (slot, value) in l1_slots(l1 == L1::EmptyScalars) {
                db.insert_account_storage(&L1_BLOCK_CONTRACT, &U256::from(slot), &value);
            }
        }
    }
    let reader = [
        asm::sload(1),
        asm::sload(2),
        asm::call(REVERTER, None, 0),
        asm::call(HALTER, Some(50_000), 0),
        asm::store_blockhash(BLOCK - 1),
        vec![0x00],
    ]
    .concat();
    db.insert_account_info(&READER, contract(reader));
    db.insert_account_storage(&READER, &U256::from(1), &U256::from(11));
    db.insert_account_info(&REVERTER, contract([asm::sload(1), vec![0x5f, 0x5f, 0xfd]].concat()));
    db.insert_account_storage(&REVERTER, &U256::from(1), &U256::from(21));
    db.insert_account_info(&HALTER, contract([asm::sload(1), vec![0xfe]].concat()));
    db.insert_account_storage(&HALTER, &U256::from(1), &U256::from(31));
    db.insert_account_info(&PROBE, contract([asm::balance(HOLDER), vec![0x00]].concat()));
    db.insert_account_info(&HOLDER, contract(vec![0x60, 0x2a, 0x00]));
    db.insert_account_info(&NOOP, contract(vec![0x00]));
    let oog =
        [get_slot_in_memory(ORACLE_SLOT), asm::call(ORACLE, Some(1_250), 36), vec![0x00]].concat();
    db.insert_account_info(&ORACLE_OOG, contract(oog));
    db.insert_account_info(&HASHER, contract([asm::store_blockhash(900), vec![0x00]].concat()));
    // MSTORE(3,300,000, 0) expands memory by about 21,300,000 gas, over the 20,000,000 cap.
    let stopped =
        [asm::sload(1), vec![0x42, 0x50], vec![0x5f], asm::push(3_300_000), vec![0x52, 0x00]]
            .concat();
    db.insert_account_info(&STOPPED, contract(stopped));
    db.insert_account_storage(&STOPPED, &U256::from(1), &U256::from(41));
    for number in BLOCK - 256..BLOCK {
        db.insert_block_hash(&U256::from(number), &hash_of(number));
    }
    db
}

/// Block [`BLOCK`]'s input: a parent one block and one second before it, hash zero.
pub fn input() -> BlockInput {
    common::block_input(BLOCK, 10_000 + BLOCK, 30_000_000, Some(B256::ZERO))
}

/// A transaction as a block takes it: the envelope and its 2718 bytes.
#[derive(Clone, Debug)]
pub struct Tx {
    pub tx: Recovered<OpTxEnvelope>,
    pub encoded: Vec<u8>,
}

fn signed(envelope: OpTxEnvelope, from: Address) -> Tx {
    let encoded = envelope.enveloped_tx.clone().expect("a signed envelope has its bytes").to_vec();
    Tx { tx: Recovered::new_unchecked(envelope, from), encoded }
}

fn signature() -> Signature {
    Signature::new(U256::from(1), U256::from(1), false)
}

/// A legacy transaction from `from` with nonce `nonce` of kind `to`, moving `value`, with `input`.
pub fn legacy(from: Address, nonce: u64, to: TxKind, value: u64, input: Vec<u8>) -> Tx {
    legacy_with_gas(from, nonce, to, value, input, 2_000_000)
}

/// [`legacy`] with gas limit `gas_limit`.
pub fn legacy_with_gas(
    from: Address,
    nonce: u64,
    to: TxKind,
    value: u64,
    input: Vec<u8>,
    gas_limit: u64,
) -> Tx {
    let tx = TxLegacy {
        chain_id: Some(common::CHAIN_ID),
        nonce,
        gas_price: 0,
        gas_limit,
        to,
        value: U256::from(value),
        input: input.into(),
    };
    signed(OpTxEnvelope::from_signed(tx.into_signed(signature())), from)
}

/// A call from `from` with nonce `nonce` to `to`.
pub fn call(from: Address, nonce: u64, to: Address) -> Tx {
    legacy(from, nonce, TxKind::Call(to), 0, Vec::new())
}

/// An EIP-1559 call from `from` with nonce `nonce` to `to`.
pub fn call_1559(from: Address, nonce: u64, to: Address) -> Tx {
    let tx = TxEip1559 {
        chain_id: common::CHAIN_ID,
        nonce,
        gas_limit: 2_000_000,
        to: TxKind::Call(to),
        ..Default::default()
    };
    signed(OpTxEnvelope::from_signed(tx.into_signed(signature())), from)
}

/// A deposit from [`DEPOSITOR`] calling `to`.
pub fn deposit(to: Address) -> Tx {
    let tx = TxDeposit {
        source_hash: B256::with_last_byte(0x42),
        from: DEPOSITOR,
        to: TxKind::Call(to),
        mint: 0,
        value: U256::ZERO,
        gas_limit: 2_000_000,
        is_system_transaction: false,
        input: Bytes::new(),
    };
    Tx { tx: Recovered::new_unchecked(OpTxEnvelope::deposit(tx), DEPOSITOR), encoded: Vec::new() }
}

/// A Mega System Transaction with nonce `nonce`: `setSlot(slot, value)` on the Oracle, from the
/// live system address. It pays no fee.
pub fn system_tx(nonce: u64, slot: u64, value: u64) -> Tx {
    let mut input = keccak256("setSlot(uint256,bytes32)")[..4].to_vec();
    input.extend_from_slice(&U256::from(slot).to_be_bytes::<32>());
    input.extend_from_slice(&U256::from(value).to_be_bytes::<32>());
    legacy(SYSTEM, nonce, TxKind::Call(ORACLE), 0, input)
}

/// Init code that writes 1 into the new contract's slot 0 and deploys code reading its slots 0
/// and 1.
pub fn creation_code() -> Vec<u8> {
    let runtime = [asm::sload(0), asm::sload(1), vec![0x00]].concat();
    let len = runtime.len() as u8;
    // SSTORE(0, 1) (4 bytes); CODECOPY(0, 14, len) (6 bytes); RETURN(0, len) (4 bytes)
    let mut code = asm::sstore(0, 1);
    code.extend([0x60, len, 0x60, 14, 0x5f, 0x39, 0x60, len, 0x5f, 0xf3]);
    assert_eq!(code.len(), 14);
    code.extend(runtime);
    code
}

/// What a step does with its transaction.
#[derive(Clone, Debug)]
pub enum Step {
    /// Executed and committed: in the block.
    Include(Tx),
    /// Executed and dropped: not in the block.
    Drop(Tx),
}

/// A block a building node executed: its output, the witness derived from it, and the
/// transactions it includes, in order.
pub struct Produced {
    pub output: BlockOutput,
    pub keys: WitnessKeys,
    pub witness: Witness,
    pub included: Vec<Tx>,
}

/// The SALT snapshot the building node reads: every bucket at the minimum capacity.
pub fn salt() -> Arc<dyn SaltSnapshot> {
    common::minimum_salt(B256::ZERO)
}

/// Executes `steps` in block [`BLOCK`] over `database`, with `oracle`, and returns the block and
/// its witness, resolved against `parent`.
pub fn produce_over<'a>(
    parent: &InMemoryDB,
    database: impl DynDatabase + 'a,
    oracle: OracleServices,
    steps: &[Step],
) -> Produced {
    let services = BlockServices { oracle, ..BlockServices::new(salt()) };
    let mut block =
        common::factory().start_block(&input(), database, services).expect("the block starts");
    let mut included = Vec::new();
    for step in steps {
        match step {
            Step::Include(tx) => {
                let candidate = block.execute_candidate(&tx.tx, &tx.encoded).expect("it runs");
                block.commit_candidate(candidate).expect("it commits");
                included.push(tx.clone());
            }
            Step::Drop(tx) => {
                drop(block.execute_candidate(&tx.tx, &tx.encoded).expect("it runs"));
            }
        }
    }
    let output = block.finish().expect("the block finishes");
    let keys = WitnessKeys::of(&output);
    let witness =
        Witness::resolve(&keys, &mut parent.clone(), salt().as_ref()).expect("the parent holds it");
    Produced { output, keys, witness, included }
}

/// [`produce_over`] on a cold copy of `parent`, with no Oracle service.
pub fn produce(parent: &InMemoryDB, steps: &[Step]) -> Produced {
    produce_over(parent, Db::new(parent.clone()), OracleServices::default(), steps)
}

/// Every step included.
pub fn included(txs: &[Tx]) -> Vec<Step> {
    txs.iter().cloned().map(Step::Include).collect()
}

/// The evidence a replay read.
pub type Read = Arc<Mutex<BTreeSet<Evidence>>>;

/// A [`WitnessDb`] recording each piece of evidence a read was answered from.
#[derive(Debug)]
struct Recording {
    inner: WitnessDb,
    read: Read,
}

impl Database for Recording {
    type Error = MissingEvidence;

    fn is_fatal(e: &Self::Error) -> bool {
        WitnessDb::is_fatal(e)
    }

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        let info = self.inner.get_account(address)?;
        self.read.lock().unwrap().insert(Evidence::Account(*address));
        Ok(info)
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        let code = self.inner.get_code_by_hash(code_hash)?;
        if self.inner.witness().code(code_hash).is_some() {
            self.read.lock().unwrap().insert(Evidence::Code(*code_hash));
        }
        Ok(code)
    }

    fn get_storage(&mut self, address: &Address, key: &U256) -> Result<U256, Self::Error> {
        let value = self.inner.get_storage(address, key)?;
        if self.inner.witness().slot(address, key).is_some() {
            self.read.lock().unwrap().insert(Evidence::Slot(*address, *key));
        }
        Ok(value)
    }

    fn get_block_hash(&mut self, number: &U256) -> Result<B256, Self::Error> {
        let hash = self.inner.get_block_hash(number)?;
        self.read.lock().unwrap().insert(Evidence::BlockHash(*number));
        Ok(hash)
    }
}

/// The witness as the replay's SALT snapshot, recording each capacity read.
struct RecordingSalt {
    witness: Arc<Witness>,
    read: Read,
}

impl SaltSnapshot for RecordingSalt {
    fn id(&self) -> SnapshotId {
        self.witness.id()
    }

    fn capacity(&self, bucket: BucketId) -> Result<u64, SaltReadError> {
        let capacity = self.witness.capacity(bucket)?;
        self.read.lock().unwrap().insert(Evidence::Capacity(bucket));
        Ok(capacity)
    }
}

/// A validator's replay of block [`BLOCK`] with `txs` on exactly `witness`, with no Oracle
/// service, and the evidence it read. A failure that decides the block is returned.
pub fn replay(
    witness: Witness,
    txs: &[Tx],
) -> (Result<BlockOutput, BlockError>, BTreeSet<Evidence>) {
    let witness = Arc::new(witness);
    let read = Read::default();
    let database = Recording { inner: WitnessDb::new(witness.clone()), read: read.clone() };
    let salt = Arc::new(RecordingSalt { witness, read: read.clone() });
    let result = (|| {
        let mut block =
            common::factory().start_block(&input(), Db::new(database), BlockServices::new(salt))?;
        for tx in txs {
            let run = block
                .execute_candidate(&tx.tx, &tx.encoded)
                .and_then(|candidate| block.commit_candidate(candidate).map(|_| ()));
            match run {
                Ok(()) => {}
                Err(CandidateError::Block(e)) => return Err(e),
                Err(other) => panic!("the replay refused a transaction of the block: {other:?}"),
            }
        }
        block.finish()
    })();
    let read = read.lock().unwrap().clone();
    (result, read)
}

/// A replay that must succeed.
pub fn replayed(witness: Witness, txs: &[Tx]) -> (BlockOutput, BTreeSet<Evidence>) {
    let (result, read) = replay(witness, txs);
    (result.unwrap_or_else(|e| panic!("the replay failed: {e:?}")), read)
}

/// The replay produced the same block (`SATIN-WIT-016`): receipts (status, cumulative gas, logs),
/// state changes, the pre-block and transaction states, gas ledgers and block counters, the live
/// system address; and exports that are a subset of the building node's.
pub fn assert_same_block(replayed: &BlockOutput, built: &BlockOutput) {
    let receipts = |o: &BlockOutput| o.receipts.iter().map(encode_receipt).collect::<Vec<_>>();
    assert_eq!(receipts(replayed), receipts(built), "receipts");
    assert_eq!(replayed.changes, built.changes, "state changes");
    assert_eq!(replayed.pre_block_states, built.pre_block_states, "pre-block states");
    assert_eq!(replayed.tx_states, built.tx_states, "transaction states");
    assert_eq!(format!("{:?}", replayed.limiter), format!("{:?}", built.limiter), "ledgers");
    assert_eq!(replayed.system_address, built.system_address);
    assert!(replayed.bucket_reads.is_subset(&built.bucket_reads), "buckets");
    assert!(
        replayed.block_hash_reads.iter().all(|(n, h)| built.block_hash_reads.get(n) == Some(h)),
        "block hashes"
    );
}

/// The evidence whose lack failed a replay: an internal error, never an invalid block.
pub fn missing(error: &BlockError) -> Evidence {
    assert_eq!(error.disposition(), Disposition::Internal, "{error:?}");
    MissingEvidence::of(error)
        .unwrap_or_else(|| panic!("not a missing piece of evidence: {error:?}"))
        .0
}

/// The L1 block contract's entry in `state`: its account as recorded and the slots, ascending.
pub fn l1_entry(state: &PendingState) -> Option<(Option<AccountInfo>, Vec<u64>)> {
    let l1 = state.account_changes_for(&L1_BLOCK_CONTRACT)?;
    let mut slots: Vec<u64> = l1.storage.slots.keys().map(|k| k.to::<u64>()).collect();
    slots.sort_unstable();
    Some((l1.current.cloned(), slots))
}

/// The block's started session over a cold copy of `parent`, with no Oracle service.
pub fn session(parent: &InMemoryDB) -> BlockSession<'static> {
    common::start_block(&input(), Db::new(parent.clone()))
}

/// An EIP-7702 transaction from `from` with nonce `nonce` to [`NOOP`] whose one entry delegates
/// `authority` (whose nonce is `authority_nonce`) to `delegate`.
pub fn delegation(
    from: Address,
    nonce: u64,
    authority: Address,
    authority_nonce: u64,
    delegate: Address,
) -> Tx {
    delegations(from, nonce, 2_000_000, &[(authority, authority_nonce, delegate)])
}

/// An EIP-7702 transaction from `from` with nonce `nonce` and gas limit `gas_limit` to [`NOOP`]
/// whose entries delegate each `(authority, its nonce, delegate)` in turn.
pub fn delegations(
    from: Address,
    nonce: u64,
    gas_limit: u64,
    entries: &[(Address, u64, Address)],
) -> Tx {
    let list = entries
        .iter()
        .map(|(_, authority_nonce, delegate)| {
            SignedAuthorization::new_unchecked(
                Authorization {
                    chain_id: U256::from(common::CHAIN_ID),
                    address: *delegate,
                    nonce: *authority_nonce,
                },
                0,
                U256::from(1),
                U256::from(1),
            )
        })
        .collect();
    let tx = TxEip7702 {
        chain_id: common::CHAIN_ID,
        nonce,
        gas_limit,
        to: NOOP,
        authorization_list: list,
        ..Default::default()
    };
    let authorities = entries.iter().map(|(authority, ..)| Some(*authority)).collect();
    let lazy = LazyTxEip7702::from_cached_recovered_authorizations(tx, authorities);
    let encoded = vec![0x04; 120];
    let envelope = OpTxEnvelope::new(lazy, Some(Bytes::from(encoded.clone())));
    Tx { tx: Recovered::new_unchecked(envelope, from), encoded }
}

/// A database error a [`Backend`] serves.
#[derive(Debug)]
pub struct Unavailable;

impl core::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("unavailable")
    }
}

impl std::error::Error for Unavailable {}

/// Every account read (`None`) and slot read (`Some(key)`) that reached a [`Backend`].
pub type Reached = Rc<RefCell<Vec<(Address, Option<U256>)>>>;

/// The chain as a node's database: it records every account and slot read that reaches it, and
/// fails the read of `fail`'s account (`None`) or slot with `fail`'s fatality.
#[derive(Debug)]
pub struct Backend {
    pub inner: InMemoryDB,
    pub reached: Reached,
    pub fail: Option<(Address, Option<U256>, bool)>,
}

impl Backend {
    pub fn new(inner: InMemoryDB) -> (Self, Reached) {
        let reached = Reached::default();
        (Self { inner, reached: reached.clone(), fail: None }, reached)
    }

    fn reach(&mut self, address: Address, slot: Option<U256>) -> Result<(), DatabaseError> {
        self.reached.borrow_mut().push((address, slot));
        match self.fail {
            Some((a, s, fatal)) if a == address && s == slot => {
                Err(DatabaseError::new(Unavailable, fatal))
            }
            _ => Ok(()),
        }
    }
}

impl Database for Backend {
    type Error = DatabaseError;

    fn is_fatal(e: &Self::Error) -> bool {
        e.is_fatal()
    }

    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.reach(*address, None)?;
        Database::get_account(&mut self.inner, address)
    }

    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Self::Error> {
        Database::get_code_by_hash(&mut self.inner, code_hash)
    }

    fn get_storage(&mut self, address: &Address, key: &U256) -> Result<U256, Self::Error> {
        self.reach(*address, Some(*key))?;
        Database::get_storage(&mut self.inner, address, key)
    }

    fn get_block_hash(&mut self, number: &U256) -> Result<B256, Self::Error> {
        Database::get_block_hash(&mut self.inner, number)
    }
}

/// The reads of the L1 block contract that reached a [`Backend`].
pub fn l1_reached(reached: &Reached) -> Vec<Option<u64>> {
    reached
        .borrow()
        .iter()
        .filter(|(a, _)| *a == L1_BLOCK_CONTRACT)
        .map(|(_, s)| s.map(|s| s.to::<u64>()))
        .collect()
}
