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
- `reference` — its own workspace (separate lockfile): generates the reference outputs from the Satin implementation under review, mega-evm `4e39cfd6`, which stays reachable through `refs/pull/417/head` because the repository forbids force-pushes; the pin moves only when the reference itself changes, with `regen-reference.sh`.
- `scripts` — CI setup and gates: `fetch-evm2-fork.sh` checks the pinned evm2 fork out next to the repository, `install-zstd.sh` installs zstd, and `mega-eest-gate.sh` runs the pinned Mega EEST slice.
- `evm2-fork.env`, [`evm2-fork.md`](evm2-fork.md) — the evm2 fork pin, and what each fork patch adds, why, and when it goes.
- `mega-eest.env` — the Mega EEST repository and reviewed suite revision used by the conformance gate.

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
bash scripts/mega-eest-gate.sh                                 # Mega EEST slice and negative control
bash diff/scripts/rule-coverage.sh <rules.md>                  # which rule ids of the specification have a scenario
bash diff/scripts/probe.sh diff/probe-examples.jsonl           # inputs of your own, on both sides
cargo bench -p mega-evm2-diff --bench engine                   # fixed workloads, plain and inspected
```

`gate.sh` prints one `GATE` line per gate (synthetic main scenarios, fuzz batches, engine replay and block executor on recorded blocks) and compares it with the expected counts.
Every gate script refuses to run when a `MEGA_EVM2_*` variable other than `MEGA_EVM2_CORPUS_DIR` is set, so an inherited selection cannot narrow an official run; a direct `cargo test -p mega-evm2-diff` with such a variable prints `PARTIAL GATE` lines and is for debugging.
Each script extracts its inputs afresh into its own directory under `<target>/mega-evm2-diff/`, so different scripts can run at the same time; do not run one script twice at once on the same cargo target.
Building the reference (`check-reference.sh`, `regen-reference.sh`, `probe.sh`, `bench-blocks.sh`, `bench-engine.sh`) needs Foundry's `forge` on the path: its mega-evm dependency compiles the system contracts in its build script; CI's `reference` job installs it.

## Chain configuration, factory and block session

`mega_evm2::chain::ChainConfig` is the validated chain configuration every consumer builds the engine from: the chain id, the fork schedule, the genesis timestamp and the `SequencerRegistry`'s bootstrap values.
`ChainConfig::from_genesis_json` and `from_config_json` read the genesis file's text; `ChainConfig::new` validates an in-memory configuration the same way.
`satinTime` is read exactly from the JSON text (no floating-point step); a key starting with `satin` in any ASCII case that is not `satinTime` is refused; a repeated key is read as its last copy.
A configuration that schedules Satin must schedule the Mega ladder below it in non-decreasing order, have every base fork active at `satinTime` and carry non-zero `rex5InitialSequencer`, `rex5InitialAdmin` and `rex6MinRotationDelay`.
`ChainConfig::protocol` exports the protocol values every Satin block executes on, for readers that need them before execution.

`mega_evm2::factory::SatinFactory` makes every EVM the engine runs, with Satin's version, the configured chain id, the transaction limits, the transaction registry and the precompiles installed.
`SatinFactory::start_block` checks a block's input against its parent context and the schedule, derives whether the block is block zero, whether Satin activates in it and whether it admits only deposits, runs its pre-block sequence and returns a `BlockSession`; `SatinFactory::evm` makes a single transaction's EVM.
Simulations use the separately named `start_simulated_block` and `simulated_evm` with the switches of a `Simulation`.

A block's pre-block sequence (`mega-evm2/src/block/pre_block.rs`, `SATIN-PBLK-003`) runs, each step committed to the session before the next:

1. the EIP-2935 call;
2. the EIP-4788 call (block zero makes neither, and its parent beacon block root must be zero);
3. in the block in which Satin activates and no other, the check that the EIP-7997 factory holds its code hash;
4. the deployment of the Oracle, the High-Precision Timestamp, KeylessDeploy, MegaAccessControl and MegaLimitControl, each its own state: an absent one installed, one with other code given its code (nonce, balance and storage kept), a correct one read;
5. the registry's due-change read, and its `applyPendingChanges()` call when a change is due;
6. the registry's deployment: version 2.0.0 left as it is, 1.0.0 migrated writing slot 13 alone, an absent account or one without code seeded from the bootstrap values (an existing account's storage cleared), other code an invalid block;
7. the resolution of the block's live system address from the registry's account and slot 0, committed last.

No step reads the L1 block contract: a transaction reads it when it needs it (see "State and witness exports").

Every transaction of the block is classified against that address (`BlockSession::system_address`, `Lite::system_address`); none reads the registry, or finds its account or slot warm, for its classification.
A single transaction's EVM (`SatinFactory::evm`, `simulated_evm`) resolves the address once, when it is made, by the same direct read of the registry's account and slot 0 off the state it runs on (no warming, no charge), so a call or an estimate prices a transaction as its block would; outside a block a registry that is absent, holds other code or names no address leaves no system address, and an embedder that knows the block's resolved address may set `Lite::system_address` instead.
The gates count that read in a single transaction's read set, where the reference's transaction reads the same account and slot as it is classified.
A pre-block call runs from `SYSTEM_CALLER` on 30,000,000 of regular gas and a reservoir of 1,566,720, whatever the block's gas limit; the session's inspector observes it.
The system contracts' code hashes and bytecode are in `mega-evm2/src/system_contracts.rs`.

Every EVM reads SALT's bucket capacities from a `mega_evm2::SaltSnapshot` its caller gives it: `BlockServices::salt` for a block session, an argument of `SatinFactory::evm` and `simulated_evm` for a single transaction.
A snapshot is the node's fallible reader of one immutable state, the block's parent state, and names it (`SnapshotId`: the parent's hash and its state root); `start_block` refuses a snapshot of another parent as an internal error, so the same height under another parent is another snapshot.
A capacity the snapshot cannot give is an error (`SaltReadError`), never a default: the transaction that asked has no outcome and the block fails with an internal error, whatever the snapshot's backend wraps.
`SaltCapacities` is an in-memory snapshot; `SaltCapacities::new` refuses a bucket it does not list, as production does, and only `SaltCapacities::test_minimum` (policy `Unlisted::TestMinimum`) answers one with the minimum capacity, for tests and tools.
The harness and the engine tests run every recorded block and every scenario on that named test policy, plus the capacities a scenario lists.
Each attempt (one execution of a transaction) reads a bucket at its first state charge there, refuses a capacity below 256, and keeps `floor(capacity / 256)` for every later charge and give-back in the bucket, across frame reverts, until the next attempt; a system-originated transaction and a system call price at `m = 1` and read no capacity.
Errors are typed (`mega_evm2::error`): each block-level error carries its phase, its context and what it decides about the block; a session that meets one that decides the block executes nothing after it and cannot finish.

## The Oracle seam

Every EVM is connected to the node's oracle service and an Oracle observer its caller gives it (`mega_evm2::oracle::OracleServices`): `BlockServices::oracle` for a block session, an argument of `SatinFactory::evm` and `simulated_evm` for a single transaction.
Both are optional and separate: a node that validates or replays a block, or answers an RPC call, connects neither, and every Oracle read then pushes the slot's value on the chain; a builder may observe without a service.
`OracleService` is synchronous and infallible.
Its `answer(slot)` is asked at every Oracle read that asks it, each time, so two reads of one slot may push two values; `None`, a failed service's answer included, pushes the slot's value (`SATIN-ORCL-002`).
No read of a pre-block system call, a deposit or a Mega System Transaction asks it (`SATIN-ORCL-005`).
Its `hint(from, topic, data)` is called as an admitted `sendHint` is counted, before the call's frame start (`SATIN-ORCL-006`, `SATIN-ORCL-007`, `SATIN-ICPT-007`): a hint made before a read reaches the service before that read asks.
An admitted input that does not decode is counted and reaches no service; the engine keeps no hint.
`OracleObserver::observe` is handed each execution's Oracle accesses in execution order, with the execution's boundary: a pre-block system call, or a transaction with an index and whether it has an outcome.
Each read carries its slot, the value it pushed, the value the slot held through the journal (`held`, what a node with no service pushes there), whether that value is a write of the execution's own still in effect at the read (`own`), whether the service gave the value, and whether it was pushed (a read whose frame cannot pay its cold access is made and pushes nothing).
Each completed `SSTORE` to the Oracle's storage that stands is reported in its place, one writing the value the slot holds included.
Reads are kept whatever their frame does afterwards, with what they saw when they were made, and a write whose frame, or any frame above it, fails is never reported; a block's pre-block deployment of the Oracle is no execution and is not reported.
The builder decides each read on its own, with no look at the writes: a pushed read with `own` must have `value` equal to `held`, or the transaction is left out, and nothing is persisted for it; for the pushed reads without `own` the slot must hold `value` before the transaction, so a served value that differs from `held` is persisted before it (`SATIN-ORCL-012`), and those of one slot must all agree, or the transaction is left out (`SATIN-ORCL-013`); a read that pushed nothing asks nothing.
A single transaction's EVM hands a transaction over at its outcome and hands over none without one, its caller having the error.
A block session hands each attempt over during the `execute_candidate` call that ran it, once the attempt has ended: as executed when the call returns a candidate, which makes it that candidate's record, and as not executed, with none of its writes, when the call returns an error.
Its index is the number of transactions the executing session had committed, so it does not identify a candidate: a candidate executed again, or one executed before another is committed, carries the same index.
Connect each session to its own observer: the boundary names no session, because an observer object can already carry whatever its caller needs to know about its session.
A candidate executed before a Mega System Transaction that writes a slot it read must be executed again after it before it is committed: the commit does not check the Oracle slots a candidate read.
The observer changes nothing the engine executes, and no outcome, receipt or block carries what it saw; nothing is recorded without one.
A builder's service has duties the engine does not take on, listed on `OracleService`: interior mutability behind `&self`, since a hint can change later answers; one handle per session, which the builder tells where each attempt begins and ends, since the engine says nothing about which execution asks; answers pinned for a re-execution, so the values a Mega System Transaction persisted are answered again; and acceptance of hints that arrive outside any attempt (pre-block system calls, deposits, Mega System Transactions, re-executions).
Neither trait requires `Send` or `Sync`, as `SaltSnapshot` does not: a session and its services are built on the thread that runs them.
An RPC call that connects the node's service also hands it the call's hints, which pay nothing; a caller that wants answers without hints wraps its service in an adapter that keeps the default `hint`.
`OracleAnswers` answers from fixed values (the differential scenarios' answers), and `RecordingOracle` records every call it passes on (the harness lists a scenario's hints from it); both are for tests and tools.

## State and witness exports

`BlockSession::pre_block_states` retains each complete pre-block state in commit order, including read-only and absent-account entries.
The states follow the sequence above: each deployment is a state of its own, a read-only one included; the registry due-change read is a separate state before its optional application call; the resolution's registry account and slot 0 are the last state.
`BlockSession::tx_states` retains each committed transaction's complete state in commit order: every account and slot it loaded, in a frame that reverted, halted or was stopped too, the authorities its authorization preflight consulted, and, for a failed deposit, its loads while every write except its sender's nonce bump and mint is undone.
`BlockOutput` exports both.
The change set (`BlockSession::changes`) is the block's state transition: a registry seeded over an account without code clears that account's earlier storage there, which its pre-block state does not mark.
A witness collector consumes these states, not the change set, which holds no read-only entry.

The L1 block contract is read outside any transaction's journal, by two reads (`mega-evm2/src/l1_reads.rs`, `SATIN-WIT-011`): the fee computation of a non-deposit transaction that pays an L1 fee reads its account and slots 1, 3, 7, 8 and, when the Ecotone scalars are empty, 5, once per block, into a cache that keeps the keys it was loaded from; and the block reads the account and slot 8, the footprint scalar, before every non-deposit transaction, a Mega System Transaction included.
An absent contract is recorded as absent, with no slot; a block of deposits reads none of it.
`Candidate::l1_reads` holds what its attempt read, the cache's source keys included when the cache served it; its commit adds every key no earlier commit reported to its state as a read-only entry, never replacing the transaction's own entry for the account or a slot, so the first included transaction that read a key reports it and a dropped candidate reports nothing.
A failed fee read is the transaction's (internal or an invalid block, by the database's fatality); a failed footprint read is the block's own, always internal (`SATIN-VAL-010`).

`BlockSession::bucket_reads` is the union of the valid SALT lookups of every attempt the session made, including candidates that were dropped or aborted after an earlier successful lookup; it is always recorded, and `BlockOutput::bucket_reads` exports it with the snapshot's id.
A failed lookup is never in it.
Each `Candidate::bucket_reads` holds its own attempt's lookups, and `BlockSession::last_attempt_bucket_reads` the last attempt's, also after a failure.
The set starts empty for each session; committing a transaction executed by another session of the same block adds no lookup to it.
That session hands its lookups over explicitly: `BlockSession::drain_bucket_reads` returns them as a `SaltExport` and empties its set, and `merge_bucket_reads` takes them into the building session, refusing an export of another block or snapshot.

`BlockSession::block_hash_reads` is every block hash `BLOCKHASH` was served during the block, by block number: in a pre-block call and in every attempt the session made, including candidates that were dropped or failed after the read; `BlockOutput::block_hash_reads` exports it.
The engine's own `BLOCKHASH` records each hash where the host serves it, above the EVM's overlay and any state cache under it, so a hash a cache answers is in the export and a hash a cache holds from an earlier block is not.
A `BLOCKHASH` of the current block, a future block or a block older than the last 256 reads nothing and is not recorded; neither is one refused in a frame whose volatile-data access is off, nor one the database failed to answer.
Each `Candidate::block_hash_reads` holds its own attempt's reads, and `BlockSession::last_attempt_block_hash_reads` the last attempt's, also after a failure.
The export starts empty for each session, and is handed over as the SALT lookups are: `BlockSession::drain_block_hash_reads` returns it as a `BlockHashExport`, and `merge_block_hash_reads` takes one into the building session, refusing an export of another block and one that holds another hash for a block the session was served.
A single transaction's EVM keeps the reads of its last transaction in `Lite::block_hashes`, `Lite::salt` and `Lite::l1_reads`, emptied when the next one begins: every engine handler begins its attempt (`Lite::begin_attempt`) before any check that can refuse the transaction, so a refused transaction leaves no record of the one before it; a transaction of a type the EVM has no handler for is refused before any engine code runs, and leaves them as they were.

`mega_evm2::witness` builds a block's witness and replays the block on it (`SATIN-WIT-016`).
`WitnessKeys::of` takes the keys from a finished block's pre-block and transaction states and its exports alone, so a node's caches change none of them.
`Witness::resolve` resolves them against the block's parent state and SALT snapshot: an absent account as absent with no slot, an existing empty account as a leaf, every slot of an existing account with its value (zero for an empty one), and every existing account's code as the parent holds it, whether or not the block loaded it — never the code a later state carries after a redelegation.
A validator fills a `Witness` from the proofs it verified instead, and replays the block through `WitnessDb`, a database that answers exactly the witness, with the `Witness` itself as the SALT snapshot and no Oracle service.
A read of anything the witness does not hold fails with `MissingEvidence`, a fatal error, so the block fails as an internal error of the node, never an invalid block; `MissingEvidence::of` names the missing piece from the block's error, a SALT capacity included.
A replay's exports are a subset of the building node's, a proper one when the node dropped candidates.
The fork's `EmptyDB`, which answers any number with a hash it makes up, is not a witness database.

## Coverage and mutation testing in CI

The `coverage` job of `.github/workflows/next.yml` runs `scripts/coverage.sh`: branch coverage (`cargo llvm-cov`) of `mega-evm2` and `op-evm2` under their own tests and the sample gate's test targets, which run every scenario, fuzz seed and recorded block of the sample through the engine.
The diff harness is only the driver and is left out of the report.
It writes `lcov.info`, `summary.txt` (a `TOTAL` line and per-crate line and branch totals) and an HTML report to `$COV_DIR`, uploads them as an artifact and `lcov.info` to Codecov with the flag `next`.
The upload needs the repository secret `CODECOV_TOKEN` and never fails the job, so a fork without the secret keeps the artifact only.

The `mutants` job runs `scripts/mutants.sh` with cargo-mutants (pinned in the workflow; configuration in `.cargo/mutants.toml`).
The oracle is `cargo test` over the engine crates and the harness with the sample gate's environment: the main scenarios, recorded-block replay and block execution (one synthetic run takes one reference file, so the fuzz batches are not part of it).
A mutant is caught when any unit test or gate turns red; a survivor is a scenario the gates lack, or code no input reaches.
On a pull request only the changed engine lines are mutated, and a survivor or timeout that `diff/mutants/suppressions.toml` does not list fails the job.
Pushes to `cz/feat/satin-evm2`, the daily run and manual runs mutate the full engine in eight shards and report survivors and timeouts without failing.
Locally, `bash scripts/mutants.sh smoke` runs a fixed set (the mutants of `mega-evm2/src/satin_ext.rs`) and writes `mutants.out/outcomes.json` under `$OUT`; `scripts/mutants.sh pr <base>`, `shard <k>/<n>` and `full` are the modes CI uses.

## Comparing interpreter performance

Run from `next/`:

```bash
bash diff/scripts/bench-blocks.sh quick
MEGA_EVM2_CORPUS_DIR=<corpus> bash diff/scripts/bench-blocks.sh full
bash diff/scripts/bench-engine.sh
python3 diff/scripts/bench-summary-test.py
```

Both benchmark scripts build the engine and the pinned reference in release mode in separate workspaces.
The reference builds under `<target>/mega-evm2-reference`; set `CARGO_TARGET_DIR` to choose the target directory.
These reference measurements come from this harness, and are not official results from the other implementation's team.
Neither side enables JIT or AOT.

`bench-blocks.sh` uses the gates' exact inputs: 20 blocks in `quick`, 263 in `full`.
Before timing, it regenerates the reference's block outcomes and compares them with the engine using the existing block gate, including receipts, counters and post-state under that gate's documented normalization policy.
It loads the blocks and their exact transaction envelopes into memory and decodes all transactions before measuring.
Every iteration clones fresh initial database contents and constructs an executor outside the timer.
The timer includes pre-block execution, all attempted transactions, receipt construction and state commits; it excludes file access, JSON rendering, envelope decoding, initial database cloning, executor construction and final destruction.
Both sides start each block with cold execution state and their existing in-memory database adapters; bytecode analysis and caching during execution remain part of the measurement.
These are harness measurements, including adapter overhead, rather than estimates of a production node's throughput.

One warm-up pass precedes seven measured passes; `BENCH_RUNS` changes the measured count, with a minimum of three.
The total median and p95 describe complete corpus passes, summing corresponding repetitions across blocks.
The per-block median and p95 describe the distribution of each block's median time.
Spread is reported as total minimum and maximum, and gas/s uses included transactions' cumulative receipt gas divided by the median total time.
Transaction counts include refused transactions, whose attempted execution also consumes time.
The script checks that block identities and transaction counts agree across the timed sides, and inclusion, refusals and receipt gas across each side's repetitions; across the sides those are each side's own, which the block gate that ran first holds to each other, an accepted class's difference included (a deposit's history gas can leave a later transaction without room in the block's gas).
It ends with `BENCH blocks=<n> txs=<n> engine_ms=<total> reference_ms=<total>`.
Raw timings and gate logs remain under `<target>/mega-evm2-diff/bench-sample` or `bench-full`.
`BENCH_ORDER=reference-first` reverses the default timing order for a follow-up run.

`bench-engine.sh` runs the six shared synthetic workloads and the fixed 24-transaction block, plain and inspected, on both implementations.
It checks synthetic outcomes using the existing scenario comparison before engine measurement; the block is covered by the block script's gate.
Criterion excludes per-iteration setup for synthetic workloads and includes execution and state commit on both sides.
The engine commits to its accepted state overlay; the reference commits to its in-memory database.
The fixed-block workload includes executor construction and transaction decoding, matching the original engine benchmark.
Criterion's samples and estimates remain in each target directory's `criterion/` directory.
Extra arguments are forwarded to Criterion, for example `bash diff/scripts/bench-engine.sh --save-baseline comparison`.
Both scripts print load averages before and after each side; run without competing jobs and repeat measurements taken under load.

## The gates and their accepted differences

The gates run every scenario, fuzz seed and recorded transaction on both sides, plain and inspected, and compare every emitted field outside the exclusion list in `diff/src/gate.rs`, which states a reason for each entry.
The reference is a cross-check, not the definition of Satin: where the engine and the reference differ, the Satin specification (`docs/spec/upgrades/satin.md` at the reference's pinned commit) decides which side is wrong.
A fix on the reference's side is a change to the Satin implementation itself; until it is made, the gates hold the engine to the reference, except for the accepted differences below.
A difference fails a gate unless `diff/known-differences.toml` lists it: by scenario for the synthetic gate, and by set, block, transaction index and hash for the engine replay gate.
A listed entry that differs otherwise, or no longer differs, fails too; the summaries count the accepted ones as `known=`.
Each entry states its reason and whether the owner has signed it off.
Twelve classes of differences are accepted by their shape instead of case by case (`[[class]]` tables), because generated and probed inputs reach them under any name.
`halted_first_frame` (`gate::halted_first_frame`) is evm2 #871's halted first frame: a case without an entry of its own whose only difference is that the reference recorded no frame and the engine one halted level-0 frame, of a transaction that ran out of gas or is a failed deposit.
`invalid_immediate_label` (`gate::invalid_immediate_label`) is the halt label of DUPN, SWAPN and EXCHANGE with an immediate EIP-8024 rejects: a case whose only difference is `halt`, `OpcodeNotFound` against `InvalidImmediateEncoding`.
`dup_stack_label` (`gate::dup_stack_label`) is the halt label of a failed DUP (`DUP1` to `DUP16` or `DUPN` deeper than the stack): a case whose only difference is `halt`, `StackOverflow` against `StackUnderflow`.
Two more are the differences of two changes the owner decided on 2026-11-08 for both implementations, which the engine has made and the reference has not yet; each goes once the reference makes its change.
What such a change makes differ is the program's to decide, so each is checked the same way (`gate::scenario_verdict`): the case is of the class only when the engine, run again under the reference's rule (`Lite::reference_rules`, feature `compare` only), has no difference from the reference at all, or only the shape of one of the three classes above (a failed deposit's halted first frame, for one):
`auth_preflight_reads` (`SATIN-AUTH-005`): every EIP-7702 list is simulated whatever its length, so a list that runs out of gas before the first frame has loaded more authorities; rerun without the simulation of a list too short to cross a limit;
`modexp_formula_price` (`SATIN-PCMP-007`, `SATIN-DET-023`): ModExp charges its formula price before EIP-7823's bound, so an over-bound input with gas between the 500 minimum and the formula price is labelled `PrecompileOOG` against `PrecompileError`, and under gas detention it is priced by its formula against the reference's 500; rerun with the bound checked first.
A third, `reservoir_give_back` (`SATIN-HIST-012`, `SATIN-POOL-014`, `SATIN-STATE-012`), is the owner's decision of 2026-11-09, checked the same way: every give-back of state or history gas — a refill, the destruction table's, a start's charge given back, a failed frame's rollback — credits a reservoir and never regular gas, so a frame's regular gas rises only by what a child returns, and a fresh slot, a new account and a creation's account take a flat legacy share from regular gas first (10,000, 25,000, 16,000 for a `SELFDESTRUCT`'s beneficiary, 20,000, `satin_version`); the evm2 fork runs that rule when the version has `EvmFeatures::EIP8037_RESERVOIR_REFILLS` and the four legacy-share gas parameters ([`evm2-fork.md`](evm2-fork.md)), and the rerun takes both away (`satin_version_under`), which is EIP-8037's pinned rule: a give-back credited to regular gas first, up to the state gas that gas paid.
Gas detention, `remainingComputeGas()` and a keyless deployment's `gasUsed` follow the rule the version runs.
Six more are the owner's decisions of 2026-11-09 that bring Satin's system-contract and block rules back to Rex6's, checked the same way (each class's `why` in `known-differences.toml` states its change; the `par` scenarios exercise the first three with their rule ids, the engine's `mst_packing` test the fourth, the recorded blocks the fifth, and every gate the sixth):
`deposit_history` (`SATIN-HIST-016`, `SATIN-HIST-022`, `SATIN-DEP-001`): deposits and system-originated transactions pay history gas — the body's in a deposit's intrinsic validation, its start record before the first frame — and a failed deposit reports its body and calldata as history bytes; rerun with them exempt;
`oracle_read_before_charge` (`SATIN-ORCL-004`): an Oracle `SLOAD` loads, asks and marks before its cold access, `COLD_SLOAD_COST` with its warm base, is charged; rerun with the reference's order: the warm base first, as the opcode's static gas, then the rest of the cold access checked before the read, and the read marked once both are paid;
`refusal_costs_nothing` (`SATIN-MAC-003`, `SATIN-MAC-004`, `SATIN-MAC-007`): a refused volatile read reverts on the gas the frame held when the opcode was reached, before its static gas and its body, a call target's delegate read for the check; rerun with the static gas first and the refusal at the load;
`mst_packing_budgets` (`SATIN-BLK-007`): a Mega System Transaction is held to the block's packing budgets; rerun with it exempt (a block's only);
`system_contract_deployment` (`SATIN-SYSC-003`, `SATIN-REG-008`): every block deploys the system contracts before its transactions, so a recorded block of an earlier spec, whose state holds an earlier Oracle or lacks a later contract, gets them; rerun without the deployments (a block's only);
`once_per_block_resolution` (`SATIN-REG-002`): the live system address is resolved once, by a block or a single transaction's EVM, and no transaction finds the registry warm from its classification, so a transaction that reads the registry, a Mega System Transaction among them (its Oracle method checks its caller against the registry), pays the registry's cold access; rerun with every transaction reading the registry as it is classified.
Four of the decided changes — `SLOTNUM` pushing zero (`SATIN-BASE-005`), a `sendHint` admitted whether it carries value or not (`SATIN-ORCL-006`), no Oracle read of a deposit or a Mega System Transaction asking the service (`SATIN-ORCL-005`), and a keyless deployment recognized only once its arguments decode (`SATIN-ICPT-006`) — the reference has made (mega-evm `4e39cfd6`), so the gates compare them directly and no class accepts them.
A case that meets several of the decided changes at once (a ModExp call in a frame that later reverts, a deposit whose Oracle read cannot pay its cold access, a deposit that gives a slot's charge back) is of one of their classes when every enabled class's rule, run at once, leaves no difference (`gate::rerun_class`): a rule a case does not meet changes nothing in its run, so that run is the run under the rules it meets; the case is the first class's whose rule that run cannot do without, the give-back rule's last. The give-back rule is a setting of the version, the others of `Lite::reference_rules`, so they combine.
The engine replay gate runs a transaction again under the classes recorded blocks reach (`gate::REPLAY_RERUN_CLASSES`: the give-back rule, a reentrancy lock set and cleared giving its record back; deposits' history, every block opening with a deposit; the once-per-block resolution of the live system address; and, for blocks, a Mega System Transaction's packing budgets and the system contracts' deployment, `gate::BLOCK_RERUN_CLASSES`); a case of the other classes there differs.
The block executor gate runs a block that differs again, whole, under those rules, and takes it only when that run, plain and inspected, has no difference from any of the reference's rows (its transactions, counters and post-state): a transaction that ran close to its gas can end otherwise under the new rule, a deposit's gas used moves the block's cumulative gas, and every later row of its block moves with it.
Such a block's rows count in the `GATE block` line as the rerun's verdicts; the number of blocks it took is pinned on its own (`SAMPLE_BLOCK_RERUN_CLASS`, `FULL_BLOCK_RERUN_CLASS` in `common.sh`, checked by the block gate's test).
Such a case is accepted and counted as `class=`; any other field that differs, or any other shape, fails it.
No rerun is made for a case whose own records have a fault (`gate::candidate_differences`, and the replay and block gates' checks of the same): an engine record without exactly the fields the engine renders, or the engine's plain and inspected records disagreeing; a rerun stands in for the comparison with the reference, never for the engine's own checks, and such a case differs.

What the summaries certify for `reservoir_give_back`: a `class=` case is one whose engine records are complete and agree, plain and inspected, and whose every difference from the reference is the old rule's, since the engine run under EIP-8037's rule matches the reference in every field.
That proves the change is confined to the give-back rule and the legacy share; it says nothing of the new rule's own figures, which the reference cannot check.
Those rest on the regular-gas invariant and on tests.
The invariant (`mega-evm2/src/regular_check.rs`, feature `compare`; `diff/src/invariant.rs`) is checked on every case of the four gates, on the engine's own runs under Satin's rule, plain and inspected (not on a rerun): within a frame regular gas, the withheld part included, never rises, except when a child returns, and then by at most the unused regular gas the child returned.
With it the transaction's gas is conserved: a frame's whole gas — its regular gas, its reservoir and the state and history gas it holds charged, signed, since a give-back for a charge another frame made moves gas between frames — never rises but by what a returning child hands back; a returning message, a frame or an answer, hands back at most the forward and the reservoir share it received, and a halt at most the share; a message no frame ran for hands back no more regular gas than it was forwarded, and a call answered without running anything (an interceptor's answer, a call of an account with no code, a start evm2 refused) exactly that (`SATIN-ICPT-004`), and no more reservoir than its share, so no such message converts one pool into the other; and the transaction's give-backs never exceed the state and history charges it made, those made before its first frame included.
It is observed when a frame is built, after every instruction the engine defines (each state-gas site, call, creation and volatile read among them), once a call, creation or keyless deployment has taken its forward and the child's share, once a returning child has merged back into its caller (against what its end was checked with), when a code deposit is admitted, and when the frame's message ends, after its settlement; between those points only evm2's stock instructions run, none of which gives gas back.
A violation puts the field `regular_gas_invariant` in the case's engine record, so the case differs whatever its class and no rerun waives it.
`gate.sh` sums the gates' counts into its last line, `INVARIANT regular-gas scope=<sample|full> checked=<cases> violations=0`, and fails on a violation or a gate without a count.
The tests pin the rule's worked figures: `mega-evm2/tests/give_back.rs` and `migration_checks.rs`, `diff/tests/give_back.rs`, and the evm2 fork's tests of the rule ([`evm2-fork.md`](evm2-fork.md)).

A rerun proves a class's case differs from the reference only by the old rule; it cannot check the new rule's own figures, and an engine defect confined to a decided change's own path passes it whenever the reference's old rule happens to agree.
Two things hold that path instead.
First, the block gate holds the engine's own run of every recorded block under Satin's rules, the run a rerun replaces in the comparison, to figures pinned per block in `diff/data/block-satin.txt`: the transactions it included and refused, and a digest of the run — the keccak of each transaction's status and gas used, the block counters row and the post-state row — so a change of a recorded transaction's gas, of the block's counters or of its post-state is seen even when no inclusion changes; printed as `GATE block-satin blocks=<b> included=<i> refused=<r> pinned=<b> digests=<b> differ=0`, a block whose run differs from its counts or its digest, or that has no figure, fails the gate (the faults `block-satin-pinned` and `block-satin-digest` show it).
Second, an engine test holds each decided change's own figures:

| Class                        | What the differential cannot see                                                                        | Engine test                                                                                                                                             |
| ---------------------------- | ------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `reservoir_give_back`        | the new rule's credits, shares and pools                                                                | `mega-evm2/tests/give_back.rs`, `migration_checks.rs`, `diff/tests/give_back.rs`, and the regular-gas invariant on every gate case                      |
| `deposit_history`            | a deposit's and a Mega System Transaction's history gas and bytes                                       | `mega-evm2/tests/deposit_history.rs`, `system_origin.rs`; for recorded blocks, `GATE block-satin`                                                       |
| `oracle_read_before_charge`  | the read made and marked before `COLD_SLOAD_COST`, its warm base included                               | `mega-evm2/tests/oracle_reads.rs` (the mark at the compute before the 2,100, a read below the warm base, a looping caller stopped at the compute limit) |
| `refusal_costs_nothing`      | the gas a refused read keeps                                                                            | `mega-evm2/tests/access_disabled_call.rs`, `access_disabled_delegate_read.rs`                                                                           |
| `mst_packing_budgets`        | a Mega System Transaction held to the packing budgets (no gate case reaches them)                       | `mega-evm2/tests/mst_packing.rs`                                                                                                                        |
| `system_contract_deployment` | each deployment's installed code, kept nonce, balance and storage, the registry's seeding and migration | `mega-evm2/tests/block_lifecycle.rs`, `pre_block_witness.rs`; for recorded blocks, `GATE block-satin`                                                   |
| `once_per_block_resolution`  | the resolution's own reads, and a transaction's cold first access to the registry                       | `mega-evm2/tests/block_lifecycle.rs`, `mandatory_read_errors.rs`; for recorded blocks, `GATE block-satin`                                               |
| `auth_preflight_reads`       | the authorities a list too short to cross a limit loads                                                 | `mega-evm2/tests/authorization_preflight.rs`                                                                                                            |
| `modexp_formula_price`       | ModExp's formula price charged before EIP-7823's bound                                                  | none in the engine: the evm2 fork runs upstream evm2's ModExp unchanged, and upstream's tests hold it                                                   |

The `class=` counts are pinned like the others: `MAIN_EXPECT`, `SAMPLE_FUZZ_CLASS` and `FULL_FUZZ_CLASS` in `diff/scripts/common.sh`, and for the replay gate `count_expected.py` (an applied transaction no entry lists whose reference recorded no frame) plus `SAMPLE_REPLAY_RERUN_CLASS` and `FULL_REPLAY_RERUN_CLASS` in `common.sh`, the transactions the replay gate's rerun takes, which `gate_inputs` moves from `match` to `class`.

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
A fix no gate case can show (a database that fails a read, the Oracle read in a pre-block system call, a block without a parent beacon block root, the destruction table's rows for a slot whose original value is not zero, a call past the call-stack limit at an interceptor, the per-transaction limits and detention caps the engine fixes, SALT's production bucket mapping, the delegation indicator's state gas rebuilt from the cost per state byte, a value call past the call-stack limit, which makes no record and is charged none, or the contents of returned witness states) has no switch; an engine test in `mega-evm2/tests` or a unit test proves it instead.
The fixes only a simulation switch reaches (a keyless deployment or a system sender with a validation switch off, a creation of type 0, 1 or 2 from a sender whose nonce cannot be bumped) have no switch either: their `port_*` scenarios, run with `skip_nonce_check` or `skip_eip3607`, differ when the fix is undone, and an engine test proves each as well.
A change the engine made that the reference has not made yet (an accepted class above) has no switch either: turned off, the engine would match the reference, which no control can catch; the class's pinned `class=` count catches the change being undone, and an engine test proves it.
The fixes that keep a class's rerun exact — the engine run under the reference's rule must reproduce the reference — can have switches: `blockhash_underflow`, `refuse_at_load` and `refusal_gas` keep the reference's refusal order, which only the rerun of `refusal_costs_nothing` reaches; one turned off, that rerun no longer matches the reference and the class's case differs.
The harness's fixture choices that make the engine's inputs match the reference's are switches too, `off("<name>")` in `diff`.

`diff/negative-controls.toml` names, for each switch, one gate case that must differ when the switch is off, and in `why` what the fix does.
`controls.sh` runs each case with the switch on (the gate must pass) and off (the gate must report a difference for that case); it fails on a miss, on a switch without a table or a table without a switch, and when a gate ran no control.
To add a control, run a gate test with `MEGA_EVM2_DISABLE=<name>`, pick a differing case (from the sample when one exists), and add its table; the manifest's header lists the keys.

## Fault injection

`faults.sh` injects faults into a copy of the sample's reference data, one at a time, and runs the gate each concerns; the repository is not modified.
The faults are a representative set, listed in `diff/scripts/faults.py`: for each kind of check a gate makes, one wrong value, entry, count or accepted difference.
A candidate-side fault changes the engine's own first records of one case instead, through `MEGA_EVM2_CANDIDATE_FAULT` (`gate::candidate_fault`), which `gate.sh` refuses: one plain and inspected disagreement and one missing field for a case of the give-back class in each of the synthetic, replay and block gates, and a disagreement for a case only the combined rerun accepts; each must differ, not be waived by its rerun.

A fault counts as rejected only when its gate fails and the output shows the check the fault names, while the unmutated run's output does not, and every test of the run completed: a run killed or crashed before its libtest result proves no check, whatever it printed (the fault `main-killed-after-differ` kills one after its difference to show that).
The unmutated copy must pass every gate first.
When a gate gains a new kind of check, add a fault for it.

## CI

`.github/workflows/next.yml` checks this workspace and does not build the root one; its header says which job runs when and what each runs.
It checks the evm2 fork out at the pin in `evm2-fork.env`; [`evm2-fork.md`](evm2-fork.md) says how to move that pin.
Pull requests retain the sample differential gate.
Every push to `cz/feat/satin-evm2` matching the workflow's path filter also runs the `full` job: the full corpus gate followed by its negative controls.
Set the repository variable `MEGA_EVM2_CORPUS_URL` on `RealiCZ/mega-evm` to a release asset URL of `RealiCZ/mega-evm2-corpus`, such as `https://github.com/RealiCZ/mega-evm2-corpus/releases/download/<tag>/<archive>`.
The job falls back to a secret of the same name when the variable is empty.
A missing URL fails integration pushes with an error; scheduled and manual runs retain the notice and successful skip.
The schedule and manual trigger remain available, but GitHub requires the workflow on the default branch for those triggers to work.

The `mega-eest` job runs on pull requests and pushes matching the same path filter.
`scripts/mega-eest-gate.sh` clones the revision in `mega-eest.env` into a fresh temporary directory, links its vendor paths to this checkout and the adjacent evm2 fork, and builds only adapter (b) plus the suite's consumer with this checkout's toolchain.
It runs every reviewed slice case and the deliberately wrong expectation, verifies the adapter's implementation and fork commits, and fails on any failed, unsupported, missing or errored case, or a negative control that does not fail at the expected field.
The three summary lines report the implementation and suite commits, case totals, and the negative verdict.
`MEGA_EEST_REPO` can override the clone source with another URL or a local repository; the suite revision remains pinned and the source checkout is only read.
Build artifacts are cached under `${CARGO_TARGET_DIR:-next/target}/mega-eest`; the temporary clone and reports are removed on exit.
The public suite needs no additional repository variable or secret.
