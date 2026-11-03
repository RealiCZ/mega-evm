# The thin evm2 fork

`next/` builds against a thin fork of [alloy-rs/evm2](https://github.com/alloy-rs/evm2).

|               |                                                                                         |
| ------------- | --------------------------------------------------------------------------------------- |
| Repository    | [`RealiCZ/evm2`](https://github.com/RealiCZ/evm2) (`EVM2_FORK_REPO` in `evm2-fork.env`) |
| Branch        | `cz/feat/thin-fork`                                                                     |
| Pinned commit | `91576d1ebade72753f562bb1ab4e6bf8a7351a26` (`EVM2_FORK_REV`)                            |
| Upstream base | `c47757e13860cb2e9f688101020b3feaa5a86760`                                              |
| Base date     | 2026-11-01 14:23:38 +0100                                                               |
| Base subject  | `ci(fuzzer): pin nightly to 2026-10-31 (#981)` (DaniPopes)                              |
| Diff vs base  | 27 files changed, 1,416 insertions, 108 deletions                                       |

The base is `git merge-base origin/main 91576d1e` in the evm2 checkout.
The diff line is `git diff --shortstat` of that base against the pin.
Commit dates below are `git log` committer dates.

Two merged upstream changes are already in that base, so they are not patches on this fork.
[alloy-rs/evm2#887](https://github.com/alloy-rs/evm2/pull/887) (`5d311311`, merged 2026-10-30) added `call_with` and `create_with` and closed [#843](https://github.com/alloy-rs/evm2/issues/843).
[alloy-rs/evm2#842](https://github.com/alloy-rs/evm2/pull/842) (`300e7c98`, merged 2026-10-29) counts reservoir-funded state gas in `system_call` totals and closed [#841](https://github.com/alloy-rs/evm2/issues/841).
The base contains both merge commits.
The feasibility study measured the system-call bug as 12,121 gas reported against 110,041 expected.

Checked against `origin/main` on 2026-11-03 (`bba13787`): none of the APIs added below are on upstream main.
`origin/main` is eight commits ahead of the base, and those commits do not add them.

## `a9549934` select consensus and observation message hooks statically

Commit date 2026-11-02 15:31:46 +0800.
9 files changed, 474 insertions, 32 deletions.

Files: `crates/evm2/src/evm/message_hooks.rs`, `crates/evm2/src/evm/mod.rs`, `crates/evm2/src/evm/config.rs`, `crates/evm2/src/evm/registry.rs`, `crates/evm2/src/lib.rs`, `crates/evm2/src/test_utils.rs`, `crates/evm2/tests/message_hooks.rs`, `crates/evm2/examples/custom_evm/config.rs`, `crates/jit/runtime/src/evm2_evm.rs`.

`EvmTypesHost::MessageHooks` is an associated type.
`()` is the no-op.
The `MessageHooks` trait runs on every message, on the plain path and the inspected path.
Consensus methods: `before` (may answer the message), `started` (after the checkpoint, value transfer, and creation setup), `frame_built`, `before_commit` (may rewrite the stop before commit or rollback), `after`.
Observers: `enter`, `unsettled`, `exit`.
`before_code_deposit` and `admit_code_deposit` are added by a later commit.
The JIT host tests set `MessageHooks = ()`.

Satin needs this for frame-start rules: stop a frame before its code runs, and take back a success answer.
The feasibility study measured six scenarios matching with the hooks.
Without them the value moves, 360,584 gas against 169,944.
The feasibility study measured interceptors matching on 8 of 9 cases from the precompile dispatch rebase alone, and on 9 of 9 once `before` runs, because evm2 moves value before precompile dispatch.
Withheld gas keeps the spendable part in `remaining` and the withheld part in a side record.
The feasibility study measured 36 of 36 detention scenarios equal to the Satin engine with these hooks, the call helpers, and the failed-charge record.
Dropping any one of those fixes broke at least one run.
History gas is evm2 state gas plus a signed per-frame record.
The feasibility study measured that this observer subset, the call helpers, code-deposit admission, and original account info are what that history needs.

`MegaTypes::MessageHooks` is `StartHooks` (`mega-evm2/src/lib.rs:104`).
The impl is `mega-evm2/src/lib.rs:1046`.
It uses `enter` (`mega-evm2/src/lib.rs:1047`), `before` (`mega-evm2/src/lib.rs:1053`, including `keyless.rs:80` and `intercept.rs:127`), `started` (`mega-evm2/src/lib.rs:1108`), `frame_built` (`mega-evm2/src/lib.rs:1135`, `detention.rs:261`), and `before_commit` (`mega-evm2/src/lib.rs:1139`, `detention.rs:277`).
`op-evm2` sets the associated type to `()` (`op-evm2/src/evm.rs:53`).

Upstream: none filed.
A search for `MessageHooks` in `alloy-rs/evm2` on 2026-11-03 returned no issues or pull requests.
The feasibility study measured these hooks below the call helpers that have already merged.
The same study measured a dynamic hook at 13–17% wall-clock on a loop of empty calls, which is why the fork uses a static associated type.
Drop when upstream offers a static per-message hook that runs for every message, including answers with no interpreter frame, before the commit decision.

## `5cf8d414` hook call and create before state charges and around children

Commit date 2026-11-02 15:32:28 +0800.
2 files changed, 146 insertions, 3 deletions.

Files: `crates/evm2/src/interpreter/instructions/system.rs`, `crates/evm2/tests/message_hooks.rs`.

The base already has `call_with` and `create_with` from #887.
Those `before` / `after` closures run around the child, after the opcode's own state-gas debit.
This commit adds `call_with_state_charge` and `create_with_state_charge`.
Each takes one more closure, `before_state_charge(gas, state, destination, price)`, called with the destination and the captured price before that debit.
The hook may adjust funding or refuse the opcode.
It must not pay the debit itself.
`call_with` and `create_with` forward to the new functions with an empty pre-charge closure.

Satin's withheld gas has to fund a state-gas spill before `spend_state`.
The feasibility study measured spills charged before the around-child `before` still halting inside the debit when the spill is larger than spendable gas.
Those spills are a value call that creates an account, `CREATE`, and `SELFDESTRUCT`.
The same measurement treats the code-deposit precheck separately.
`SELFDESTRUCT` is funded in the engine, before the base opcode (`mega-evm2/src/lib.rs:562`).
This commit is what funds the call and create spills.
The 98/100 reservoir share is the around-child hook, which is upstream #887.
The feasibility study measured that share matching on 4 of 4 scenarios with `call_with`, and a whole-opcode wrapper producing a different child reservoir.
The pre-charge closure is the extra point #887's body described and did not add.

`mega_call` passes the pre-charge closure through `call_with` (`mega-evm2/src/lib.rs:793`), which calls `call_with_state_charge` (`mega-evm2/src/call_emulation.rs:306`, `mega-evm2/src/call_emulation.rs:329`).
The closure calls `lend_state` (`mega-evm2/src/lib.rs:800`).
`CREATE` / `CREATE2` call `create_with_state_charge` directly (`mega-evm2/src/lib.rs:927`).
When a frame is withholding gas, `call_emulation.rs` copies the call body so the skip-cold check sees spendable plus withheld (`mega-evm2/src/call_emulation.rs:1`).

Upstream: no separate issue or pull request for `call_with_state_charge` (search on 2026-11-03, zero hits).
The request is in the body of merged #887, under "A related point, raised here rather than added."
Drop when upstream offers a hook before the CALL new-account charge and the CREATE account-creation charge.

## `52c4854d` price state charges by account and storage location

Commit date 2026-11-02 15:32:28 +0800.
8 files changed, 135 insertions, 33 deletions.

Files: `crates/evm2/src/evm/config.rs`, `crates/evm2/src/evm/mod.rs`, `crates/evm2/src/ethereum/mod.rs`, `crates/evm2/src/ethereum/eip7702.rs`, `crates/evm2/src/interpreter/instructions/host.rs`, `crates/evm2/src/interpreter/instructions/system.rs`, `crates/evm2/src/version/gas_params.rs`, `crates/evm2/src/version/mod.rs`.

`EvmTypesHost::state_gas_price(host, id, site) -> Result<u64, ExecutionError>` prices one schedule entry at the state it touches.
`StateGasSite` is `Account(address)` or `Storage(address, key)`.
The default ignores `site` and returns `gas_params.get(id)`.
An error aborts the transaction.
Callers must not turn a failed lookup into an EVM halt.
The price has to stay stable for a location for the whole transaction.
The call, create, SSTORE, and EIP-7702 charge sites call it.

Satin's SALT price is the schedule entry times the bucket multiplier of that account or slot.
The feasibility study measured that price as symmetric at multipliers 1, 2, and 8.
The same measurement builds history as shared-spill state gas, so the price hook is on that path too.
The default would charge the bare schedule at every site, including crowded buckets.

`MegaTypes::state_gas_price` is `mega-evm2/src/lib.rs:108`.
A deposit's created caller is priced there (`mega-evm2/src/tx.rs:383`).
Keyless deployment prices through the same method (`mega-evm2/src/keyless.rs:181`).
SSTORE refills use it (`mega-evm2/src/detention.rs:421`).
`op-evm2` does not override the method, so it keeps the default schedule price.

Upstream: none filed.
A search for `state_gas_price` on 2026-11-03 returned no issues or pull requests.
The hook sits on every state-gas charge site, so the caller shape has to be agreed before a patch is written.
Drop when upstream can price a state charge by account and storage location.

## `3c25bc3b` expose code deposit funding and admission hooks

Commit date 2026-11-02 15:32:49 +0800.
3 files changed, 129 insertions, 13 deletions.

Files: `crates/evm2/src/evm/message_hooks.rs`, `crates/evm2/src/evm/mod.rs`, `crates/evm2/tests/message_hooks.rs`.

`MessageHooks::before_code_deposit` runs after the code-size and prefix checks, before the deposit gas precheck and the regular and state charges.
It may adjust `gas`.
It must not install the code.
An EVM halt rejects the deployment.
`MessageHooks::admit_code_deposit` runs after those charges, before installation and `before_commit`.
`None` accepts.
`Some((stop, output))` rejects with a non-success stop.
`Revert` restores `gas_before`, the snapshot from before `before_code_deposit`, including hashing and state charges.
Other stops keep the charged gas until ordinary settlement.

Satin's code-deposit admission has to refuse after the charges and still restore the pre-charge gas, and the deposit's state spill has to draw withheld gas first.
The feasibility study measured 3 of 3 scenarios matching when admission receives `gas_before`.
Refusing in `before_commit` was 180 gas off.
The feasibility study charges the history of the deposited bytes in `admit_code_deposit`.

`StartHooks::before_code_deposit` is `mega-evm2/src/lib.rs:1180` and calls `detention.rs:441`.
`StartHooks::admit_code_deposit` is `mega-evm2/src/lib.rs:1193`.

Upstream: none filed.
Searches for `before_code_deposit` and `admit_code_deposit` on 2026-11-03 returned no issues or pull requests.
Drop when upstream offers a code-deposit funding hook and an admission hook that can restore the pre-charge gas on revert.

## `a819dc44` retain failed debit before normalization on every interpreter path

Commit date 2026-11-02 15:32:49 +0800.
4 files changed, 98 insertions, 3 deletions.

Files: `crates/evm2/src/interpreter/gas.rs`, `crates/evm2/src/interpreter/dispatch/table/packed.rs`, `crates/evm2/src/interpreter/dispatch/tco.rs`, `crates/evm2/tests/message_hooks.rs`.

`GasTracker::failed_charge` returns the last failed execution-gas charge as `(remaining_before, cost)`.
`record_failed_charge` and `clear_failed_charge` write and clear it.
The record is written on the failure side of `spend` and of the dispatcher's static gas, on the plain path and the inspected path, before wrapping or inspector normalization zeroes `remaining`.
It is one debit, not the whole opcode cost.
Successful charges leave the record in place.
Child records are not merged.
`try_spend` and `try_spend_state` failures do not record.
Skipped cold loads and the SSTORE sentry do not record, because they never debit.

Satin classifies an out-of-gas halt by that record: a charge the spendable plus withheld gas could pay is a compute stop, and any other halt burns.
The inspected path zeroes `remaining`, so the amount the frame held is gone unless the record survived (`mega-evm2/src/detention.rs:572`).
The feasibility study measured this record as part of the 36 of 36 detention matches.
The same study measured that compute at a failed charge needs the record.
The base already contains [#752](https://github.com/alloy-rs/evm2/pull/752) (`da5e6ac`, merged 2026-10-29), which reports the gas delta as the inspector step cost on out-of-gas.
This commit adds a consensus record on the tracker after that change.
That consensus record is a separate ask from the inspector step-cost change in #752.

`before_commit` reads `failed_charge` (`mega-evm2/src/detention.rs:308`).
The code-deposit precheck records a charge the spendable gas cannot pay (`mega-evm2/src/detention.rs:456`).
A skipped cold load records a charge no gas could pay (`mega-evm2/src/detention.rs:582`).
SELFDESTRUCT does the same when the cold access does not fit (`mega-evm2/src/lib.rs:557`).
A start charge the caller cannot pay rewrites the record after handing the forward back (`mega-evm2/src/lib.rs:704`).
Precompile prices that have no input-only quote read the record off a tracker with no gas (`mega-evm2/src/detention.rs:604`).

Upstream: none filed for this record.
A search for `failed_charge` on 2026-11-03 returned #564, #577, #752, and #837, all closed inspector or tracing changes.
Drop when upstream keeps the failed execution-gas debit, on both interpreter paths, after `remaining` is normalized.

## `5b9a8ec3` expose transaction-original account information

Commit date 2026-11-02 15:32:49 +0800.
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

SELFDESTRUCT reads `original` for the absent-at-start refund (`mega-evm2/src/lib.rs:535`) and for the beneficiary (`mega-evm2/src/lib.rs:586`).
The write-record helper `changed` reads the same accessor.
The engine does not call `is_changed`.

Upstream: none filed.
Searches for `AccountHandle` on 2026-11-03 did not return a pull request that exposes `original`.
Drop when `AccountHandle::original` is public.

## `7352d38f` expose optional input-only gas quotes

Commit date 2026-11-02 15:32:49 +0800.
1 file changed, 50 insertions, 4 deletions.

File: `crates/evm2/src/precompiles/table.rs`.

`Precompile::with_required_gas(fn(&[u8]) -> u64)` attaches a pure input price.
`Precompile::required_gas(input)` returns `Some(price)` or `None` when the price is unknown.
Replacing the precompile clears the quote.
The quote does not execute the precompile and does not see the environment.
Input validity and chain size caps still need their normal checks.
The same pair exists on the precompile data type that moves with the map.

Detention holds a precompile call whose price is over the allowance and inside the forward, and it has to know that price before the precompile runs (`mega-evm2/src/detention.rs:513`).
The feasibility study measured the precompile quote among the patches that measurement needed.
Entries without a quote are priced by running them on an empty tracker and reading `failed_charge` (`mega-evm2/src/detention.rs:596`).

The Satin map attaches quotes for the catalogued addresses (`mega-evm2/src/lib.rs:253`).
`precompile_price_of` prefers `required_gas` (`mega-evm2/src/detention.rs:596`).

Upstream: none filed.
A search for `required_gas` on 2026-11-03 returned no issues or pull requests.
Drop when a precompile can publish an input-only price without running.

## `e0a93cf0` rebase native precompile message dispatch and reentry context

Commit date 2026-11-02 15:34:26 +0800.
6 files changed, 322 insertions, 37 deletions.

Files: `crates/evm2/src/evm/mod.rs`, `crates/evm2/src/evm/precompile.rs`, `crates/evm2/src/evm/async.rs`, `crates/evm2/src/precompiles/mod.rs`, `crates/evm2/examples/precompile_subcall.rs`, `docs/native-precompile-reentry.md`.

This commit rebases the behavior in open pull request [#523](https://github.com/alloy-rs/evm2/pull/523) onto the fork base.
`PrecompileProvider::contains_message` decides whether a call enters native dispatch.
The default is `contains(call_target)`.
A rejection falls through to bytecode.
`execute` takes `&self` instead of `&mut self`, and dispatch holds a shared provider so a child call can reenter.
`Evm::precompile_tx_env` is the transaction environment captured at native entry, restored on return and on unwind.

Compared on 2026-11-03, `gh pr diff 523 --repo alloy-rs/evm2` and this commit touch the same six files and no others.
Both add `contains_message` (default `contains(call_target)`), change `execute` to `&self`, replace `boxed_precompile_provider` with `shared_precompile_provider` (`Arc`), and add the `precompile_tx_env` field, `Evm::precompile_tx_env`, and `PrecompileTxEnvGuard`.
`precompiles_mut` uses `Arc::get_mut` and expects an idle provider to be exclusively owned.
`set_precompiles` uses the shared provider.
The new `docs/native-precompile-reentry.md` text matches.
The two diffs are not the same text.
The pull request diff is against its own base, which still passes a transaction environment into `Host::execute_message`.
There, `execute_precompile` takes `tx_env: &TxEnv` and stores that clone, and `execute_call_precompile` threads the same argument.
This commit keeps `execute_precompile(message, gas)` and snapshots `Evm::tx_env`.
The pull request's reentry test clones `precompile_tx_env` and passes it into the child `execute_precompile`.
This commit writes the child environment through `tx_env_mut` and calls `execute_precompile` with no transaction-environment argument.
The example differs the same way: the pull request passes `&tx_env` into `Host::execute_message`, and this commit assigns `tx_env_mut` before `execute_message` without that argument.
The pull request's `contains_message` test checks `call_target`.
This commit's test checks `code_address`.
The trait default is `call_target` on both sides.
`set_boxed_precompiles` uses `Arc::from` in the pull request and `into()` in this commit.
The pull request's rust doc says interpreter subcalls must clone `precompile_tx_env` before borrowing the EVM.
This commit's rust doc says the value is a snapshot of `tx_env` at native entry and that interpreter subcalls use the EVM-owned environment.
This commit's `contains_precompile` comment adds that a provider may reject a message, which then runs its bytecode.
The pull request is +328/−43.
This commit is +322/−37.

Satin's limit and access-control contracts share an address with bytecode.
Dispatch has to look at the call kind and the selector, and `DELEGATECALL`, `CALLCODE`, and an unknown selector must run the bytecode (`mega-evm2/src/intercept.rs:1`).
The feasibility study measured interceptor scenarios matching on 8 of 9 cases from this rebase, then on 9 of 9 with the `before` hook.
The same study measured keyless semantics matching with `precompile_tx_env`.
The current engine does not call `precompile_tx_env`.
Keyless deployment is answered from `MessageHooks::before` (`mega-evm2/src/keyless.rs:80`).

`SatinProvider::contains_message` is `mega-evm2/src/intercept.rs:60`.
`SatinProvider::execute` takes `&self` (`mega-evm2/src/intercept.rs:81`).
`OpPrecompiles::execute` takes `&self` (`op-evm2/src/precompiles.rs:310`).

Upstream: [#523](https://github.com/alloy-rs/evm2/pull/523) is open.
On 2026-11-03 it was conflicting with main, review required, last updated 2026-10-29.
Its head is `dan/native-precompile-tx-context`.
A search for `contains_message` and `precompile_tx_env` returned no other issue.
Drop when #523 merges, then delete this rebase if the merged API still provides `contains_message`, `execute(&self)`, and `precompile_tx_env`.

## `35019710` read uncached storage of a database-absent account

Commit date 2026-11-02 15:34:26 +0800.
2 files changed, 15 insertions, 3 deletions.

Files: `crates/evm2/src/evm/db/cache.rs`, `crates/evm2/src/evm/state/mod.rs`.

The recorded subject carries an internal prefix.

`CacheDB::read_absent_storage` defaults to `false`.
While it is false, uncached storage of an account the cache holds as absent is answered as zero and the backing database is not read.
`State::set_read_absent_storage` turns the flag on.
Values stay the same for a consistent database.
The read set changes: the read is actually performed, which is what revm does.

A strict-replay witness needs those reads.
The engine cannot work around it: storage reads go through `CacheDB`.

The differential driver sets the flag (`next/diff/src/driver.rs:219`).
Replay sets it the same way (`next/diff/src/replay.rs:123`).
The gas-cap tests set it on the Satin EVM (`mega-evm2/tests/tx_gas_cap.rs:99`).

Upstream: none filed.
A search for `read_absent` on 2026-11-03 returned no issues or pull requests.
Drop when upstream reads uncached storage of a database-absent account the way revm does, or offers this switch.

## `45817aad` finish rebased hook compatibility and formatting

Commit date 2026-11-02 15:56:15 +0800.
6 files changed, 40 insertions, 38 deletions.

Files: `crates/evm2/src/evm/mod.rs`, `crates/evm2/src/evm/db/cache.rs`, `crates/evm2/src/evm/state/mod.rs`, `crates/evm2/src/interpreter/instructions/system.rs`, `crates/evm2/tests/message_hooks.rs`, `crates/evm2/examples/precompile_subcall.rs`.

No new API.
`State::set_read_absent_storage` becomes a `const fn`.
The absent-storage checks are reformatted.
Tests call `Host::execute_message` without a separate transaction environment, matching the rebased precompile dispatch.
The rest is rustfmt.

`next/` has no call site that exists only because of this commit.
It keeps the precompile rebase and the absent-storage flag building together.

Upstream: none.
This commit is formatting and call-site glue.
Drop it with the commits it repairs.

## `91576d1e` make the EIP-8037 total transaction gas cap configurable

Commit date 2026-11-03 10:41:01 +0800.
2 files changed, 51 insertions, 2 deletions.

Files: `crates/evm2/src/version/mod.rs`, `crates/evm2/src/ethereum/mod.rs`.

`Version::tx_total_gas_cap` bounds `tx.gas` when EIP-8037 is on.
`u64::MAX` disables the cap.
The default is `MAX_TX_TOTAL_GAS_LIMIT_AMSTERDAM` (`2^32 - 1`), written into both base-version tables, so an unmodified Ethereum version rejects a gas limit of `2^32`.
`Version::tx_gas_caps` returns `(tx_total_gas_cap, tx_gas_limit_cap)` under EIP-8037.
Without EIP-8037 the field is unused and `tx_gas_limit_cap` bounds `tx.gas`.

Satin turns EIP-8037 on and sets the execution cap to 200,000,000 (`mega-evm2/src/lib.rs:226`).
Under EIP-8037 that execution cap bounds required execution gas, and the excess of `tx.gas` funds the state-gas reservoir.
Satin sets `tx_total_gas_cap` to `u64::MAX` (`mega-evm2/src/lib.rs:227`), so the block gas limit is the only bound on `tx.gas`.
`mega-evm2/tests/tx_gas_cap.rs:1` states that, and `test_legacy_tx_above_total_cap_executes` (`mega-evm2/tests/tx_gas_cap.rs:150`) sends a gas limit of `2^32`.
The feasibility study predates this commit, so it does not measure this patch.

Upstream: none filed.
A search for `tx_total_gas_cap` on 2026-11-03 returned no issues or pull requests.
Drop when upstream lets a chain set or disable the EIP-8037 total transaction gas cap without changing the default.

## Rebasing the fork

Fetch `origin` (alloy-rs/evm2) and rebase `cz/feat/thin-fork` onto the new `origin/main`.

Textual conflicts are the easy part.
The feasibility study measured the binding cost as a semantic audit of new `remaining()` readers, plus the JIT.
Withheld gas stores only the spendable part in `remaining`.
A new upstream opcode that reads `remaining()` and is not wrapped in `mega-evm2` ignores the withheld part.
The same measurement saw a new `try_spend_state` reader merge with no textual conflict and still change behavior.
After a rebase, read every new `remaining()`, `spend`, and `try_spend_state` site and decide whether Satin's wrappers must see it.

`mega-evm2/src/call_emulation.rs` is a copy of the fork's CALL body (`mega-evm2/src/call_emulation.rs:1`).
It exists so the skip-cold check sees spendable plus withheld.
A clean rebase of `call_with_state_charge` can still leave this copy behind the upstream body.
Diff it against `crates/evm2/src/interpreter/instructions/system.rs` on the rebased pin.

The message-hook associated type has to be set on every `EvmTypesHost` impl, including the JIT host tests in `crates/jit/runtime/src/evm2_evm.rs`.
Build the JIT crate after the rebase, or the interpreter can be fine while the runtime tests are not.

Move `next/evm2-fork.env` only after the new commit is on the public fork.
CI checks the pin out from `EVM2_FORK_REPO` by hash (`.github/workflows/next.yml`).
A pin that was never pushed fails the step "Check that the evm2 fork pin is pushed".
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

`ci-local.sh` is the check job: fmt, clippy, `mega-evm2` and `op-evm2` tests, the `no_std` check, the check that `regen-reference.sh` refuses an output directory in the corpus, `gate.sh sample`, `controls.sh sample` (the negative controls: every switch, off, makes its gate fail) and `faults.sh` (the fault injection: every wrong reference value is rejected).
After a rebase the negative controls matter most: a fix that the new upstream code no longer reaches shows up as a missed control even while the gates pass.
`gate.sh full` and `controls.sh full` need the corpus archive.

## Upstream candidates

Smallest diff first.
Sizes are the commit stat against its parent.
Nothing here was opened or posted on 2026-11-03.

1. Read-absent storage (`35019710`, +15/−3).
   The flag defaults off, so existing chains keep today's answer of zero and no backing read.
   A strict-replay witness needs the read, because revm performs it.
   A search for `read_absent` on 2026-11-03 returned no issues or pull requests.
2. Transaction-original account info (`5b9a8ec3`, +16).
   One accessor on a cold path.
   Write records and start-of-transaction refunds compare an account with its info at the transaction boundary.
   A snapshot at first observation is exact only when that observation precedes every change.
3. Input-only precompile quote (`7352d38f`, +50/−4).
   Optional, and `None` means unknown.
   A holder has to know the price before the precompile runs.
   Entries without a quote are priced by running them.
4. Configurable total gas cap (`91576d1e`, +51/−2).
   The default stays `2^32 - 1`, written into both base version tables.
   A chain that only wants the Amsterdam cap is unchanged (inferred from that default).
5. Failed-charge record (`a819dc44`, +98/−3).
   #752 already reports the inspector out-of-gas step cost, and that change is in the base.
   File this as a consensus record that survives normalization, or the inspector discussion will be taken to cover it.
6. Code-deposit admission (`3c25bc3b`, +129/−13).
   One creation site, off the interpreter loop.
   A revert has to restore the pre-charge gas.
   The feasibility study measured that refusing in `before_commit` was 180 gas off, because `before_commit` does not see the pre-charge snapshot.
7. State-gas price (`52c4854d`, +135/−33).
   Any chain that prices state by location can use it.
   The call, create, SSTORE, and EIP-7702 charge sites all call it, so the caller shape should be agreed before a patch is written.
8. Pre-state-charge closure (`5cf8d414`, +146/−3).
   #887 merged the around-child hooks and left this closure in the pull request body, under "A related point, raised here rather than added."
   A follow-up to #887 is the smallest place to ask again.
9. Static message hooks (`a9549934`, +474/−32).
   Largest patch, and it runs on every message.
   The feasibility study measured these hooks below the call helpers that have already merged.
   The same study measured a dynamic hook at 13–17% wall-clock on empty calls, which is why the proposal is a static associated type.
   It waits until the smaller hooks have an answer, because it changes every message.

[#523](https://github.com/alloy-rs/evm2/pull/523) is already open.
The rebase in `e0a93cf0` should not become a second pull request.
`45817aad` is formatting and call-site glue, not an upstream change.

#842 and #887 have already merged and are in the base.
They are not candidates.
