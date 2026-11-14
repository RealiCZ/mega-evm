//! Replays the corpus once under an inspector on KECCAK256, as validation of the build's Keccak
//! backend and cache inside the engine, before any timing:
//!
//! - every successful KECCAK256's result must equal an independent Keccak-256 (tiny-keccak) of the
//!   memory it hashed;
//! - every receipt's logs Bloom must equal one rebuilt from its logs with that independent hash.
//!
//! Prints each block's outcome as a JSON line on stdout (to compare with the timing runs) and a
//! summary line on stderr. With `KEC_TRACE_OUT=<dir>` it also writes the exact inputs, for the
//! trace-cache benchmark: `opcode-inputs.bin` and `bloom-inputs.bin`, each a little-endian u32
//! length then the bytes, `u32::MAX` after the pre-block calls and after every transaction,
//! `u32::MAX - 1` after every block.

use alloy_primitives::{B256, Bloom, BloomInput};
use evm2::{InspectorConfig, evm::Db, inspector::Inspector, interpreter::Interpreter};
use kec_block::{
    VARIANT, blockdb,
    replay::{self, Evm2Db},
};
use mega_evm2::{MegaTypes, factory::BlockServices};
use serde_json::json;
use std::{
    cell::RefCell,
    collections::HashMap,
    fs::File,
    io::{BufWriter, Write},
    rc::Rc,
};

fn independent(b: &[u8]) -> B256 {
    use tiny_keccak::Hasher;
    let mut h = tiny_keccak::Keccak::v256();
    h.update(b);
    let mut o = [0u8; 32];
    h.finalize(&mut o);
    B256::new(o)
}

#[derive(Default)]
struct State {
    pending: Option<(usize, usize)>,
    hashes: u64,
    hash64: u64,
    failed: u64,
    trace: Option<BufWriter<File>>,
}

impl State {
    fn mark(&mut self, marker: u32) {
        assert!(self.pending.is_none());
        if let Some(w) = &mut self.trace {
            w.write_all(&marker.to_le_bytes()).unwrap();
        }
    }
}

struct Inspect(Rc<RefCell<State>>);

impl Inspector<MegaTypes> for Inspect {
    fn inspector_config(&self) -> InspectorConfig {
        InspectorConfig::without_steps().with_steps([0x20].into_iter().collect())
    }
    fn step(&mut self, i: &mut Interpreter<'_, '_, MegaTypes>) {
        let mut s = self.0.borrow_mut();
        s.pending = i.stack().peekn::<2>().and_then(|[o, n]| {
            let len = usize::try_from(n).ok()?;
            let off = if len == 0 { 0 } else { usize::try_from(o).ok()? };
            Some((off, len))
        });
        if s.pending.is_none() {
            s.failed += 1;
        }
    }
    fn step_end(&mut self, i: &mut Interpreter<'_, '_, MegaTypes>) {
        let mut s = self.0.borrow_mut();
        let Some((off, len)) = s.pending.take() else { return };
        if i.result().is_err() {
            s.failed += 1;
            return;
        }
        let bytes = if len == 0 { &[][..] } else { i.memory().slice(off, len) };
        let result = B256::from(i.stack().peek(0).expect("the hash").to_be_bytes::<32>());
        assert_eq!(
            result,
            independent(bytes),
            "KECCAK256 result differs from tiny-keccak, len {len}"
        );
        if let Some(w) = &mut s.trace {
            w.write_all(&(len as u32).to_le_bytes()).unwrap();
            w.write_all(bytes).unwrap();
        }
        s.hashes += 1;
        if len == 64 {
            s.hash64 += 1;
        }
    }
}

fn main() {
    let out = std::env::var("KEC_TRACE_OUT").ok();
    let create = |name: &str| {
        out.as_ref().map(|d| BufWriter::new(File::create(format!("{d}/{name}")).unwrap()))
    };
    let state =
        Rc::new(RefCell::new(State { trace: create("opcode-inputs.bin"), ..State::default() }));
    let mut bloom_trace = create("bloom-inputs.bin");
    let (mut bloom_inputs, mut receipts, mut identities) = (0u64, 0u64, Vec::new());
    let (mut included_total, mut refused_total, mut gas_total) = (0usize, 0usize, 0u64);
    for (block, txs) in replay::load_corpus() {
        let database = Db::new(Evm2Db {
            db: Rc::new(RefCell::new(kec_block::bench_blocks::fresh_db(&block))),
            code_cache: Rc::new(RefCell::new(HashMap::new())),
        });
        let services = BlockServices {
            inspector: Some(Box::new(Inspect(state.clone()))),
            ..replay::services(&block.header)
        };
        let mut session = replay::factory(block.chain_id)
            .start_block(&replay::block_input(&block.header), database, services)
            .expect("pre-block changes");
        state.borrow_mut().mark(u32::MAX);
        let mut refused = 0;
        for (tx, raw) in &txs {
            match session.execute_candidate(tx, raw) {
                Ok(outcome) => {
                    if session.commit_candidate(outcome).is_err() {
                        refused += 1;
                    }
                }
                Err(_) => refused += 1,
            }
            state.borrow_mut().mark(u32::MAX);
        }
        for r in session.receipts() {
            let mut bloom = Bloom::ZERO;
            for l in &r.logs {
                let mut input = |b: &[u8]| {
                    bloom.accrue(BloomInput::Hash(independent(b)));
                    bloom_inputs += 1;
                    if let Some(w) = &mut bloom_trace {
                        w.write_all(&(b.len() as u32).to_le_bytes()).unwrap();
                        w.write_all(b).unwrap();
                    }
                };
                input(l.address.as_slice());
                for t in l.data.topics() {
                    input(t.as_slice());
                }
            }
            assert_eq!(
                bloom, r.bloom,
                "receipt Bloom differs from tiny-keccak's, block {}",
                block.number
            );
            receipts += 1;
        }
        if let Some(w) = &mut bloom_trace {
            w.write_all(&(u32::MAX - 1).to_le_bytes()).unwrap();
        }
        state.borrow_mut().mark(u32::MAX - 1);
        let included = session.receipts().len();
        let gas = session.limiter().block_gas_used;
        println!(
            "{}",
            json!({"set": block.set, "block": block.number, "txs": block.txs.len(),
                "included": included, "refused": refused, "gas": gas})
        );
        included_total += included;
        refused_total += refused;
        gas_total += gas;
        identities.push(format!("{}:{}", block.set, block.number));
    }
    assert!(blockdb::check_processed_identities(&identities) || blockdb::selection_is_custom());
    if let Some(mut w) = bloom_trace.take() {
        w.flush().expect("bloom trace");
    }
    if let Some(mut w) = state.borrow_mut().trace.take() {
        w.flush().expect("opcode trace");
    }
    let s = state.borrow();
    eprintln!(
        "{}",
        json!({"variant": VARIANT, "blocks": identities.len(), "included": included_total,
            "refused": refused_total, "gas": gas_total, "keccak256_checked": s.hashes,
            "keccak256_64_bytes": s.hash64, "keccak256_failed": s.failed,
            "receipts_bloom_checked": receipts, "bloom_inputs": bloom_inputs,
            "independent": "tiny-keccak 2.0.2"})
    );
}
