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
- `scripts` — the CI setup the workflow calls: `fetch-evm2-fork.sh` (checks the
  pinned evm2 fork out at `evm2/`) and `install-zstd.sh`.

## Differential gates

`diff/scripts/gate.sh sample|full` runs the strict gates (synthetic scenarios, fuzz batches,
engine replay and block executor on recorded blocks) against the reference outputs and prints one
`GATE` summary line per gate.
`sample` uses the in-repo data under `diff/data` (see `diff/data/README.md`); `full` reads the
full corpus from `MEGA_EVM2_CORPUS_DIR`.
The accepted differences are listed in `diff/known-differences.toml`: by scenario for the synthetic gate, and by set, block, transaction index and hash for the engine replay gate.
An unlisted difference fails a gate, and so does a listed one that differs otherwise or no longer differs (a stale entry); the replay summary counts the accepted ones as `known=`.
A replay entry whose block a run does not include is not evaluated in that run; `gate.sh` still pins how many are, because the expected summary's `known=` is counted by `count_expected.py` from the same reference files.
Besides the open `p6_dupn` label question, four owner-accepted entries (2026-11-05) cover one inspector-only class: since evm2 #871, a transaction whose runtime gas phase runs out of gas reports one halted level-0 frame to the inspector, as go-ethereum's `haltTopFrame` does, where the reference opens none.
Every gate script extracts its inputs fresh into its own directory under `<target>/mega-evm2-diff/` at the start of a run, so different scripts can run at the same time and no run reuses an earlier extraction.
`diff/scripts/check-reference.sh sample` regenerates the sample's reference outputs with
`reference/` and checks they equal the stored ones.
`diff/scripts/regen-reference.sh [--check] <out-dir>` does it for the full corpus and, without `--check`, rewrites the sample and `diff/data/expected.env`; moving the reference pin requires running it (see `diff/data/README.md`).
`<out-dir>` must be outside the corpus: the script refuses a directory whose new archive would replace the corpus's, before it builds anything.

`gate.sh` and the other scripts refuse to run when a `MEGA_EVM2_*` variable other than `MEGA_EVM2_CORPUS_DIR` is set:
the official runner decides what is gated, so an inherited block selection or override cannot narrow a gate.
Running `cargo test -p mega-evm2-diff` directly with those variables stays available for debugging; a run with `MEGA_EVM2_BLOCKS`, `MEGA_EVM2_SETS` or `MEGA_EVM2_ONLY` prints `PARTIAL GATE ...` lines.
A synthetic gate also fails on a reference entry that no selected scenario has, except in a run that selects scenarios by name (`MEGA_EVM2_ONLY`).

The reference is pinned to mega-evm `5e3f30ad`, the head of the Satin review pull request (reachable through `refs/pull/409/head`).
Move the pin to a tag or a merged commit before that pull request's branch disappears, or the reference can no longer be built.

## Negative controls and fault injection

A green gate means something only if the gate goes red on a wrong engine or a wrong reference.
Two scripts show that it does, and CI runs both.

`diff/scripts/controls.sh sample|full` runs the negative controls listed in `diff/negative-controls.toml`.
Every fix the engine makes on top of stock evm2 sits behind a named switch, `off("<name>")` in `mega-evm2/src`, which `MEGA_EVM2_DISABLE=<name>` turns off (feature `compare` only); the variable takes a comma-separated list of names.
Each engine switch has a `[[control]]` table.
The gate harness's fixture choices that make the engine's inputs match the reference's are switches too, `off("<name>")` in `diff`, and each has a `[[harness]]` table.
Each control names one case (a scenario, a fuzz seed or a recorded block), the gate that runs it, and in `why` what the fix does.
The script runs the case with the switch on, where the gate must pass, and with the switch off, where the gate must report a difference for that case; a panic, a harness error or a failure for another reason is a miss.
`sample` runs the controls whose case is in the in-repo sample, `full` runs all of them on the full corpus.
The last line is `CONTROLS scope=<s> total=<n> caught=<n> missed=<m> baseline_failed=<b> unlisted=<u> stale=<t>`, and the script fails unless `missed`, `baseline_failed`, `unlisted` and `stale` are all 0 and every gate (main, fuzz, replay and block) ran at least one control (a gate that ran none is reported as `NO_CONTROL <gate>`), so a run that proves nothing cannot pass.
A switch in `mega-evm2/src` or `diff` without a table is `unlisted` and a table whose switch is gone from its source is `stale`, so a new `off("…")` in either without a manifest entry fails CI.
The source scan is textual: it finds `off("<name>")` with whitespace or line breaks inside the parentheses and an optional trailing comma, as rustfmt writes a broken call, but not a switch name built at run time.

To add a control for a new switch, find a case that differs with the switch off: run a gate test directly with `MEGA_EVM2_DISABLE=<name>` and look for `DIFFER` lines or a non-zero `differ=`.
Prefer a case of the sample, and add a `[[control]]` with the switch, the gate, the case, its scope and a one-line `why` (the manifest's header lists the keys).
Use `scope = "full"` only when no sample case catches the switch, and say so in `why`.

`diff/scripts/faults.sh` injects faults into a copy of the sample's reference data, one at a time, and runs the gate each one concerns through the code `gate.sh` accepts and rejects with; the repository is not modified.
The 29 faults (listed in `diff/scripts/faults.py`) are a representative set: for each gate a changed compared value, plus an inspected frame and a frame count (main), a pre-block call with its reads, its reference plain-vs-inspected result and its registry decision (replay), and block counters and post-state (block), a missing, duplicate or extra entry, a wrong expected count, a reference whose plain and inspected runs disagree, and an accepted difference of `known-differences.toml` missing (a stale entry, another field, another case; a replay entry that is stale, lists other frames or another transaction hash).
A fault counts as rejected only when the gate fails and its output shows the check the fault names, while the unmutated run's output does not.
The unmutated copy must pass every gate first (the baselines).
The last line is `FAULTS total=<n> rejected=<n> accepted=<a> baselines=<k> passed=<k>`, and the script fails unless `accepted` is 0 and every baseline passed.
When a gate gains a new kind of check, add a fault for it.

## Building

The toolchain is pinned in `next/rust-toolchain.toml`.
Local builds use a path dependency on a checkout of our thin evm2 fork next to this repository (`../evm2` from the repository root).

```bash
cd next && cargo check --workspace
```

## Benchmarks

`cargo bench -p mega-evm2-diff --bench engine` times a few fixed workloads, plain and inspected (the workloads are listed in `diff/benches/engine.rs`); it needs `zstd` for the recorded block.
Add `-- --save-baseline <name>` to save a run and `-- --baseline <name>` to compare a later run with it, with the same `CARGO_TARGET_DIR` and on the same machine.

## CI

`.github/workflows/next.yml` checks this workspace.
It does not build the root workspace.

`evm2-fork.env` is the only fork pin CI reads (`EVM2_FORK_REPO`, `EVM2_FORK_REV`).
Each commit on that pin is described in [`evm2-fork.md`](evm2-fork.md).
The repository must be public: the pin check fetches it over HTTPS without credentials, so a private fork fails it even when the commit is pushed.
The commit must be pushed to that repository: a red "Check out the evm2 fork at its pin" step means "fork pin not pushed", so push the commit or update `evm2-fork.env`.
The workflow checks this repository out at `mega-evm/` and that revision out at `evm2/` (by `scripts/fetch-evm2-fork.sh`), so `../../evm2` from `next/` resolves.
Local builds keep the path dependency above.
`diff/scripts/ci-local.sh` runs the check job's commands against that local checkout.

**check** runs on every pull request and on every push that touches `next/**` or the workflow file.
It installs the toolchain from `rust-toolchain.toml`, adds the `riscv64imac-unknown-none-elf` target, and caches the build.
It then runs `cargo fmt --all --check`, `cargo clippy --locked --all-targets -- -D warnings` for `mega-evm2` (without `--features compare`, then with it), `op-evm2`, and `mega-evm2-diff`, then `cargo test --locked -p mega-evm2 -p op-evm2`, a `no_std` check of `op-evm2` for `riscv64imac-unknown-none-elf`, `diff/scripts/check-regen-refusal.sh` (on a scratch corpus, `regen-reference.sh` refuses an output directory or archive path that would reach the corpus), `diff/scripts/gate.sh sample`, `diff/scripts/controls.sh sample` and `diff/scripts/faults.sh`.

**reference** runs every day, and on pull requests and pushes that touch what it reads: `next/reference/**`, `next/diff/data/**`, `next/diff/shared/**`, `next/diff/scripts/**`, `next/scripts/**`, `next/rust-toolchain.toml`, or the workflow file.
It runs `diff/scripts/check-reference.sh sample`, which rebuilds the Satin implementation under review from git and compares it with the stored sample.

**full** runs every day and from `workflow_dispatch`.
It downloads the corpus archive from the repository variable `MEGA_EVM2_CORPUS_URL`, or from the secret of the same name when the variable is unset, into a temporary directory.
The archive (tar.zst, tar.gz, tar.xz, tar.bz2, zip, or tar) must unpack to the layout in `diff/data/README.md`: a directory that contains `real-blocks/` and `reference/reference-outputs.tar.zst` (or the original `reference/q11-oracles.tar.zst`).
The job then runs `MEGA_EVM2_CORPUS_DIR=<that directory> diff/scripts/gate.sh full`, then `diff/scripts/controls.sh full` on the same directory.
When neither the variable nor the secret is set, the job prints a notice and succeeds without running the gate.
