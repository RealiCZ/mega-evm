# Keccak benchmark (bench-only)

Measures Keccak-256 for the Satin engine: the hash backends Alloy can use, and Alloy's exact-input cache, from single hashes up to the 263-block recorded corpus.
It lives on a bench-only branch and is never merged; `.github/workflows/keccak-bench.yml` runs it on GitHub's hosted x86-64 and arm64 Linux runners.

## What is compared

- **Backends.** RustCrypto (`sha3`/`keccak`, Alloy's default), `tiny-keccak` and `keccak-asm` 0.1.9 (Alloy's `asm-keccak`).
  `keccak-asm` picks its backend at compile time: on x86-64, Cryptogams scalar assembly for the generic target, AVX2 intrinsics with `x86-64-v3`, AVX-512VL intrinsics with `x86-64-v4`; on aarch64, NEON intrinsics, using the SHA3 instructions only when compiled with `+sha3`.
  RustCrypto is scalar Rust on x86-64 and detects the aarch64 SHA3 instructions at run time.
- **Cache.** Alloy's `keccak-cache-global`: a process-wide, fixed-size cache of inputs up to 87 bytes, keyed by the complete input, which every `alloy_primitives::keccak256` call goes through (the KECCAK256 opcode and receipt Blooms among them; streaming hashes do not).
- **Block variants.** `baseline` (no feature), `asm`, `cache`, `asm-cache`; on x86-64, `baseline-v3`/`asm-v3` and `baseline-v4`/`asm-v4` (the same under `target-cpu=x86-64-v3`/`-v4`) where the CPU has them; on aarch64, `asm-sha3` (`asm` built with `+sha3`) and the scalar backends `tiny`, `tiny-cache` (Alloy's `tiny-keccak`) and `soft` (RustCrypto under `--cfg keccak_backend="soft"`).
  All build the engine with its default features; the differential harness's `compare` and `salt-test-hasher` are not enabled.

## Crates

- `micro/`: `kec-micro` (one-shot sizes; validates every backend first), `prefix-bench` (64 bytes as 32+32 streaming against one shot), `trace-cache` (the captured exact inputs through bounded caches, outside the EVM), `validate-cache` (Alloy's cache against uncached hashing), `isa-probe` (which ISA variants the CPU runs).
- `block/`: `kec-block` (the gates' block timing, serial or on several threads) and `kec-trace` (one replay under an inspector that checks every KECCAK256 result and receipt Bloom against a Keccak the build's engine does not use, and captures the exact inputs).
  It reads the corpus as the gates do, with `next/diff/shared/blockdb.rs` and `next/diff/shared/src/bench_blocks.rs` included unchanged.

Build one package per cargo invocation: the two enable different Alloy features.

## Running it

The workflow does all of this; locally (from this directory, with the corpus extracted):

```sh
export MEGA_EVM2_BLOCK_DIR=<corpus>/real-blocks
export MEGA_EVM2_REPLAY_REF=<the reference archive's raw/satin>
bash scripts/build.sh bin generic baseline,asm,cache,asm-cache
python3 -I scripts/kec.py validate --bin bin --out out --isas generic
python3 -I scripts/kec.py micro --bin bin --out out --isas generic --core 3
python3 -I scripts/kec.py blocks --bin bin --out out --core 3
python3 -I scripts/kec.py parallel --bin bin --out out --configs 1:split,4:split,4:shared
python3 -I scripts/kec.py profile --bin bin --out out --variants baseline,asm --core 3
python3 -I scripts/kec.py summarize --out out --arch local
```

`--core` pins with `taskset` where it exists.
The block timing refuses a narrowed block selection: it always runs the whole corpus.

## Outputs

- `micro.csv`, `prefix.csv`, `trace-cache.csv`: every sample, with its variant, round and load average.
- `blocks/*.jsonl`, `blocks.json`, `blocks.csv`: per-block warm samples and first-traversal times per process; whole-corpus totals per traversal.
- `parallel.jsonl`: one line per multi-threaded traversal (summed block time and wall time).
- `profile.json`, `perf-*.txt`: the sampled leaf-symbol share of Keccak in the timed traversals.
- `validation.json`, `trace-*.jsonl`: validation results and per-block outcomes.
- `env/`: `lscpu`, `/proc/cpuinfo`, the image, the compiler, `rustc --print cfg` per flag set, the CPU quota, the ISA probe.
- `summary.json`, `summary.md`.
