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
  depend on `op-revm` (revm-based) for its OP semantics.
- `diff` — shared scenarios and strict differential gates that compare the Satin
  engine with the Satin implementation under review (outputs produced by
  `reference/`).
- `reference` — its own workspace (separate lockfile); generates reference
  outputs from the Satin implementation under review.

## Building

The toolchain is pinned in `next/rust-toolchain.toml`.

```bash
cd next && cargo check --workspace
```
