# `next/` — the Satin engine workspace

This directory holds the next generation MegaETH execution engine, built on [evm2](https://github.com/alloy-rs/evm2) instead of revm.
The existing engine in `crates/` (REX1–REX6, built on revm) is frozen: it keeps shipping the current chains and is not modified by work done here.
`next/` is its own Cargo workspace with its own `Cargo.lock`, because the root workspace pins `alloy` 2.0.5 exactly while the evm2 line needs `alloy` 2.5.

## Layout

- `mega-evm2` — the Satin engine: Satin and later specs, resource metering, system contracts and block execution.
- `op-evm2` — a vendored OP Stack execution layer for evm2, so the engine does not depend on `op-revm`; `op-evm2/VENDORED.md` records its upstream base and every local change.
- `diff` — the strict differential gates that compare the engine with the Satin implementation under review, and the engine side of the two-sided probe; `diff/data/README.md` describes the in-repo sample and the full corpus.
- `diff/shared` — `mega-evm2-scenarios`, engine-neutral: the scenarios (the hand-written groups and the fuzz generator), the outcome record both harnesses render (`src/record.rs`) and the probe's input format (`src/probe.rs`).
  It depends on `alloy-primitives` only; `diff` and `reference` depend on it by path.
  It sits under `diff/shared` because CI's reference check runs on changes there.
  `diff/shared/blockdb.rs`, the recorded-block reader of both replay harnesses, is not part of it: both include it by `#[path]`, since it parses blocks with `serde_json`.
- `reference` — its own workspace (separate lockfile): generates the reference outputs from the Satin implementation under review, mega-evm `5e3f30ad`, which stays reachable through `refs/pull/409/head` because the repository forbids force-pushes; the pin moves only when the reference itself changes, with `regen-reference.sh`.
- `scripts` — CI setup: `fetch-evm2-fork.sh` checks the pinned evm2 fork out next to the repository, `install-zstd.sh`.
- `evm2-fork.env`, [`evm2-fork.md`](evm2-fork.md) — the evm2 fork pin, and what each fork patch adds, why, and when it goes.

## Commands

From `next/`, with the evm2 fork checked out at its pin next to this repository (`../evm2` from the repository root; the toolchain is pinned in `rust-toolchain.toml`):

```bash
cargo check --workspace
bash diff/scripts/ci-local.sh                                  # what CI's check job runs
bash diff/scripts/gate.sh sample                               # the gates on the in-repo sample
MEGA_EVM2_CORPUS_DIR=<corpus> bash diff/scripts/gate.sh full   # the gates on the full corpus
bash diff/scripts/controls.sh sample|full                      # negative controls
bash diff/scripts/faults.sh                                    # fault injection
bash diff/scripts/check-reference.sh sample                    # rebuild the reference, compare the sample
bash diff/scripts/probe.sh diff/probe-examples.jsonl           # inputs of your own, on both sides
cargo bench -p mega-evm2-diff --bench engine                   # fixed workloads, plain and inspected
```

`gate.sh` prints one `GATE` line per gate (synthetic main scenarios, fuzz batches, engine replay and block executor on recorded blocks) and compares it with the expected counts.
Every gate script refuses to run when a `MEGA_EVM2_*` variable other than `MEGA_EVM2_CORPUS_DIR` is set, so an inherited selection cannot narrow an official run; a direct `cargo test -p mega-evm2-diff` with such a variable prints `PARTIAL GATE` lines and is for debugging.
Each script extracts its inputs afresh into its own directory under `<target>/mega-evm2-diff/`, so different scripts can run at the same time; do not run one script twice at once on the same cargo target.

## The gates and their accepted differences

The gates run every scenario, fuzz seed and recorded transaction on both sides, plain and inspected, and compare every emitted field outside the exclusion list in `diff/src/gate.rs`, which states a reason for each entry.
A difference fails a gate unless `diff/known-differences.toml` lists it: by scenario for the synthetic gate, and by set, block, transaction index and hash for the engine replay gate.
A listed entry that differs otherwise, or no longer differs, fails too; the summaries count the accepted ones as `known=`.
Each entry states its reason and whether the owner has signed it off.
One class of differences is accepted by its shape instead of case by case, because the generators draw it: evm2 #871's halted first frame (`[[class]]` `halted_first_frame`, `gate::halted_first_frame`).
A case without an entry of its own whose only difference is that the reference recorded no frame and the engine one halted level-0 frame, of a transaction that ran out of gas or is a failed deposit, is accepted and counted as `class=`; any other field that differs, or any other frame shape, fails the case.
The `class=` counts are pinned like the others: `MAIN_EXPECT`, `SAMPLE_FUZZ_CLASS` and `FULL_FUZZ_CLASS` in `diff/scripts/common.sh`, and `count_expected.py` for the replay gate (an applied transaction no entry lists whose reference recorded no frame).

## Probing an input on both sides

`diff/scripts/probe.sh <file.jsonl>` runs inputs no gate holds on the reference and on the engine, and judges each as the synthetic gate does (`gate::scenario_verdict`, accepted differences included).
Each line of the file is a scenario by name (`base`: a hand-written scenario or a fuzz seed of any mode) with fields overridden, or a whole scenario; the keys are listed in `diff/shared/src/probe.rs`, and `diff/probe-examples.jsonl` has examples.
Per input it prints both records and `MATCH`, `KNOWN`, `CLASS <class>` or `DIFFER [<fields>]` with each differing field's two values; the exit status is non-zero when an input differs.
`MEGA_EVM2_DISABLE` turns engine switches off, as in the gates.
An input that finds a difference belongs in a scenario of the gates once the difference is understood.

## Switches and negative controls

Every fix the engine makes on top of stock evm2 sits behind a named switch, `off("<name>")` in `mega-evm2/src`; `MEGA_EVM2_DISABLE=<name>,…` turns it off, with feature `compare` only, and without that feature `off` is a constant `false`.
The harness's fixture choices that make the engine's inputs match the reference's are switches too, `off("<name>")` in `diff`.

`diff/negative-controls.toml` names, for each switch, one gate case that must differ when the switch is off, and in `why` what the fix does.
`controls.sh` runs each case with the switch on (the gate must pass) and off (the gate must report a difference for that case); it fails on a miss, on a switch without a table or a table without a switch, and when a gate ran no control.
To add a control, run a gate test with `MEGA_EVM2_DISABLE=<name>`, pick a differing case (from the sample when one exists), and add its table; the manifest's header lists the keys.

## Fault injection

`faults.sh` injects faults into a copy of the sample's reference data, one at a time, and runs the gate each concerns; the repository is not modified.
The faults are a representative set, listed in `diff/scripts/faults.py`: for each kind of check a gate makes, one wrong value, entry, count or accepted difference.

A fault counts as rejected only when its gate fails and the output shows the check the fault names, while the unmutated run's output does not; the unmutated copy must pass every gate first.
When a gate gains a new kind of check, add a fault for it.

## CI

`.github/workflows/next.yml` checks this workspace and does not build the root one; its header says which job runs when and what each runs.
It checks the evm2 fork out at the pin in `evm2-fork.env`; [`evm2-fork.md`](evm2-fork.md) says how to move that pin.
