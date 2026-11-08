# `next/` — the Satin engine workspace

This directory holds the next generation MegaETH execution engine, built on [evm2](https://github.com/alloy-rs/evm2) instead of revm.
The existing engine in `crates/` (REX1–REX6, built on revm) is frozen: it keeps shipping the current chains and is not modified by work done here.
`next/` is its own Cargo workspace with its own `Cargo.lock`, because the root workspace pins `alloy` 2.0.5 exactly while the evm2 line needs `alloy` 2.5.

## Layout

- `mega-evm2` — the Satin engine: Satin and later specs, resource metering, system contracts and block execution.
- `op-evm2` — a vendored OP Stack execution layer for evm2, so the engine does not depend on `op-revm`; `op-evm2/VENDORED.md` records its upstream base and every local change.
- `diff` — the strict differential gates that compare the engine with the Satin implementation under review, and the engine side of the two-sided probe; `diff/data/README.md` describes the in-repo sample and the full corpus.
- `diff/shared` — `mega-evm2-scenarios`, engine-neutral: the scenarios (the hand-written groups, the pre-frame sweep and the fuzz generator), the outcome record both harnesses render (`src/record.rs`) and the probe's input format (`src/probe.rs`).
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
bash diff/scripts/rule-coverage.sh <rules.md>                  # which rule ids of the specification have a scenario
bash diff/scripts/probe.sh diff/probe-examples.jsonl           # inputs of your own, on both sides
cargo bench -p mega-evm2-diff --bench engine                   # fixed workloads, plain and inspected
```

`gate.sh` prints one `GATE` line per gate (synthetic main scenarios, fuzz batches, engine replay and block executor on recorded blocks) and compares it with the expected counts.
Every gate script refuses to run when a `MEGA_EVM2_*` variable other than `MEGA_EVM2_CORPUS_DIR` is set, so an inherited selection cannot narrow an official run; a direct `cargo test -p mega-evm2-diff` with such a variable prints `PARTIAL GATE` lines and is for debugging.
Each script extracts its inputs afresh into its own directory under `<target>/mega-evm2-diff/`, so different scripts can run at the same time; do not run one script twice at once on the same cargo target.

## State and witness exports

`BlockExecutor::pre_block_states` retains each complete pre-block state in commit order, including read-only and absent-account entries.
The registry due-change read is a separate state before its optional application call; the L1 block account and its five slots are the last state.
A witness collector must consume these states and each transaction's `Executed::pending` before reducing them to changed entries.
A failed deposit keeps its loaded accounts and slots in that returned state while undoing every write except its sender's nonce bump and mint.

With feature `witness`, `BlockExecutor::bucket_reads` exports the union of valid SALT capacity lookups made by this block execution, including candidates that were dropped or aborted after an earlier successful lookup.
The set starts empty for each executor; committing a transaction executed elsewhere adds no lookup to it.
The per-transaction `Lite::bucket_reads` remains available for integrations that collect their own union.

## Coverage and mutation testing in CI

The `coverage` job of `.github/workflows/next.yml` runs `scripts/coverage.sh`: branch coverage (`cargo llvm-cov`) of `mega-evm2` and `op-evm2` under their own tests and the sample gate's test targets, which run every scenario, fuzz seed and recorded block of the sample through the engine.
The diff harness is only the driver and is left out of the report.
It writes `lcov.info`, `summary.txt` (a `TOTAL` line and per-crate line and branch totals) and an HTML report to `$COV_DIR`, uploads them as an artifact and `lcov.info` to Codecov with the flag `next`.
The upload needs the repository secret `CODECOV_TOKEN` and never fails the job, so a fork without the secret keeps the artifact only.

The `mutants` job runs `scripts/mutants.sh` with cargo-mutants (pinned in the workflow; configuration in `.cargo/mutants.toml`).
The oracle is `cargo test` over the engine crates and the harness with the sample gate's environment: the main scenarios, recorded-block replay and block execution (one synthetic run takes one reference file, so the fuzz batches are not part of it).
A mutant is caught when any unit test or gate turns red; a survivor is a scenario the gates lack, or code no input reaches.
On a pull request only the changed engine lines are mutated, and a survivor or timeout that `diff/mutants/suppressions.toml` does not list fails the job.
The daily run mutates the full engine in eight shards and reports without failing.
Locally, `bash scripts/mutants.sh smoke` runs a fixed set (the mutants of `mega-evm2/src/satin_ext.rs`) and writes `mutants.out/outcomes.json` under `$OUT`; `scripts/mutants.sh pr <base>`, `shard <k>/<n>` and `full` are the modes CI uses.

## The gates and their accepted differences

The gates run every scenario, fuzz seed and recorded transaction on both sides, plain and inspected, and compare every emitted field outside the exclusion list in `diff/src/gate.rs`, which states a reason for each entry.
The reference is a cross-check, not the definition of Satin: where the engine and the reference differ, the Satin specification (`docs/spec/upgrades/satin.md` at the reference's pinned commit) decides which side is wrong.
A fix on the reference's side is a change to the Satin implementation itself; until it is made, the gates hold the engine to the reference, except for the accepted differences below.
A difference fails a gate unless `diff/known-differences.toml` lists it: by scenario for the synthetic gate, and by set, block, transaction index and hash for the engine replay gate.
A listed entry that differs otherwise, or no longer differs, fails too; the summaries count the accepted ones as `known=`.
Each entry states its reason and whether the owner has signed it off.
Six classes of differences are accepted by their shape instead of case by case (`[[class]]` tables), because generated and probed inputs reach them under any name.
`halted_first_frame` (`gate::halted_first_frame`) is evm2 #871's halted first frame: a case without an entry of its own whose only difference is that the reference recorded no frame and the engine one halted level-0 frame, of a transaction that ran out of gas or is a failed deposit.
`invalid_immediate_label` (`gate::invalid_immediate_label`) is the halt label of DUPN, SWAPN and EXCHANGE with an immediate EIP-8024 rejects: a case whose only difference is `halt`, `OpcodeNotFound` against `InvalidImmediateEncoding`.
`dup_stack_label` (`gate::dup_stack_label`) is the halt label of a failed DUP (`DUP1` to `DUP16` or `DUPN` deeper than the stack): a case whose only difference is `halt`, `StackOverflow` against `StackUnderflow`.
Three more are the differences of three changes the owner decided on 2026-11-08 for both implementations, which the engine has made and the reference has not yet; each goes once the reference makes its change.
What such a change makes differ is the program's to decide, so each is checked the same way (`gate::scenario_verdict`): the case is of the class only when the engine, run again under the reference's rule (`Lite::reference_rule`, feature `compare` only), has no difference from the reference at all:
`auth_preflight_reads` (`SATIN-AUTH-005`): every EIP-7702 list is simulated whatever its length, so a list that runs out of gas before the first frame has loaded more authorities; rerun without the simulation of a list too short to cross a limit;
`modexp_formula_price` (`SATIN-PCMP-007`, `SATIN-DET-023`): ModExp charges its formula price before EIP-7823's bound, so an over-bound input with gas between the 500 minimum and the formula price is labelled `PrecompileOOG` against `PrecompileError`, and under gas detention it is priced by its formula against the reference's 500; rerun with the bound checked first;
`slotnum_zero` (`SATIN-BASE-005`): `SLOTNUM` pushes zero; rerun with `SLOTNUM` pushing the block environment's slot number.
The engine replay gate runs no rerun, so a case of these classes there differs.
Such a case is accepted and counted as `class=`; any other field that differs, or any other shape, fails it.
The `class=` counts are pinned like the others: `MAIN_EXPECT`, `SAMPLE_FUZZ_CLASS` and `FULL_FUZZ_CLASS` in `diff/scripts/common.sh`, and `count_expected.py` for the replay gate (an applied transaction no entry lists whose reference recorded no frame).

## Blocks of several transactions, simulation switches and rule ids

A scenario can be one transaction of a block (`Scenario::prior` in `diff/shared/src/lib.rs`): both harnesses run the earlier transactions of the block first, on one EVM, committing each outcome, and render the last one alone, with `block_accounts` and `block_storage`, the changes of the block so far.
A refused earlier transaction leaves nothing.
The database reads (`reads.accounts`, `storage`, `codes`, `block_hashes`) are recorded for a block's first transaction only, because the engine's state keeps what an earlier transaction loaded; the SALT buckets and the Oracle reads are per transaction on both sides.
`blk` holds hand-written blocks (`<name>_t1`, `_t2`, …) and fuzz mode 15 draws random ones.
`Scenario::skip_nonce_check` and `skip_eip3607` switch off the two checks the specification lets a node switch off in a simulation; the limits and the detention caps cannot be switched.
`Scenario::rules` lists the `SATIN-` rule ids a scenario exercises and is ignored by the comparison; `diff/scripts/rule-coverage.sh <rules.md>` reports which rule ids have a scenario.
A known difference with `field = "record"` pins both whole normalized records, for a case one side executes and the other refuses.

## Probing an input on both sides

`diff/scripts/probe.sh <file.jsonl>` runs inputs no gate holds on the reference and on the engine, and judges each as the synthetic gate does (`gate::scenario_verdict`, accepted differences included).
Each line of the file is a scenario by name (`base`: a hand-written scenario or a fuzz seed of any mode) with fields overridden, or a whole scenario; the keys are listed in `diff/shared/src/probe.rs`, and `diff/probe-examples.jsonl` has examples.
Per input it prints both records and `MATCH`, `KNOWN`, `CLASS <class>` or `DIFFER [<fields>]` with each differing field's two values; the exit status is non-zero when an input differs.
`MEGA_EVM2_DISABLE` turns engine switches off, as in the gates.
An input that finds a difference belongs in a scenario of the gates once the difference is understood.

## Switches and negative controls

Every fix the engine makes on top of stock evm2 sits behind a named switch, `off("<name>")` in `mega-evm2/src`; `MEGA_EVM2_DISABLE=<name>,…` turns it off, with feature `compare` only, and without that feature `off` is a constant `false`.
A fix no gate case can show (a database that fails a read, the Oracle read in a pre-block system call, a block without a parent beacon block root, the destruction table's rows for a slot whose original value is not zero, a call past the call-stack limit at an interceptor, the per-transaction limits and detention caps the engine fixes, SALT's production bucket mapping, the delegation indicator's state gas rebuilt from the cost per state byte, or the contents of returned witness states) has no switch; an engine test in `mega-evm2/tests` or a unit test proves it instead.
The fixes only a simulation switch reaches (a keyless deployment or a system sender with a validation switch off, a creation of type 0, 1 or 2 from a sender whose nonce cannot be bumped) have no switch either: their `port_*` scenarios, run with `skip_nonce_check` or `skip_eip3607`, differ when the fix is undone, and an engine test proves each as well.
A change the engine made that the reference has not made yet (an accepted class above) has no switch either: turned off, the engine would match the reference, which no control can catch; the class's pinned `class=` count catches the change being undone, and an engine test proves it.
The harness's fixture choices that make the engine's inputs match the reference's are switches too, `off("<name>")` in `diff`.

`diff/negative-controls.toml` names, for each switch, one gate case that must differ when the switch is off, and in `why` what the fix does.
`controls.sh` runs each case with the switch on (the gate must pass) and off (the gate must report a difference for that case); it fails on a miss, on a switch without a table or a table without a switch, and when a gate ran no control.
To add a control, run a gate test with `MEGA_EVM2_DISABLE=<name>`, pick a differing case (from the sample when one exists), and add its table; the manifest's header lists the keys.

## Fault injection

`faults.sh` injects faults into a copy of the sample's reference data, one at a time, and runs the gate each concerns; the repository is not modified.
The faults are a representative set, listed in `diff/scripts/faults.py`: for each kind of check a gate makes, one wrong value, entry, count or accepted difference.

A fault counts as rejected only when its gate fails and the output shows the check the fault names, while the unmutated run's output does not, and every test of the run completed: a run killed or crashed before its libtest result proves no check, whatever it printed (the fault `main-killed-after-differ` kills one after its difference to show that).
The unmutated copy must pass every gate first.
When a gate gains a new kind of check, add a fault for it.

## CI

`.github/workflows/next.yml` checks this workspace and does not build the root one; its header says which job runs when and what each runs.
It checks the evm2 fork out at the pin in `evm2-fork.env`; [`evm2-fork.md`](evm2-fork.md) says how to move that pin.
