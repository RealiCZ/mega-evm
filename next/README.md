# `next/` — the Satin engine workspace

This directory holds the next generation MegaETH execution engine, built on
[evm2](https://github.com/alloy-rs/evm2) instead of revm. The existing engine in
`crates/` (REX1–REX6, built on revm) is frozen: it keeps shipping the current
chains and is not modified by work done here.

The two engines cannot share a Cargo workspace. The root workspace pins `alloy`
2.0.5 exactly for consensus-stable builds, while the evm2 line needs `alloy` 2.5.
So `next/` is its own workspace with its own `Cargo.lock`, outside the root one.

## Directories

- `mega-evm2` — the Satin engine on evm2: Satin and later specs, resource
  metering, system contracts and block execution.
- `op-evm2` — a vendored Optimism execution layer, so the Satin engine does not
  depend on `op-revm` (revm-based) for its OP semantics. `op-evm2/VENDORED.md`
  records its upstream base and every local change.
- `diff` — shared scenarios and strict differential gates that compare the Satin
  engine with the Satin implementation under review (outputs produced by
  `reference/`).
- `reference` — its own workspace (separate lockfile); generates reference
  outputs from the Satin implementation under review.

## Differential gates

`diff/scripts/gate.sh sample|full` runs the strict gates (synthetic scenarios, fuzz batches,
engine replay and block executor on recorded blocks) against the reference outputs and prints one
`GATE` summary line per gate.
`sample` uses the in-repo data under `diff/data` (see `diff/data/README.md`); `full` reads the
full corpus from `MEGA_EVM2_CORPUS_DIR`.
The only accepted difference is listed in `diff/known-differences.toml`.
`diff/scripts/check-reference.sh sample` regenerates the sample's reference outputs with
`reference/` and checks they equal the stored ones.

## Building

The toolchain is pinned in `next/rust-toolchain.toml`.
Until our thin evm2 fork is pushed and pinned by git revision, `evm2` is a path
dependency on a checkout of the fork next to this repository (`../evm2`).

```bash
cd next && cargo check --workspace
```
