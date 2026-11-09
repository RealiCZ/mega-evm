# The thin evm2 fork

`next/` builds against a thin fork of [alloy-rs/evm2](https://github.com/alloy-rs/evm2): [`RealiCZ/evm2`](https://github.com/RealiCZ/evm2), branch `cz/feat/thin-fork-v2`.
`evm2-fork.env` holds the pin (`EVM2_FORK_REPO`, `EVM2_FORK_REV`); it is the only place CI reads it from.
The series sits on upstream `85395c54`, not on upstream's head: that is the evm2 revision reth's pull request #28276 moves reth to, so the node and the engine can share one evm2.
The earlier series, branch `cz/feat/thin-fork` at `91576d1e`, is left as it was, so earlier pins still resolve.

A patch exists only when the engine needs something evm2 cannot give it from outside: data from inside evm2's own flow, or a hook point.
Every patch keeps stock behavior as its default, and each says what upstream change would let it go.
One patch, `b9fa9909`, is a stock fix instead: it moves a check to revm's order, which evm2 follows, and is written to be proposed upstream as it is.
ModExp keeps upstream's order (its 500 minimum, its formula price, then EIP-7823's bound, upstream [#874](https://github.com/alloy-rs/evm2/pull/874)): the Satin specification follows upstream there, so the series carries no ModExp patch.
The engine also uses two upstream changes that are already in the base: `call_with` / `create_with` ([#887](https://github.com/alloy-rs/evm2/pull/887)) and reservoir-funded state gas in `system_call` totals ([#842](https://github.com/alloy-rs/evm2/pull/842)).

| Patch                                                                               | Adds                                                             | Size     | Upstream                                                                                                                  | Drop when upstream                                                                         |
| ----------------------------------------------------------------------------------- | ---------------------------------------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `70bbeb67` select consensus and observation message hooks statically                | `MessageHooks`, a static per-message hook type                   | +576/−36 | none yet                                                                                                                  | has a static hook on every message, answers included, before the commit decision           |
| `9df29b6a` price state charges by account and storage location                      | `EvmTypesHost::state_gas_price`, `StateGasSite`                  | +471/−34 | none yet                                                                                                                  | can price a state charge by its account or storage slot                                    |
| `f623a776` expose a code deposit admission hook                                     | `MessageHooks::admit_code_deposit`                               | +83/−9   | none yet                                                                                                                  | has an admission hook whose revert restores the pre-charge gas                             |
| `41525289` retain failed debit before normalization on every interpreter path       | `GasTracker::failed_charge`                                      | +158/−4  | none yet                                                                                                                  | keeps the failed execution-gas debit on both paths after `remaining` is normalized         |
| `f37259b6` let a type family hold a frame's gas in reserve                          | `gas_reserve` / `draw_gas_reserve`, `BudgetUse`, `ReserveDraw`   | +768/−26 | none yet                                                                                                                  | lets execution gas outside `remaining` reach budget decisions and non-execution deductions |
| `0f373350` expose transaction-original account information                          | `AccountHandle::original`, `AccountHandle::is_changed`           | +59      | issue [#1024](https://github.com/alloy-rs/evm2/issues/1024), PR [#1025](https://github.com/alloy-rs/evm2/pull/1025), open | makes `AccountHandle::original` public                                                     |
| `94011dec` optionally read uncached storage of accounts the database reports absent | `CacheDB::read_absent_storage`, `State::set_read_absent_storage` | +61/−3   | none yet                                                                                                                  | reads that storage as revm does, or offers this switch                                     |
| `28301ff3` make the EIP-8037 total transaction gas cap configurable                 | `Version::tx_total_gas_cap`                                      | +51/−2   | PR [#1023](https://github.com/alloy-rs/evm2/pull/1023), open                                                              | lets a chain set or disable the total cap, default unchanged                               |
| `f6d21862` let a type family refuse an instruction's account load                   | `EvmTypesHost::before_account_load`, `AccountLoadSite`           | +395/−5  | none yet                                                                                                                  | lets a type family stop an instruction at its own account load                             |
| `b9fa9909` pop CREATE2's salt after the init-code and memory charges                | none (instruction order)                                         | +33/−1   | not yet proposed                                                                                                          | pops CREATE2's salt after the init-code charge and the memory expansion                    |
| `cee7bfb4` let a type family act on the transaction before its first frame          | `TxHandlerHooks::before_initial_frame`                           | +375/−45 | not yet proposed                                                                                                          | has a handler hook before the first frame that can charge the transaction or settle it     |
| `546c689f` let a version credit state-gas give-backs to the reservoir only          | `EvmFeatures::EIP8037_RESERVOIR_REFILLS`, `give_back_state_gas`, `merge_returned_gas` | +523/−11 | the rule is ethereum/EIPs [#12581](https://github.com/ethereum/EIPs/pull/12581), open; none in evm2 | has the reservoir-only give-back rule as a version option                                  |
| `ee61e8b3` let a version take a legacy share of a state charge from execution gas   | the four `GasId::*LegacyShare` parameters, `spend_state_with_share_reserve_first` | +232/−8  | the share is ethereum/EIPs [#12581](https://github.com/ethereum/EIPs/pull/12581), open; none in evm2 | has a legacy share of the new-state charges as gas parameters                             |

Size is the commit's own stat against its parent; most of the insertions of `f37259b6`, `9df29b6a`, `f6d21862`, `b9fa9909`, `cee7bfb4`, `546c689f` and `ee61e8b3` are tests.
Upstream order, smallest and least contested first: `0f373350`, `28301ff3` (both filed), `b9fa9909`, `cee7bfb4`, `94011dec`, `f623a776`, `41525289` with the halt record of `f37259b6`, `9df29b6a`, the rest of `f37259b6`, `f6d21862`, and `70bbeb67` last, because it runs on every message.
`546c689f` and `ee61e8b3` follow ethereum/EIPs #12581: offered with it if Ethereum takes that rule into EIP-8037, kept as Satin's own departure if it does not.

## The patches

Each engine rule a patch serves sits behind a named switch with a negative control in `diff/negative-controls.toml` (a harness switch for `94011dec`); turning the switch off makes the control's case differ from the reference.
`28301ff3`'s setting is a version constant, proved by tests instead; `b9fa9909` changes evm2's own instruction, which the `edge_create2_*` scenarios show; `cee7bfb4` carries the start-record charge, whose switches (`start_at_runtime`, `start_reads_no_target`, `tx_spill_unwind`) have their controls.

### `70bbeb67` static message hooks

- **API.** `EvmTypesHost::MessageHooks` is an associated type; `()` is the no-op, which the stock families, `op-evm2` and the JIT host tests use.
  The `MessageHooks` trait runs on every message, on the plain and the inspected path: consensus methods `before` (may answer the message), `started` (after the checkpoint, value transfer and creation setup), `frame_built`, `before_commit` (may rewrite the stop before commit or rollback) and `after`; observers `enter`, `unsettled` and `exit`.
  Every built-in result decided after the message's checkpoint reaches `before_commit`: a frame's, a precompile's, an account without code's, a call's out-of-funds answer and a creation's failure to create its account (a collision, an endowment the creator cannot pay); a result decided before the checkpoint (a call or creation too deep, a creation's balance or nonce check) does not (`out_of_funds_call_reaches_before_commit_on_both_paths`, `create_collision_reaches_before_commit_on_both_paths`).
  A message that halts without executing (`Evm::execute_halted_message`) reaches none of them, on either path (`halted_messages_run_no_hooks_on_either_path`).
- **Why.** Satin's frame-start rules stop a frame before its code runs, take back a success answer, and answer the system contracts before evm2 moves value.
  The engine's `StartHooks` implements them: `pre_frame_stop`, `tx_beneficiary`, `keyless`, `keyless_value_hold`, `access_control`, `oracle_hints`, `precompile_hold` and `top_precompile_hold` (`before`); `first_frame_hold` and `hold_start` (`started`); `hold` (`frame_built`); `classify`, `built_check`, `unbuilt_burn`, `deposit_loan_reset` and `first_frame_hold` (`before_commit`).
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

### `41525289` failed-charge record

- **API.** `GasTracker::failed_charge` returns the last failed execution-gas debit as `(remaining_before, cost)`; `record_failed_charge` and `clear_failed_charge` write and clear it.
  It is written on the failure side of `spend` and of the dispatcher's static gas, on both interpreter paths, before wrapping or inspector normalization zeroes `remaining`.
  `try_spend` and `try_spend_state` failures do not record; child records are not merged.
  The record is interpreter-only: the JIT builtins record no failed charge.
- **Layout.** Compiled code loads and debits the remaining gas as the first word of `Gas` (at the `EvmContext`'s `gas` field), so `GasTracker` is `#[repr(C)]` with `remaining` first; Rust's default layout put the new record ahead of it.
  The record grows `Gas` from 64 to 80 bytes, which moves every `EvmContext` field after it, so the commit moves the compiled-artifact ABI (`evm2-jit-codegen`'s `ABI_VERSION`) from 6 to 7, and artifacts persisted at 6 are not loaded.
  Tests: `test_remaining_gas_is_the_first_word` (`interpreter/gas.rs`) and `compiled_charge_debits_the_remaining_gas` (`evm2-jit-context`), which charge through the context's gas field as compiled code does.
- **Why.** Detention classifies an out-of-gas halt by it: a debit that spendable plus withheld gas could pay is a crossing, anything else burns (`detention::before_commit`).
  Precompiles outside the engine's price table are priced by running them on a tracker with no gas and reading the record (`precompile_price_of`).
  Controls: `classify`, `precompile_catalog`.
- **Upstream.** None filed for this record.
  [#752](https://github.com/alloy-rs/evm2/pull/752), in the base, reports the inspector's out-of-gas step cost; this is a separate, consensus-side ask.
- **Drop** when upstream keeps the failed execution-gas debit, on both interpreter paths, after `remaining` is normalized.

### `f37259b6` gas reserve

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

### `0f373350` transaction-original account info

- **API.** `AccountHandle::original` returns the account info at the transaction boundary (`None` for an account absent then); loading bytecode does not move it.
  `AccountHandle::is_changed` reports whether existence or persisted account fields differ from it; storage-only writes do not count (`original_is_the_transaction_baseline`).
- **Why.** Write records and SELFDESTRUCT's absent-at-start refund compare an account with its start-of-transaction info; a snapshot at first observation is exact only when that observation precedes every change.
  The engine reads `original` in SELFDESTRUCT (the absent-at-start test and the beneficiary's record) and in the write-record helper `ledger::changed`; it does not call `is_changed`.
  Controls: `sd_existing_record`, `sd_records`, `sd_absent_leaf`.
- **Upstream.** Issue [#1024](https://github.com/alloy-rs/evm2/issues/1024) and PR [#1025](https://github.com/alloy-rs/evm2/pull/1025), open; the PR exposes `original` only.
- **Drop** when `AccountHandle::original` is public.

### `94011dec` reads of absent accounts' storage

- **API.** `CacheDB::read_absent_storage`, default `false`: uncached storage of an account the cache holds as absent is answered as zero without a backing read.
  `State::set_read_absent_storage` turns it on; values stay the same for a consistent database, and the read is performed, as revm performs it (`absent_account_storage_is_read_only_when_asked`).
  The flag is part of the state: a restored snapshot, a clone and a committed copy keep it (`read_absent_storage_survives_snapshots_and_copies`).
- **Why.** A strict-replay witness needs those reads, and storage reads go through `CacheDB`, so the engine cannot add them from outside.
  The gate harness sets the flag (harness control `absent_storage_read`), and so do the engine's gas-cap tests.
- **Upstream.** None filed.
- **Drop** when upstream reads uncached storage of a database-absent account the way revm does, or offers this switch.

### `28301ff3` configurable total gas cap

- **API.** `Version::tx_total_gas_cap` bounds `tx.gas` when EIP-8037 is on; `u64::MAX` disables it.
  The default is `MAX_TX_TOTAL_GAS_LIMIT_AMSTERDAM` (2^32 − 1) in both base version tables, so an unmodified Ethereum version still rejects 2^32.
  `Version::tx_gas_caps` returns `(tx_total_gas_cap, tx_gas_limit_cap)` under EIP-8037; without it the field is unused.
- **Why.** Satin applies no total cap: the block's gas limit alone bounds `tx.gas`, while the execution cap of 200,000,000 bounds required execution gas and the rest funds the reservoir.
  `satin_version` sets `tx_total_gas_cap` to `u64::MAX`; a version constant, not a switch.
  Proof: `mega-evm2/tests/tx_gas_cap.rs` (gas limits of 2^32 and above for every transaction type and in a block) and the `cap_` scenarios of the main gate.
- **Upstream.** PR [#1023](https://github.com/alloy-rs/evm2/pull/1023), open (the cap itself is issue #807 and PR #862 upstream).
- **Drop** when upstream lets a chain set or disable the EIP-8037 total transaction gas cap without changing the default.

### `f6d21862` account-load hook

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

### `b9fa9909` CREATE2's salt order

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

### `cee7bfb4` the first-frame hook

- **API.** `TxHandlerHooks::before_initial_frame(host, envelope, tx_gas) -> HandlerResult<Option<MessageResult<T>>>` runs in every standard `execute_prepared` (legacy, EIP-2930, EIP-1559, EIP-4844, EIP-7702) after the pre-execution changes and the transaction environment, immediately before `prepare_initial_frame`, on the transaction-level gas tracker; EIP-7702 runs it after its authorizations are applied, and not when their charges ran out of gas (that transaction settles as a halted first frame first).
  A family may charge `tx_gas` (the charge is the transaction's, kept whatever the first frame's execution does; its spill is unwound by a reservoir the first frame returns), or return a result that `settle_transaction` settles without a frame (an EIP-7702 transaction then keeps none of its delegations).
  The default returns `Ok(None)`, inlined and statically dispatched like the other hooks.
  Tests cover the default's placement (after the environment, the nonce bump and the delegations, once, on the gas the first frame starts from) and both non-default uses on every envelope (`tests::before_initial_frame`).
- **Why.** Satin validates a transaction's gas against the intrinsic gas and the body's history and charges the first frame's start record after validation, before the first frame; a transaction that cannot pay it is included as an out-of-gas that runs no frame.
  The engine charges it in `MegaHooks::before_initial_frame` and runs types 0, 1 and 2 through evm2's own `execute_prepared`, so a creation's address comes from the prepared state nonce, not the envelope, also with the nonce check off (`mega-evm2/tests/nonce_check_off.rs`).
  Charging on the transaction's tracker records the record's spill where evm2's merge unwinds it with a reservoir the first frame returns, under EIP-8037's give-back rule, so a creation destroyed in its constructor needs no case of its own (`tx_spill_unwind`); under Satin's rule (`546c689f`) a returned reservoir unwinds nothing, and the switch shows in the rerun of the class `reservoir_give_back`, which runs EIP-8037's.
  Controls: `start_at_runtime`, `start_reads_no_target`, `tx_spill_unwind`.
- **Upstream.** Not yet proposed.
- **Drop** when upstream has a handler hook before the first frame that can charge the transaction's gas or settle it without a frame.

### `546c689f` reservoir-only give-backs

- **API.** `EvmFeatures::EIP8037_RESERVOIR_REFILLS`, off by default, credits every give-back of EIP-8037 state gas to a reservoir and never to execution gas.
  `GasTracker::refill_reservoir_only` is the refill: the reservoir rises by the amount and `state_gas_spent` falls by it; `remaining` does not move.
  `GasTracker::settle_gas_into_reservoir(stop, received_reservoir)` settles a returning frame: a revert gives `state_gas_spent`, the charges the frame and its successful children made less the give-backs they received, into its own reservoir (`reservoir + state_gas_spent`) and returns `remaining` as it is; a halt restores the reservoir the frame received (the message's) and consumes `remaining`.
  `GasTracker::merge_child_gas_into_reservoir` makes the child's reservoir the caller's without unwinding the caller's spill, and merges the child's `state_gas_spilled` whatever the child ended with, so under the flag that counter is every state charge the frame and its returned children paid from execution gas, which nothing gives back.
  The frame exit (`execute_message_impl`) and the first frame's settlement (`settle_initial_frame_gas_into_reservoir`) choose by the flag; a halted first frame gets back only the part of its upfront account charge the reservoir paid (`InitialFrame::charged_state_gas_from_reservoir`), and the part execution gas paid is consumed as execution gas, off the state gas spent, as a halted frame's own charges are.
  The opcodes reach the rule through `give_back_state_gas` (SSTORE's 0 → x → 0, a failed call's or creation's upfront account charge) and `merge_returned_gas` (the call and creation bodies), which a type family's own opcodes use too.
  Without the flag every path is EIP-8037's, the tracker's layout is unchanged, and the JIT builtins keep EIP-8037's rule whatever the flag says.
  Tests: `gas::tests` (`test_refill_reservoir_only_leaves_remaining_and_spill`, `test_settle_gas_into_reservoir_by_stop`, `test_merge_child_gas_into_reservoir_never_unwinds`) and `tests::eip8037_reservoir_refills_*` (a sibling's refill, a reverted child, a halted first frame whose account charge the reservoir paid in part).
- **Why.** Satin's give-back rule (`SATIN-HIST-012`, `SATIN-POOL-014`): a frame's regular gas never rises within the frame but by the unused regular gas a child returns, so a lock cleared after a starved call or an intermediary's revert cannot hand spent gas back to `GAS`.
  `satin_version` sets the flag; the engine's own give-backs (a record's history, the destruction table, a start's charge, KeylessDeploy's) call `give_back_state_gas`, its keyless creation merges through `merge_returned_gas`, and gas detention reads the never-lowered `state_gas_spilled` (`SATIN-DET-009`).
  No switch: the change is one the reference has not made yet, so turned off the engine would match the reference; the class `reservoir_give_back` checks the difference by running the case again without the flag (`satin_version_under`).
- **Upstream.** The rule is ethereum/EIPs [#12581](https://github.com/ethereum/EIPs/pull/12581), open; nothing filed in evm2.
- **Drop** when evm2 carries the reservoir-only rule as a version option, or when EIP-8037 takes it and evm2 follows.

### `ee61e8b3` the legacy share

- **API.** Four gas parameters give a state charge an opcode makes for new state a legacy share, the part of the charge, at most all of it, that execution gas pays before the reservoir: `SstoreSetStateLegacyShare` (a fresh slot), `CallNewAccountStateLegacyShare` (a value call's new account, taken before the forward), `SelfdestructNewAccountStateLegacyShare` (a `SELFDESTRUCT`'s new beneficiary) and `CreateStateLegacyShare` (a creation's account, before the forward).
  They are zero in every Ethereum version.
  `GasTracker::spend_state_with_share(cost, share)` takes the share and the reservoir's shortfall as one execution-gas charge (a frame that cannot pay both runs out of gas whatever its reservoir holds) and counts both as state gas and as `state_gas_spilled`; `spend_state_with_share_reserve_first` draws the gas reserve first for that execution gas, so withheld gas pays a share as it pays a spill.
  The charges made before the first frame and the code deposit have no share; the JIT builtins take none.
  Tests: `test_spend_state_with_share_takes_the_share_first` and `tests::legacy_share_*` (each of the four sites, the call's and the creation's share before the forward, the `SELFDESTRUCT` entry kept apart from the call's).
- **Why.** `SATIN-STATE-012`: the share is the pre-Satin price less the regular cost the schedule gives the component, flat — 10,000, 25,000, 16,000 and 20,000 with Satin's prices — so a contract written for Rex keeps paying at least that in regular gas for new state with a reservoir.
  `satin_version_at` sets the four parameters from the schedule, and KeylessDeploy's steps 10 and 14 take the call's and the creation's share (`keyless.rs`).
  No switch, as for `546c689f`; the class's rerun sets the shares to zero.
- **Upstream.** The share is ethereum/EIPs [#12581](https://github.com/ethereum/EIPs/pull/12581), open; nothing filed in evm2.
- **Drop** with `546c689f`.

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

Rebase `cz/feat/thin-fork-v2` onto the revision the node pins, which need not be `origin/main`'s head.
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
- **Compiled code's fixed offsets keep their fields.**
  The JIT addresses `EvmContext` fields by `offset_of!` and reads the remaining gas as the first word of its `gas` field; `GasTracker` and `Gas` are `#[repr(C)]` with `remaining` first.
  A field added to `GasTracker`, `Gas`, `EvmContext`, or to a struct `HostLayout` takes offsets in (`Interpreter`'s message, `Message`, `TxEnv`, `BlockEnv`) must keep those offsets right, and a change of their size or order bumps `ABI_VERSION`.
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
