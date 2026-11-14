//! The replay helpers of `next/diff/src/replay.rs` the block timing uses, copied verbatim (only
//! the items not needed here are left out), plus [`load_corpus`]. They are copied because that
//! crate enables the engine's `compare` and `salt-test-hasher` features, and depending on it
//! would put both into every variant here.

use crate::blockdb::{self, BlockDb, EMPTY_CODE_HASH, Header, addr, b256, bytes, u64v, u256};
use alloy_consensus::transaction::Recovered;
use alloy_eips::Decodable2718;
use alloy_primitives::{Address, B256, Bytes, TxKind};
use evm2::{
    bytecode::Bytecode,
    evm::{AccountInfo, Database},
    interpreter::Word,
};
use mega_evm2::{
    SaltCapacities, SaltSnapshot, SnapshotId,
    chain::{BootstrapInput, ChainConfig},
    factory::{BlockHeader, BlockInput, BlockServices, ParentContext, SatinFactory},
};
use op_evm2::{OpTxEnvelope, TxDeposit};
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap, convert::Infallible, rc::Rc, sync::Arc};

/// The block state as an evm2 `Database`. Bytecode is kept per hash, analysed once (as a node's
/// code cache keeps it), so neither the jump-table analysis nor the hashing is in a run.
pub struct Evm2Db {
    pub db: Rc<RefCell<BlockDb>>,
    pub code_cache: Rc<RefCell<HashMap<B256, Bytecode>>>,
}

impl core::fmt::Debug for Evm2Db {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("Evm2Db")
    }
}

impl Evm2Db {
    fn bytecode(&self, h: B256, code: Bytes) -> Bytecode {
        self.code_cache
            .borrow_mut()
            .entry(h)
            .or_insert_with(|| {
                let bc = Bytecode::new_raw(code);
                let _ = bc.legacy_jump_table();
                bc
            })
            .clone()
    }
}

impl Database for Evm2Db {
    type Error = Infallible;
    fn get_account(&mut self, address: &Address) -> Result<Option<AccountInfo>, Infallible> {
        let a = self.db.borrow_mut().account(*address);
        Ok(a.map(|(balance, nonce, code_hash, code)| {
            let mut info = AccountInfo::default().with_balance(balance);
            info.nonce = nonce;
            if code_hash != EMPTY_CODE_HASH {
                info.code = Some(self.bytecode(code_hash, code));
                info.code_hash = code_hash;
            }
            info
        }))
    }
    fn get_code_by_hash(&mut self, code_hash: &B256) -> Result<Bytecode, Infallible> {
        let code = self.db.borrow_mut().code(*code_hash);
        Ok(self.bytecode(*code_hash, code))
    }
    fn get_storage(&mut self, address: &Address, key: &Word) -> Result<Word, Infallible> {
        Ok(self.db.borrow_mut().storage(*address, *key))
    }
    fn get_block_hash(&mut self, number: &Word) -> Result<B256, Infallible> {
        Ok(self.db.borrow_mut().block_hash(number.to::<u64>()))
    }
}

/// The chain the gates execute the recorded blocks on: Satin and every fork below it active from
/// genesis (so every recorded block is a Satin block in which nothing activates), the recorded
/// block's chain id, and placeholder registry bootstrap values (no recorded block deploys the
/// registry).
pub fn gate_chain(chain_id: u64) -> ChainConfig {
    let bootstrap = BootstrapInput {
        initial_sequencer: Some(Address::repeat_byte(0x5e)),
        initial_admin: Some(Address::repeat_byte(0xad)),
        min_rotation_delay: Some(1),
    };
    ChainConfig::satin_from_genesis(chain_id, bootstrap).expect("the gate chain is valid")
}

/// The engine's factory on [`gate_chain`].
pub fn factory(chain_id: u64) -> SatinFactory {
    SatinFactory::new(gate_chain(chain_id))
}

/// The SALT snapshot the gates execute the recorded block `h` on: the recording carries no
/// capacity, so every bucket is at the minimum, the test policy `Unlisted::TestMinimum` (as the
/// reference's replay prices it). It is of `h`'s parent, with a zero state root.
pub fn minimum_salt(h: &Header) -> Arc<dyn SaltSnapshot> {
    let id = SnapshotId { parent_hash: h.parent_hash, state_root: B256::ZERO };
    Arc::new(SaltCapacities::test_minimum(id))
}

/// The services of the recorded block `h`: its [`minimum_salt`], an empty environment, no
/// inspector.
pub fn services(h: &Header) -> BlockServices<'static> {
    BlockServices::new(minimum_salt(h))
}

/// The recorded block's input: its header and its parent. On [`gate_chain`], where every fork is
/// active from genesis, no parent timestamp changes what the block's activation is, so the second
/// before the block is used (the genesis timestamp, zero, for block 1's parent; block zero has
/// none), not the recording's `parent_timestamp` (`blockdb::Block::parent_timestamp`).
pub fn block_input(h: &Header) -> BlockInput {
    BlockInput {
        header: BlockHeader {
            number: h.number,
            timestamp: h.timestamp,
            beneficiary: h.beneficiary,
            gas_limit: h.gas_limit,
            base_fee: h.base_fee,
            difficulty: h.difficulty,
            prevrandao: h.mix_hash,
            excess_blob_gas: h.excess_blob_gas,
            parent_hash: h.parent_hash,
            parent_beacon_block_root: h.parent_beacon_block_root,
        },
        parent: match h.number {
            0 => ParentContext::BlockZero,
            1 => ParentContext::Parent { number: 0, hash: h.parent_hash, timestamp: 0 },
            n => ParentContext::Parent {
                number: n - 1,
                hash: h.parent_hash,
                timestamp: h.timestamp.saturating_sub(1),
            },
        },
    }
}

/// The transaction the Satin side recorded: its 2718 bytes for every non-deposit (an Ethereum
/// envelope, with the bytes kept for the L1 fee), the RPC fields for a deposit.
pub fn decode(raw: &Bytes, t: &Value) -> Result<Recovered<OpTxEnvelope>, String> {
    let sender = addr(&t["from"]);
    let ty = u64v(&t["type"]);
    let env = if ty == 0x7e {
        OpTxEnvelope::deposit(TxDeposit {
            source_hash: b256(&t["sourceHash"]),
            from: sender,
            to: match t.get("to") {
                Some(to) if !to.is_null() => TxKind::Call(addr(to)),
                _ => TxKind::Create,
            },
            mint: u256(&t["mint"]).to(),
            value: u256(&t["value"]),
            gas_limit: u64v(&t["gas"]),
            is_system_transaction: t.get("isSystemTx").and_then(Value::as_bool).unwrap_or(false),
            input: bytes(&t["input"]),
        })
    } else {
        let e = alloy_consensus::EthereumTxEnvelope::<alloy_consensus::TxEip4844>::decode_2718(
            &mut raw.as_ref(),
        )
        .map_err(|e| format!("decode: {e}"))?;
        OpTxEnvelope::try_from(e).map_err(|e| format!("{e}"))?
    };
    Ok(Recovered::new_unchecked(env, sender))
}

/// A block's decoded transactions, each with its exact bytes.
pub type Txs = Vec<(Recovered<OpTxEnvelope>, Bytes)>;

/// Every block of a run with its decoded transactions.
pub type Corpus = Vec<(blockdb::Block, Txs)>;

/// Reads the corpus a run selects (`MEGA_EVM2_BLOCK_DIR`, `MEGA_EVM2_REPLAY_REF`) and decodes
/// every transaction once, outside any measurement.
pub fn load_corpus() -> Corpus {
    blockdb::block_list()
        .into_iter()
        .map(|(set, path)| {
            let block = blockdb::load_block(&path, &set);
            let raws = crate::bench_blocks::raw_transactions(&block);
            let txs = block
                .txs
                .iter()
                .zip(&raws)
                .map(|(tx, raw)| (decode(raw, tx).expect("decode"), raw.clone()))
                .collect();
            (block, txs)
        })
        .collect()
}
