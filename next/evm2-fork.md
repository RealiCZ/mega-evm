# The thin evm2 fork

`next/` builds against a thin fork of [alloy-rs/evm2](https://github.com/alloy-rs/evm2): [`RealiCZ/evm2`](https://github.com/RealiCZ/evm2), branch `cz/feat/thin-fork-v3` (`1a70ed94`), and one commit past it that makes the message hooks' copy-skipping constant the no-op hooks' own (`c58811c2`).
`evm2-fork.env` holds the pin (`EVM2_FORK_REPO`, `EVM2_FORK_REV`); it is the only place CI reads it from.
The series sits on upstream `e0d398a6`, upstream `main` on 2026-11-10, which has merged two patches of the earlier series ([#1023](https://github.com/alloy-rs/evm2/pull/1023) and [#1025](https://github.com/alloy-rs/evm2/pull/1025), see [Merged upstream](#merged-upstream)) and recovers EIP-7702 authorities with the installed crypto provider ([#963](https://github.com/alloy-rs/evm2/pull/963)).
The earlier series sat on `85395c54`, the evm2 revision reth's pull request #28276 moves reth to; until the node moves to `e0d398a6` or later, the node and the engine run different evm2 revisions.
The earlier series, branches `cz/feat/thin-fork` at `91576d1e` and `cz/feat/thin-fork-v2` at `28e20011`, are left as they were, so earlier pins still resolve.

A patch exists only when the engine needs something evm2 cannot give it from outside: data from inside evm2's own flow, or a hook point.
Every patch keeps stock behavior as its default, and each says what upstream change would let it go.
Two patches are stock fixes instead, written to be proposed upstream as they are: `b9117de6` moves a check to revm's order, which evm2 follows, and `b90eac57` keeps a nightly aarch64 build free of an unused-feature warning.
ModExp keeps upstream's order (its 500 minimum, its formula price, then EIP-7823's bound, upstream [#874](https://github.com/alloy-rs/evm2/pull/874)): the Satin specification follows upstream there, so the series carries no ModExp patch.
The engine also uses upstream changes that are already in the base: `call_with` / `create_with` ([#887](https://github.com/alloy-rs/evm2/pull/887)), reservoir-funded state gas in `system_call` totals ([#842](https://github.com/alloy-rs/evm2/pull/842)), and the two merged patches.

| Patch                                                                                                        | Adds                                                                                  | Size     | Upstream                                                                                                 | Drop when upstream                                                                         |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------- | -------- | -------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `b90eac57` enable `fn_align` only on x86-64, where the tco handlers use it                                   | none (a crate attribute)                                                              | +1/−1    | not yet proposed                                                                                         | enables `fn_align` only where the tco handlers are aligned                                 |
| `8d137704` select consensus and observation message hooks statically                                         | `MessageHooks`, a static per-message hook type                                        | +797/−36 | none yet                                                                                                 | has a static hook on every message, answers included, before the commit decision           |
| `1de8b147` price state charges by account and storage location                                               | `EvmTypesHost::state_gas_price`, `StateGasSite`                                       | +471/−34 | none yet                                                                                                 | can price a state charge by its account or storage slot                                    |
| `1baf47da` expose a code deposit admission hook                                                              | `MessageHooks::admit_code_deposit`                                                    | +83/−9   | none yet                                                                                                 | has an admission hook whose revert restores the pre-charge gas                             |
| `eb469387` retain failed debit before normalization on every interpreter path                                | `GasTracker::failed_charge`                                                           | +165/−4  | none yet                                                                                                 | keeps the failed execution-gas debit on both paths after `remaining` is normalized         |
| `d01b0e32` let a type family hold a frame's gas in reserve                                                   | `gas_reserve` / `draw_gas_reserve`, `BudgetUse`, `ReserveDraw`                        | +768/−26 | none yet                                                                                                 | lets execution gas outside `remaining` reach budget decisions and non-execution deductions |
| `0d9f096e` optionally read uncached storage of accounts the database reports absent                          | `CacheDB::read_absent_storage`, `State::set_read_absent_storage`                      | +61/−3   | none yet                                                                                                 | reads that storage as revm does, or offers this switch                                     |
| `f4749e70` let a type family refuse an instruction's account load                                            | `EvmTypesHost::before_account_load`, `AccountLoadSite`                                | +395/−5  | none yet                                                                                                 | lets a type family stop an instruction at its own account load                             |
| `b9117de6` pop CREATE2's salt after the init-code and memory charges                                         | none (instruction order)                                                              | +33/−1   | not yet proposed                                                                                         | pops CREATE2's salt after the init-code charge and the memory expansion                    |
| `cf70453a` let a type family act on the transaction before its first frame                                   | `TxHandlerHooks::before_initial_frame`                                                | +375/−45 | not yet proposed                                                                                         | has a handler hook before the first frame that can charge the transaction or settle it     |
| `736f76b2` let a version credit state-gas give-backs to the reservoir only                                   | `EvmFeatures::EIP8037_RESERVOIR_REFILLS`, `give_back_state_gas`, `merge_returned_gas` | +603/−16 | the rule is ethereum/EIPs [#12581](https://github.com/ethereum/EIPs/pull/12581), open; none in evm2      | has the reservoir-only give-back rule as a version option                                  |
| `a3b2779f` let a version take a legacy share of a state charge from execution gas                            | the four `GasId::*LegacyShare` parameters, `spend_state_with_share_reserve_first`     | +232/−8  | the share is ethereum/EIPs [#12581](https://github.com/ethereum/EIPs/pull/12581), open; none in evm2     | has a legacy share of the new-state charges as gas parameters                              |
| `1a70ed94` pin the tracker's accessors and equality, and the handler's bounds and first-frame paths          | tests only                                                                            | +586     | none (tests of the series and of upstream paths)                                                         | tests                                                                                      |
| `c58811c2` make the message hooks' copy-skipping constant the no-op hooks' own                               | none (`MessageHooks::RUNS_BEFORE` hidden; documentation and tests)                    | +165/−8  | none (goes with `8d137704`)                                                                              | goes with `8d137704`                                                                       |

Size is the commit's own stat against its parent; most of the insertions of `8d137704`, `d01b0e32`, `1de8b147`, `f4749e70`, `b9117de6`, `cf70453a`, `736f76b2`, `a3b2779f` and `c58811c2` are tests, and all of `1a70ed94`'s.
Upstream order, smallest and least contested first: `b90eac57`, `b9117de6`, `cf70453a`, `0d9f096e`, `1baf47da`, `eb469387` with the halt record of `d01b0e32`, `1de8b147`, the rest of `d01b0e32`, `f4749e70`, and `8d137704` last (with `c58811c2`), because it runs on every message.
`736f76b2` and `a3b2779f` follow ethereum/EIPs #12581: offered with it if Ethereum takes that rule into EIP-8037, kept as Satin's own departure if it does not.

## The patches

Each engine rule a patch serves sits behind a named switch with a negative control in `diff/negative-controls.toml` (a harness switch for `0d9f096e`); turning the switch off makes the control's case differ from the reference.
`b9117de6` changes evm2's own instruction, which the `edge_create2_*` scenarios show; `cf70453a` carries the start-record charge, whose switches (`start_at_runtime`, `start_reads_no_target`, `tx_spill_unwind`) have their controls.

### `b90eac57` `fn_align` on x86-64 only

- **API.** None: the crate enables the `fn_align` feature only for a tco build on x86-64, the one target whose tco handlers `rustc_align(64)` aligns (upstream [#1053](https://github.com/alloy-rs/evm2/pull/1053)).
- **Why.** On another 64-bit target with a nightly compiler, aarch64 among them, the tco backend is the default and the feature is enabled but unused, so `unused_features` warns and `cargo clippy -- -D warnings` fails on upstream's own code; upstream's clippy job runs on x86-64 only.
  The engine does not need it; the fork's checks do.
- **Upstream.** Not yet proposed; a one-line fix.
- **Drop** when upstream enables `fn_align` only where it uses it.

### `8d137704` static message hooks

- **API.** `EvmTypesHost::MessageHooks` is an associated type; `()` is the no-op, which the stock families, `op-evm2` and the JIT host tests use.
  The `MessageHooks` trait runs on every message, on the plain and the inspected path: consensus methods `before` (may answer the message), `started` (after the checkpoint, value transfer and creation setup), `frame_built`, `before_commit` (may rewrite the stop before commit or rollback) and `after`; observers `enter`, `unsettled` and `exit`.
  Every built-in result decided after the message's checkpoint reaches `before_commit`: a frame's, a precompile's, an account without code's, a call's out-of-funds answer and a creation's failure to create its account (a collision, an endowment the creator cannot pay); a result decided before the checkpoint (a call or creation too deep, a creation's balance or nonce check) does not (`out_of_funds_call_reaches_before_commit_on_both_paths`, `create_collision_reaches_before_commit_on_both_paths`).
  A message that halts without executing (`Evm::execute_halted_message`) reaches none of them, on either path (`halted_messages_run_no_hooks_on_either_path`).
  `before` gets the message by `&mut` and may keep its input, so the message path first copies a call input that borrows the caller's memory (upstream [#1030](https://github.com/alloy-rs/evm2/pull/1030) lends it to the child; `kept_input::before_keeps_an_input_the_caller_overwrites_after_the_call`).
  The no-op `()`, which runs no `before`, skips both through `MessageHooks::RUNS_BEFORE = false`, so the stock families keep the borrow (`input_copy::the_no_op_hooks_leave_a_call_input_in_the_callers_memory`).
  The constant is `#[doc(hidden)]` and `()`'s alone (`c58811c2`): a family that overrides `before` leaves it at `true`, because with `false` its `before` would never be called and nothing would catch it.
  The other hooks get the message by shared reference, which cannot move the input out.
  With an inspector installed, `Host::execute_message` copies the input before `enter` too: a message `enter` runs gets the inspector's hooks, which get the suspended caller and can change or grow its memory (`nested_enter::entered_input_survives_a_nested_inspector_that_moves_the_caller_memory`, which also runs under Miri).
- **Why.** Satin's frame-start rules stop a frame before its code runs, take back a success answer, and answer the system contracts before evm2 moves value.
  The engine's `StartHooks` implements them: `pre_frame_stop`, `tx_beneficiary`, `keyless`, `keyless_value_hold`, `access_control`, `oracle_hints`, `precompile_hold` and `top_precompile_hold` (`before`); `first_frame_hold` and `hold_start` (`started`); `hold` (`frame_built`); `classify`, `built_check`, `unbuilt_burn`, `deposit_loan_reset` and `first_frame_hold` (`before_commit`).
  Detention counts the whole forward of a message that halts without a built frame as burned in `before_commit` (`unbuilt_burn`), so a creation that collides after a volatile read is burned, not compute (the `bd` collision scenarios).
  The type is static so that a family without hooks compiles no hook calls on the message path.
  `StartHooks` has a `before`, so a call's input is copied before it, as it was before #1030: no output changes, and the engine does not get #1030's saving.
- **Upstream.** None filed.
- **Drop** when upstream offers a static per-message hook that runs for every message, including answers with no interpreter frame, before the commit decision.

### `1de8b147` state-gas price by location

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

### `1baf47da` code-deposit admission

- **API.** `MessageHooks::admit_code_deposit` runs after the code-deposit checks and charges, before installation and `before_commit`.
  `None` accepts; `Some((stop, output))` rejects with a non-success stop.
  `Revert` restores `gas_before`, the frame's gas from before the deposit charges, except under the reservoir-only rule (`EvmFeatures::EIP8037_RESERVOIR_REFILLS`, `736f76b2`), where the charges stay and the revert's settlement gives the state charges into the reservoir while the hashing stays spent; other stops keep the charged gas until ordinary settlement.
- **Why.** Satin's admission charges the deposited bytes' history and refuses after the charges, restoring the pre-charge gas, which `before_commit` cannot see (`StartHooks::admit_code_deposit`).
  The deposit's state spill draws withheld gas through the gas reserve, and the admission settles that draw (`DepositLoan`), because `gas_before` does not hold it; under Satin's rule the drawn gas went into the reservoir with the spill, and the withheld gas does not get it back.
  Controls: `deposit_loan_back`, `deposit_site`, `keyless_empty_deposit`.
- **Upstream.** None filed.
- **Drop** when upstream offers an admission hook that can restore the pre-charge gas on revert.

### `eb469387` failed-charge record

- **API.** `GasTracker::failed_charge` returns the last failed execution-gas debit as `(remaining_before, cost)`; `record_failed_charge` and `clear_failed_charge` write and clear it.
  It is written on the failure side of `spend` and of the dispatcher's static gas, on both interpreter paths, before wrapping or inspector normalization zeroes `remaining`.
  `try_spend` and `try_spend_state` failures do not record; child records are not merged.
  The record is interpreter-only: the JIT builtins record no failed charge.
- **Layout.** Compiled code loads and debits the remaining gas as the first word of `Gas` (at the `EvmContext`'s `gas` field), so `GasTracker` is `#[repr(C)]` with `remaining` first; Rust's default layout put the new record ahead of it.
  The record grows `Gas` from 64 to 80 bytes, which moves every `EvmContext` field after it, so the commit moves the compiled-artifact ABI (`evm2-jit-codegen`'s `ABI_VERSION`) from upstream's 7 (#1030 moved it from 6) to 8, and artifacts persisted at 7 are not loaded.
  Compile-time assertions beside `Gas` pin its size (80 bytes on 64-bit targets) and the tracker's offset (0), so a layout change does not build until `ABI_VERSION` and the assertions move.
  Tests: `test_remaining_gas_is_the_first_word` (`interpreter/gas.rs`) and `compiled_charge_debits_the_remaining_gas` (`evm2-jit-context`), which charge through the context's gas field as compiled code does.
- **Why.** Detention classifies an out-of-gas halt by it: a debit that spendable plus withheld gas could pay is a crossing, anything else burns (`detention::before_commit`).
  Precompiles outside the engine's price table are priced by running them on a tracker with no gas and reading the record (`precompile_price_of`).
  Controls: `classify`, `precompile_catalog`.
- **Upstream.** None filed for this record.
  [#752](https://github.com/alloy-rs/evm2/pull/752), in the base, reports the inspector's out-of-gas step cost; this is a separate, consensus-side ask.
- **Drop** when upstream keeps the failed execution-gas debit, on both interpreter paths, after `remaining` is normalized.

### `d01b0e32` gas reserve

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

### `0d9f096e` reads of absent accounts' storage

- **API.** `CacheDB::read_absent_storage`, default `false`: uncached storage of an account the cache holds as absent is answered as zero without a backing read.
  `State::set_read_absent_storage` turns it on; values stay the same for a consistent database, and the read is performed, as revm performs it (`absent_account_storage_is_read_only_when_asked`).
  The flag is part of the state: a restored snapshot, a clone and a committed copy keep it (`read_absent_storage_survives_snapshots_and_copies`).
- **Why.** A strict-replay witness needs those reads, and storage reads go through `CacheDB`, so the engine cannot add them from outside.
  The gate harness sets the flag (harness control `absent_storage_read`), and so do the engine's gas-cap tests.
- **Upstream.** None filed.
- **Drop** when upstream reads uncached storage of a database-absent account the way revm does, or offers this switch.

### `f4749e70` account-load hook

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

### `b9117de6` CREATE2's salt order

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

### `cf70453a` the first-frame hook

- **API.** `TxHandlerHooks::before_initial_frame(host, envelope, tx_gas) -> HandlerResult<Option<MessageResult<T>>>` runs in every standard `execute_prepared` (legacy, EIP-2930, EIP-1559, EIP-4844, EIP-7702) after the pre-execution changes and the transaction environment, immediately before `prepare_initial_frame`, on the transaction-level gas tracker; EIP-7702 runs it after its authorizations are applied, and not when their charges ran out of gas (that transaction settles as a halted first frame first).
  A family may charge `tx_gas` (the charge is the transaction's, kept whatever the first frame's execution does; its spill is unwound by a reservoir the first frame returns), or return a result that `settle_transaction` settles without a frame (an EIP-7702 transaction then keeps none of its delegations).
  The default returns `Ok(None)`, inlined and statically dispatched like the other hooks.
  Tests cover the default's placement (after the environment, the nonce bump and the delegations, once, on the gas the first frame starts from) and both non-default uses on every envelope (`tests::before_initial_frame`).
- **Why.** Satin validates a transaction's gas against the intrinsic gas and the body's history and charges the first frame's start record after validation, before the first frame; a transaction that cannot pay it is included as an out-of-gas that runs no frame.
  The engine charges it in `MegaHooks::before_initial_frame` and runs types 0, 1 and 2 through evm2's own `execute_prepared`, so a creation's address comes from the prepared state nonce, not the envelope, also with the nonce check off (`mega-evm2/tests/nonce_check_off.rs`).
  Charging on the transaction's tracker records the record's spill where evm2's merge unwinds it with a reservoir the first frame returns, under EIP-8037's give-back rule, so a creation destroyed in its constructor needs no case of its own (`tx_spill_unwind`); under Satin's rule (`736f76b2`) a returned reservoir unwinds nothing, and the switch shows in the rerun of the class `reservoir_give_back`, which runs EIP-8037's.
  Controls: `start_at_runtime`, `start_reads_no_target`, `tx_spill_unwind`.
- **Upstream.** Not yet proposed.
- **Drop** when upstream has a handler hook before the first frame that can charge the transaction's gas or settle it without a frame.

### `736f76b2` reservoir-only give-backs

- **API.** `EvmFeatures::EIP8037_RESERVOIR_REFILLS`, off by default, credits every give-back of EIP-8037 state gas to a reservoir and never to execution gas.
  `GasTracker::refill_reservoir_only` is the refill: the reservoir rises by the amount and `state_gas_spent` falls by it; `remaining` does not move.
  `GasTracker::settle_gas_into_reservoir(stop, received_reservoir)` settles a returning frame: a revert gives `state_gas_spent`, the charges the frame and its successful children made less the give-backs they received, into its own reservoir (`reservoir + state_gas_spent`) and returns `remaining` as it is; a halt restores the reservoir the frame received (the message's) and consumes `remaining`.
  `GasTracker::merge_child_gas_into_reservoir` makes the child's reservoir the caller's without unwinding the caller's spill, and merges the child's `state_gas_spilled` whatever the child ended with, so under the flag that counter is every state charge the frame and its returned children paid from execution gas, which nothing gives back.
  The frame exit (`execute_message_impl`) and the first frame's settlement (`settle_initial_frame_gas_into_reservoir`) choose by the flag; a halted first frame gets back only the part of its upfront account charge the reservoir paid (`InitialFrame::charged_state_gas_from_reservoir`), and the part execution gas paid is consumed as execution gas, off the state gas spent, as a halted frame's own charges are.
  The opcodes reach the rule through `give_back_state_gas` (SSTORE's 0 → x → 0, a failed call's or creation's upfront account charge) and `merge_returned_gas` (the call and creation bodies), which a type family's own opcodes use too.
  A code deposit that `MessageHooks::admit_code_deposit` refuses with a revert keeps its charges under the flag: the frame's settlement gives its state charges into the reservoir, and its keccak word gas stays spent, instead of the gas from before the deposit being restored.
  Without the flag every path is EIP-8037's, the tracker's layout is unchanged, and the JIT builtins keep EIP-8037's rule whatever the flag says.
  Tests: `gas::tests` (`test_refill_reservoir_only_leaves_remaining_and_spill`, `test_settle_gas_into_reservoir_by_stop`, `test_merge_child_gas_into_reservoir_never_unwinds`), `tests::eip8037_reservoir_refills_*` (a sibling's refill, a reverted child, a halted first frame whose account charge the reservoir paid in part) and `message_hooks::code_admission_revert_under_the_reservoir_only_rule_gives_the_deposit_state_gas_to_the_reservoir`.
- **Why.** Satin's give-back rule (`SATIN-HIST-012`, `SATIN-POOL-014`): a frame's regular gas never rises within the frame but by the unused regular gas a child returns, so a lock cleared after a starved call or an intermediary's revert cannot hand spent gas back to `GAS`.
  `satin_version` sets the flag; the engine's own give-backs (a record's history, the destruction table, a start's charge, KeylessDeploy's) call `give_back_state_gas`, its keyless creation merges through `merge_returned_gas`, and gas detention reads the never-lowered `state_gas_spilled` (`SATIN-DET-009`).
  No switch: the change is one the reference has not made yet, so turned off the engine would match the reference; the class `reservoir_give_back` checks the difference by running the case again without the flag (`satin_version_under`).
- **Upstream.** The rule is ethereum/EIPs [#12581](https://github.com/ethereum/EIPs/pull/12581), open; nothing filed in evm2.
- **Drop** when evm2 carries the reservoir-only rule as a version option, or when EIP-8037 takes it and evm2 follows.

### `a3b2779f` the legacy share

- **API.** Four gas parameters give a state charge an opcode makes for new state a legacy share, the part of the charge, at most all of it, that execution gas pays before the reservoir: `SstoreSetStateLegacyShare` (a fresh slot), `CallNewAccountStateLegacyShare` (a value call's new account, taken before the forward), `SelfdestructNewAccountStateLegacyShare` (a `SELFDESTRUCT`'s new beneficiary) and `CreateStateLegacyShare` (a creation's account, before the forward).
  They are zero in every Ethereum version.
  `GasTracker::spend_state_with_share(cost, share)` takes the share and the reservoir's shortfall as one execution-gas charge (a frame that cannot pay both runs out of gas whatever its reservoir holds) and counts both as state gas and as `state_gas_spilled`; `spend_state_with_share_reserve_first` draws the gas reserve first for that execution gas, so withheld gas pays a share as it pays a spill.
  The charges made before the first frame and the code deposit have no share; the JIT builtins take none.
  Tests: `test_spend_state_with_share_takes_the_share_first` and `tests::legacy_share_*` (each of the four sites, the call's and the creation's share before the forward, the `SELFDESTRUCT` entry kept apart from the call's).
- **Why.** `SATIN-STATE-012`: the share is the pre-Satin price less the regular cost the schedule gives the component, flat — 10,000, 25,000, 16,000 and 20,000 with Satin's prices — so a contract written for Rex keeps paying at least that in regular gas for new state with a reservoir.
  `satin_version_at` sets the four parameters from the schedule, and KeylessDeploy's steps 10 and 14 take the call's and the creation's share (`keyless.rs`).
  No switch, as for `736f76b2`; the class's rerun sets the shares to zero.
- **Upstream.** The share is ethereum/EIPs [#12581](https://github.com/ethereum/EIPs/pull/12581), open; nothing filed in evm2.
- **Drop** with `736f76b2`.

### `1a70ed94` tests

- **Adds** tests only: the tracker's equality and hash over every counted field and its accessors; every `Transaction` accessor of `TxEnvelope`; the validation bounds at the limit itself; EIP-3607 and the balance top-up; warming; the upfront state gas taken from the reservoir first; a delegated recipient's warm and cold delegate under EIP-2780; the message of a first frame that runs out of gas in the runtime gas phase; a creation transaction's account charge, under both give-back rules; the result's refund; and the account charges a failed call or creation gives back.
- **Why.** A mutation run over `interpreter/gas.rs`, `ethereum/mod.rs` and the lines `736f76b2` and `a3b2779f` change found these paths unpinned by the crate's tests; with these tests every mutant there is caught except ones that cannot change a result (a `<` against `<=` where both arms give the same value, a field whose default is the value it is set to).
- **Upstream.** Those of upstream paths can be offered as they are.
- **Drop** with the patches they test, or once upstream has them.

### `c58811c2` the copy-skipping constant is the no-op hooks' own

- **API.** `MessageHooks::RUNS_BEFORE` is `#[doc(hidden)]`, documented as set by `()` alone (to `false`), and `before`'s documentation says that a family overriding `before` leaves the constant at `true`.
  The message path is as `8d137704` has it; nothing behaves differently.
- **Why.** With `false`, an overridden `before` is never called and nothing catches it, so a rule written there would be silently off.
  The engine's `StartHooks` overrides `before` and keeps the default.
- **Tests.** `input_copy::the_no_op_hooks_leave_a_call_input_in_the_callers_memory` (under `()` the callee's input is the caller's memory) and `input_copy::a_family_that_keeps_the_defaults_has_a_call_input_copied`; `hooks_run_for_every_message_on_both_paths` already pins that a family with the default runs `before`.
  The `nested_enter` test's family no longer sets the constant: it overrides no `before`, and its result does not depend on it.
- **Upstream.** Goes with `8d137704`'s proposal.
- **Drop** with `8d137704`, or when `before` gets the message by shared reference and the constant goes.

## Merged upstream

Upstream merged two patches of the earlier series on 2026-11-09; the base carries them, the series does not, and the engine uses upstream's API.

### [#1025](https://github.com/alloy-rs/evm2/pull/1025) transaction-original account info (was `0f373350`)

- **API.** `AccountHandle::original` returns the account info at the transaction boundary (`None` for an account absent then); loading bytecode does not move it.
  Upstream merged `original` alone: the earlier patch's `AccountHandle::is_changed`, which the engine never called, is not public upstream, and its test went with it (upstream's `account_original_reports_transaction_baseline` covers `original`).
- **Why.** Write records and SELFDESTRUCT's absent-at-start refund compare an account with its start-of-transaction info; a snapshot at first observation is exact only when that observation precedes every change.
  The engine reads `original` in SELFDESTRUCT (the absent-at-start test and the beneficiary's record) and in the write-record helper `ledger::changed`; it does not call `is_changed`.
  Controls: `sd_existing_record`, `sd_records`, `sd_absent_leaf`.
- **Upstream.** Issue [#1024](https://github.com/alloy-rs/evm2/issues/1024), closed by PR [#1025](https://github.com/alloy-rs/evm2/pull/1025), merged on 2026-11-09 as `52c57092`.

### [#1023](https://github.com/alloy-rs/evm2/pull/1023) configurable total gas cap (was `28301ff3`)

- **API.** `Version::tx_total_gas_cap` bounds `tx.gas` when EIP-8037 is on; `u64::MAX` disables it.
  The default is `MAX_TX_TOTAL_GAS_LIMIT_AMSTERDAM` (2^32 − 1) in both base version tables, so an unmodified Ethereum version still rejects 2^32.
  `Version::tx_gas_caps` returns `(tx_total_gas_cap, tx_gas_limit_cap)` under EIP-8037; without it the field is unused.
- **Why.** Satin applies no total cap: the block's gas limit alone bounds `tx.gas`, while the execution cap of 200,000,000 bounds required execution gas and the rest funds the reservoir.
  `satin_version` sets `tx_total_gas_cap` to `u64::MAX`; a version constant, not a switch.
  Proof: `mega-evm2/tests/tx_gas_cap.rs` (gas limits of 2^32 and above for every transaction type and in a block) and the `cap_` scenarios of the main gate.
- **Upstream.** PR [#1023](https://github.com/alloy-rs/evm2/pull/1023), merged on 2026-11-09 as `71a736c9` with the field, its default and its `u64::MAX` meaning unchanged (the cap itself is issue #807 and PR #862 upstream); the earlier patch's three tests went with it, and upstream's `amsterdam_total_gas_cap_is_configurable` and `tx_total_gas_cap_is_configurable` cover the same cases.

## Dropped from the earlier series

- `0f50973c`, which checked ModExp's EIP-7823 bound before its formula price (revm's order): the Satin specification follows upstream's order, the formula price charged before the bound, so an over-bound input is priced, and decided under gas detention, by its formula ([#874](https://github.com/alloy-rs/evm2/pull/874)).
  The engine's two-step price probe (`precompile_price_of`) reads the minimum, then the rest of the formula price, for such an input too; the `edge_modexp_*` scenarios show the order.
- The rebase of open upstream PR [#523](https://github.com/alloy-rs/evm2/pull/523) (native precompile calls): the engine answers `MegaLimitControl` and `MegaAccessControl` from `MessageHooks::before`, which runs before dispatch.
- Input-only precompile gas quotes: the engine prices the fixed-formula entries itself (`precompile_price`) and probes the others (`precompile_price_of`).
- Formatting and call-site glue: folded into the commits it repaired.
- The pre-state-charge closures `call_with_state_charge` and `create_with_state_charge`: the state spills draw the gas reserve inside evm2's own bodies, and the engine calls `call_with` and `create_with`.
- The code-deposit funding hook `MessageHooks::before_code_deposit`: the deposit's state spill draws the gas reserve.

## Known follow-up

The engine's own EIP-7702 path (`mega-evm2/src/eip7702.rs`) settles a runtime-gas-phase out-of-gas without a frame, unlike upstream's `eip7702::execute_prepared`, which reports a halted first frame.
It is left as a follow-up; the reference opens no frame there either, so no gate shows it.
The first-frame hook does not let that path go: Satin decides the list rule before the list is applied (a list it skips is never applied), needs the accepted authorities for their records and growth, and detains a transaction whose applied authority is the beneficiary; evm2's `execute_prepared` applies the list with its own accounting and offers no seam for those three.
Its first frame takes no nonce from the envelope that matters: an EIP-7702 transaction cannot create.

## Rules for the next rebase

Rebase the series onto the upstream revision chosen for the engine (the node's pin, or upstream `main`), onto a new branch.
Textual conflicts are the easy part; these are the checks a clean merge does not make:

- **Budget readers and state spills go through the seam.**
  Read every new `remaining()`, `spend_state` and `try_spend_state` site under `crates/evm2/src/interpreter/instructions` and in `crates/evm2/src/evm/mod.rs`.
  A budget decision (a skip-cold check, a guard, a forward base, a value pushed to the stack) must read `gas_budget` with its `BudgetUse`; a state charge must call `spend_state_reserve_first`, and one an opcode makes for new state `spend_state_with_share_reserve_first` with its legacy-share parameter; a forward must call `spend_reserve_first`.
  A new reader that merges without a conflict still ignores the withheld gas.
  Recount the sites, update the count above, and give a new `BudgetUse` or draw its own engine switch and control.
- **Every give-back and child merge follows the version's rule.**
  Read every `refill_reservoir`, `merge_child_gas`, `rollback_state_gas` and `settle_gas` call under `crates/evm2/src`: an instruction must call `give_back_state_gas` or `merge_returned_gas`, and the frame exit and the first frame's settlement must branch on `EIP8037_RESERVOIR_REFILLS`; a new call that merges without a conflict credits execution gas under Satin's rule.
  The engine's own give-backs and its keyless merge go through the same helpers (`ledger::refill_history`, `mega_selfdestruct`, `keyless.rs`).
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
- **A hook that gets a message by `&mut` sees an owned input.**
  A call's input may borrow the caller's memory (#1030), and `&mut` lets the hook move it out and keep it past the call.
  Every hook the series adds that gets a `Message` by `&mut`, and every path that reaches one, must copy the input first, as `execute_message_impl` does before `MessageHooks::before` and upstream does before its inspector and `call_with` hooks; keep `kept_input::before_keeps_an_input_the_caller_overwrites_after_the_call` passing.
  `RUNS_BEFORE` stays `()`'s alone; keep both `input_copy` tests passing.
  Every hook gets the host and can run a message of its own while the caller is suspended, and that message's inspector hooks get the caller's frame: with an inspector installed, a hook that runs before the inspected path's own copy must see a copied input, as `Host::execute_message` gives `enter`; keep `nested_enter::entered_input_survives_a_nested_inspector_that_moves_the_caller_memory` passing.
- **Compiled code's fixed offsets keep their fields.**
  The JIT addresses `EvmContext` fields by `offset_of!` and reads the remaining gas as the first word of its `gas` field; `GasTracker` and `Gas` are `#[repr(C)]` with `remaining` first.
  A field added to `GasTracker`, `Gas`, `EvmContext`, or to a struct `HostLayout` takes offsets in (`Interpreter`'s message, `Message`, `TxEnv`, `BlockEnv`) must keep those offsets right, and a change of their size or order bumps `ABI_VERSION`.
  `eb469387` must leave `ABI_VERSION` one above upstream's: when upstream bumps it to the value the series had, the two bumps merge without a conflict and the series' layout change loses its bump.
  Run the JIT crates' tests and the EEST JIT suite with LLVM installed, against upstream's own failures: the interpreter suites cannot see a compiled-code regression.
- **Every `execute_prepared` calls the first-frame hook.**
  Read every `execute_prepared` upstream has (and any new one): each must call `before_initial_frame` after the transaction environment and the pre-execution changes, immediately before `prepare_initial_frame` (EIP-7702's only when its authorization charges were paid), settle the result it returns without a frame, and recompute nothing from the envelope that `PreparedTx` holds (the creation nonce is the prepared state nonce); keep `tests::before_initial_frame` covering every envelope.
  Types 0, 1 and 2 run evm2's `execute_prepared` with `MegaHooks`, so the start record moves with that call.
- **The engine restates EIP-7702's `execute_prepared`.**
  `mega-evm2/src/eip7702.rs` runs its own copy (see Known follow-up for why the hook does not replace it); compare it with upstream's `eip7702::execute_prepared` after a rebase, as a clean merge does not.
  It reads the intrinsic gas, the initial state gas and the floor from what `adjust_intrinsic_gas` recorded (`PreparedTx` cannot be read outside evm2) and the nonce from the envelope, which only a creation would use and an EIP-7702 transaction cannot make; check that both still hold.
- **The engine restates op-evm2's deposit execution.**
  `execute_deposit_inner` (`mega-evm2/src/tx.rs`) is its own copy of op-evm2's `execute_deposit_tx` around evm2's `prepare_initial_frame` and `execute_initial_frame`, adding the created caller's state gas, the first frame's start, the creation from a sender whose nonce cannot be bumped and the out-of-funds failure; it runs no `execute_prepared`, so the first-frame hook does not reach it.
  After a rebase, compare it with those functions and with `execute_deposit_tx`, and check that the creation nonce still comes from `PreparedDeposit` (the state nonce).
- **Run the negative controls, not only the gates.**
  A fix the new upstream code no longer reaches shows up as a missed control while every gate still passes; `diff/scripts/ci-local.sh` runs the sample controls, `controls.sh full` the rest.

## Moving the pin

CI checks the pin out from `EVM2_FORK_REPO` by hash (`scripts/fetch-evm2-fork.sh`), without credentials.
Push the new commit to the public fork first, then set `EVM2_FORK_REV`; a pin that was never pushed fails the step "Check out the evm2 fork at its pin".
Keep the fork repository public.
Rebase onto a new branch rather than force-pushing, so that every earlier pin stays fetchable.
Local builds use the checkout next to this repository (`../evm2` from the repository root).
