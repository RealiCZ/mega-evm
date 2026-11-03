# Vendored: `op-evm2`

This crate is a vendored copy of the OP Stack execution layer for evm2.
The Satin engine (`next/mega-evm2`) builds on it instead of on `op-revm`.

## Upstream

- Repository: <https://github.com/foundry-rs/evm2-networks>, directory `crates/op` (package `op-evm2`).
- Base commit: `6dc9c0de2f02f963f57099668ba795d971976a01` (`main`, 2026-10-30).
  `crates/op` is the same at every commit from `c32df185a07c15b526d93acef168e02f76709618` (#10, which moved the deposit handler to the EVM-owned transaction environment) to this base.
  The two later commits on `main` (#13, #14) do not touch `crates/op`.
- The upstream tests are kept: the unit tests in `src/tests.rs` and the op-revm parity test in `tests/op_revm_parity.rs`.
  The parity test's dev-dependencies (`op-revm` at the revision Foundry pins, `revm` 43) are the same as upstream's.

## Local changes

Each line is one change against the base commit.

1. Fail a post-Regolith deposit whose top-level frame stops for any reason other than `REVERT` (for example `OutOfFunds`), as op-revm does; the parity test classifies outcomes at the transaction level and gains the two deposit cases that show it — upstream PR [foundry-rs/evm2-networks#12](https://github.com/foundry-rs/evm2-networks/pull/12) (commit `13e0995`), issue [#11](https://github.com/foundry-rs/evm2-networks/issues/11).
2. Make the handlers, the transaction registry and the precompile map generic over an `OpTypes` type family, so a chain built on the OP Stack reuses them with its own spec ids and EVM extension — not proposed upstream.
3. Export `prepare_deposit_tx`, `execute_deposit_tx` and `PreparedDeposit`, so a transaction that arrives in another envelope can run as a deposit (MegaETH's system-address transactions) — not proposed upstream.
4. Add a default `std` feature instead of the upstream workspace forcing `evm2/std` — not proposed upstream.
5. Build without `std`: `no_std` attribute, `alloc` imports, and a precompile map built per call where there is no `OnceLock` — not proposed upstream.
6. Set `type MessageHooks = ()` on `OpEvmTypes`, the associated type our thin evm2 fork adds for message hooks — fork-only; it goes away with the fork's message-hook patch.
7. Take `&self` in `PrecompileProvider::execute` for `OpPrecompiles`, as our thin evm2 fork's native precompile dispatch requires — follows [alloy-rs/evm2#523](https://github.com/alloy-rs/evm2/pull/523).
8. Packaging only: `Cargo.toml` uses the `next/` workspace dependencies (evm2 is the thin fork, not upstream's git pin), and carries upstream's workspace lints as package lints.

## Re-syncing

1. In a checkout of `foundry-rs/evm2-networks`, list what changed upstream since the base: `git log --oneline <base>..origin/main -- crates/op`.
2. Export that range as a patch and apply it here from the repository root: `git diff <base> <new> -- crates/op | git apply -p3 --directory=next/op-evm2 --3way`.
3. Resolve conflicts against the local changes above.
   Drop a local change once upstream has it (for example change 1 once #12 is merged), and remove its line here.
4. If upstream moved its evm2 pin, check that the thin fork has the API it needs; changes 6 and 7 follow the fork, not upstream.
5. If upstream changed the parity test's `op-revm` or `revm` versions, update them in `next/Cargo.toml`.
6. Run, from `next/`: `cargo test -p op-evm2`, `cargo clippy -p op-evm2 --all-targets -- -D warnings`, and `cargo check -p op-evm2 --no-default-features --target riscv64imac-unknown-none-elf`.
7. Update the base commit at the top of this file.
