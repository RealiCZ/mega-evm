# The thin evm2 fork

`next/` builds against a thin fork of [alloy-rs/evm2](https://github.com/alloy-rs/evm2): [`RealiCZ/evm2`](https://github.com/RealiCZ/evm2), branch `cz/feat/thin-fork-v2`.
`evm2-fork.env` holds the pin (`EVM2_FORK_REPO`, `EVM2_FORK_REV`); it is the only place CI reads it from.
The series sits on upstream `85395c54`, not on upstream's head: that is the evm2 revision reth's pull request #28276 moves reth to, so the node and the engine can share one evm2.
The earlier series, branch `cz/feat/thin-fork` at `91576d1e`, is left as it was, so earlier pins still resolve.

A patch exists only when the engine needs something evm2 cannot give it from outside: data from inside evm2's own flow, or a hook point.
Every patch keeps stock behavior as its default, and each says what upstream change would let it go.
One patch, `04f0e24f`, is a stock fix instead: it moves an instruction to revm's order, which evm2 follows, and is written to be proposed upstream as it is.
The engine also uses two upstream changes that are already in the base: `call_with` / `create_with` ([#887](https://github.com/alloy-rs/evm2/pull/887)) and reservoir-funded state gas in `system_call` totals ([#842](https://github.com/alloy-rs/evm2/pull/842)).

| Patch                                                                               | Adds                                                             | Size     | Upstream                                                                                                                  | Drop when upstream                                                                         |
| ----------------------------------------------------------------------------------- | ---------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `70bbeb67` select consensus and observation message hooks statically                | `MessageHooks`, a static per-message hook type                   | +576/−36 | none yet                                                                                                                  | has a static hook on every message, answers included, before the commit decision           |
| `9df29b6a` price state charges by account and storage location                      | `EvmTypesHost::state_gas_price`, `StateGasSite`                  | +471/−34 | none yet                                                                                                                  | can price a state charge by its account or storage slot                                    |
| `f623a776` expose a code deposit admission hook                                     | `MessageHooks::admit_code_deposit`                               | +83/−9   | none yet                                                                                                                  | has an admission hook whose revert restores the pre-charge gas                             |
| `099810ca` retain failed debit before normalization on every interpreter path       | `GasTracker::failed_charge`                                      | +106/−3  | none yet                                                                                                                  | keeps the failed execution-gas debit on both paths after `remaining` is normalized         |
| `09965c55` let a type family hold a frame's gas in reserve                          | `gas_reserve` / `draw_gas_reserve`, `BudgetUse`, `ReserveDraw`   | +768/−26 | none yet                                                                                                                  | lets execution gas outside `remaining` reach budget decisions and non-execution deductions |
| `2227aa3c` expose transaction-original account information                          | `AccountHandle::original`, `AccountHandle::is_changed`           | +59      | issue [#1024](https://github.com/alloy-rs/evm2/issues/1024), PR [#1025](https://github.com/alloy-rs/evm2/pull/1025), open | makes `AccountHandle::original` public                                                     |
| `68547bdf` optionally read uncached storage of accounts the database reports absent | `CacheDB::read_absent_storage`, `State::set_read_absent_storage` | +61/−3   | none yet                                                                                                                  | reads that storage as revm does, or offers this switch                                     |
| `82427081` make the EIP-8037 total transaction gas cap configurable                 | `Version::tx_total_gas_cap`                                      | +51/−2   | PR [#1023](https://github.com/alloy-rs/evm2/pull/1023), open                                                              | lets a chain set or disable the total cap, default unchanged                               |
| `2789b9e5` let a type family refuse an instruction's account load                   | `EvmTypesHost::before_account_load`, `AccountLoadSite`           | +395/−5  | none yet                                                                                                                  | lets a type family stop an instruction at its own account load                             |
| `04f0e24f` pop CREATE2's salt after the init-code and memory charges                | none (instruction order)                                         | +33/−1   | not yet proposed                                                                                                          | pops CREATE2's salt after the init-code charge and the memory expansion                    |

Size is the commit's own stat against its parent; most of the insertions of `09965c55`, `9df29b6a`, `2789b9e5` and `04f0e24f` are tests.
Upstream order, smallest and least contested first: `2227aa3c`, `82427081` (both filed), `04f0e24f`, `68547bdf`, `f623a776`, `099810ca` with the halt record of `09965c55`, `9df29b6a`, the rest of `09965c55`, `2789b9e5`, and `70bbeb67` last, because it runs on every message.

## The patches

Each engine rule a patch serves sits behind a named switch with a negative control in `diff/negative-controls.toml` (a harness switch for `68547bdf`); turning the switch off makes the control's case differ from the reference.
`82427081`'s setting is a version constant, proved by tests instead.

### `70bbeb67` static message hooks

- **API.** `EvmTypesHost::MessageHooks` is an associated type; `()` is the no-op, which the stock families, `op-evm2` and the JIT host tests use.
  The `MessageHooks` trait runs on every message, on the plain and the inspected path: consensus methods `before` (may answer the message), `started` (after the checkpoint, value transfer and creation setup), `frame_built`, `before_commit` (may rewrite the stop before commit or rollback) and `after`; observers `enter`, `unsettled` and `exit`.
  Every built-in result decided after the message's checkpoint reaches `before_commit`: a frame's, a precompile's, an account without code's, a call's out-of-funds answer and a creation's failure to create its account (a collision, an endowment the creator cannot pay); a result decided before the checkpoint (a call or creation too deep, a creation's balance or nonce check) does not (`out_of_funds_call_reaches_before_commit_on_both_paths`, `create_collision_reaches_before_commit_on_both_paths`).
  A message that halts without executing (`Evm::execute_halted_message`) reaches none of them, on either path (`halted_messages_run_no_hooks_on_either_path`).
- **Why.** Satin's frame-start rules stop a frame before its code runs, take back a success answer, and answer the system contracts before evm2 moves value.
  The engine's `StartHooks` implements them: `pre_frame_stop`, `tx_beneficiary`, `keyless`, `access_control`, `oracle_hints`, `precompile_hold` and `top_precompile_hold` (`before`); `hold_start` (`started`); `hold` (`frame_built`); `classify`, `built_check`, `unbuilt_burn` and `deposit_loan_reset` (`before_commit`).
  Detention counts the whole forward of a message that halts without a built frame as burned in `before_commit` (`unbuilt_burn`), so a creation that collides after a volatile read is burned, not compute (the `bd` collision scenarios).
  The type is static so that a family without hooks compiles no hook calls on the message path.
- **Upstream.** None filed.
- **Drop** when upstream offers a static per-message hook that runs for every message, including answers with no interpreter frame, before the commit decision.

### `9df29b6a` state-gas price by location

- **API.** `EvmTypesHost::state_gas_price(host, id, site) -> Result<u64, ExecutionError>` prices one schedule entry at the state it touches; `StateGasSite` is `Account(address)` or `Storage(address, key)`.
  The default returns `gas_params.get(id)`.
  The call, create, SSTORE, SELFDESTRUCT, code-deposit, first-frame and EIP-7702 charge sites call it; an error aborts the transaction, never a halt; a code deposit of empty code is not priced.
  Tests price every site at a multiple of the schedule and make the hook fail (`instruction_state_charges_take_the_family_price`, `initial_frame_state_charges_take_the_family_price`, `accepted_authorization_state_charges_take_the_family_price`, `a_failed_price_aborts_the_message`, `empty_code_deposit_is_not_priced`).
  The seam is interpreter-only: the JIT builtins price state gas from the schedule directly.
- **Why.** Satin's SALT price is the schedule entry times the bucket multiplier of that account or slot, stable for a location within a transaction (`MegaTypes::state_gas_price`, `reprice`).
  A bucket whose capacity is below the minimum fails the transaction through the hook's error (`salt_min_bucket`).
  Controls: `salt`, `deposit_caller`, `salt_min_bucket`.
- **Upstream.** None filed; the caller shape has to be agreed first, because every state-gas charge site calls it.
- **Drop** when upstream can price a state charge by account and storage location.

### `f623a776` code-deposit admission

- **API.** `MessageHooks::admit_code_deposit` runs after the code-deposit checks and charges, before installation and `before_commit`.
  `None` accepts; `Some((stop, output))` rejects with a non-success stop.
  `Revert` restores `gas_before`, the frame's gas from before the deposit charges; other stops keep the charged gas until ordinary settlement.
- **Why.** Satin's admission charges the deposited bytes' history and refuses after the charges, restoring the pre-charge gas, which `before_commit` cannot see (`StartHooks::admit_code_deposit`).
  The deposit's state spill draws withheld gas through the gas reserve, and the admission settles that draw (`DepositLoan`), because `gas_before` does not hold it.
  Controls: `deposit_loan_back`, `deposit_site`, `keyless_empty_deposit`.
- **Upstream.** None filed.
- **Drop** when upstream offers an admission hook that can restore the pre-charge gas on revert.

### `099810ca` failed-charge record

- **API.** `GasTracker::failed_charge` returns the last failed execution-gas debit as `(remaining_before, cost)`; `record_failed_charge` and `clear_failed_charge` write and clear it.
  It is written on the failure side of `spend` and of the dispatcher's static gas, on both interpreter paths, before wrapping or inspector normalization zeroes `remaining`.
  `try_spend` and `try_spend_state` failures do not record; child records are not merged.
  The record is interpreter-only: the JIT builtins record no failed charge.
- **Why.** Detention classifies an out-of-gas halt by it: a debit that spendable plus withheld gas could pay is a crossing, anything else burns (`detention::before_commit`).
  Precompiles outside the engine's price table are priced by running them on a tracker with no gas and reading the record (`precompile_price_of`).
  Controls: `classify`, `precompile_catalog`.
- **Upstream.** None filed for this record.
  [#752](https://github.com/alloy-rs/evm2/pull/752), in the base, reports the inspector's out-of-gas step cost; this is a separate, consensus-side ask.
- **Drop** when upstream keeps the failed execution-gas debit, on both interpreter paths, after `remaining` is normalized.

### `09965c55` gas reserve

- **API.** `EvmTypesHost::gas_reserve(host, BudgetUse) -> u64` reports execution gas the running frame holds outside `remaining`; `draw_gas_reserve(host, amount, ReserveDraw) -> u64` moves up to `amount` of it back and returns what it moved.
  Both default to no reserve and are `#[inline(always)]`.
  `BudgetUse` names the decision (`Gas`, `ColdAccount`, `ColdStorage`, `SstoreSentry`, `DelegationGuard`, `Forward`); `ReserveDraw` names the deduction (`Forward`, `StateSpill(GasId)`).
  Instructions reach it through `gas_budget`, `spend_reserve_first` and `spend_state_reserve_first`: 10 budget-reader sites (the CALL and CREATE forward bases are two), 5 state-spill sites and 2 forward draws.
  `GasTracker::record_no_debit_halt` records a halt that made no debit; `GasTracker::halt_remaining` returns what a halting frame had, from that record or the failed charge.
  The halt record is left out of `GasTracker`'s equality and hash: a stock family's plain and stepped results of a no-debit halt compare equal (`stock_no_debit_halt_settles_equal_on_both_paths`).
  The seam is interpreter-only: the JIT builtins read `remaining` alone.
- **Why.** Detention withholds part of a frame's regular gas outside `remaining`; budget decisions must see it, and deductions that are not the frame's own execution must draw it first (`MegaTypes::gas_reserve` and `draw_gas_reserve`, over `detention::reserve_for` and `detention::draw_reserve`).
  Each `BudgetUse` and each draw has its own control: `gas_total`, `cold_account`, `sload_copy`, `sstore_sentry`, `delegate_no_debit`, `forward`, `lend`, `create_lend`, `new_account_lend`, `deposit_site`; the halt record has `no_debit_record` and `hand_back`.
  With the defaults the helpers reduce to the stock code.
- **Upstream.** None filed; it needs a design issue first, since the JIT builtins would need the same helpers.
- **Drop** when upstream lets a type family keep execution gas outside `remaining` that budget decisions see and non-execution deductions draw first, with a halt record that survives the inspected path.

### `2227aa3c` transaction-original account info

- **API.** `AccountHandle::original` returns the account info at the transaction boundary (`None` for an account absent then); loading bytecode does not move it.
  `AccountHandle::is_changed` reports whether existence or persisted account fields differ from it; storage-only writes do not count (`original_is_the_transaction_baseline`).
- **Why.** Write records and SELFDESTRUCT's absent-at-start refund compare an account with its start-of-transaction info; a snapshot at first observation is exact only when that observation precedes every change.
  The engine reads `original` in SELFDESTRUCT (the absent-at-start test and the beneficiary's record) and in the write-record helper `ledger::changed`; it does not call `is_changed`.
  Controls: `sd_existing_record`, `sd_records`, `sd_absent_leaf`.
- **Upstream.** Issue [#1024](https://github.com/alloy-rs/evm2/issues/1024) and PR [#1025](https://github.com/alloy-rs/evm2/pull/1025), open; the PR exposes `original` only.
- **Drop** when `AccountHandle::original` is public.

### `68547bdf` reads of absent accounts' storage

- **API.** `CacheDB::read_absent_storage`, default `false`: uncached storage of an account the cache holds as absent is answered as zero without a backing read.
  `State::set_read_absent_storage` turns it on; values stay the same for a consistent database, and the read is performed, as revm performs it (`absent_account_storage_is_read_only_when_asked`).
  The flag is part of the state: a restored snapshot, a clone and a committed copy keep it (`read_absent_storage_survives_snapshots_and_copies`).
- **Why.** A strict-replay witness needs those reads, and storage reads go through `CacheDB`, so the engine cannot add them from outside.
  The gate harness sets the flag (harness control `absent_storage_read`), and so do the engine's gas-cap tests.
- **Upstream.** None filed.
- **Drop** when upstream reads uncached storage of a database-absent account the way revm does, or offers this switch.

### `82427081` configurable total gas cap

- **API.** `Version::tx_total_gas_cap` bounds `tx.gas` when EIP-8037 is on; `u64::MAX` disables it.
  The default is `MAX_TX_TOTAL_GAS_LIMIT_AMSTERDAM` (2^32 − 1) in both base version tables, so an unmodified Ethereum version still rejects 2^32.
  `Version::tx_gas_caps` returns `(tx_total_gas_cap, tx_gas_limit_cap)` under EIP-8037; without it the field is unused.
- **Why.** Satin applies no total cap: the block's gas limit alone bounds `tx.gas`, while the execution cap of 200,000,000 bounds required execution gas and the rest funds the reservoir.
  `satin_version` sets `tx_total_gas_cap` to `u64::MAX`; a version constant, not a switch.
  Proof: `mega-evm2/tests/tx_gas_cap.rs` (gas limits of 2^32 and above for every transaction type and in a block) and the `cap_` scenarios of the main gate.
- **Upstream.** PR [#1023](https://github.com/alloy-rs/evm2/pull/1023), open (the cap itself is issue #807 and PR #862 upstream).
- **Drop** when upstream lets a chain set or disable the EIP-8037 total transaction gas cap without changing the default.

### `2789b9e5` account-load hook

- **API.** `EvmTypesHost::before_account_load(state, address, AccountLoadSite) -> Result` runs where an instruction is about to load an account: after every charge the instruction makes before that load, before the load and its EIP-2929 skip-cold check.
  A stop it returns fails the instruction there, with nothing loaded; the default admits every load and is `#[inline(always)]`.
  `AccountLoadSite` names the load: `Balance`, `SelfBalance`, `ExtCodeSize`, `ExtCodeHash`, `ExtCodeCopy`, `CallTarget`, `CallDelegate` (after the delegation guard), `SelfDestructContract`, `SelfDestructTarget`, `CreateCaller` and `CreateDestination`.
  The hook gets the interpreter state, so it sees the loading frame's message; the plain and the inspected path run the same instruction bodies.
  Call sites: the account opcodes' load helper (BALANCE, EXTCODESIZE, EXTCODEHASH, EXTCODECOPY), SELFBALANCE, the call family's target and delegate, CREATE's and CREATE2's creator and created address, SELFDESTRUCT's account and beneficiary.
  Tests: `every_instruction_load_reaches_the_hook`, `a_refused_load_stops_the_instruction_before_the_load`, `the_hook_runs_after_the_charges_before_the_load`, `the_hook_sees_the_loading_frame`.
  The seam is interpreter-only: the JIT builtins load accounts without consulting it.
- **Why.** Satin refuses a read of the block beneficiary's account in a frame whose volatile access is off where the account is loaded, after the charges the instruction makes before that load.
  `MegaTypes::before_account_load` refuses the beneficiary's load at every site but a creation's, and the opcode's wrapper reverts the frame on the gas it had after its static gas (`settle_refusal`), so the engine does not restate evm2's charge order before a load.
  The engine's SELFBALANCE keeps a body of its own, because revm refuses before the push's stack check and evm2's body checks the stack first (`selfbalance_stack_order`); evm2's `SelfBalance` site is not reached.
  Controls: `refuse_at_load`, `refusal_gas`, `delegate_refusal`; the `bd` scenarios run every refusable instruction through the gas windows around each charge before its load.
- **Upstream.** None filed.
- **Drop** when upstream lets a type family stop an instruction at each of its account loads, after the instruction's charges before the load and before its skip-cold check.

### `04f0e24f` CREATE2's salt order

- **API.** None: `create_with` (and so CREATE2) pops `value`, `offset` and `len`, makes the EIP-3860 init-code charge and the memory expansion, and only then pops the salt and charges the CREATE2 cost, as revm's `create` does.
  A CREATE2 whose salt is missing now fails on the first of those charges it cannot pay, and otherwise underflows with both paid (`create2_pops_its_salt_after_the_init_code_and_memory_charges`).
  The JIT checks a section's stack before the section runs and is unchanged.
- **Why.** Under detention a charge the frame cannot pay is a crossing: the frame stops at the compute limit and keeps its gas, while a stack underflow halts and burns it.
  With the salt popped first, a detained frame running CREATE2 on three stack items halted where the reference reverts at the crossing (the `edge_create2_*` scenarios).
  Under Ethereum's rules both halts consume the frame's gas, so on a stock family the change is the halt reason and the gas a halted frame reports.
  No switch: the order is evm2's own, so the engine has nothing to turn off; the scenarios show it.
- **Upstream.** Not yet proposed.
  evm2 follows revm's control flow and gas accounting, so the change can be proposed as it is.
- **Drop** when upstream pops CREATE2's salt after the init-code charge and the memory expansion.

## Dropped from the earlier series

- The rebase of open upstream PR [#523](https://github.com/alloy-rs/evm2/pull/523) (native precompile calls): the engine answers `MegaLimitControl` and `MegaAccessControl` from `MessageHooks::before`, which runs before dispatch.
- Input-only precompile gas quotes: the engine prices the fixed-formula entries itself (`precompile_price`) and probes the others (`precompile_price_of`).
- Formatting and call-site glue: folded into the commits it repaired.
- The pre-state-charge closures `call_with_state_charge` and `create_with_state_charge`: the state spills draw the gas reserve inside evm2's own bodies, and the engine calls `call_with` and `create_with`.
- The code-deposit funding hook `MessageHooks::before_code_deposit`: the deposit's state spill draws the gas reserve.

## Known follow-up

The engine's mirrored EIP-7702 path (`mega-evm2/src/eip7702.rs`) settles a runtime-gas-phase out-of-gas without a frame, unlike upstream's `eip7702::execute_prepared`, which reports a halted first frame; owner decision 2026-11-05: left as a follow-up, not a gate entry.

## Rules for the next rebase

Rebase `cz/feat/thin-fork-v2` onto the revision the node pins, which need not be `origin/main`'s head.
Textual conflicts are the easy part; these are the checks a clean merge does not make:

- **Budget readers and state spills go through the seam.**
  Read every new `remaining()`, `spend_state` and `try_spend_state` site under `crates/evm2/src/interpreter/instructions` and in `crates/evm2/src/evm/mod.rs`.
  A budget decision (a skip-cold check, a guard, a forward base, a value pushed to the stack) must read `gas_budget` with its `BudgetUse`; a state charge must call `spend_state_reserve_first`; a forward must call `spend_reserve_first`.
  A new reader that merges without a conflict still ignores the withheld gas.
  Recount the sites, update the count above, and give a new `BudgetUse` or draw its own engine switch and control.
- **The no-debit halt record stays at the inspected zeroing point.**
  `inspect_step_end` in `crates/evm2/src/interpreter/runtime.rs` zeroes `remaining` at an out-of-gas instruction the inspector steps; the record must be written there, before the zeroing, and any upstream rework of that function has to keep it.
  The failed-charge record must cover every dispatch backend upstream adds.
- **Fork tests of the inspected path need a stepping inspector.**
  An inspector chooses the opcodes it steps, and `NoopInspector` steps none: its frames run the plain loop and never reach the zeroing point.
  Use an inspector that steps every opcode, as the gates' frame recorder does.
- **Halted messages reach no hook.**
  A first frame whose runtime gas phase runs out of gas is reported through `Evm::execute_halted_message`, which runs no message hook on either path; keep `halted_messages_run_no_hooks_on_either_path` passing, and decide any new message path upstream adds against it.
- **Every result decided after a message's checkpoint reaches `before_commit`.**
  Read every early return of `execute_call_message`, `execute_create_message` and their helpers in `crates/evm2/src/evm/mod.rs`: one after `self.state.checkpoint()`, other than an answer of the `started` hook (the family's own), must run `hook_before_commit` before its commit or rollback, as the out-of-funds and collision answers do; detention counts a halting unbuilt message's forward as burned only there.
- **Instructions that run a child message are `hooked`.**
  The engine's CALL and CREATE family and its keyless dispatcher are declared with `hooked` in `#[instruction(...)]`; an inspected run panics at the first message otherwise.
- **Every account load of an instruction offers the account-load hook first.**
  Read every `load_account` and `selfdestruct` host call under `crates/evm2/src/interpreter/instructions`: each must call `before_account_load` with its site right before the load, after the instruction's charges before it and before its skip-cold check.
  A new load that merges without a conflict cannot be refused; a charge moved across a load moves the refusal with it, which the `bd` scenarios' gas windows show.
  Give a new site its engine decision in `refuses_load`; the controls `refuse_at_load`, `refusal_gas` and `delegate_refusal` show the rule.
- **Every `EvmTypesHost` impl sets `MessageHooks`**, including the JIT host tests in `crates/jit/runtime/src/evm2_evm.rs`.
  Build the JIT crate after the rebase: the interpreter can build while the runtime tests do not.
- **The engine mirrors `execute_prepared`.**
  Types 0, 1 and 2 (`execute_user` in `mega-evm2/src/tx.rs`) and EIP-7702 (`mega-evm2/src/eip7702.rs`) run their own copy of evm2's `execute_prepared`, so that the first frame's start record is charged after validation; compare both with upstream's `legacy`, `eip2930`, `eip1559` and `eip7702::execute_prepared` after a rebase, as a clean merge does not.
- **Run the negative controls, not only the gates.**
  A fix the new upstream code no longer reaches shows up as a missed control while every gate still passes; `diff/scripts/ci-local.sh` runs the sample controls, `controls.sh full` the rest.

## Moving the pin

CI checks the pin out from `EVM2_FORK_REPO` by hash (`scripts/fetch-evm2-fork.sh`), without credentials.
Push the new commit to the public fork first, then set `EVM2_FORK_REV`; a pin that was never pushed fails the step "Check out the evm2 fork at its pin".
Keep the fork repository public.
Rebase onto a new branch rather than force-pushing, so that every earlier pin stays fetchable.
Local builds use the checkout next to this repository (`../evm2` from the repository root).
