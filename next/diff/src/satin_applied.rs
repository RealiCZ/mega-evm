//! Test harness: a transaction this engine does not build (EIP-7702, keyless) is "Satin-applied" in
//! a block run: Satin's state diff is committed into the EVM's overlay and the block's change set,
//! and its receipt and counts (from Satin's block-executor run) are taken as given, so the block's
//! later transactions run on the same state on both sides. Not engine code.

use crate::replay::blockdb;
use alloy_primitives::{Address, B256};
use evm2::{
    Evm,
    bytecode::Bytecode,
    evm::{AccountChangeRef, AccountInfo, StateChangeSink, StateChangeSource, StorageChange},
};
use mega_evm2::{
    MegaTypes,
    block::{BlockExecutor, Receipt},
};
use serde_json::Value;

/// An account of the diff: address, before, after, created, destroyed.
type DiffAccount = (Address, Option<AccountInfo>, Option<AccountInfo>, bool, bool);

/// Satin's committed diff of a transaction (`diff_of` in the Satin replay), as a change source.
pub struct SatinDiff {
    accounts: Vec<DiffAccount>,
    storage: Vec<StorageChange>,
    code: Vec<(B256, Bytecode)>,
}

impl SatinDiff {
    pub fn new(diff: &Value, evm: &mut Evm<'_, MegaTypes>) -> Self {
        let mut s = SatinDiff { accounts: Vec::new(), storage: Vec::new(), code: Vec::new() };
        for (a, acc) in diff["accounts"].as_object().unwrap() {
            let address: Address = a.parse().unwrap();
            let before = evm.read_account_info(&address).ok().flatten();
            let destroyed = acc["destroyed"].as_bool() == Some(true);
            let mut info = before.clone().unwrap_or_default();
            info.balance = blockdb::u256(&acc["balance"]);
            info.nonce = acc["nonce"].as_u64().unwrap();
            let hash = blockdb::b256(&acc["code_hash"]);
            if let Some(code) = acc.get("code").filter(|c| !c.is_null()) {
                let bc = Bytecode::new_raw(blockdb::bytes(code));
                s.code.push((hash, bc.clone()));
                info.code = Some(bc);
            }
            info.code_hash = hash;
            let created = acc["created"].as_bool() == Some(true);
            s.accounts.push((address, before, (!destroyed).then_some(info), created, destroyed));
            for (k, v) in acc["storage"].as_object().unwrap() {
                let key = blockdb::u256(&Value::String(k.clone()));
                let original =
                    evm.state_mut().storage_slot_untracked(&address, &key).unwrap_or_default();
                s.storage.push(StorageChange { address, key, original, current: blockdb::u256(v) });
            }
        }
        s
    }
}

impl StateChangeSource for SatinDiff {
    fn visit<S: StateChangeSink>(&self, sink: &mut S) -> Result<(), S::Error> {
        for (h, c) in &self.code {
            sink.bytecode(*h, c)?;
        }
        for st in &self.storage {
            sink.storage(*st)?;
        }
        for (address, original, current, created, selfdestructed) in &self.accounts {
            sink.account(AccountChangeRef {
                address: *address,
                original: original.as_ref(),
                current: current.as_ref(),
                created: *created,
                selfdestructed: *selfdestructed,
            })?;
        }
        Ok(())
    }
}

/// Applies Satin's block-run line `s` (an included transaction of type `ty`) to `ex`: state,
/// counters and receipt.
pub fn apply(ex: &mut BlockExecutor<'_>, s: &Value, ty: u8) {
    let diff = SatinDiff::new(&s["diff"], &mut ex.evm);
    ex.evm.commit_source(&diff);
    let _ = diff.visit(&mut ex.changes);
    let l = &mut ex.limiter;
    l.block_gas_used += s["receipt_gas"].as_u64().unwrap();
    for (k, v) in s["gas_split"].as_array().unwrap().iter().enumerate() {
        l.gas[k] += v.as_u64().unwrap();
    }
    for (k, v) in s["usage"].as_array().unwrap().iter().enumerate() {
        l.usage[k] += v.as_u64().unwrap();
    }
    l.block_tx_size_used += s["tx_size"].as_u64().unwrap();
    l.block_da_size_used += s["da_size"].as_u64().unwrap();
    let logs = s["logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|lg| {
            alloy_primitives::Log::new_unchecked(
                lg["address"].as_str().unwrap().parse().unwrap(),
                lg["topics"].as_array().unwrap().iter().map(blockdb::b256).collect(),
                blockdb::bytes(&lg["data"]),
            )
        })
        .collect();
    let logs: Vec<alloy_primitives::Log> = logs;
    let bloom = alloy_primitives::logs_bloom(logs.iter());
    ex.receipts.push(Receipt {
        ty,
        status: s["receipt_status"].as_bool().unwrap(),
        cumulative_gas_used: l.block_gas_used,
        bloom,
        logs,
        deposit_nonce: None,
    });
}
