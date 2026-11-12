//! Reference side of the block executor gate: every recorded block through `MegaBlockExecutor`,
//! built as `mega-evme replay` builds it for a Satin block: revm `State` with bundle update over
//! the block's state, `MegaEvmFactory` with mega-evme's external environments
//! (`TestExternalEnvs<Infallible, AHashBucketHasher>`, no capacities), the chain's schedule with
//! Satin from genesis, the recorded parent block's timestamp (from which the executor decides the
//! activation-block rules), `BlockLimits::default()` (a validator's), the block's own gas limit;
//! then `apply_pre_execution_changes`, and per transaction `run_transaction` and
//! `commit_transaction_outcome`, a refusal recorded and skipped.
//!
//! Writes one JSON line per transaction (status, gas, usage, the receipt's 2718 bytes, the state
//! diff), then the block's counters and post-block bundle state, to
//! `$MEGA_EVM2_BLOCK_OUT/<set>/<n>.jsonl`.
//!
//! Environment: `MEGA_EVM2_BLOCK_DIR` (recorded blocks), `MEGA_EVM2_BLOCK_OUT`, optionally
//! `MEGA_EVM2_SETS` / `MEGA_EVM2_BLOCKS` and the count expectations `MEGA_EVM2_EXPECT`,
//! `_EXPECT_REFUSED`, `_EXPECT_BLOCKS`.

#![allow(unreachable_pub, deprecated, missing_docs, clippy::all)]

use alloy_consensus::transaction::Recovered;
use alloy_eips::{Decodable2718, Encodable2718};
use alloy_evm::Evm as _;
use mega_evm::{
    AHashBucketHasher, BlockLimits, MegaBlockExecutionCtx, MegaBlockExecutor, MegaEvmFactory,
    MegaHardfork, MegaSpecId, MegaTxEnvelope, TestExternalEnvs,
    alloy_evm::{EvmEnv, EvmFactory, block::BlockExecutor},
    alloy_hardforks::ForkCondition,
    alloy_op_evm::block::OpAlloyReceiptBuilder,
};
use mega_evm2_reference::{Frames, SatinDb, block_env, blockdb, diff_of, envelope};
use revm::{
    context::{CfgEnv, result::ExecutionResult},
    database::State,
    state::AccountInfo,
};
use serde_json::{Map, Value, json};
use std::{cell::RefCell, convert::Infallible, io::Write, rc::Rc};

type EvmeExternalEnvs = TestExternalEnvs<Infallible, AHashBucketHasher>;

fn status_of(r: &ExecutionResult<mega_evm::MegaHaltReason>) -> &'static str {
    match r {
        ExecutionResult::Success { .. } => "success",
        ExecutionResult::Revert { .. } => "revert",
        ExecutionResult::Halt { .. } => "halt",
    }
}

fn logs_json(logs: &[alloy_primitives::Log]) -> Value {
    json!(logs
        .iter()
        .map(|l| json!({"address": format!("{}", l.address), "topics": l.topics().iter().map(|t| format!("{t}")).collect::<Vec<_>>(), "data": blockdb::hex(&l.data.data)}))
        .collect::<Vec<_>>())
}

/// Runs one block; `render` adds the receipts, diffs and post-block state (correctness run).
fn run_block(
    b: blockdb::Block,
    render: bool,
    inspected: bool,
    out: &mut impl Write,
) -> (usize, usize) {
    let db = Rc::new(RefCell::new(blockdb::BlockDb::default()));
    {
        let mut d = db.borrow_mut();
        d.accounts = b.db.accounts.clone();
        d.codes = b.db.codes.clone();
        d.block_hashes = b.db.block_hashes.clone();
    }
    let h = &b.header;
    let hardforks = mega_evm::hardfork_schedule(b.chain_id)
        .unwrap_or_default()
        .with(MegaHardfork::Satin, ForkCondition::Timestamp(0));
    let mut cfg = CfgEnv::new_with_spec(MegaSpecId::SATIN);
    cfg.chain_id = b.chain_id;
    let block_env = block_env(&b);
    let evm_factory = MegaEvmFactory::new().with_external_env_factory(EvmeExternalEnvs::new());
    let block_ctx = MegaBlockExecutionCtx::new(
        h.parent_hash,
        h.parent_beacon_block_root,
        Default::default(),
        BlockLimits::default(),
        b.parent_timestamp,
    );
    let mut state =
        State::builder().with_database(SatinDb(db.clone())).with_bundle_update().build();
    let (mut included, mut refused) = (0, 0);
    {
        let mut evm = evm_factory
            .create_evm(&mut state, EvmEnv::new(cfg, block_env))
            .with_inspector(Frames::default());
        evm.set_inspector_enabled(inspected);
        let mut executor =
            MegaBlockExecutor::new(evm, block_ctx, &hardforks, OpAlloyReceiptBuilder::default());
        executor.apply_pre_execution_changes().expect("pre-block changes");
        for (i, t) in b.txs.iter().enumerate() {
            let (_, raw) = envelope(t).expect("envelope");
            let envelope = MegaTxEnvelope::decode_2718(&mut raw.as_ref()).expect("decode");
            let _warm_hash = envelope.tx_hash();
            let recovered = Recovered::new_unchecked(envelope, blockdb::addr(&t["from"]));
            let r = executor.run_transaction(recovered.as_recovered_ref());
            let mut line = json!({"kind": "tx", "block": b.number, "index": i, "hash": t["hash"], "type": t["type"]});
            let o = line.as_object_mut().unwrap();
            match r {
                Err(e) => {
                    refused += 1;
                    o.insert("status".into(), json!("refused"));
                    o.insert("reason".into(), json!(e.to_string()));
                }
                Ok(outcome) => {
                    o.insert("status".into(), json!(status_of(&outcome.result_and_state.result)));
                    let result = &outcome.result_and_state.result;
                    o.insert(
                        "halt".into(),
                        match result {
                            ExecutionResult::Halt { reason, .. } => json!(format!("{reason:?}")),
                            _ => Value::Null,
                        },
                    );
                    o.insert(
                        "output".into(),
                        json!(blockdb::hex(
                            result.output().map(|b| b.as_ref()).unwrap_or_default()
                        )),
                    );
                    o.insert(
                        "gas_used".into(),
                        json!(outcome.result_and_state.result.tx_gas_used()),
                    );
                    if render {
                        let u = outcome.usage;
                        o.insert(
                            "usage".into(),
                            json!([u.data_size, u.write_records, u.state_growth]),
                        );
                        o.insert("receipt_gas".into(), json!(outcome.gas.gas_used));
                        let g = &outcome.gas;
                        o.insert(
                            "gas_split".into(),
                            json!([g.block_execution_gas(), g.state, g.history, g.history_bytes]),
                        );
                        o.insert("system_tx".into(), json!(outcome.is_system_transaction));
                        o.insert("da_size".into(), json!(outcome.da_size));
                        o.insert("tx_size".into(), json!(outcome.tx_size));
                        o.insert("diff".into(), diff_of(&outcome.result_and_state.state));
                    }
                    let c = executor.commit_transaction_outcome(outcome);
                    match c {
                        Err(e) => {
                            refused += 1;
                            o.insert("status".into(), json!("refused_at_commit"));
                            o.insert("reason".into(), json!(e.to_string()));
                        }
                        Ok(_) => {
                            included += 1;
                            if render {
                                let receipt = executor.receipts().last().unwrap();
                                o.insert(
                                    "receipt".into(),
                                    json!(blockdb::hex(&receipt.encoded_2718())),
                                );
                                o.insert("receipt_status".into(), json!(receipt.status()));
                                o.insert(
                                    "cumulative_gas_used".into(),
                                    json!(receipt.cumulative_gas_used()),
                                );
                                o.insert("logs".into(), logs_json(receipt.logs()));
                            }
                        }
                    }
                }
            }
            writeln!(out, "{line}").unwrap();
        }
        if render {
            let l = executor.limiter();
            let g = executor.gas();
            writeln!(
                out,
                "{}",
                json!({"kind": "block_counters", "block": b.number, "gas_used": l.block_gas_used, "da_footprint_used": l.block_da_footprint_used,
                    "da_size_used": l.block_da_size_used, "tx_size_used": l.block_tx_size_used,
                    "usage": [l.usage.data_size, l.usage.write_records, l.usage.state_growth],
                    "gas": [g.execution, g.state, g.history, g.history_bytes]})
            )
            .unwrap();
        }
    }
    if render {
        // The block's net state changes (pre-block calls and every committed transaction), from
        // the bundle: accounts whose info changed, slots whose value changed.
        state.merge_transitions(revm::database::states::bundle_state::BundleRetention::Reverts);
        let bundle = state.take_bundle();
        let mut accounts = Map::new();
        let mut storage = Map::new();
        for (a, acc) in bundle.state.iter() {
            let (orig, cur) = (acc.original_info.clone(), acc.info.clone());
            let render_info = |i: &Option<AccountInfo>| match i {
                Some(i)
                    if !(i.balance.is_zero() &&
                        i.nonce == 0 &&
                        i.code_hash == alloy_primitives::KECCAK256_EMPTY) =>
                {
                    json!(format!("{}/n{}/{}", i.balance, i.nonce, i.code_hash))
                }
                _ => json!("absent"),
            };
            if render_info(&orig) != render_info(&cur) {
                accounts.insert(format!("{a}"), render_info(&cur));
            }
            for (k, s) in acc.storage.iter() {
                if s.previous_or_original_value != s.present_value {
                    storage.insert(format!("{a}:{k:#x}"), json!(format!("{:#x}", s.present_value)));
                }
            }
        }
        writeln!(out, "{}", json!({"kind": "post_state", "block": b.number, "accounts": accounts, "storage": storage})).unwrap();
    }
    (included, refused)
}

#[test]
fn test_reference_block() {
    let out_dir = std::env::var("MEGA_EVM2_BLOCK_OUT").expect("MEGA_EVM2_BLOCK_OUT");
    let (mut inc, mut refd, mut blocks) = (0, 0, 0);
    for (set, path) in blockdb::block_list() {
        let b = blockdb::load_block(&path, &set);
        let n = b.number;
        let mut plain = Vec::new();
        let mut inspected = Vec::new();
        let (i, r) = run_block(b, true, false, &mut plain);
        let (ii, rr) = run_block(blockdb::load_block(&path, &set), true, true, &mut inspected);
        let rows = |bytes: &[u8]| -> Vec<Value> {
            std::str::from_utf8(bytes)
                .unwrap()
                .lines()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect()
        };
        let mut p = rows(&plain);
        assert_eq!((i, r), (ii, rr), "reference inclusion {set}:{n}");
        assert_eq!(p, rows(&inspected), "reference plain/inspected {set}:{n}");
        std::fs::create_dir_all(format!("{out_dir}/{set}")).unwrap();
        let mut f = std::io::BufWriter::new(
            std::fs::File::create(format!("{out_dir}/{set}/{n}.jsonl")).unwrap(),
        );
        for row in &mut p {
            row["inspected_equal"] = json!(true);
            writeln!(f, "{row}").unwrap();
        }
        println!("{set}:{n} included {i} refused {r} inspected_equal=true");
        inc += i;
        refd += r;
        blocks += 1;
    }
    println!("TOTAL included {inc} refused {refd} blocks {blocks}");
    for (key, n) in [
        ("MEGA_EVM2_EXPECT", inc),
        ("MEGA_EVM2_EXPECT_REFUSED", refd),
        ("MEGA_EVM2_EXPECT_BLOCKS", blocks),
    ] {
        if let Ok(s) = std::env::var(key) {
            assert_eq!(n, s.parse::<usize>().unwrap(), "{key}");
        }
    }
}
