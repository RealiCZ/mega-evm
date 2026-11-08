//! Shared recorded-block workload and extraction outside the timed interval.
use crate::blockdb;

/// Corpus subset containing the fixed block workload.
pub const BLOCK_SET: &str = "p9";
const BLOCK_NUMBER: u64 = 26_400_004;

/// The recorded block and the 2718 bytes of its transactions, unpacked once.
pub struct RecordedBlock {
    /// Uncompressed input path used for untimed setup.
    pub path: String,
    /// The exact envelopes consumed by both implementations.
    pub raws: Vec<alloy_primitives::Bytes>,
}

fn unpack(from: String, to: &std::path::Path) {
    let status = std::process::Command::new("zstd")
        .args(["-q", "-d", "-f", "-o"])
        .arg(to)
        .arg(from)
        .status()
        .expect("run zstd");
    assert!(status.success(), "unpack the recorded block");
}

/// Extracts the fixed block once, before any measurement.
pub fn recorded_block(data: &str) -> RecordedBlock {
    let dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target"))
        .join("mega-evm2-bench-blocks");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let block = dir.join(format!("{BLOCK_SET}-{BLOCK_NUMBER}.json"));
    unpack(format!("{data}/data/real-blocks/{BLOCK_SET}/{BLOCK_NUMBER}.json.zst"), &block);
    // The replay reference holds each transaction's 2718 bytes, which the block does not.
    let replay = dir.join(format!("{BLOCK_SET}-{BLOCK_NUMBER}.jsonl"));
    unpack(format!("{data}/data/replay/{BLOCK_SET}/{BLOCK_NUMBER}.jsonl.zst"), &replay);
    let raws = std::fs::read_to_string(replay)
        .expect("replay lines")
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect("replay line"))
        .filter(|l| l["kind"] == "tx")
        .map(|l| blockdb::bytes(&l["raw"]))
        .collect();
    RecordedBlock { path: block.to_str().expect("utf-8 path").to_owned(), raws }
}
