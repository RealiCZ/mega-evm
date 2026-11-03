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
Local builds use a path dependency on a checkout of our thin evm2 fork next to this repository (`../evm2` from the repository root).

```bash
cd next && cargo check --workspace
```

## CI

`.github/workflows/next.yml` checks this workspace.
It does not build the root workspace.

`evm2-fork.env` is the only fork pin CI reads (`EVM2_FORK_REPO`, `EVM2_FORK_REV`).
The workflow checks this repository out at `mega-evm/` and that revision out at `evm2/`, so `../../evm2` from `next/` resolves.
Local builds keep the path dependency above.
`diff/scripts/ci-local.sh` runs the check job's commands against that local checkout.

**check** runs on every pull request and on every push that touches `next/**` or the workflow file.
It installs the toolchain from `rust-toolchain.toml`, adds the `riscv64imac-unknown-none-elf` target, and caches the build.
It then runs `cargo fmt --all --check`, `cargo clippy --locked --all-targets -- -D warnings` for `mega-evm2` (without `--features compare`, then with it), `op-evm2`, and `mega-evm2-diff`, then `cargo test --locked -p mega-evm2 -p op-evm2`, a `no_std` check of `op-evm2` for `riscv64imac-unknown-none-elf`, and `diff/scripts/gate.sh sample`.

**reference** runs every day, and on pull requests and pushes that touch `next/reference/**`, `next/diff/data/**`, or `next/diff/shared/**`.
It runs `diff/scripts/check-reference.sh sample`, which rebuilds the Satin implementation under review from git and compares it with the stored sample.

**full** runs every day and from `workflow_dispatch`.
It downloads the corpus archive from the repository variable `MEGA_EVM2_CORPUS_URL`, or from the secret of the same name when the variable is unset, into a temporary directory.
The archive (tar.zst, tar.gz, tar.xz, tar.bz2, zip, or tar) must unpack to the layout in `diff/data/README.md`: a directory that contains `real-blocks/` and `reference/q11-oracles.tar.zst`.
The job then runs `MEGA_EVM2_CORPUS_DIR=<that directory> diff/scripts/gate.sh full`.
When neither the variable nor the secret is set, the job prints a notice and succeeds without running the gate.
