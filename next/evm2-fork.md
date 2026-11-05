# The thin evm2 fork

`next/` builds against a thin fork of [alloy-rs/evm2](https://github.com/alloy-rs/evm2).

|               |                                                                                         |
| ------------- | --------------------------------------------------------------------------------------- |
| Repository    | [`RealiCZ/evm2`](https://github.com/RealiCZ/evm2) (`EVM2_FORK_REPO` in `evm2-fork.env`) |
| Branch        | `cz/feat/thin-fork-v2`                                                                  |
| Pinned commit | `068b6c92b87c5f73d3cec9495803e97aad221e84` (`EVM2_FORK_REV`)                            |
| Upstream base | `85395c5440bef1f685142e79c66051014f46f9b5`                                              |
| Base date     | 2026-11-04 14:35:36 +0100                                                               |
| Base subject  | ``refactor: move function-local `use`s to module scope (#1017)`` (joshieDo)             |
| Diff vs base  | 26 files changed, 1,620 insertions, 83 deletions                                        |

The base is not upstream's head: it is the evm2 revision that reth's pull request #28276 moves reth to, so that the node and the engine can share one evm2.
It is `git merge-base origin/main 068b6c92` in the evm2 checkout.
The diff line is `git diff --shortstat` of that base against the pin.
Commit dates below are `git log` committer dates.
The branch rebuilds the earlier `cz/feat/thin-fork` (pin `91576d1e`, 11 commits) as 8 commits; that branch is left as it was, so earlier pins still resolve.
A second rebuild of this branch (previous head `05b612fb`) replaced the pre-state-charge closures and the code-deposit funding hook with the gas-reserve seam.
A third moved the 8 commits from `c47757e1` onto this base (previous head `7154ee0a`); its notes are under "Rebasing the fork".
What the rebuilds dropped is listed under "Dropped in the rebuild".

Two merged upstream changes are already in that base, so they are not patches on this fork.
[alloy-rs/evm2#887](https://github.com/alloy-rs/evm2/pull/887) (`5d311311`, merged 2026-10-30) added `call_with` and `create_with` and closed [#843](https://github.com/alloy-rs/evm2/issues/843).
[alloy-rs/evm2#842](https://github.com/alloy-rs/evm2/pull/842) (`300e7c98`, merged 2026-10-29) counts reservoir-funded state gas in `system_call` totals and closed [#841](https://github.com/alloy-rs/evm2/issues/841).
The base contains both merge commits.
The feasibility study measured the system-call bug as 12,121 gas reported against 110,041 expected.

Checked against `origin/main` on 2026-11-05 (`a026a5a3`): none of the APIs added below are on upstream main.
`origin/main` is three commits ahead of the base, and those commits do not add them.

## `baff629d` select consensus and observation message hooks statically

Commit date 2026-11-05 10:49:45 +0800.
9 files changed, 489 insertions, 31 deletions.

Files: `crates/evm2/src/evm/message_hooks.rs`, `crates/evm2/src/evm/mod.rs`, `crates/evm2/src/evm/config.rs`, `crates/evm2/src/evm/registry.rs`, `crates/evm2/src/lib.rs`, `crates/evm2/src/test_utils.rs`, `crates/evm2/tests/message_hooks.rs`, `crates/evm2/examples/custom_evm/config.rs`, `crates/jit/runtime/src/evm2_evm.rs`.

`EvmTypesHost::MessageHooks` is an associated type.
`()` is the no-op.
The `MessageHooks` trait runs on every message, on the plain path and the inspected path.
Consensus methods: `before` (may answer the message), `started` (after the checkpoint, value transfer, and creation setup), `frame_built`, `before_commit` (may rewrite the stop before commit or rollback), `after`.
Observers: `enter`, `unsettled`, `exit`.
A message that halts without executing (`Evm::execute_halted_message`, the first frame of a transaction whose runtime gas phase ran out of gas) reaches none of them, on either path (`halted_messages_run_no_hooks_on_either_path`).
`admit_code_deposit` is added by a later commit.
The JIT host tests set `MessageHooks = ()`.

Satin needs this for frame-start rules: stop a frame before its code runs, and take back a success answer.
The feasibility study measured six scenarios matching with the hooks.
Without them the value moves, 360,584 gas against 169,944.
The feasibility study measured interceptors matching on 8 of 9 cases from a precompile dispatch rebase alone, and on 9 of 9 once `before` runs, because evm2 moves value before precompile dispatch.
The engine answers the system contracts from `before` alone; precompile dispatch is evm2's own.
Withheld gas keeps the spendable part in `remaining` and the withheld part in a side record.
The feasibility study measured 36 of 36 detention scenarios equal to the Satin engine with these hooks, the call helpers, and the failed-charge record.
Dropping any one of those fixes broke at least one run.
History gas is evm2 state gas plus a signed per-frame record.
The feasibility study measured that this observer subset, the call helpers, code-deposit admission, and original account info are what that history needs.

`MegaTypes::MessageHooks` is `StartHooks` (`mega-evm2/src/lib.rs:102`).
The impl is `mega-evm2/src/lib.rs:988`.
It uses `enter` (`mega-evm2/src/lib.rs:989`), `before` (`mega-evm2/src/lib.rs:995`, including `keyless.rs:80` and `intercept.rs:73`), `started` (`mega-evm2/src/lib.rs:1048`), `frame_built` (`mega-evm2/src/lib.rs:1075`, `detention.rs:319`), and `before_commit` (`mega-evm2/src/lib.rs:1079`, `detention.rs:335`).
`op-evm2` sets the associated type to `()` (`op-evm2/src/evm.rs:53`).

Upstream: none filed.
A search for `MessageHooks` in `alloy-rs/evm2` on 2026-11-03 returned no issues or pull requests.
The feasibility study measured these hooks below the call helpers that have already merged.
The same study measured a dynamic hook at 13–17% wall-clock on a loop of empty calls, which is why the fork uses a static associated type.
Drop when upstream offers a static per-message hook that runs for every message, including answers with no interpreter frame, before the commit decision.

## `3997b9c8` price state charges by account and storage location

Commit date 2026-11-05 10:49:45 +0800.
8 files changed, 196 insertions, 33 deletions.

Files: `crates/evm2/src/evm/config.rs`, `crates/evm2/src/evm/mod.rs`, `crates/evm2/src/ethereum/mod.rs`, `crates/evm2/src/ethereum/eip7702.rs`, `crates/evm2/src/interpreter/instructions/host.rs`, `crates/evm2/src/interpreter/instructions/system.rs`, `crates/evm2/src/version/gas_params.rs`, `crates/evm2/src/version/mod.rs`.

`EvmTypesHost::state_gas_price(host, id, site) -> Result<u64, ExecutionError>` prices one schedule entry at the state it touches.
`StateGasSite` is `Account(address)` or `Storage(address, key)`.
The default ignores `site` and returns `gas_params.get(id)`.
An error aborts the transaction.
Callers must not turn a failed lookup into an EVM halt.
The price has to stay stable for a location for the whole transaction.
The call, create, SSTORE, and EIP-7702 charge sites call it.
A code deposit of empty code is not priced: it deposits nothing, so a location-priced chain reads no state for it (`empty_code_deposit_is_not_priced` in `crates/evm2/src/evm/mod.rs`).

Satin's SALT price is the schedule entry times the bucket multiplier of that account or slot.
The feasibility study measured that price as symmetric at multipliers 1, 2, and 8.
The same measurement builds history as shared-spill state gas, so the price hook is on that path too.
The default would charge the bare schedule at every site, including crowded buckets.

`MegaTypes::state_gas_price` is `mega-evm2/src/lib.rs:106`.
A deposit's created caller is priced there (`mega-evm2/src/tx.rs:383`).
Keyless deployment prices through the same method (`mega-evm2/src/keyless.rs:176`).
SSTORE's sets and refills are evm2's own body, which calls it.
Satin prices no empty deposit, so the engine needs no flag of its own for it.
`op-evm2` does not override the method, so it keeps the default schedule price.

Upstream: none filed.
A search for `state_gas_price` on 2026-11-03 returned no issues or pull requests.
The hook sits on every state-gas charge site, so the caller shape has to be agreed before a patch is written.
Drop when upstream can price a state charge by account and storage location.

## `75a6557d` expose a code deposit admission hook

Commit date 2026-11-05 10:49:45 +0800.
3 files changed, 83 insertions, 9 deletions.

Files: `crates/evm2/src/evm/message_hooks.rs`, `crates/evm2/src/evm/mod.rs`, `crates/evm2/tests/message_hooks.rs`.

`MessageHooks::admit_code_deposit` runs after the code-deposit checks and charges, before installation and `before_commit`.
`None` accepts.
`Some((stop, output))` rejects with a non-success stop.
`Revert` restores `gas_before`, the frame's gas from before the deposit charges, including hashing and state charges.
Other stops keep the charged gas until ordinary settlement.

Satin's code-deposit admission has to refuse after the charges and still restore the pre-charge gas.
The feasibility study measured 3 of 3 scenarios matching when admission receives `gas_before`.
Refusing in `before_commit` was 180 gas off.
The feasibility study charges the history of the deposited bytes in `admit_code_deposit`.
The deposit's state spill draws withheld gas through the gas reserve (`e6c12f06`); the admission settles that draw (`DepositLoan`, `mega-evm2/src/ledger.rs:185`), because `gas_before` does not hold it.

`StartHooks::admit_code_deposit` is `mega-evm2/src/lib.rs:1126`.

Upstream: none filed.
Searches for `admit_code_deposit` on 2026-11-03 returned no issues or pull requests.
Drop when upstream offers an admission hook that can restore the pre-charge gas on revert.

## `568e0cf8` retain failed debit before normalization on every interpreter path

Commit date 2026-11-05 10:49:45 +0800.
4 files changed, 104 insertions, 3 deletions.

Files: `crates/evm2/src/interpreter/gas.rs`, `crates/evm2/src/interpreter/dispatch/table/packed.rs`, `crates/evm2/src/interpreter/dispatch/tco.rs`, `crates/evm2/tests/message_hooks.rs`.

`GasTracker::failed_charge` returns the last failed execution-gas charge as `(remaining_before, cost)`.
`record_failed_charge` and `clear_failed_charge` write and clear it.
The record is written on the failure side of `spend` and of the dispatcher's static gas, on the plain path and the inspected path, before wrapping or inspector normalization zeroes `remaining`.
It is one debit, not the whole opcode cost.
Successful charges leave the record in place.
Child records are not merged.
`try_spend` and `try_spend_state` failures do not record.
Skipped cold loads and the SSTORE sentry do not record a failed charge, because they never debit; `e6c12f06` adds a typed record for such halts.

Satin classifies an out-of-gas halt by that record: a charge the spendable plus withheld gas could pay is a compute stop, and any other halt burns.
The inspected path zeroes `remaining`, so the amount the frame held is gone unless the record survived.
The feasibility study measured this record as part of the 36 of 36 detention matches.
The same study measured that compute at a failed charge needs the record.
The base already contains [#752](https://github.com/alloy-rs/evm2/pull/752) (`da5e6ac`, merged 2026-10-29), which reports the gas delta as the inspector step cost on out-of-gas.
This commit adds a consensus record on the tracker after that change.
That consensus record is a separate ask from the inspector step-cost change in #752.

`before_commit` classifies a crossing by `failed_charge` (`mega-evm2/src/detention.rs:366`) and reads what a halting frame had from `halt_remaining` (`mega-evm2/src/detention.rs:385`).
A start charge the caller cannot pay records a no-debit halt after handing the forward back (`mega-evm2/src/lib.rs:650`).
Precompiles outside the engine's quote table are priced by reading the record off a tracker with no gas (`mega-evm2/src/detention.rs:512`).

Upstream: none filed for this record.
A search for `failed_charge` on 2026-11-03 returned #564, #577, #752, and #837, all closed inspector or tracing changes.
Drop when upstream keeps the failed execution-gas debit, on both interpreter paths, after `remaining` is normalized.

## `e6c12f06` let a type family hold a frame's gas in reserve

Commit date 2026-11-05 10:50:08 +0800.
10 files changed, 679 insertions, 25 deletions (about 460 of the insertions are tests).

Files: `crates/evm2/src/interpreter/instructions/reserve.rs` (new), `crates/evm2/src/evm/config.rs`, `crates/evm2/src/evm/mod.rs`, `crates/evm2/src/interpreter/gas.rs`, `crates/evm2/src/interpreter/runtime.rs`, `crates/evm2/src/interpreter/instructions/{control,env,host,mod,system}.rs`.

`EvmTypesHost::gas_reserve(host, BudgetUse) -> u64` reports execution gas the running frame holds outside `remaining`.
`EvmTypesHost::draw_gas_reserve(host, amount, ReserveDraw) -> u64` moves up to `amount` of it back and returns what it moved.
Both default to no reserve and are `#[inline(always)]`.
`BudgetUse` names the decision: `Gas`, `ColdAccount`, `ColdStorage`, `SstoreSentry`, `DelegationGuard`, `Forward`.
`ReserveDraw` names the deduction: `Forward` or `StateSpill(GasId)`.
The helpers `gas_budget`, `spend_reserve_first` and `spend_state_reserve_first` are used at evm2's own sites.
Budget readers: GAS, the skip-cold checks of the account opcodes, SLOAD, SSTORE, the call family's target and SELFDESTRUCT, the EIP-7702 delegation guard, the EIP-2200 sentry, and the forward base of CALL and CREATE.
Draws: the forward of CALL and CREATE, and every state spill (SSTORE, a value call's new account, CREATE, SELFDESTRUCT's new beneficiary, the code deposit).
The code-deposit charge is a regular charge and does not see the reserve.
`GasTracker::record_no_debit_halt(remaining)` records a halt that made no debit; `GasTracker::halt_remaining()` returns what a halting frame had, from that record or the failed charge.
The inspected path writes the no-debit record, when the frame has none, before it zeroes `remaining` at an out-of-gas instruction the inspector steps, so both paths keep that gas readable.
An instruction the inspector does not step runs a plain handler and is neither zeroed nor recorded.
The seam is interpreter-only: the JIT builtins read `remaining` alone and do not consult it.

Satin's detention withholds part of a frame's regular gas outside `remaining`.
Budget decisions must see it, and deductions that are not the frame's own execution must draw it first.
Before this commit the engine carried copies of evm2's CALL body, SLOAD, the account load, SSTORE and a SELFDESTRUCT probe, a forward repair, a GAS wrapper and lend closures for this; they are gone.
With the default the plain-path dispatch handlers of the stock type families compile byte-identical to the previous pin.

`MegaTypes::gas_reserve` and `draw_gas_reserve` are `mega-evm2/src/lib.rs:126` and `mega-evm2/src/lib.rs:132`, over `detention::reserve_for` (`mega-evm2/src/detention.rs:153`) and `detention::draw_reserve` (`mega-evm2/src/detention.rs:169`).
Each `BudgetUse` and each draw has its own negative control there.
Keyless deployment draws through the same helpers (`mega-evm2/src/keyless.rs:233`, `mega-evm2/src/keyless.rs:299`).

Upstream: none filed.
A search for a reserve or withheld-gas API on 2026-11-04 found no second adopter.
Drop when upstream lets a type family keep execution gas outside `remaining` that budget decisions see and non-execution deductions draw first, with a halt record that survives the inspected path.

## `af07574b` expose transaction-original account information

Commit date 2026-11-05 10:50:08 +0800.
1 file changed, 16 insertions.

File: `crates/evm2/src/evm/state/account.rs`.

`AccountHandle::original` returns the account info at the transaction boundary, before any frame mutated it.
`None` means the account was absent.
Loading bytecode into the present cache does not move that baseline.
`AccountHandle::is_changed` reports whether existence or persisted account fields differ from that baseline.
Storage-only writes do not count.
On the base, `is_changed` is `pub(crate)` on the inner account and there is no public `original` on the handle.

Write records and destruction refunds compare the account with its start-of-transaction info.
A snapshot taken at first observation is exact only when that observation happens before every change.
The feasibility study measured that write records need original info in general.
The same study measured this accessor on history account records and on created-and-destroyed refunds for accounts that were absent at the start.

SELFDESTRUCT reads `original` for the absent-at-start refund (`mega-evm2/src/lib.rs:509`) and for the beneficiary (`mega-evm2/src/lib.rs:528`).
The write-record helper `changed` (`mega-evm2/src/ledger.rs:443`) reads the same accessor.
The engine does not call `is_changed`.

Upstream: none filed.
Searches for `AccountHandle` on 2026-11-03 did not return a pull request that exposes `original`.
Drop when `AccountHandle::original` is public.

## `758e51da` optionally read uncached storage of accounts the database reports absent

Commit date 2026-11-05 10:50:08 +0800.
2 files changed, 25 insertions, 3 deletions.

Files: `crates/evm2/src/evm/db/cache.rs`, `crates/evm2/src/evm/state/mod.rs`.

`CacheDB::read_absent_storage` defaults to `false`.
While it is false, uncached storage of an account the cache holds as absent is answered as zero and the backing database is not read.
`State::set_read_absent_storage` turns the flag on.
Values stay the same for a consistent database.
The read set changes: the read is actually performed, which is what revm does.

A strict-replay witness needs those reads.
The engine cannot work around it: storage reads go through `CacheDB`.

The differential driver sets the flag (`next/diff/src/driver.rs:332`).
Replay sets it the same way (`next/diff/src/replay.rs:115`).
The gas-cap tests set it on the Satin EVM (`mega-evm2/tests/tx_gas_cap.rs:98`).

Upstream: none filed.
A search for `read_absent` on 2026-11-03 returned no issues or pull requests.
Drop when upstream reads uncached storage of a database-absent account the way revm does, or offers this switch.

## `068b6c92` make the EIP-8037 total transaction gas cap configurable

Commit date 2026-11-05 10:50:08 +0800.
2 files changed, 51 insertions, 2 deletions.

Files: `crates/evm2/src/version/mod.rs`, `crates/evm2/src/ethereum/mod.rs`.

`Version::tx_total_gas_cap` bounds `tx.gas` when EIP-8037 is on.
`u64::MAX` disables the cap.
The default is `MAX_TX_TOTAL_GAS_LIMIT_AMSTERDAM` (`2^32 - 1`), written into both base-version tables, so an unmodified Ethereum version rejects a gas limit of `2^32`.
`Version::tx_gas_caps` returns `(tx_total_gas_cap, tx_gas_limit_cap)` under EIP-8037.
Without EIP-8037 the field is unused and `tx_gas_limit_cap` bounds `tx.gas`.

Satin turns EIP-8037 on and sets the execution cap to 200,000,000 (`mega-evm2/src/lib.rs:230`).
Under EIP-8037 that execution cap bounds required execution gas, and the excess of `tx.gas` funds the state-gas reservoir.
Satin sets `tx_total_gas_cap` to `u64::MAX` (`mega-evm2/src/lib.rs:231`), so the block gas limit is the only bound on `tx.gas`.
`mega-evm2/tests/tx_gas_cap.rs:1` states that, and `test_legacy_tx_above_total_cap_executes` (`mega-evm2/tests/tx_gas_cap.rs:149`) sends a gas limit of `2^32`.
The feasibility study predates this commit, so it does not measure this patch.

Upstream: none filed.
A search for `tx_total_gas_cap` on 2026-11-03 returned no issues or pull requests.
Drop when upstream lets a chain set or disable the EIP-8037 total transaction gas cap without changing the default.

## Dropped in the rebuild

- `e0a93cf0`, the rebase of open pull request [#523](https://github.com/alloy-rs/evm2/pull/523) (native precompile message dispatch and reentry context, +322/−37).
  The engine answers `MegaLimitControl` and `MegaAccessControl` from `MessageHooks::before`, which runs before dispatch, so the provider path it served was unreachable; scheme guard and selector are `is_intercepted` (`mega-evm2/src/intercept.rs:29`).
  The provider is evm2's `Precompiles` over the Satin map (`mega-evm2/src/lib.rs:263`), and `op-evm2`'s `PrecompileProvider::execute` takes `&mut self` again, as upstream.
- `7352d38f`, input-only precompile gas quotes (+50/−4).
  The engine prices the seven fixed-formula entries from its own table (`precompile_price`, `mega-evm2/src/detention.rs:419`) and probes every other entry on a tracker with no gas (`mega-evm2/src/detention.rs:502`).
- `45817aad`, formatting and call-site glue (+40/−38), folded into the commits it repaired; the parts that only served `e0a93cf0` went with it.
- `1ed2b823`, the pre-state-charge closures `call_with_state_charge` and `create_with_state_charge` (+154/−3), in the second rebuild.
  The state spills they funded draw the gas reserve inside evm2's bodies, so the engine calls `call_with` and `create_with` (`mega-evm2/src/lib.rs:749`, `mega-evm2/src/lib.rs:876`).
- The funding half of the code-deposit hooks, `MessageHooks::before_code_deposit`, in the second rebuild.
  The deposit's state spill draws the gas reserve.
  The engine's precheck of the deposit's regular charge went with it: Satin's schedule has no per-byte deposit cost, so that charge is the hash alone, which evm2 debits once and records when it fails (`test_code_deposit_regular_charge_is_the_hash_alone`, `mega-evm2/src/detention.rs`).

## Rebasing the fork

Fetch `origin` (alloy-rs/evm2) and rebase `cz/feat/thin-fork-v2` onto the revision chosen as the base (the one the node pins, not necessarily `origin/main`'s head).

Textual conflicts are the easy part.
The feasibility study measured the binding cost as a semantic audit of new `remaining()` readers, plus the JIT.
Withheld gas stores only the spendable part in `remaining`, and evm2's instructions see the rest through the gas reserve.
A new upstream site that reads `remaining()` for a budget decision directly, instead of through `gas_budget`, ignores the withheld part, and so does a new state charge that calls `spend_state` instead of `spend_state_reserve_first`.
The same measurement saw a new `try_spend_state` reader merge with no textual conflict and still change behavior.
After a rebase, read every new `remaining()`, `spend_state`, and `try_spend_state` site under `crates/evm2/src/interpreter/instructions` and `crates/evm2/src/evm/mod.rs`, and route budget decisions and state spills through the helpers in `crates/evm2/src/interpreter/instructions/reserve.rs`.
The JIT builtins carry their own readers and do not consult the reserve; the engine runs the interpreter only.
The no-debit halt record sits at the inspected out-of-gas zeroing point (`inspect_step_end` in `crates/evm2/src/interpreter/runtime.rs`); upstream work on that function has to keep it.

What the move from `c47757e1` to `85395c54` (36 upstream commits) needed, beyond textual conflicts in `ethereum/eip7702.rs`, `ethereum/mod.rs`, `interpreter/instructions/system.rs` and `interpreter/runtime.rs`:

- [#873](https://github.com/alloy-rs/evm2/pull/873) replaced the `doc(hidden)` `Instruction::DYNAMIC_GAS` with `FLAGS`, and an instruction that runs a child message must be `#[instruction(hooked)]`, or an inspected run panics at the first message.
  The engine's CALL and CREATE family and its keyless dispatcher are hooked; `Volatile` keeps its wrapped opcode's flags plus `DYNAMIC_GAS`.
- Since #873 an inspector chooses the opcodes it steps, and `NoopInspector` steps none: its frames run the plain loop and never reach the zeroing point.
  The fork's tests of the inspected path use an inspector that steps every opcode, as the gates' frame recorder does.
- [#956](https://github.com/alloy-rs/evm2/pull/956) moved the step cost into the inspectors; gas a `call_with` or `create_with` before-hook spends now counts toward a tracer's step cost.
- [#871](https://github.com/alloy-rs/evm2/pull/871) reports the first frame of a transaction whose runtime gas phase runs out of gas to the inspector (`execute_halted_message`); `prepare_initial_frame` returns that halted frame instead of `None`.
  Message hooks do not run for it on either path.
  The reference opens no such frame; the cases the gates reach are listed in `diff/known-differences.toml`.
  The engine's mirrored EIP-7702 path (`mega-evm2/src/eip7702.rs`) still settles a runtime-phase out-of-gas without a frame, unlike upstream's `eip7702::execute_prepared` (`halted_initial_frame` then `execute_initial_frame`); this is a known follow-up, not a gate entry.
- [#949](https://github.com/alloy-rs/evm2/pull/949) and [#1006](https://github.com/alloy-rs/evm2/pull/1006) added no budget reader or state spill: the count stayed at 10 budget-reader sites (6 `BudgetUse` kinds; the CALL and CREATE forward bases are two sites), 5 state-spill sites and 2 forward draws.

The message-hook associated type has to be set on every `EvmTypesHost` impl, including the JIT host tests in `crates/jit/runtime/src/evm2_evm.rs`.
Build the JIT crate after the rebase, or the interpreter can be fine while the runtime tests are not.

Move `next/evm2-fork.env` only after the new commit is on the public fork.
CI checks the pin out from `EVM2_FORK_REPO` by hash (`.github/workflows/next.yml`).
A pin that was never pushed fails the step "Check out the evm2 fork at its pin" (`next/scripts/fetch-evm2-fork.sh`).
Push the rebased commit to `RealiCZ/evm2` first.
Keep that repository public.
Then set `EVM2_FORK_REV` to the new hash.
Local builds keep using the path checkout next to this repository (`../evm2` from the repository root).

Gates, from `next/`:

```bash
bash diff/scripts/ci-local.sh
MEGA_EVM2_CORPUS_DIR=<corpus> bash diff/scripts/gate.sh full
MEGA_EVM2_CORPUS_DIR=<corpus> bash diff/scripts/controls.sh full
```

`ci-local.sh` is the check job: fmt, clippy, `mega-evm2` and `op-evm2` tests, the `no_std` check, the check that `regen-reference.sh` refuses an output that would reach the corpus, `gate.sh sample`, `controls.sh sample` (the negative controls: every switch, off, makes its gate fail) and `faults.sh` (the fault injection: a representative set of wrong reference data is rejected).
After a rebase the negative controls matter most: a fix that the new upstream code no longer reaches shows up as a missed control even while the gates pass.
`gate.sh full` and `controls.sh full` need the corpus archive.

## Upstream candidates

Smallest diff first.
Sizes are the commit stat against its parent.
Nothing here was opened or posted.

1. Transaction-original account info (`af07574b`, +16).
   One accessor on a cold path.
   Write records and start-of-transaction refunds compare an account with its info at the transaction boundary.
   A snapshot at first observation is exact only when that observation precedes every change.
2. Read-absent storage (`758e51da`, +25/−3).
   The flag defaults off, so existing chains keep today's answer of zero and no backing read.
   A strict-replay witness needs the read, because revm performs it.
   A search for `read_absent` on 2026-11-03 returned no issues or pull requests.
3. Configurable total gas cap (`068b6c92`, +51/−2).
   The default stays `2^32 - 1`, written into both base version tables.
   A chain that only wants the Amsterdam cap is unchanged (inferred from that default).
4. Code-deposit admission (`75a6557d`, +83/−9).
   One creation site, off the interpreter loop.
   A revert has to restore the pre-charge gas.
   The feasibility study measured that refusing in `before_commit` was 180 gas off, because `before_commit` does not see the pre-charge snapshot.
5. Failed-charge record (`568e0cf8`, +104/−3) with the gas reserve's typed no-debit record (`e6c12f06`).
   #752 already reports the inspector out-of-gas step cost, and that change is in the base.
   File this as a consensus record that survives normalization, or the inspector discussion will be taken to cover it.
6. State-gas price (`3997b9c8`, +196/−33).
   Any chain that prices state by location can use it.
   The call, create, SSTORE, and EIP-7702 charge sites all call it, so the caller shape should be agreed before a patch is written.
7. Gas reserve (`e6c12f06`, +679/−25, most of it tests).
   A design issue first: budget readers and non-execution deductions are interpreter sites, so the JIT builtins would need the same helpers before the API is complete.
8. Static message hooks (`baff629d`, +489/−31).
   Largest patch, and it runs on every message.
   The feasibility study measured these hooks below the call helpers that have already merged.
   The same study measured a dynamic hook at 13–17% wall-clock on empty calls, which is why the proposal is a static associated type.
   It waits until the smaller hooks have an answer, because it changes every message.

#842 and #887 have already merged and are in the base.
They are not candidates.
