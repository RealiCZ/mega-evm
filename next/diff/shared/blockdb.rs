//! One recorded MegaETH block, as both replay harnesses read it (std + alloy_primitives +
//! serde_json only, included verbatim by the reference harness and the evm2 harness).
//!
//! The pre-state is the block cache's sparse prestate: the read set of the engine that recorded
//! it. A read outside it is a **miss**, decided as `mega-evme`'s `BlockState` does without an RPC
//! fallback (bin/mega-evme/src/block/state.rs @ 5e3f30ad): an account the prestate does not hold,
//! a slot its account does not record, a code, a block hash. A miss answers the default (no
//! account, zero, empty code, zero hash) and is counted, the same on both sides. An account the
//! block itself created or destroyed (a committed diff says so) holds its storage whole: an
//! unrecorded slot of it reads zero without a miss, as a node's state cache would answer.
//!
//! Every transaction commits a state diff in one JSON shape (`apply_diff`), so both sides keep
//! exactly the same state from one transaction to the next.

#![allow(dead_code)]

use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    str::FromStr,
};

pub const EMPTY_CODE_HASH: B256 = alloy_primitives::KECCAK256_EMPTY;

pub fn u256(v: &Value) -> U256 {
    match v {
        Value::String(s) => {
            let s = s.trim_start_matches("0x");
            if s.is_empty() {
                U256::ZERO
            } else {
                U256::from_str_radix(s, 16).expect("hex quantity")
            }
        }
        Value::Number(n) => U256::from(n.as_u64().expect("u64")),
        Value::Null => U256::ZERO,
        _ => panic!("not a quantity: {v}"),
    }
}

pub fn u64v(v: &Value) -> u64 {
    u256(v).to::<u64>()
}

pub fn addr(v: &Value) -> Address {
    Address::from_str(v.as_str().expect("address")).expect("address")
}

pub fn b256(v: &Value) -> B256 {
    B256::from_str(v.as_str().expect("hash")).expect("hash")
}

pub fn bytes(v: &Value) -> Bytes {
    match v {
        Value::String(s) => Bytes::from_str(s).expect("hex bytes"),
        _ => Bytes::new(),
    }
}

pub fn hex(b: &[u8]) -> String {
    let mut s = String::with_capacity(2 + 2 * b.len());
    s.push_str("0x");
    for x in b {
        s.push_str(&format!("{x:02x}"));
    }
    s
}

#[derive(Clone, Debug, Default)]
pub struct Acc {
    pub balance: U256,
    pub nonce: u64,
    pub code_hash: Option<B256>,
    pub storage: HashMap<U256, U256>,
    /// The block created or destroyed it: its storage is known whole.
    pub whole: bool,
}

impl Acc {
    pub fn is_absent(&self) -> bool {
        self.balance.is_zero() && self.nonce == 0 && self.code_hash.is_none()
    }
}

/// What the database served, as Satin's `RecordingDatabase` keys it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reads {
    pub accounts: BTreeSet<Address>,
    pub storage: BTreeSet<(Address, U256)>,
    pub codes: BTreeSet<B256>,
    pub block_hashes: BTreeSet<u64>,
}

impl Reads {
    pub fn to_json(&self) -> Value {
        json!({
            "accounts": self.accounts.iter().map(|a| format!("{a}")).collect::<Vec<_>>(),
            "storage": self.storage.iter().map(|(a, k)| format!("{a}:{k:#x}")).collect::<Vec<_>>(),
            "codes": self.codes.iter().map(|h| format!("{h}")).collect::<Vec<_>>(),
            "block_hashes": self.block_hashes.iter().collect::<Vec<_>>(),
        })
    }
}

/// The block's header fields both engines' block environments are built from.
#[derive(Clone, Debug)]
pub struct Header {
    pub number: u64,
    pub beneficiary: Address,
    pub timestamp: u64,
    pub gas_limit: u64,
    pub base_fee: u64,
    pub difficulty: U256,
    pub mix_hash: B256,
    pub excess_blob_gas: u64,
    pub parent_hash: B256,
    pub parent_beacon_block_root: Option<B256>,
}

pub struct Block {
    pub set: String,
    pub number: u64,
    pub chain_id: u64,
    pub header: Header,
    pub txs: Vec<Value>,
    pub receipts: Vec<Value>,
    pub db: BlockDb,
}

#[derive(Default)]
pub struct BlockDb {
    pub accounts: HashMap<Address, Acc>,
    pub codes: HashMap<B256, Bytes>,
    pub block_hashes: BTreeMap<u64, B256>,
    /// Recording of every read (on in the correctness runs, off in the measured ones).
    pub record: bool,
    pub reads: Reads,
    /// Every miss, in order (`what`), and the per-run count.
    pub misses: Vec<String>,
    pub run_misses: u64,
}

impl BlockDb {
    pub fn account(&mut self, a: Address) -> Option<(U256, u64, B256, Bytes)> {
        if self.record {
            self.reads.accounts.insert(a);
        }
        let Some(acc) = self.accounts.get(&a) else {
            self.miss(format!("account {a}"));
            return None;
        };
        if acc.is_absent() {
            return None;
        }
        let (balance, nonce, hash) =
            (acc.balance, acc.nonce, acc.code_hash.unwrap_or(EMPTY_CODE_HASH));
        // The code comes with the account (as Satin's replay database serves it): its hash is in
        // the read set whenever the account has code.
        let code = if hash == EMPTY_CODE_HASH { Bytes::new() } else { self.code(hash) };
        Some((balance, nonce, hash, code))
    }

    pub fn code(&mut self, h: B256) -> Bytes {
        if h == EMPTY_CODE_HASH {
            return Bytes::new();
        }
        if self.record {
            self.reads.codes.insert(h);
        }
        match self.codes.get(&h) {
            Some(c) => c.clone(),
            None => {
                self.miss(format!("code {h}"));
                Bytes::new()
            }
        }
    }

    pub fn storage(&mut self, a: Address, k: U256) -> U256 {
        if self.record {
            self.reads.storage.insert((a, k));
        }
        if let Some(acc) = self.accounts.get(&a) {
            if let Some(v) = acc.storage.get(&k) {
                return *v;
            }
            if acc.whole {
                return U256::ZERO;
            }
        }
        self.miss(format!("storage {a}[{k:#x}]"));
        U256::ZERO
    }

    pub fn block_hash(&mut self, n: u64) -> B256 {
        if self.record {
            self.reads.block_hashes.insert(n);
        }
        match self.block_hashes.get(&n) {
            Some(h) => *h,
            None => {
                self.miss(format!("hash of block {n}"));
                B256::ZERO
            }
        }
    }

    fn miss(&mut self, what: String) {
        self.run_misses += 1;
        if self.misses.len() < 100_000 {
            self.misses.push(what);
        }
    }

    /// Begins a run: clears the read record and the run's miss count.
    pub fn begin_run(&mut self) {
        self.reads = Reads::default();
        self.run_misses = 0;
    }

    /// Applies a diff in the shared shape:
    /// `{"accounts": {addr: {"balance","nonce","code_hash","code"?,"created","destroyed",
    /// "storage": {slot: value}}}}`.
    pub fn apply_diff(&mut self, diff: &Value) {
        let Some(accounts) = diff.get("accounts").and_then(Value::as_object) else { return };
        for (a, d) in accounts {
            let a = Address::from_str(a).expect("address");
            if let Some(code) = d.get("code").filter(|c| !c.is_null()) {
                let code = bytes(code);
                self.codes.insert(keccak256(&code), code);
            }
            let acc = self.accounts.entry(a).or_default();
            if d["destroyed"].as_bool() == Some(true) {
                *acc = Acc { whole: true, ..Acc::default() };
                continue;
            }
            if d["created"].as_bool() == Some(true) {
                acc.storage.clear();
                acc.whole = true;
            }
            acc.balance = u256(&d["balance"]);
            acc.nonce = u64v(&d["nonce"]);
            let h = b256(&d["code_hash"]);
            acc.code_hash = (h != EMPTY_CODE_HASH && h != B256::ZERO).then_some(h);
            if let Some(st) = d.get("storage").and_then(Value::as_object) {
                for (k, v) in st {
                    acc.storage.insert(u256(&Value::String(k.clone())), u256(v));
                }
            }
        }
    }
}

/// Loads `<dir>/<n>.json` (written by `tools/convert.py`).
pub fn load_block(path: &str, set: &str) -> Block {
    let v: Value = serde_json::from_str(&std::fs::read_to_string(path).expect("block file"))
        .expect("block json");
    let h = &v["header"];
    let header = Header {
        number: u64v(&h["number"]),
        beneficiary: addr(&h["miner"]),
        timestamp: u64v(&h["timestamp"]),
        gas_limit: u64v(&h["gasLimit"]),
        base_fee: u64v(&h["baseFeePerGas"]),
        difficulty: u256(&h["difficulty"]),
        mix_hash: b256(&h["mixHash"]),
        excess_blob_gas: u64v(&h["excessBlobGas"]),
        parent_hash: b256(&h["parentHash"]),
        parent_beacon_block_root: h.get("parentBeaconBlockRoot").filter(|x| !x.is_null()).map(b256),
    };
    let mut db = BlockDb::default();
    for (a, acc) in v["accounts"].as_object().expect("accounts") {
        let mut storage = HashMap::new();
        if let Some(st) = acc.get("storage").and_then(Value::as_object) {
            for (k, val) in st {
                storage.insert(u256(&Value::String(k.clone())), u256(val));
            }
        }
        db.accounts.insert(
            Address::from_str(a).expect("address"),
            Acc {
                balance: u256(&acc["balance"]),
                nonce: acc.get("nonce").map_or(0, u64v),
                code_hash: acc.get("code_hash").filter(|x| !x.is_null()).map(b256),
                storage,
                whole: false,
            },
        );
    }
    for (h, code) in v["codes"].as_object().expect("codes") {
        if !code.is_null() {
            db.codes.insert(B256::from_str(h).expect("hash"), bytes(code));
        }
    }
    for (n, hash) in v["block_hashes"].as_object().expect("block hashes") {
        db.block_hashes.insert(n.parse().expect("number"), b256(hash));
    }
    Block {
        set: set.to_string(),
        number: v["number"].as_u64().expect("number"),
        chain_id: v["chain_id"].as_u64().expect("chain id"),
        header,
        txs: v["txs"].as_array().expect("txs").clone(),
        receipts: v["receipts"].as_array().expect("receipts").clone(),
        db,
    }
}

/// The block files of a run: `MEGA_EVM2_BLOCKS=<set>:<n>[,<set>:<n>...]` or `MEGA_EVM2_SETS=d4,p9`
/// (all blocks of each set, in block order), under `MEGA_EVM2_BLOCK_DIR` (required).
pub fn block_list() -> Vec<(String, String)> {
    let data = std::env::var("MEGA_EVM2_BLOCK_DIR").expect("MEGA_EVM2_BLOCK_DIR");
    if let Ok(list) = std::env::var("MEGA_EVM2_BLOCKS") {
        return list
            .split(',')
            .filter(|s| !s.is_empty())
            .map(|s| {
                let (set, n) = s.split_once(':').expect("set:number");
                (set.to_string(), format!("{data}/{set}/{n}.json"))
            })
            .collect();
    }
    let sets = std::env::var("MEGA_EVM2_SETS").unwrap_or("d4,p9".into());
    let mut out = Vec::new();
    for set in sets.split(',') {
        let man: Value = serde_json::from_str(
            &std::fs::read_to_string(format!("{data}/{set}/manifest.json")).expect("manifest"),
        )
        .expect("manifest json");
        for b in man.as_array().expect("manifest") {
            out.push((set.to_string(), format!("{data}/{set}/{}.json", b["number"])));
        }
    }
    out
}
