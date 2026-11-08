//! The reference implementation on the engine benchmark's exact workloads and timing boundaries.

use alloy_consensus::transaction::Recovered;
use alloy_eips::Decodable2718;
use alloy_evm::Evm as _;
use alloy_op_evm::OpTx;
use alloy_primitives::{B256, Bytes, TxKind, U256};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use mega_evm::{
    AHashBucketHasher, BlockLimits, EvmTxRuntimeLimits, ExternalEnvFactory, MegaBlockExecutionCtx,
    MegaBlockExecutor, MegaContext, MegaEvm, MegaEvmFactory, MegaHardfork, MegaSpecId,
    MegaTxEnvelope, TestExternalEnvs,
    alloy_evm::{EvmEnv, EvmFactory, block::BlockExecutor},
    alloy_hardforks::ForkCondition,
    alloy_op_evm::block::OpAlloyReceiptBuilder,
    test_utils::{MemoryDatabase, RecordingDatabase, RecordingEnvFactory, op_transaction},
};
use mega_evm2_reference::{Fixed, Frames, SatinDb, Test, block_env, blockdb, synthetic};
use mega_evm2_scenarios::{self as scenarios, Scenario};
use revm::{
    DatabaseCommit,
    context::{BlockEnv, CfgEnv, TxEnv},
    database::State,
};
use std::{cell::RefCell, convert::Infallible, hint::black_box, rc::Rc};

#[path = "../../diff/shared/src/bench_recorded.rs"]
mod bench_recorded;
#[path = "../../diff/shared/src/bench_workloads.rs"]
mod bench_workloads;

fn bench_scenario(c: &mut Criterion, name: &str, s: Scenario) {
    let expected = synthetic::run(&s);
    assert_eq!(expected["status"], "success");
    assert_eq!(expected["inspected_equal"], true);
    println!("WORKLOAD {} gas_used={}", s.name, expected["gas_used"]);
    if let Ok(dir) = std::env::var("BENCH_WORKLOAD_RECORDS") {
        std::fs::create_dir_all(&dir).expect("workload record directory");
        std::fs::write(format!("{dir}/{}.json", s.name), expected.to_string())
            .expect("write workload record");
    }
    // These fixed workloads need no custom SALT, Oracle, L1 fee or typed-envelope setup.
    assert!(s.salt.is_empty() && s.salt_capacity.is_empty() && s.oracle.is_empty());
    assert!(s.l1_fee.is_none() && s.auths.is_empty() && s.tx_type == 0 && !s.create);
    for inspected in [false, true] {
        let setup = || {
            let mut db = MemoryDatabase::default();
            for a in &s.accounts {
                if !a.balance.is_zero() {
                    db = db.account_balance(a.address, a.balance);
                }
                if !a.code.is_empty() {
                    db = db.account_code(a.address, Bytes::from(a.code.clone()));
                }
                if a.nonce != 0 {
                    db = db.account_nonce(a.address, a.nonce);
                }
                for (key, value) in &a.storage {
                    db = db.account_storage(a.address, U256::from(*key), U256::from(*value));
                }
            }
            let record = Rc::new(RefCell::new(Default::default()));
            let factory = RecordingEnvFactory::new(Fixed(Test::new()), record.clone());
            let defaults = EvmTxRuntimeLimits::default();
            let limits = EvmTxRuntimeLimits {
                tx_data_size_limit: s.limits.data.unwrap_or(defaults.tx_data_size_limit),
                tx_kv_update_limit: s.limits.kv.unwrap_or(defaults.tx_kv_update_limit),
                tx_state_growth_limit: s.limits.growth.unwrap_or(defaults.tx_state_growth_limit),
                block_env_access_compute_gas_limit: s
                    .limits
                    .block_env_cap
                    .unwrap_or(defaults.block_env_access_compute_gas_limit),
                oracle_access_compute_gas_limit: s
                    .limits
                    .oracle_cap
                    .unwrap_or(defaults.oracle_access_compute_gas_limit),
                ..defaults
            };
            let context = MegaContext::new_with_external_envs(
                RecordingDatabase::new(db, record),
                MegaSpecId::SATIN,
                factory.external_envs(1),
            )
            .with_block(BlockEnv {
                number: U256::from(1),
                timestamp: U256::from(1_000),
                gas_limit: scenarios::BLOCK_GAS_LIMIT,
                slot_num: 42,
                beneficiary: s.beneficiary,
                ..Default::default()
            })
            .with_tx_runtime_limits(limits);
            let mut evm = MegaEvm::new(context).with_inspector(Frames::default());
            evm.set_inspector_enabled(inspected);
            let mut tx = OpTx(op_transaction(TxEnv {
                caller: s.sender,
                kind: TxKind::Call(s.to),
                data: Bytes::from(s.data.clone()),
                value: s.value,
                gas_limit: s.gas_limit,
                gas_price: s.gas_price as u128,
                chain_id: if s.deposit.is_some() { Some(1) } else { s.chain_id },
                ..Default::default()
            }));
            if let Some(mint) = s.deposit {
                tx.0.deposit.source_hash = B256::with_last_byte(0x42);
                tx.0.deposit.mint = Some(mint as u128);
                tx.0.enveloped_tx = None;
            }
            (evm, tx)
        };
        let (mut evm, tx) = setup();
        let outcome = evm.execute_transaction(tx).expect("workload executes");
        assert!(outcome.result.is_success());
        assert_eq!(outcome.result.tx_gas_used(), expected["gas_used"].as_u64().expect("gas"));
        let (accounts, storage) = mega_evm2_reference::changed_json(&outcome.state);
        assert_eq!(serde_json::json!(accounts), expected["accounts"]);
        assert_eq!(serde_json::json!(storage), expected["storage"]);
        assert_eq!(
            blockdb::hex(outcome.result.output().map(|bytes| bytes.as_ref()).unwrap_or_default()),
            expected["output"].as_str().expect("output")
        );
        evm.db_mut().commit(outcome.result_and_state.state);
        let id = format!("{name}/{}", if inspected { "inspected" } else { "plain" });
        c.bench_function(&id, |b| {
            b.iter_batched(
                setup,
                |(mut evm, tx)| {
                    let outcome = evm.execute_transaction(tx).expect("transact");
                    evm.db_mut().commit(outcome.result_and_state.state);
                    black_box(outcome.result_and_state.result)
                },
                BatchSize::LargeInput,
            )
        });
    }
}

fn run_block(b: blockdb::Block, raws: &[Bytes], inspected: bool) -> usize {
    let hardforks = mega_evm::hardfork_schedule(b.chain_id)
        .unwrap_or_default()
        .with(MegaHardfork::Satin, ForkCondition::Timestamp(0));
    let mut cfg = CfgEnv::new_with_spec(MegaSpecId::SATIN);
    cfg.chain_id = b.chain_id;
    let env = block_env(&b);
    let factory = MegaEvmFactory::new()
        .with_external_env_factory(TestExternalEnvs::<Infallible, AHashBucketHasher>::new());
    let context = MegaBlockExecutionCtx::new(
        b.header.parent_hash,
        b.header.parent_beacon_block_root,
        Default::default(),
        BlockLimits::default(),
    )
    .with_no_user_tx_activation_block(false)
    .with_satin_activation_block(false);
    let db = Rc::new(RefCell::new(b.db));
    let mut state = State::builder().with_database(SatinDb(db)).with_bundle_update().build();
    let mut evm =
        factory.create_evm(&mut state, EvmEnv::new(cfg, env)).with_inspector(Frames::default());
    evm.set_inspector_enabled(inspected);
    let mut executor =
        MegaBlockExecutor::new(evm, context, &hardforks, OpAlloyReceiptBuilder::default());
    executor.apply_pre_execution_changes().expect("pre-block changes");
    assert_eq!(b.txs.len(), raws.len());
    for (tx, raw) in b.txs.iter().zip(raws) {
        let envelope = MegaTxEnvelope::decode_2718(&mut raw.as_ref()).expect("decode");
        black_box(envelope.tx_hash());
        let recovered = Recovered::new_unchecked(envelope, blockdb::addr(&tx["from"]));
        let outcome =
            executor.run_transaction(recovered.as_recovered_ref()).expect("run transaction");
        executor.commit_transaction_outcome(outcome).expect("commit");
    }
    executor.receipts().len()
}

fn engine(c: &mut Criterion) {
    for (name, scenario) in bench_workloads::workloads() {
        bench_scenario(c, name, scenario);
    }
    let rec = bench_recorded::recorded_block(concat!(env!("CARGO_MANIFEST_DIR"), "/../diff"));
    for inspected in [false, true] {
        assert_eq!(
            run_block(
                blockdb::load_block(&rec.path, bench_recorded::BLOCK_SET),
                &rec.raws,
                inspected
            ),
            rec.raws.len()
        );
        let id = format!("block/{}", if inspected { "inspected" } else { "plain" });
        c.bench_function(&id, |b| {
            b.iter_batched(
                || blockdb::load_block(&rec.path, bench_recorded::BLOCK_SET),
                |block| black_box(run_block(block, &rec.raws, inspected)),
                BatchSize::LargeInput,
            )
        });
    }
}

fn config() -> Criterion {
    Criterion::default()
        .sample_size(30)
        .warm_up_time(std::time::Duration::from_secs(1))
        .measurement_time(std::time::Duration::from_secs(2))
}

criterion_group! { name = benches; config = config(); targets = engine }
criterion_main!(benches);
