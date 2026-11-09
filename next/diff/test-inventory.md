# Satin tests of the other implementation, against the gates

Every test function of the other implementation (mega-evm `5e3f30ad`, `crates/mega-evm/tests`) is `covered` by named scenarios or a fuzz mode, `ported` into a scenario of this change (group `port`, `rev`, `blk`), a `gap` (portable, no scenario does the same yet) or `not-portable` (the scenario format cannot express it, with the reason).
A file whose tests are not classified yet is marked `unclassified`.

| File | Tests | Covered | Ported | Gap | Not portable | Unclassified |
| --- | --- | --- | --- | --- | --- | --- |
| `block/activation.rs` | 3 | 0 | 0 | 0 | 3 | 0 |
| `block/block_hashes.rs` | 3 | 0 | 0 | 0 | 3 | 0 |
| `block/bucket_ids.rs` | 5 | 0 | 0 | 0 | 5 | 0 |
| `block/building_policy.rs` | 2 | 0 | 0 | 0 | 2 | 0 |
| `block/counters.rs` | 7 | 0 | 0 | 0 | 7 | 0 |
| `block/deposits.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `block/detention.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `block/eips.rs` | 15 | 0 | 0 | 0 | 15 | 0 |
| `block/external_envs.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `block/factory.rs` | 4 | 0 | 0 | 0 | 4 | 0 |
| `block/inspector.rs` | 4 | 0 | 0 | 0 | 4 | 0 |
| `block/l1_attributes.rs` | 2 | 0 | 2 | 0 | 0 | 0 |
| `block/limits.rs` | 19 | 0 | 0 | 0 | 19 | 0 |
| `block/predeploys.rs` | 4 | 0 | 1 | 0 | 3 | 0 |
| `block/receipt_goldens.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `block/receipts.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `block/rules.rs` | 12 | 0 | 0 | 0 | 12 | 0 |
| `block/salt.rs` | 2 | 0 | 0 | 0 | 2 | 0 |
| `block/schedule.rs` | 8 | 0 | 0 | 0 | 8 | 0 |
| `block/sequencer_registry.rs` | 17 | 0 | 2 | 0 | 15 | 0 |
| `block/sequences.rs` | 2 | 0 | 0 | 0 | 2 | 0 |
| `block/stale.rs` | 5 | 0 | 0 | 0 | 5 | 0 |
| `block/state_gas.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `block/system_tx_budgets.rs` | 6 | 0 | 0 | 0 | 6 | 0 |
| `block/transfer_logs.rs` | 4 | 3 | 1 | 0 | 0 | 0 |
| `block/witness/basics.rs` | 8 | 0 | 0 | 0 | 8 | 0 |
| `block/witness/history.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `block/witness/keyless.rs` | 8 | 0 | 0 | 0 | 8 | 0 |
| `block/witness/oracle.rs` | 11 | 0 | 0 | 0 | 11 | 0 |
| `block/witness/predeploys.rs` | 2 | 0 | 0 | 0 | 2 | 0 |
| `block/witness/skips.rs` | 6 | 0 | 0 | 0 | 6 | 0 |
| `block/witness/state.rs` | 13 | 0 | 0 | 0 | 13 | 0 |
| `block/witness/stops.rs` | 7 | 0 | 0 | 0 | 7 | 0 |
| `block/witness/system.rs` | 2 | 0 | 0 | 0 | 2 | 0 |
| `fuzz/blocks.rs` | 2 | 0 | 0 | 0 | 2 | 0 |
| `fuzz/differential.rs` | 2 | 0 | 0 | 0 | 2 | 0 |
| `fuzz/harness.rs` | 6 | 0 | 0 | 0 | 6 | 0 |
| `fuzz/keyless.rs` | 1 | 1 | 0 | 0 | 0 | 0 |
| `fuzz/prices.rs` | 2 | 0 | 0 | 0 | 2 | 0 |
| `fuzz/properties.rs` | 8 | 6 | 0 | 0 | 2 | 0 |
| `fuzz/regressions.rs` | 3 | 1 | 1 | 0 | 1 | 0 |
| `satin/compute_gas.rs` | 20 | 12 | 6 | 0 | 2 | 0 |
| `satin/contract_size.rs` | 6 | 0 | 6 | 0 | 0 | 0 |
| `satin/corpus.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `satin/count_order.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `satin/create_opcodes.rs` | 10 | 2 | 8 | 0 | 0 | 0 |
| `satin/data_size.rs` | 24 | 1 | 20 | 0 | 3 | 0 |
| `satin/data_size_counts.rs` | 7 | 5 | 2 | 0 | 0 | 0 |
| `satin/db_error.rs` | 4 | 1 | 0 | 0 | 3 | 0 |
| `satin/deposit_charge.rs` | 5 | 0 | 3 | 0 | 2 | 0 |
| `satin/detained_transfers.rs` | 6 | 3 | 3 | 0 | 0 | 0 |
| `satin/detention.rs` | 34 | 15 | 18 | 0 | 1 | 0 |
| `satin/equivalence.rs` | 17 | 15 | 0 | 0 | 2 | 0 |
| `satin/failed_deposit.rs` | 5 | 1 | 3 | 0 | 1 | 0 |
| `satin/floor_exempt.rs` | 1 | 0 | 1 | 0 | 0 | 0 |
| `satin/gas_forward.rs` | 4 | 3 | 0 | 0 | 1 | 0 |
| `satin/history_allowance.rs` | 19 | 5 | 11 | 0 | 3 | 0 |
| `satin/history_bytes.rs` | 6 | 4 | 1 | 0 | 1 | 0 |
| `satin/history_exemption.rs` | 9 | 6 | 0 | 0 | 3 | 0 |
| `satin/history_gas.rs` | 13 | 7 | 6 | 0 | 0 | 0 |
| `satin/history_reservoir.rs` | 7 | 4 | 3 | 0 | 0 | 0 |
| `satin/inherited_costs.rs` | 6 | 4 | 0 | 0 | 2 | 0 |
| `satin/inspector.rs` | 18 | 1 | 0 | 0 | 17 | 0 |
| `satin/inspector_cheatcodes.rs` | 6 | 0 | 0 | 0 | 6 | 0 |
| `satin/inspector_limits.rs` | 4 | 0 | 0 | 0 | 4 | 0 |
| `satin/inspector_settlement.rs` | 23 | 0 | 0 | 0 | 23 | 0 |
| `satin/intrinsic.rs` | 14 | 8 | 1 | 0 | 5 | 0 |
| `satin/kv_updates.rs` | 7 | 1 | 5 | 0 | 1 | 0 |
| `satin/limit_exemption.rs` | 4 | 0 | 3 | 0 | 1 | 0 |
| `satin/limit_matrix/block.rs` | 7 | 0 | 0 | 0 | 7 | 0 |
| `satin/limit_matrix/deposits.rs` | 4 | 0 | 2 | 0 | 2 | 0 |
| `satin/limit_matrix/destruction.rs` | 30 | 7 | 23 | 0 | 0 | 0 |
| `satin/limit_matrix/forwarding.rs` | 13 | 4 | 8 | 0 | 1 | 0 |
| `satin/limit_matrix/keyless.rs` | 11 | 4 | 7 | 0 | 0 | 0 |
| `satin/limit_matrix/limits.rs` | 14 | 0 | 14 | 0 | 0 | 0 |
| `satin/limit_matrix/records.rs` | 7 | 3 | 4 | 0 | 0 | 0 |
| `satin/limit_matrix/stop.rs` | 7 | 1 | 5 | 0 | 1 | 0 |
| `satin/limit_stop.rs` | 5 | 0 | 1 | 0 | 4 | 0 |
| `satin/neutral.rs` | 12 | 0 | 0 | 0 | 12 | 0 |
| `satin/outcome.rs` | 4 | 2 | 1 | 0 | 1 | 0 |
| `satin/pool_validation.rs` | 9 | 1 | 1 | 0 | 7 | 0 |
| `satin/precompile_gas.rs` | 5 | 4 | 1 | 0 | 0 | 0 |
| `satin/pricing_table.rs` | 3 | 0 | 0 | 0 | 3 | 0 |
| `satin/reservoir_drain.rs` | 6 | 0 | 6 | 0 | 0 | 0 |
| `satin/reservoir_settlement.rs` | 2 | 1 | 1 | 0 | 0 | 0 |
| `satin/reservoir_share.rs` | 5 | 2 | 1 | 0 | 2 | 0 |
| `satin/reuse_after_error.rs` | 6 | 0 | 0 | 0 | 6 | 0 |
| `satin/salt.rs` | 28 | 18 | 5 | 0 | 5 | 0 |
| `satin/salt_delegation.rs` | 13 | 2 | 10 | 0 | 1 | 0 |
| `satin/salt_deposit.rs` | 6 | 1 | 4 | 0 | 1 | 0 |
| `satin/salt_failure.rs` | 14 | 2 | 0 | 0 | 12 | 0 |
| `satin/salt_refund.rs` | 15 | 9 | 4 | 0 | 2 | 0 |
| `satin/schedule.rs` | 11 | 8 | 3 | 0 | 0 | 0 |
| `satin/spec_numbers.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `satin/start_ledger.rs` | 7 | 0 | 0 | 0 | 7 | 0 |
| `satin/state.rs` | 4 | 3 | 1 | 0 | 0 | 0 |
| `satin/state_growth.rs` | 25 | 12 | 12 | 0 | 1 | 0 |
| `satin/static_callee.rs` | 6 | 0 | 0 | 0 | 6 | 0 |
| `satin/stops.rs` | 25 | 6 | 14 | 0 | 5 | 0 |
| `satin/strategic_stop.rs` | 12 | 4 | 5 | 0 | 3 | 0 |
| `satin/synthetic_frame_gas.rs` | 11 | 1 | 5 | 0 | 5 | 0 |
| `satin/system_call.rs` | 8 | 1 | 0 | 0 | 7 | 0 |
| `satin/tool_writes.rs` | 3 | 0 | 0 | 0 | 3 | 0 |
| `satin/transfer_logs.rs` | 20 | 6 | 10 | 0 | 4 | 0 |
| `satin/volatile_access.rs` | 21 | 11 | 9 | 0 | 1 | 0 |
| `satin/withheld_gas.rs` | 31 | 15 | 12 | 0 | 4 | 0 |
| `satin/write_records.rs` | 51 | 24 | 20 | 0 | 7 | 0 |
| `system/access_control.rs` | 37 | 20 | 17 | 0 | 0 | 0 |
| `system/control.rs` | 9 | 6 | 3 | 0 | 0 | 0 |
| `system/dispatch.rs` | 11 | 6 | 3 | 0 | 2 | 0 |
| `system/keyless/charges.rs` | 11 | 8 | 3 | 0 | 0 | 0 |
| `system/keyless/deploy.rs` | 38 | 18 | 19 | 0 | 1 | 0 |
| `system/keyless/detention.rs` | 12 | 3 | 8 | 0 | 1 | 0 |
| `system/keyless/differential.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `system/keyless/dispatch.rs` | 7 | 6 | 1 | 0 | 0 | 0 |
| `system/keyless/frame.rs` | 7 | 3 | 3 | 0 | 1 | 0 |
| `system/keyless/inspector.rs` | 11 | 4 | 1 | 0 | 6 | 0 |
| `system/keyless/interface.rs` | 1 | 0 | 0 | 0 | 1 | 0 |
| `system/keyless/limits.rs` | 12 | 2 | 9 | 0 | 1 | 0 |
| `system/keyless/precedence.rs` | 6 | 0 | 6 | 0 | 0 | 0 |
| `system/keyless/rules.rs` | 42 | 17 | 21 | 0 | 4 | 0 |
| `system/limit_control.rs` | 5 | 4 | 1 | 0 | 0 | 0 |
| `system/oracle.rs` | 23 | 13 | 10 | 0 | 0 | 0 |
| `system/oracle_storage.rs` | 18 | 11 | 6 | 0 | 1 | 0 |
| `system/predeploys.rs` | 5 | 0 | 0 | 0 | 5 | 0 |
| `system/remaining_compute_gas.rs` | 16 | 3 | 13 | 0 | 0 | 0 |
| `system/system_tx.rs` | 29 | 22 | 4 | 0 | 3 | 0 |
| `tracers/scenarios.rs` | 15 | 11 | 4 | 0 | 0 | 0 |
| **total** | 1271 | 418 | 428 | 0 | 425 | 0 |

## Per test

| File | Test | Class | Scenarios or reason |
| --- | --- | --- | --- |
| `satin/compute_gas.rs` | `test_a_floor_bound_transaction_counts_its_floor_and_history_comes_out_first` | covered | p6_floor_zero_calldata (1000 zero bytes, floor-bound; below cap only, nothing at ABOVE_CAP) |
| `satin/compute_gas.rs` | `test_gas_an_inspector_edits_moves_the_compute_figure_with_the_receipt` | not-portable | other: a custom inspector that charges and erases gas mid-run |
| `satin/compute_gas.rs` | `test_an_empty_call_computes_its_intrinsic_gas` | covered | p6_stop, p6_add (plain call, intrinsic + body history); c1_value_tx_ok_res above the cap |
| `satin/compute_gas.rs` | `test_arithmetic_is_computed_opcode_by_opcode` | ported | port_cg_arith_10m, port_cg_arith_300m |
| `satin/compute_gas.rs` | `test_memory_expansion_is_computed` | covered | p7_memory_crossing_noread (MLOAD expansion), i1_limit_call (MSTORE/MLOAD of a word via store_returndata_word) |
| `satin/compute_gas.rs` | `test_storage_computes_what_is_neither_state_nor_history` | covered | ctl_sstore, ctl_sstore_res (fresh-slot SSTORE below and above the cap) |
| `satin/compute_gas.rs` | `test_a_refund_does_not_lower_the_compute_figure` | covered | blk_slot_flip_flop_t2 (clear of a committed slot: refund), blk_slot_across_transactions_t2 (overwrite of a non-zero slot); below cap only |
| `satin/compute_gas.rs` | `test_state_gas_stays_off_the_compute_figure_whatever_it_costs` | ported | port_cg_state_gas_m1_10m, _m4_10m, _m1_300m, _m4_300m |
| `satin/compute_gas.rs` | `test_a_nested_call_s_computation_is_the_transaction_s` | covered | ctl_call_chain_res, p6_forward_98 (child compute counted on the transaction) |
| `satin/compute_gas.rs` | `test_a_reverted_frame_s_computation_stays_on_the_figure` | covered | p6_history_child_reverts, s3_value_call_reverting_child_res (reverted child's compute stays) |
| `satin/compute_gas.rs` | `test_each_transaction_computes_its_own` | covered | blk_counter_t1..t3 (one call repeated on one EVM) |
| `satin/compute_gas.rs` | `test_millions_of_gas_of_computation_are_all_on_the_figure` | covered | edge_create2_missing_salt_default_cap, edge_modexp_over_bound_detained_* (millions of gas of loop compute) |
| `satin/compute_gas.rs` | `test_below_the_cap_the_gas_limit_is_the_compute_budget` | ported | port_cg_gas_limit_is_budget_0, port_cg_gas_limit_is_budget_1, port_cg_gas_limit_is_budget_2, port_cg_gas_limit_is_budget_101 |
| `satin/compute_gas.rs` | `test_computation_past_the_execution_cap_is_an_ordinary_out_of_gas` | ported | port_cg_memory_past_cap_250m, _300m |
| `satin/compute_gas.rs` | `test_a_nested_call_cannot_compute_past_the_cap` | ported | port_cg_nested_memory_past_cap |
| `satin/compute_gas.rs` | `test_intrinsic_gas_past_the_execution_cap_is_rejected_before_it_runs` | not-portable | other: EIP-2930 access list entries (47,844 addresses) not expressible |
| `satin/compute_gas.rs` | `test_precompiles_are_computed_at_their_price_and_add_up` | covered | q09_det_precompile_price_fits (SHA256 from a contract); fz mode 7: depth-0 precompiles at their prices (SHA256, RIPEMD160, ECRECOVER) |
| `satin/compute_gas.rs` | `test_a_precompile_runs_on_exactly_its_price` | covered | fz mode 7: top-level precompile, gas limit drawn around needs+price (SHA256 and BN254 pairing with 0-2 pairs) |
| `satin/compute_gas.rs` | `test_a_precompile_in_a_nested_call_is_computed` | covered | q09_det_precompile_price_fits, q10_det_precompile_price_over_forward (SHA256 called from a contract) |
| `satin/compute_gas.rs` | `test_opcodes_and_precompiles_add_up` | ported | port_cg_sum_10m, port_cg_sum_300m |
| `satin/contract_size.rs` | `test_the_satin_initcode_size_is_accepted` | ported | port_size_initcode_at_limit |
| `satin/contract_size.rs` | `test_one_byte_past_the_satin_initcode_size_is_rejected` | ported | port_size_initcode_one_over |
| `satin/contract_size.rs` | `test_the_satin_code_size_deploys` | ported | port_size_code_at_limit |
| `satin/contract_size.rs` | `test_one_byte_past_the_satin_code_size_halts` | ported | port_size_code_one_over |
| `satin/contract_size.rs` | `test_create_of_the_satin_code_size_succeeds` | ported | port_size_create_code_at_limit |
| `satin/contract_size.rs` | `test_create_one_byte_past_the_satin_code_size_fails` | ported | port_size_create_code_one_over |
| `satin/corpus.rs` | `test_corpus_scenarios_keep_their_outcome_and_gas` | not-portable | snapshot (pins the bench corpus benches/scenarios/*.json outcome and gas) |
| `satin/count_order.rs` | `test_a_record_crossing_both_counts_names_the_frame_budget` | not-portable | other: per-frame data-size cap (frame_data_size_limit = 120) is not a scenario limit |
| `satin/create_opcodes.rs` | `test_create2_with_missing_operands_halts_with_stack_underflow` | covered | fz mode 13: CREATE2 with its last operands missing (len 0x8000, offset 0), detained and undetained; edge_create2_missing_salt_detained (memory size after the halt is not in the record) |
| `satin/create_opcodes.rs` | `test_create2_len_zero_offset_max_succeeds` | ported | port_create2_len0_offset_max |
| `satin/create_opcodes.rs` | `test_create2_len_nonzero_offset_max_halts` | ported | port_create2_len1_offset_max |
| `satin/create_opcodes.rs` | `test_create2_len_zero_large_offset_skips_memory_expansion` | ported | port_create2_len0_offset_large |
| `satin/create_opcodes.rs` | `test_create2_len_zero_offset_zero_succeeds` | covered | fz mode 6: CREATE2 of an empty init code (init_code's empty branch) at offset 0 |
| `satin/create_opcodes.rs` | `test_create2_with_a_32_kib_init_code_pays_its_words` | ported | port_create2_32kib, port_create2_32bytes |
| `satin/create_opcodes.rs` | `test_create2_init_code_at_the_limit_succeeds_and_one_byte_more_halts` | ported | port_create2_init_at_limit, port_create2_init_one_over |
| `satin/create_opcodes.rs` | `test_create2_oversized_init_code_halts_before_touching_memory` | ported | port_create2_init_one_over, port_create2_init_10mib |
| `satin/create_opcodes.rs` | `test_create2_oversized_init_code_halts_before_its_offset_is_read` | ported | port_create2_init_one_over_far_offset |
| `satin/create_opcodes.rs` | `test_create2_whose_init_code_memory_cannot_be_paid_halts_on_the_memory` | ported | port_create2_memory_unpayable, port_create2_memory_unpayable_no_salt |
| `satin/data_size.rs` | `test_depth_3_frame_budget_reverts_the_child_and_the_parents_resume` | not-portable | other: per-frame data-size cap (frame_data_size_limit = 20,000) is not a scenario limit |
| `satin/data_size.rs` | `test_depth_3_transaction_limit_reverts_the_child_and_the_parents_resume` | ported | port_dsx_depth3_tx_limit |
| `satin/data_size.rs` | `test_calldata_counts_in_the_body` | covered | p6_floor_zero_calldata, p6_floor_nonzero_calldata (calldata of 0 and non-zero bytes counted in the body's data size) |
| `satin/data_size.rs` | `test_deployed_code_one_byte_over_the_transaction_limit_is_not_left_deployed` | ported | port_dsx_code_fits, port_dsx_code_one_over |
| `satin/data_size.rs` | `test_deployed_code_over_the_frame_budget_reverts_the_creation_alone` | not-portable | other: per-frame data-size cap (frame_data_size_limit = 2,000) is not a scenario limit |
| `satin/data_size.rs` | `test_a_creation_is_held_to_its_share_from_its_start` | ported | port_dsx_creation_share_40_0_0, port_dsx_creation_share_41_0_0, … (10 scenarios, `port_dsx_creation_share_*`) |
| `satin/data_size.rs` | `test_code_return_create_refuses_is_not_counted` | ported | port_dsx_refused_code_ef_61, port_dsx_refused_code_oversize, port_dsx_control_code_61, port_dsx_control_code_max |
| `satin/data_size.rs` | `test_a_log_one_byte_over_stops_on_the_log` | ported | port_ds_log_fits, port_ds_log_one_over |
| `satin/data_size.rs` | `test_a_storage_write_one_byte_over_stops_on_the_write` | ported | port_ds_write_fits, port_ds_write_one_over |
| `satin/data_size.rs` | `test_an_account_write_one_byte_over_stops_on_the_recipient` | ported | port_dsx_account_write_fits, port_dsx_account_write_start_short, port_dsx_account_write_log_short |
| `satin/data_size.rs` | `test_whichever_of_gas_and_data_size_binds_first_is_reported` | ported | port_bind_top_sstore_fits_50000, port_bind_top_sstore_fits_100000, … (42 scenarios, `port_bind_top_*`) |
| `satin/data_size.rs` | `test_the_fresh_slot_boundary_is_the_sum_of_its_parts` | ported | port_ds_slot_349_oog, _349_stop, _350_oog, _350_ok |
| `satin/data_size.rs` | `test_a_frame_start_is_charged_before_its_records_are_counted` | ported | port_dsx_start_charged_first_cross_592194, port_dsx_start_charged_first_cross_592195, … (8 scenarios, `port_dsx_start_charged_first_*`) |
| `satin/data_size.rs` | `test_a_caller_one_gas_short_of_its_records_runs_out_of_gas_under_a_limit_they_would_cross` | ported | port_dsx_caller_short_592190, port_dsx_caller_short_592194, port_dsx_caller_short_592195, port_dsx_caller_short_592196 |
| `satin/data_size.rs` | `test_whichever_of_gas_and_data_size_binds_first_is_reported_at_a_childs_write` | ported | port_bind_child_sstore_fits_60000, port_bind_child_sstore_fits_100000, … (30 scenarios, `port_bind_child_*`) |
| `satin/data_size.rs` | `test_a_record_a_limit_refuses_is_not_charged_where_the_reservoir_has_one_gas_too_few` | ported | port_bind_reservoir_sstore_fits_0, port_bind_reservoir_sstore_fits_1, … (24 scenarios, `port_bind_reservoir_*`) |
| `satin/data_size.rs` | `test_a_body_over_the_limit_is_rejected_at_every_gas_limit` | ported | port_dsx_body_over_value_to_empty_0, port_dsx_body_over_value_to_empty_21000, … (9 scenarios, `port_dsx_body_over_*`) |
| `satin/data_size.rs` | `test_a_child_gets_98_percent_of_what_its_parent_has_left_in_slots` | ported | port_dsx_child_98pct_b98, port_dsx_child_98pct_b99, … (7 scenarios, `port_dsx_child_98pct_*`) |
| `satin/data_size.rs` | `test_the_body_and_the_execution_add_up_to_the_stop` | ported | port_dsx_body_and_execution_0, port_dsx_body_and_execution_200 |
| `satin/data_size.rs` | `test_a_body_over_the_limit_is_rejected_before_an_interceptor` | ported | port_dsx_interceptor_answered, port_dsx_interceptor_refused_for_body |
| `satin/data_size.rs` | `test_a_failed_creations_nonce_record_holds_its_creator_to_its_budget` | ported | port_dsx_failed_creation_nonce_create_fits, port_dsx_failed_creation_nonce_create2_fits, port_dsx_failed_creation_nonce_create_short, port_dsx_failed_creation_nonce_create2_short |
| `satin/data_size.rs` | `test_an_inspector_that_would_halt_a_resumes_stop_is_never_handed_it` | not-portable | other: a custom inspector that halts the creator on its resume |
| `satin/data_size.rs` | `test_a_creation_counting_nothing_runs_and_its_creator_is_stopped_at_the_return` | ported | port_dsx_creation_counting_nothing |
| `satin/data_size.rs` | `test_a_caller_a_failed_callee_left_over_the_transaction_limit_is_stopped` | ported | port_dsx_caller_left_over_halts, port_dsx_caller_left_over_writes |
| `satin/data_size_counts.rs` | `test_what_each_transaction_keeps` | covered | kinds each in a scenario: edge_start_record_paid (value to existing), c4_value_tx_to_empty_res (to absent), fz mode 11 (value to the sender), ctl_sstore, fz logs, p6_child_refills_ancestor_* (DELEGATECALL), q29_create_reverts_salt2_res, edge_value_returned_by_selfdestruct; the access-list row is not portable |
| `satin/data_size_counts.rs` | `test_a_reverted_childs_write_and_write_back_leave_its_callers_write` | ported | port_dsx_reverted_write_and_back |
| `satin/data_size_counts.rs` | `test_the_senders_own_authorization_applies_and_records_nothing` | covered | e09_sender_authority (the sender authorizes itself: no record of its own) |
| `satin/data_size_counts.rs` | `test_a_selfdestruct_in_a_static_frame_keeps_nothing` | covered | bd_selfdestruct_to_static (SELFDESTRUCT in a static frame halts before any move) |
| `satin/data_size_counts.rs` | `test_a_top_level_revert_keeps_the_body` | covered | c2_value_tx_revert, c2_value_tx_revert_res (the first frame reverts, the body stays counted) |
| `satin/data_size_counts.rs` | `test_a_limit_equal_to_what_is_kept_does_not_stop` | ported | port_ds_limit_equal_kept_calldata0, 100, 1000 |
| `satin/data_size_counts.rs` | `test_each_transaction_counts_from_zero` | covered | blk_limits_reset_t1..t3, blk_hint_limit_per_transaction_t1..t3 |
| `satin/db_error.rs` | `test_sstore_storage_read_error_fails_the_transaction` | not-portable | db fault (storage read error injected in SSTORE) |
| `satin/db_error.rs` | `test_value_call_account_read_error_fails_the_transaction` | not-portable | db fault (account read error of a value call's recipient) |
| `satin/db_error.rs` | `test_created_account_storage_is_never_read_from_the_database` | covered | fz mode 6 (balance-only occupant, constructor writes slot 1) and fz mode 9 (pre-funded creation address, constructor write): the read set shows no read of the created account's storage |
| `satin/db_error.rs` | `test_staticcall_account_read_error_fails_the_transaction` | not-portable | db fault (account read error of a STATICCALL target) |
| `satin/deposit_charge.rs` | `test_a_creation_one_gas_short_of_the_hash_runs_out_of_gas_above_the_cap` | ported | port_depcharge_hash_above_cap_0, port_depcharge_hash_above_cap_1, port_depcharge_hash_above_cap_2, port_depcharge_hash_above_cap_3 |
| `satin/deposit_charge.rs` | `test_a_creation_one_gas_short_of_its_state_gas_runs_out_of_gas_with_the_hash_paid` | ported | port_depcharge_state_below_cap_0, port_depcharge_state_below_cap_1, port_depcharge_state_below_cap_2, port_depcharge_state_below_cap_3 |
| `satin/deposit_charge.rs` | `test_a_creation_one_gas_short_of_its_history_runs_out_of_gas_with_the_state_gas_paid` | ported | port_depcharge_history_below_cap_0, port_depcharge_history_below_cap_1, port_depcharge_history_below_cap_2, port_depcharge_history_below_cap_3 |
| `satin/deposit_charge.rs` | `test_a_creation_that_cannot_pay_for_its_code_runs_out_of_gas_under_the_data_size_limit` | not-portable | other: per-frame data-size cap (frame_data_size_limit = 1,000) is not a scenario limit |
| `satin/deposit_charge.rs` | `test_a_creation_that_cannot_pay_the_deposit_cost_runs_out_of_gas_under_the_data_size_limit` | not-portable | other: the neutral Osaka configuration (no EIP-8037) and a per-frame cap |
| `satin/detained_transfers.rs` | `test_a_value_call_to_a_precompile_after_a_read_keeps_the_log_it_counted` | covered | q11_det_value_precompile_price_crosses, q43_det_value_call_priced_precompile_crosses (value call priced past the allowance), q09-shaped fits; fz mode 7 (top-level precompile with value from the beneficiary) |
| `satin/detained_transfers.rs` | `test_a_precompile_answered_from_its_price_at_a_data_size_boundary_is_the_compute_stop` | ported | port_dettr_precompile_price_boundary_short, port_dettr_precompile_price_boundary_fits, port_dettr_precompile_price_ample |
| `satin/detained_transfers.rs` | `test_a_value_calls_own_transfer_crossing_the_cap_stops_its_caller_before_anything_moves` | ported | port_dettr_value_call_1000_receiver_100, port_dettr_value_call_1000_receiver_140, … (28 scenarios, `port_dettr_value_call_*`) |
| `satin/detained_transfers.rs` | `test_a_value_call_its_caller_cannot_fund_after_a_read_runs_as_without_it` | covered | q12_det_unfunded_value_precompile, q42_det_unfunded_value_new_account, q44_det_unfunded_value_call_priced_precompile, w2_det_call_out_of_funds_loop |
| `satin/detained_transfers.rs` | `test_a_selfdestructs_transfer_log_goes_with_the_frame_the_cap_stops` | ported | port_dettr_selfdestruct_4999, port_dettr_selfdestruct_after_call_4999, … (14 scenarios, `port_dettr_selfdestruct_*`) |
| `satin/detained_transfers.rs` | `test_every_detained_frame_start_keeps_the_log_it_counted` | covered | w1/w2/w5/w7_det_*_loop, q04/q05 (value to new account), q11/q12/q43/q44 (precompiles), q17_det_created_destroyed_to_new_beneficiary, q18; fz mode 6 (detained CREATE2 with value) |
| `satin/detention.rs` | `test_every_volatile_read_caps_the_transaction_from_where_it_read` | ported | port_det_read_timestamp_100000000, port_det_read_timestamp_300000000, … (29 scenarios, `port_det_read_*`) |
| `satin/detention.rs` | `test_the_cap_counts_from_a_spend_larger_than_itself` | ported | port_det_cap_after_larger_spend_loop, port_det_cap_after_larger_spend_stop |
| `satin/detention.rs` | `test_without_a_read_nothing_is_capped` | covered | p7_static_crossing_noread, p7_real_oog_halts_noread (same loops without a read: the frame's own out-of-gas) |
| `satin/detention.rs` | `test_a_transaction_from_the_beneficiary_is_detained_from_the_start` | covered | t02_sender_is_beneficiary, blk_beneficiary_sender_then_user_t1 |
| `satin/detention.rs` | `test_other_accounts_and_storage_are_not_volatile` | ported | port_det_other_accounts_not_volatile |
| `satin/detention.rs` | `test_blobhash_is_not_volatile` | ported | port_det_blobhash_not_volatile |
| `satin/detention.rs` | `test_the_history_contract_reads_the_block_number_and_the_beacon_roots_contract_reads_state` | ported | port_det_real_system_contract_history, port_det_real_system_contract_beacon |
| `satin/detention.rs` | `test_a_read_answers_with_the_blocks_field` | ported | port_det_read_answers_field |
| `satin/detention.rs` | `test_the_most_restrictive_limit_binds_whatever_the_order` | ported | port_det_most_restrictive_block_then_oracle, port_det_most_restrictive_oracle_then_block |
| `satin/detention.rs` | `test_a_callers_limits_set_the_caps` | covered | d1_top_frame_crosses (block-env cap), q54_oracle_answers_then_loop (Oracle cap), fz mode 8 (caps drawn up to u64::MAX) |
| `satin/detention.rs` | `test_the_default_limits_detain` | covered | edge_create2_missing_salt_default_cap, edge_modexp_over_bound_detained_*, edge_sd_beneficiary_history_unpaid_default_cap (default caps); the executor-installed limits part is not portable |
| `satin/detention.rs` | `test_spending_exactly_the_cap_completes` | ported | port_det_exact_cap_10, port_det_exact_cap_11, … (11 scenarios, `port_det_exact_cap_*`) |
| `satin/detention.rs` | `test_a_value_called_child_reads_from_its_callers_whole_charges` | ported | port_det_value_child_reads, port_det_value_tx_reads |
| `satin/detention.rs` | `test_stipend_funded_work_is_compute` | ported | port_det_stipend_work_compute |
| `satin/detention.rs` | `test_a_returned_value_call_counts_what_its_callee_ran` | ported | port_det_returned_value_call_cap_low, port_det_returned_value_call_cap_mid, port_det_returned_value_call_cap_high |
| `satin/detention.rs` | `test_a_call_that_reads_counts_from_what_its_caller_paid` | ported | port_det_call_beneficiary_value_0, port_det_call_beneficiary_value_1000, port_det_call_beneficiary_value_10000, port_det_call_beneficiary_value_100000 |
| `satin/detention.rs` | `test_a_childs_read_caps_its_caller` | covered | d6_read_in_child_caller_loops |
| `satin/detention.rs` | `test_a_caller_resumes_after_the_stop_held_to_what_is_left` | covered | d2_child_crosses_caller_resumes (A resumes and writes after B's crossing) |
| `satin/detention.rs` | `test_the_stop_bills_the_same_above_and_below_the_execution_cap` | covered | d1_top_frame_crosses, d1_top_frame_crosses_res |
| `satin/detention.rs` | `test_running_out_of_the_frames_own_gas_still_halts` | covered | p7_real_oog_halts_read, d5_halt_burns_no_compute |
| `satin/detention.rs` | `test_writes_after_a_read_stop_at_the_cap_or_run_out_of_their_own_gas` | ported | port_det_writes_after_read_60000000, port_det_writes_after_read_372448000 |
| `satin/detention.rs` | `test_a_failed_load_caps_nothing` | covered | fz mode 12: Oracle SLOAD and value call to the beneficiary with frame gas around their charges (access on), the caller writing after |
| `satin/detention.rs` | `test_an_inspectors_reads_are_not_the_transactions` | not-portable | other: a custom inspector that reads TIMESTAMP through the Host every step |
| `satin/detention.rs` | `test_a_refused_read_reverts_the_frame_and_charges_its_static_gas` | covered | bd_timestamp_static_*, bd_slotnum_static_*, bd_blockhash_static_*, bd_balance_static_*, bd_call_target_value_*, bd_selfdestruct_to_static_*, bd_selfbalance_static_* |
| `satin/detention.rs` | `test_the_refusal_names_the_access_type` | covered | bd_blockhash_static_at (type 7), a11b_slotnum_refused, a07_oracle_read_refused (revert data in the output) |
| `satin/detention.rs` | `test_the_switch_is_scoped_to_its_subtree` | covered | a02_child_refused_parent_reenables, a04_switch_cleared_on_return, a01_disable_then_timestamp_refused |
| `satin/detention.rs` | `test_an_interceptors_answer_does_not_lift_the_cap` | covered | i9_limit_call_detained, q39_det_limit_control_after_state |
| `satin/detention.rs` | `test_a_frames_return_hands_back_what_was_withheld` | covered | p7_*_read vs p7_*_noread pairs (gas_reads_total, forward_from_total) |
| `satin/detention.rs` | `test_state_and_history_gas_are_not_compute` | covered | q24_det_reservoir_exhausted_spill, q25, p7_sstore_spill_withheld_first_read (state gas past the cap, compute within) |
| `satin/detention.rs` | `test_gas_spilled_before_the_read_is_not_compute` | ported | port_det_spill_before_read_100000000, port_det_spill_before_read_300000000 |
| `satin/detention.rs` | `test_a_refill_after_the_read_does_not_lift_the_cap` | ported | port_det_refill_after_read |
| `satin/detention.rs` | `test_a_history_refill_after_the_read_does_not_lift_the_cap` | ported | port_det_history_refill_after_read |
| `satin/detention.rs` | `test_a_start_its_caller_cannot_pay_for_counts_none_of_its_forward` | ported | port_det_unpayable_start_value_call_60000, port_det_unpayable_start_value_call_120000, … (15 scenarios, `port_det_unpayable_start_*`) |
| `satin/detention.rs` | `test_the_most_compute_a_transaction_can_spend_bounds_the_caps` | ported | port_det_max_compute_bounds_200000000, port_det_max_compute_bounds_199999000, port_det_max_compute_bounds_150000000 |
| `satin/equivalence.rs` | `test_empty_transaction_matches_op_revm` | covered | edge_sweep_tx0_funded_existing_v0_runs (empty call), c1_value_tx_ok_res (above the cap); the op-revm comparison itself is the reference's job |
| `satin/equivalence.rs` | `test_value_transfer_matches_op_revm` | covered | edge_sweep_tx0_funded_absent_v5_runs, c4_value_tx_to_empty_res (value to a new account, transfer log) |
| `satin/equivalence.rs` | `test_a_nested_value_call_with_a_reservoir_matches_op_revm` | covered | s3_value_call_ok_child_res, s3_nested_value_calls_res (nested value calls above the cap) |
| `satin/equivalence.rs` | `test_sstore_matches_op_revm` | covered | ctl_sstore |
| `satin/equivalence.rs` | `test_fees_match_op_revm` | not-portable | other: a non-zero block base fee (7) and fee-vault split; scenarios run at base fee 0 (q46_l1_fee_gas_price_salt_detained covers a priced tx at base fee 0) |
| `satin/equivalence.rs` | `test_refund_matches_op_revm` | covered | blk_slot_flip_flop_t2 (clear of a non-zero slot: refund) |
| `satin/equivalence.rs` | `test_system_call_matches_op_revm` | not-portable | internal api (system_call_with_caller, a pre-block system call) |
| `satin/equivalence.rs` | `test_invalid_transaction_matches_op_revm` | covered | edge_gas_above_block (gas limit above the block's, refused) |
| `satin/equivalence.rs` | `test_an_intercepted_call_diverges_from_op_revm` | covered | edge_sweep_tx0_funded_system_v0_runs (tx to MegaAccessControl isVolatileDataAccessDisabled, answered) |
| `satin/equivalence.rs` | `test_a_system_address_transaction_diverges_from_op_revm` | covered | edge_system_legacy_oracle_promoted |
| `satin/equivalence.rs` | `test_the_created_deposit_caller_diverges_from_op_revm` | covered | p6_deposit_new_caller_m1, rev_deposit_caller_absent_ample |
| `satin/equivalence.rs` | `test_a_keyless_deployment_diverges_from_op_revm` | covered | k01_ok, k12_malformed_call, k13_call_value_refused (dispatch, refused call billed the overhead) |
| `satin/equivalence.rs` | `test_the_history_ledger_is_what_satin_adds_to_every_transaction` | covered | p6_stop, p6_floor_nonzero_calldata (body and calldata history) |
| `satin/equivalence.rs` | `test_a_crowded_salt_bucket_is_where_satin_leaves_op_revm` | covered | p6_salt_sstore_set_m2, p6_salt_sstore_set_m8 |
| `satin/equivalence.rs` | `test_a_creation_that_destroys_itself_diverges_from_op_revm` | covered | p6_destroy_in_constructor_m1 (constructor destroys the new contract: state gas and record given back) |
| `satin/equivalence.rs` | `test_an_out_of_gas_before_the_first_frame_matches_op_revm` | covered | edge_sweep_tx0_funded_absent_v5_runtime0 (one gas short of the new account's charge) |
| `satin/equivalence.rs` | `test_an_out_of_gas_creation_before_the_first_frame_matches_op_revm` | covered | edge_sweep_tx0_funded_create_v0_start, edge_start_record_unpaid_create |
| `satin/failed_deposit.rs` | `test_a_system_deposit_over_the_limit_reports_the_failed_deposit` | not-portable | other: a deposit with is_system_transaction set (a scenario deposit carries only its mint) |
| `satin/failed_deposit.rs` | `test_a_deposit_whose_body_is_over_the_limit_is_a_failed_deposit` | covered | edge_deposit_body_over_limit, edge_sweep_dep_*_body (deposit body over the data-size limit: a failed deposit keeping its body) |
| `satin/failed_deposit.rs` | `test_a_deposit_over_the_limit_fails_at_every_gas_limit` | ported | port_faildep_over_limit_14999, port_faildep_over_limit_1000000 |
| `satin/failed_deposit.rs` | `test_a_failed_deposit_counts_the_caller_it_creates` | ported | port_faildep_caller_growth_short1_absent_zero, port_faildep_caller_growth_short1_absent_none, … (16 scenarios, `port_faildep_caller_growth_*`) |
| `satin/failed_deposit.rs` | `test_a_failed_deposit_keeps_the_hints_it_forwarded` | ported | port_faildep_keeps_hints_deposit, port_faildep_keeps_hints_call, port_faildep_keeps_hints_call_data |
| `satin/floor_exempt.rs` | `test_the_floor_of_an_exempt_transaction` | ported | port_floor_deposit_call_32_10000000, port_floor_system_call_32_10000000, … (25 scenarios, `port_floor_*`) |
| `satin/gas_forward.rs` | `test_a_start_forwards_all_but_two_percent_of_its_callers_regular_gas` | covered | p6_forward_98, ctl_call_chain_res, s3_create_res; fz modes 1-2 (every call scheme and CREATE forwarding all gas; the frames field records each forward) |
| `satin/gas_forward.rs` | `test_a_value_call_with_ten_thousand_left_forwards_9800_and_its_child_gets_12100` | not-portable | internal api (forward_cap / holding_to_keep arithmetic) |
| `satin/gas_forward.rs` | `test_a_call_forwards_all_but_two_percent_of_what_its_charge_leaves` | covered | ctl_call_chain_res, s3_value_call_ok_child_res (above the cap; frames' gas_limit) |
| `satin/gas_forward.rs` | `test_a_creation_forwards_all_but_two_percent_of_what_its_charges_leave` | covered | s3_create_res (CREATE above the cap; the frames field records the forward) |
| `satin/history_allowance.rs` | `test_the_allowance_is_one_three_topic_event_of_history` | not-portable | internal api (asserts HISTORY_ALLOWANCE_BYTES and history_allowance_gas constants) |
| `satin/history_allowance.rs` | `test_a_transfer_can_emit_one_event_because_of_the_allowance` | covered | p6_allowance_value_call (value call, LOG3 of one word on the stipend), q31_nested_value_calls_allowance |
| `satin/history_allowance.rs` | `test_the_allowance_buys_one_event_and_no_more` | covered | q06_stipend_log_over_allowance (more than the allowance on the stipend alone runs out), q31 (the extra LOG0 is charged) |
| `satin/history_allowance.rs` | `test_the_allowance_cannot_be_spent_on_computation` | ported | port_allow_computation_296, port_allow_computation_297, … (17 scenarios, `port_allow_computation_*`) |
| `satin/history_allowance.rs` | `test_an_event_larger_than_the_allowance_pays_the_rest_or_runs_out_of_gas` | ported | port_allow_big_event_100_600, port_allow_big_event_100_800, … (30 scenarios, `port_allow_big_event_*`) |
| `satin/history_allowance.rs` | `test_the_allowance_cannot_be_spent_on_a_write_record` | ported | port_allow_not_on_write_50000, port_allow_not_on_write_52000, … (12 scenarios, `port_allow_not_on_write_*`) |
| `satin/history_allowance.rs` | `test_the_allowance_cannot_be_spent_on_state` | ported | port_allow_not_on_state_50000, port_allow_not_on_state_52000, … (12 scenarios, `port_allow_not_on_state_*`) |
| `satin/history_allowance.rs` | `test_a_top_level_transfer_gets_no_allowance` | ported | port_allow_top_level_transfer |
| `satin/history_allowance.rs` | `test_a_call_without_value_gets_no_allowance` | covered | p6_allowance_zero_value_call |
| `satin/history_allowance.rs` | `test_callcode_is_granted_one_and_delegatecall_is_not` | covered | p6_allowance_value_callcode (CALLCODE with value), p6_allowance_zero_value_call (no value: none) |
| `satin/history_allowance.rs` | `test_an_intercepted_call_neither_takes_nor_adds_an_allowance` | ported | port_allow_intercepted_one_log, port_allow_intercepted_two_logs |
| `satin/history_allowance.rs` | `test_a_transaction_level_stop_does_not_hand_the_allowance_back` | ported | port_allow_tx_stop_spent, port_allow_tx_stop_unspent |
| `satin/history_allowance.rs` | `test_an_unspent_allowance_does_not_return_to_the_caller` | ported | port_allow_unspent_code_stop, port_allow_unspent_no_code |
| `satin/history_allowance.rs` | `test_a_chain_of_transfers_grants_each_frame_its_own_allowance` | ported | port_allow_chain |
| `satin/history_allowance.rs` | `test_the_gap_the_allowance_opens_between_bytes_and_gas_is_pinned` | ported | port_allow_gap_bytes_gas |
| `satin/history_allowance.rs` | `test_a_frame_budget_revert_does_not_hand_the_allowance_back` | not-portable | other: per-frame data-size cap (frame_data_size_limit = 500) is not a scenario limit |
| `satin/history_allowance.rs` | `test_a_drained_allowance_is_not_refunded_by_a_revert` | ported | port_allow_drained_revert |
| `satin/history_allowance.rs` | `test_a_transfer_forwarding_no_gas_still_buys_its_event` | covered | p6_allowance_value_call (forward 0, stipend only) |
| `satin/history_allowance.rs` | `test_a_replaced_interpreter_keeps_its_frames_allowance` | not-portable | other: a custom inspector that replaces the frame's interpreter |
| `satin/history_bytes.rs` | `test_a_reservoir_that_runs_out_changes_only_who_paid` | ported | port_hbytes_reservoir_0, port_hbytes_reservoir_100000, … (6 scenarios, `port_hbytes_reservoir_*`) |
| `satin/history_bytes.rs` | `test_a_transaction_that_cannot_pay_its_first_record_reports_its_body_alone` | covered | edge_start_record_unpaid_legacy, edge_start_record_one_below (value to an existing account short of its record) |
| `satin/history_bytes.rs` | `test_a_creation_answered_without_running_appends_no_code` | not-portable | other: a custom inspector that answers every creation |
| `satin/history_bytes.rs` | `test_a_write_back_dies_with_the_frame_that_made_it` | covered | p6_child_refills_ancestor_revert (delegate writes the slot back and reverts) |
| `satin/history_bytes.rs` | `test_the_bytes_exceed_the_history_gas_by_what_the_allowances_paid` | covered | p6_allowance_value_call, q31_nested_value_calls_allowance (history_bytes above history gas by the allowance) |
| `satin/history_bytes.rs` | `test_the_history_bytes_are_the_data_size_every_record_kept` | covered | sites each in a scenario: e01/e03/e09 (authorizations), p6_history_*, q29 (failed creation), i6_value_call_refused (interceptor refusal), h01/h08 (hint), c4/s2 (transfer logs), edge_value_returned_by_selfdestruct, q18; the access-list row is not portable |
| `satin/history_exemption.rs` | `test_a_user_transaction_pays_history_for_the_program` | covered | q04_det_value_new_account_and_stipend_log, p6_salt_code_deposit_m1, ctl_sstore, p6_history_ok (each history site billed in a user tx) |
| `satin/history_exemption.rs` | `test_the_exempt_schedule_prices_a_deposited_byte_at_zero` | covered | q20_deposit_new_caller_creates_salt (a deposit's creation deploys code: no history) |
| `satin/history_exemption.rs` | `test_a_deposit_pays_no_history_gas` | covered | q21_deposit_writes_salt2 (deposit: fresh slot, LOG0, value to a new account) |
| `satin/history_exemption.rs` | `test_a_system_transaction_pays_no_history_gas` | covered | edge_system_legacy_oracle_promoted, port_system_nonce_stale_check_off (Mega System Transaction writes the Oracle, no history) |
| `satin/history_exemption.rs` | `test_a_system_call_pays_no_history_gas` | not-portable | internal api (transact_system_call, a pre-block system call) |
| `satin/history_exemption.rs` | `test_an_exempt_transaction_reports_no_history_bytes` | covered | q21_deposit_writes_salt2, cap_deposit_2p32 (above the cap), edge_system_legacy_oracle_promoted; the system-call row is not portable |
| `satin/history_exemption.rs` | `test_the_exemption_does_not_outlive_its_transaction` | covered | blk_deposit_creates_caller_t1/t2 (a deposit, then a user tx on the same EVM) |
| `satin/history_exemption.rs` | `test_what_a_transaction_pays_is_decided_when_it_is_prepared` | not-portable | internal api (sets a transaction in the context without running it) |
| `satin/history_exemption.rs` | `test_a_context_switched_back_from_the_neutral_configuration_prices_no_history_until_prepared` | not-portable | internal api (with_neutral_cfg / with_cfg switches and run_system_call) |
| `satin/history_gas.rs` | `test_deployed_code_pays_history_for_every_byte` | covered | p6_salt_code_deposit_m1, s3_create_res (deployed code: state and history per byte) |
| `satin/history_gas.rs` | `test_a_reverted_deployment_pays_no_code_deposit_history` | covered | q29_create_reverts_salt2_res, w4_det_create_init_reverts_loop |
| `satin/history_gas.rs` | `test_calldata_costs_one_history_byte_per_byte` | covered | p6_floor_zero_calldata vs p6_floor_nonzero_calldata |
| `satin/history_gas.rs` | `test_a_log_pays_history_on_top_of_the_schedules_price` | covered | p6_history_ok, p6_history_ok_res (LOG0/LOG1 history) |
| `satin/history_gas.rs` | `test_a_call_whose_caller_cannot_pay_its_records_starts_no_frame` | ported | port_hgas_call_cannot_pay_350000, port_hgas_call_cannot_pay_400000, … (9 scenarios, `port_hgas_call_cannot_pay_*`) |
| `satin/history_gas.rs` | `test_a_creation_whose_creator_cannot_pay_its_records_starts_no_frame` | ported | port_hgas_create_cannot_pay_350000, port_hgas_create_cannot_pay_400000, … (9 scenarios, `port_hgas_create_cannot_pay_*`) |
| `satin/history_gas.rs` | `test_a_creation_onto_an_account_the_transaction_changed_charges_its_creator_one_record` | ported | port_hgas_create_onto_changed_60000, port_hgas_create_onto_changed_120000, … (6 scenarios, `port_hgas_create_onto_changed_*`) |
| `satin/history_gas.rs` | `test_every_call_kind_charges_its_records_after_its_forward` | ported | port_hgas_call_kind_call_10000_below, port_hgas_call_kind_call_10000_above, … (48 scenarios, `port_hgas_call_kind_*`) |
| `satin/history_gas.rs` | `test_no_gas_limit_buys_a_write_record_for_nothing` | ported | port_hgas_no_free_record_below_0_short, port_hgas_no_free_record_below_0_ample, … (32 scenarios, `port_hgas_no_free_record_*`) |
| `satin/history_gas.rs` | `test_the_body_carries_the_writes_every_transaction_makes` | covered | q46_l1_fee_gas_price_salt_detained, edge_start_record_paid (priced txs: fee recipients credited, body history unchanged) |
| `satin/history_gas.rs` | `test_a_deposit_pays_for_none_of_the_bodys_writes` | covered | q21_deposit_writes_salt2, p6_deposit_new_caller_m1 (no body history on a deposit) |
| `satin/history_gas.rs` | `test_a_beneficiary_that_is_a_fee_vault_changes_nothing` | ported | port_hgas_fee_vault_beneficiary_vault, port_hgas_fee_vault_beneficiary_other |
| `satin/history_gas.rs` | `test_the_first_frames_record_is_given_back_once` | covered | edge_value_returned_to_sender (returns the value and succeeds), c1_value_tx_ok (keeps it), c2_value_tx_revert (frame fails); the returns-and-reverts row is not a scenario |
| `satin/history_reservoir.rs` | `test_a_value_call_pays_its_records_out_of_the_reservoir` | covered | s3_value_call_ok_child_res |
| `satin/history_reservoir.rs` | `test_a_nested_creation_pays_its_records_out_of_the_reservoir` | covered | s3_create_res |
| `satin/history_reservoir.rs` | `test_a_reverting_child_gives_its_records_back_to_the_reservoir` | covered | s3_value_call_reverting_child_res |
| `satin/history_reservoir.rs` | `test_a_halting_child_gives_its_records_back_as_a_reverting_one_does` | ported | port_res_child_halts_300000000, port_res_child_halts_50000000, port_res_child_reverts_300000000, port_res_child_reverts_50000000 |
| `satin/history_reservoir.rs` | `test_an_intercepted_value_call_gives_its_records_back_once` | ported | port_res_intercepted_value_call_limit, port_res_intercepted_value_call_access |
| `satin/history_reservoir.rs` | `test_the_body_draws_the_reservoir_first_and_only_its_excess_from_regular_gas` | covered | fz mode 11 (gas limits just above the cap around the body's history), edge_create_tx_destroyed_in_constructor_part_reservoir |
| `satin/history_reservoir.rs` | `test_an_out_of_gas_before_the_first_frame_keeps_the_body_the_reservoir_paid` | ported | port_res_oog_before_first_frame_1635, port_res_oog_before_first_frame_3, port_res_oog_before_first_frame_100 |
| `satin/inherited_costs.rs` | `test_the_first_call_to_an_address_nothing_warmed_is_charged_cold` | covered | ctl_call_chain_res, d8_call_charge_crosses_before_child (cold calls to untouched contracts) |
| `satin/inherited_costs.rs` | `test_the_first_call_to_a_precompile_is_charged_warm` | covered | q09_det_precompile_price_fits, d9_precompile_price_crosses (CALL to SHA256, warm) |
| `satin/inherited_costs.rs` | `test_the_first_staticcall_to_a_precompile_is_charged_warm` | covered | rev_child_return_precompile (STATICCALL to IDENTITY) |
| `satin/inherited_costs.rs` | `test_the_first_call_to_an_access_list_address_is_charged_warm` | not-portable | other: EIP-2930 access list entries not expressible |
| `satin/inherited_costs.rs` | `test_the_first_callcode_to_an_access_list_address_is_charged_warm` | not-portable | other: EIP-2930 access list entries not expressible |
| `satin/inherited_costs.rs` | `test_the_first_call_to_the_beneficiary_is_charged_warm` | covered | q53_beneficiary_call_detains, a17_call_beneficiary_contract_detained |
| `satin/inspector.rs` | `test_step_end_charge_and_refund_land_on_the_receipt` | not-portable | other: a custom inspector that charges and refunds gas in step_end |
| `satin/inspector.rs` | `test_call_end_rewrite_reaches_the_caller` | not-portable | other: a custom inspector that rewrites call results |
| `satin/inspector.rs` | `test_short_circuited_value_call_moves_no_value` | not-portable | other: a custom inspector that answers value calls itself |
| `satin/inspector.rs` | `test_create_end_address_rewrite_reaches_the_caller_not_the_code` | not-portable | other: a custom inspector that rewrites a creation's address |
| `satin/inspector.rs` | `test_a_creation_its_frame_budget_stops_may_be_revived` | not-portable | other: a custom inspector that revives a stopped creation |
| `satin/inspector.rs` | `test_false_read_only_claim_fails_in_debug` | not-portable | internal api (ReadOnlyObserver debug assertion) |
| `satin/inspector.rs` | `test_false_read_only_claim_of_a_call_rewrite_fails_in_debug` | not-portable | internal api (ReadOnlyObserver debug assertion) |
| `satin/inspector.rs` | `test_read_only_tracer_changes_nothing` | not-portable | tracer output (TracingInspector under ReadOnlyObserver; node count) |
| `satin/inspector.rs` | `test_the_gas_inspector_keeps_the_byte_of_an_undefined_opcode` | not-portable | tracer output (test-utility GasInspector records) |
| `satin/inspector.rs` | `test_frame_start_and_end_stay_paired` | covered | the frames field of i7_unfunded_value_call (answered unfunded call), s3_create_res (creation), edge_first_frame_data_budget_before_growth (first frame stopped at its start) |
| `satin/inspector.rs` | `test_read_only_observer_forwards_every_callback` | not-portable | internal api (ReadOnlyObserver forwarding) |
| `satin/inspector.rs` | `test_read_only_observer_forwards_the_transfer_logs_of_frame_starts` | not-portable | tracer output (the inspector's log callback for frame-start transfer logs) |
| `satin/inspector.rs` | `test_false_read_only_claim_of_an_input_edit_fails_in_debug` | not-portable | internal api (ReadOnlyObserver debug assertion) |
| `satin/inspector.rs` | `test_false_read_only_claim_of_a_journal_write_fails_in_debug` | not-portable | internal api (ReadOnlyObserver debug assertion) |
| `satin/inspector.rs` | `test_false_read_only_claim_of_a_frame_end_rewrite_fails_in_debug` | not-portable | internal api (ReadOnlyObserver debug assertion) |
| `satin/inspector.rs` | `test_only_the_inspecting_system_call_entry_point_runs_the_inspector` | not-portable | internal api (system-call entry points) |
| `satin/inspector.rs` | `test_intrinsic_data_size_overflow_with_inspector_early_return` | not-portable | other: a custom inspector that answers every call (the plain refusal is edge_body_over_limit) |
| `satin/inspector.rs` | `test_a_rewriting_inspector_may_change_what_a_start_writes` | not-portable | other: a custom inspector that adds value to every call |
| `satin/inspector_cheatcodes.rs` | `test_deal_sets_a_balance` | not-portable | other: a cheatcode inspector (deal) writing balances through the journal |
| `satin/inspector_cheatcodes.rs` | `test_store_writes_a_slot` | not-portable | other: a cheatcode inspector (store) writing slots through the journal |
| `satin/inspector_cheatcodes.rs` | `test_pause_and_resume_gas_metering_hold_the_gas` | not-portable | other: a cheatcode inspector that pauses gas metering |
| `satin/inspector_cheatcodes.rs` | `test_expect_revert_turns_a_revert_into_a_success_that_keeps_its_reason` | not-portable | other: a cheatcode inspector (expectRevert) rewriting call results |
| `satin/inspector_cheatcodes.rs` | `test_expect_revert_is_honored_on_a_result_no_frame_ran_to_produce` | not-portable | other: a cheatcode inspector (expectRevert) rewriting call results |
| `satin/inspector_cheatcodes.rs` | `test_a_cheatcode_at_foundrys_default_gas_limit_leaves_its_caller_what_it_kept` | not-portable | other: a cheatcode inspector answering calls on Gas::new |
| `satin/inspector_limits.rs` | `test_gas_changed_at_a_frames_end_has_no_effect` | not-portable | other: a custom inspector that charges gas in step_end and call_end |
| `satin/inspector_limits.rs` | `test_an_inspector_cannot_overdraw_a_frame` | not-portable | other: a custom inspector that overdraws a frame's gas |
| `satin/inspector_limits.rs` | `test_a_refused_charge_is_read_as_what_a_later_halt_burns` | not-portable | other: a custom inspector whose charge is refused |
| `satin/inspector_limits.rs` | `test_an_answer_settles_on_the_gas_it_is_built_on` | not-portable | other: a custom inspector that answers frames on chosen gas |
| `satin/inspector_settlement.rs` | `test_a_failed_call_revived_into_a_success_settles_as_a_success` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_failed_value_call_revived_into_a_success_keeps_its_charges` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_the_transactions_own_failed_frame_revived_into_a_success_keeps_its_charges` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_failed_creation_revived_into_a_success_settles_as_a_success` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_value_call_an_inspector_answers_keeps_what_its_opcode_was_charged` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_the_transactions_own_value_transfer_an_inspector_answers_keeps_its_charges` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_creation_an_inspector_answers_keeps_what_its_opcode_was_charged` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_the_transactions_own_creation_an_inspector_answers_keeps_its_charges` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_retargeted_value_calls_upfront_charge_comes_back_at_the_new_targets_price` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_value_call_rewritten_into_a_failure_keeps_its_state_and_bills_its_body` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_creation_rewritten_into_a_failure_keeps_its_state_and_settles_as_a_failure` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_call_that_wrote_rewritten_into_a_failure_keeps_its_writes_and_settles_as_a_failure` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_write_an_inspector_makes_at_a_frames_end_stays` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_creation_whose_creator_an_inspector_rewrites_to_a_changed_account_runs` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_creation_whose_creator_an_inspector_rewrites_to_an_empty_account_counts_its_record` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_every_inspected_entry_point_runs_a_start_an_inspector_rewrote` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_rewritten_start_counts_the_records_revm_made` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_creation_refused_on_its_creators_account_counts_no_record` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_refused_creation_leaves_its_creators_pool_whole` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_refused_creation_holds_an_empty_pool_to_nothing` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_the_transactions_own_halt_burns_its_regular_gas_when_an_inspector_adds_to_its_reservoir` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_a_halting_child_handed_more_reservoir_leaves_its_caller_its_regular_gas` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/inspector_settlement.rs` | `test_the_transactions_own_halt_with_no_start_charge_keeps_what_its_settlement_left` | not-portable | other: a custom inspector that rewrites, answers or retargets frames (settlement of rewritten results) |
| `satin/intrinsic.rs` | `test_the_floor_of_empty_calldata_is_the_intrinsic_gas` | not-portable | other: the EIP-7623 floor of a user transaction is not in the record (it never binds under history gas) |
| `satin/intrinsic.rs` | `test_one_calldata_byte_costs_sixty_four_gas` | not-portable | other: the EIP-7623 floor of a user transaction is not in the record |
| `satin/intrinsic.rs` | `test_the_floor_is_sixty_four_gas_for_every_calldata_byte` | not-portable | other: the EIP-7623 floor of a user transaction is not in the record |
| `satin/intrinsic.rs` | `test_a_zero_calldata_byte_costs_the_same_as_a_non_zero_one` | covered | p6_floor_zero_calldata vs p6_floor_nonzero_calldata (the bill and history; the floor itself is not in the record) |
| `satin/intrinsic.rs` | `test_the_calldata_floor_never_binds_once_history_is_charged` | covered | p6_floor_zero_calldata, p6_floor_nonzero_calldata (gas used = intrinsic + history, above the floor) |
| `satin/intrinsic.rs` | `test_an_access_list_address_pays_its_cold_premium_and_its_bytes` | not-portable | other: EIP-2930 access list entries not expressible |
| `satin/intrinsic.rs` | `test_access_list_bytes_are_counted_in_the_floor_at_sixty_four_each` | not-portable | other: EIP-2930 access list entries not expressible |
| `satin/intrinsic.rs` | `test_the_execution_cap_bounds_the_calldata_a_transaction_may_carry` | ported | port_valid_calldata_cap_3124765, port_valid_calldata_cap_3124766 |
| `satin/intrinsic.rs` | `test_a_floor_above_the_gas_limit_is_rejected` | covered | edge_gas_below_floor (gas limit between the intrinsic gas and the calldata floor: refused) |
| `satin/intrinsic.rs` | `test_a_creation_that_cannot_pay_its_account_runs_out_of_gas` | covered | edge_sweep_tx0_funded_create_v0_runtime0, edge_start_record_unpaid_create |
| `satin/intrinsic.rs` | `test_a_gas_limit_below_the_intrinsic_charge_is_rejected` | covered | edge_gas_below_intrinsic, edge_sweep_tx0_*_intrinsic |
| `satin/intrinsic.rs` | `test_a_valid_call_still_passes` | covered | p6_stop, edge_sweep_tx0_funded_existing_v0_runs |
| `satin/intrinsic.rs` | `test_a_gas_limit_short_of_the_body_is_rejected` | covered | edge_gas_below_history, edge_start_record_below_validation_minimum, edge_start_record_at_validation_minimum |
| `satin/intrinsic.rs` | `test_the_bodys_history_is_reported_once_validation_admits_it` | covered | edge_gas_below_intrinsic, edge_gas_below_history (refused: no history), blk_nonce_gap_t2 (a refusal after an admitted transaction) |
| `satin/kv_updates.rs` | `test_the_kv_limit_is_the_specs` | not-portable | internal api (asserts the spec's TX_KV_UPDATE_LIMIT constant) |
| `satin/kv_updates.rs` | `test_the_eleventh_record_stops_the_transactions_own_frame` | ported | port_kv_eleventh_record_10, port_kv_eleventh_record_11 |
| `satin/kv_updates.rs` | `test_a_child_gets_98_percent_of_what_its_caller_has_left` | ported | port_kv_child_98pct_fit, port_kv_child_98pct_stopped |
| `satin/kv_updates.rs` | `test_the_kv_budgets_at_depth` | ported | port_kv_depth3_95 |
| `satin/kv_updates.rs` | `test_a_record_crossing_data_size_and_kv_stops_on_data_size` | ported | port_kv_cross_data_and_kv_both, port_kv_cross_data_and_kv_kv_only |
| `satin/kv_updates.rs` | `test_a_negative_net_does_not_enlarge_the_next_childs_budget` | ported | port_kv_negative_net_budget |
| `satin/kv_updates.rs` | `test_an_authorization_list_over_the_kv_limit_is_skipped_and_stops_the_first_frame` | covered | e10_list_skipped_kv, edge_sweep_auth_*_listkv |
| `satin/limit_exemption.rs` | `test_a_system_transaction_is_held_to_no_limit` | ported | port_exempt_system_data, port_exempt_system_kv, port_exempt_system_growth, port_exempt_system_all |
| `satin/limit_exemption.rs` | `test_a_system_call_is_held_to_no_limit` | not-portable | internal api (system call entry point) |
| `satin/limit_exemption.rs` | `test_a_deposit_and_a_user_call_to_a_system_contract_are_held_to_the_limits` | ported | port_exempt_deposit_data, port_exempt_deposit_kv, … (6 scenarios, `port_exempt_*`) |
| `satin/limit_exemption.rs` | `test_a_system_transaction_keeps_more_state_growth_than_the_limit` | ported | port_exempt_system_growth_1001 |
| `satin/limit_matrix/block.rs` | `test_an_ordinary_transaction_is_skipped_at_the_block_state_growth_limit` | not-portable | block executor (block state-growth counter and skip) |
| `satin/limit_matrix/block.rs` | `test_a_reduced_block_kv_limit_skips_the_next_ordinary_transaction` | not-portable | block executor (block KV limit and skip) |
| `satin/limit_matrix/block.rs` | `test_a_skip_with_every_block_dimension_reached_records_no_dimension` | not-portable | block executor (block limits and skip) |
| `satin/limit_matrix/block.rs` | `test_a_system_transaction_of_1001_slots_is_not_stopped` | not-portable | block executor (the block counter); the transaction-level part is the limit_exemption gap (1,001 slots in a Mega System Transaction) |
| `satin/limit_matrix/block.rs` | `test_a_pre_block_call_of_1001_slots_is_not_stopped_and_does_not_move_the_block` | not-portable | block executor (pre-block call) |
| `satin/limit_matrix/block.rs` | `test_a_system_transaction_after_1000_slots_is_not_skipped` | not-portable | block executor (block counter and skip) |
| `satin/limit_matrix/block.rs` | `test_a_builders_kv_and_state_growth_policy_does_not_bind_a_validator` | not-portable | block executor (builder packing policy) |
| `satin/limit_matrix/deposits.rs` | `test_a_new_depositors_1000th_slot_stops_at_999` | ported | port_depo_slots_new_1000, port_depo_slots_new_999 |
| `satin/limit_matrix/deposits.rs` | `test_an_existing_depositors_1001st_slot_stops_at_1000` | ported | port_depo_slots_existing_1001, port_depo_slots_existing_1000 |
| `satin/limit_matrix/deposits.rs` | `test_a_user_deposit_after_1000_slots_is_not_skipped` | not-portable | block executor (block state-growth counter and skip) |
| `satin/limit_matrix/deposits.rs` | `test_a_failed_deposit_from_a_new_sender_is_not_stopped` | not-portable | block executor (block counter) and a system deposit |
| `satin/limit_matrix/destruction.rs` | `test_a_kept_destruction_gives_back_the_leaf_the_code_and_the_slot` | ported | port_destr_kept_default, port_destr_kept_growth0 |
| `satin/limit_matrix/destruction.rs` | `test_a_reverted_inner_frame_discards_the_destruction` | ported | port_destr_reverted_inner |
| `satin/limit_matrix/destruction.rs` | `test_a_new_beneficiary_at_a_full_budget_stops_the_frame` | ported | port_destr_beneficiary_full_budget_979, port_destr_beneficiary_full_budget_980 |
| `satin/limit_matrix/destruction.rs` | `test_stores_after_the_mark_and_a_reverted_enclosing_frame` | ported | port_destr_stores_after_mark_kept, port_destr_stores_after_mark_reverted |
| `satin/limit_matrix/destruction.rs` | `test_a_destruction_gives_back_record_history_and_keeps_the_logs` | ported | port_destr_keeps_logs |
| `satin/limit_matrix/destruction.rs` | `test_a_destroying_childs_negative_net_does_not_raise_the_parents_budget` | ported | port_destr_negative_net_parent |
| `satin/limit_matrix/destruction.rs` | `test_a_halt_after_a_destroying_child_discards_the_mark` | ported | port_destr_halt_after_destroying_child |
| `satin/limit_matrix/destruction.rs` | `test_a_second_destruction_with_no_balance_subtracts_nothing` | ported | port_destr_second_no_balance |
| `satin/limit_matrix/destruction.rs` | `test_a_second_destruction_after_refunding_charges_only_the_beneficiary` | covered | fz mode 9 (destroyed-when-called contract, then a 1-wei call to it, heir EMPTY: the second destruction adds only the beneficiary) |
| `satin/limit_matrix/destruction.rs` | `test_value_then_create2_then_destruct_gives_the_value_calls_leaf_back` | covered | edge_transfer_then_create_destroyed; fz mode 9 (value to the CREATE2 address, then CREATE2 there, constructor destroys to an existing heir) |
| `satin/limit_matrix/destruction.rs` | `test_a_beneficiary_charge_the_frame_cannot_pay_halts_before_the_give_back` | ported | port_destr_beneficiary_unpayable_150000, port_destr_beneficiary_unpayable_200000, … (5 scenarios, `port_destr_beneficiary_unpayable_*`) |
| `satin/limit_matrix/destruction.rs` | `test_a_cleared_slot_after_the_mark_does_not_refill_again` | ported | port_destr_store_after_mark_cleared |
| `satin/limit_matrix/destruction.rs` | `test_a_cleared_account_with_no_kv_room_stops_at_the_frame_budget` | ported | port_destr_cleared_account_kv_3, port_destr_cleared_account_kv_4 |
| `satin/limit_matrix/destruction.rs` | `test_only_the_kept_destruction_gives_the_charges_back` | ported | port_destr_only_kept_gives_back |
| `satin/limit_matrix/destruction.rs` | `test_a_new_account_destroyed_before_code_or_slots_refills_the_leaf` | covered | edge_create_tx_destroyed_in_constructor; fz mode 9 (constructor destroys with no write) |
| `satin/limit_matrix/destruction.rs` | `test_a_prefunded_destruction_that_moves_no_balance_keeps_the_nonce_record` | covered | fz mode 9 (pre-funded creation address, constructor ADDRESS SELFDESTRUCT: no move) |
| `satin/limit_matrix/destruction.rs` | `test_destroyed_code_refills_its_deposit_and_its_history` | covered | p6_destroy_after_deploy_m1/m2/m8, q19_det_deploy_then_destroy_salt8 |
| `satin/limit_matrix/destruction.rs` | `test_slots_deleted_with_the_account_refill_once` | ported | port_destr_slots_refill_once |
| `satin/limit_matrix/destruction.rs` | `test_a_creation_onto_an_empty_address_with_storage_keeps_the_leaf_charge` | covered | edge_existing_empty_account_then_create_destroyed (address with only a slot, created onto and destroyed; the runtime SLOAD-reads-zero row is not a scenario) |
| `satin/limit_matrix/destruction.rs` | `test_a_leaf_an_earlier_transaction_destroyed_is_absent_at_the_next_start` | ported | port_destr_block_absent_balance_t1, port_destr_block_absent_balance_t2, port_destr_block_absent_empty_t1, port_destr_block_absent_empty_t2 |
| `satin/limit_matrix/destruction.rs` | `test_a_leaf_an_earlier_transaction_funded_is_present_at_the_next_start` | ported | port_destr_block_funded_t1, port_destr_block_funded_t2 |
| `satin/limit_matrix/destruction.rs` | `test_a_leaf_funded_after_its_destruction_is_present_at_the_next_start` | ported | port_destr_block_funded_after_t1, port_destr_block_funded_after_t2, port_destr_block_funded_after_t3 |
| `satin/limit_matrix/destruction.rs` | `test_a_fresh_slot_written_after_the_mark_is_neither_charged_nor_counted` | ported | port_destr_store_after_mark_fresh |
| `satin/limit_matrix/destruction.rs` | `test_a_detention_crossing_at_the_destruction_applies_none_of_the_table` | ported | port_destr_detention_crossing_1000, port_destr_detention_crossing_3000, … (6 scenarios, `port_destr_detention_crossing_*`) |
| `satin/limit_matrix/destruction.rs` | `test_a_store_after_the_mark_is_not_charged_what_its_frame_cannot_pay` | ported | port_destr_store_unpayable_after_mark, port_destr_store_unpayable_before_destruction |
| `satin/limit_matrix/destruction.rs` | `test_a_deposit_after_the_mark_is_not_charged_what_its_frame_cannot_pay` | ported | port_destr_deposit_after_mark_destroying, port_destr_deposit_after_mark_plain |
| `satin/limit_matrix/destruction.rs` | `test_code_deposited_after_its_constructor_destroyed_the_account_keeps_no_state_gas` | ported | port_destr_code_after_constructor_destroyed |
| `satin/limit_matrix/destruction.rs` | `test_a_creation_at_its_growth_limit_that_destroys_itself_to_a_new_account_succeeds` | ported | port_destr_create_tx_selfdestruct_1000, port_destr_create_tx_selfdestruct_1001 |
| `satin/limit_matrix/destruction.rs` | `test_a_give_back_onto_an_empty_reservoir_credits_the_spill_first` | ported | port_destr_give_back_credits_spill |
| `satin/limit_matrix/destruction.rs` | `test_a_creation_that_destroys_itself_refunds_its_record_once` | covered | edge_create_tx_destroyed_in_constructor, edge_create_tx_destroyed_in_constructor_part_reservoir (record given back once; the failing-first-frame row is not a scenario) |
| `satin/limit_matrix/forwarding.rs` | `test_a_child_receives_the_reservoir_share` | covered | ctl_call_chain_res, p6_share_reservoir_101440, p6_share_reservoir_103510 (the frames field records each frame's reservoir share) |
| `satin/limit_matrix/forwarding.rs` | `test_a_reverted_child_returns_its_reservoir_share` | covered | s3_value_call_reverting_child_res (above the cap, the child writes a slot and reverts) |
| `satin/limit_matrix/forwarding.rs` | `test_a_reverted_childs_share_unwinds_the_callers_spill_first` | ported | port_fwd_reverted_child_unwinds_spill |
| `satin/limit_matrix/forwarding.rs` | `test_a_one_gas_call_still_forwards_the_reservoir` | ported | port_fwd_one_gas_call_value0, port_fwd_one_gas_call_value1 |
| `satin/limit_matrix/forwarding.rs` | `test_a_call_with_1000_gas_left_forwards_980` | ported | port_fwd_left_1000_0, port_fwd_left_1000_1, … (10 scenarios, `port_fwd_left_1000_*`) |
| `satin/limit_matrix/forwarding.rs` | `test_a_call_with_99_gas_left_forwards_98` | ported | port_fwd_left_99_0, port_fwd_left_99_1, … (10 scenarios, `port_fwd_left_99_*`) |
| `satin/limit_matrix/forwarding.rs` | `test_a_call_with_50_gas_left_forwards_49` | ported | port_fwd_left_50_0, port_fwd_left_50_1, … (10 scenarios, `port_fwd_left_50_*`) |
| `satin/limit_matrix/forwarding.rs` | `test_a_value_call_with_10000_left_forwards_12100` | ported | port_fwd_value_left_0, port_fwd_value_left_1, … (12 scenarios, `port_fwd_value_left_*`) |
| `satin/limit_matrix/forwarding.rs` | `test_a_value_call_grants_a_history_allowance_outside_gas` | covered | q31_nested_value_calls_allowance, q32 (value calls below the top frame above the cap; frames show gas = cap + stipend and the reservoir share) |
| `satin/limit_matrix/forwarding.rs` | `test_a_reservoir_of_1_and_a_frame_budget_of_1` | ported | port_fwd_reservoir_one_budget_one |
| `satin/limit_matrix/forwarding.rs` | `test_a_detained_frame_forwards_on_spendable_plus_withheld` | covered | d4_forward_draws_withheld, p7_forward_from_total_read |
| `satin/limit_matrix/forwarding.rs` | `test_neutral_forwards_eip150_and_satin_keeps_its_cap_without_limits` | not-portable | other: the neutral Amsterdam configuration (EIP-150 forwards) |
| `satin/limit_matrix/forwarding.rs` | `test_a_returned_reservoir_unwinds_the_callers_spill` | ported | port_fwd_returned_reservoir_unwinds_spill |
| `satin/limit_matrix/keyless.rs` | `test_an_authorization_list_of_1001_new_authorities_stops_before_the_first_frame` | covered | e11_list_skipped_growth, edge_sweep_auth_*_listgrowth (a list over the growth limit is skipped and the first frame stops; at small limits, not at 1,001 authorities) |
| `satin/limit_matrix/keyless.rs` | `test_a_fitting_list_keeps_the_authorities_when_the_recipient_stops` | ported | port_depo_list_fits_recipient_stops |
| `satin/limit_matrix/keyless.rs` | `test_a_keyless_creation_receives_the_reservoir_share_and_a_980_budget` | ported | port_klim_980_n0_above |
| `satin/limit_matrix/keyless.rs` | `test_authorization_list_boundaries` | covered | e02-e11b (existing authority, two entries, wrong nonce, other chain, invalid signature, code skipped, clear then set, sender authority, list rules); the nonce 2^64-1 and the db-fault rows are not covered |
| `satin/limit_matrix/keyless.rs` | `test_a_keyless_creation_from_nonce_0_stopped_in_its_initcode` | ported | port_klim_980_n0_above, port_klim_980_n0_below |
| `satin/limit_matrix/keyless.rs` | `test_a_keyless_creation_from_nonce_1_takes_the_bump_back` | ported | port_klim_980_n1_above, port_klim_980_n1_below |
| `satin/limit_matrix/keyless.rs` | `test_a_keyless_constructor_that_self_destructs_answers_empty_code` | ported | port_klim_constructor_selfdestructs |
| `satin/limit_matrix/keyless.rs` | `test_an_authority_whose_state_gas_cannot_be_paid_is_taken_back` | covered | e17_authority_charges_out_of_gas |
| `satin/limit_matrix/keyless.rs` | `test_a_list_that_does_not_fit_is_not_the_out_of_gas_path` | ported | port_depo_list_not_fitting |
| `satin/limit_matrix/keyless.rs` | `test_an_authority_with_non_delegation_code_is_skipped` | covered | e07_authority_with_code_skipped |
| `satin/limit_matrix/keyless.rs` | `test_two_entries_for_one_new_authority_accept_the_first` | ported | port_depo_list_same_authority_twice |
| `satin/limit_matrix/limits.rs` | `test_the_1001st_fresh_slot_stops_the_transaction` | ported | port_growth_fresh_slots_1000, port_growth_fresh_slots_1000_big_gas, port_growth_fresh_slots_1001, port_growth_fresh_slots_1001_big_gas |
| `satin/limit_matrix/limits.rs` | `test_a_child_budget_of_960_then_a_later_child_of_19` | ported | port_budget_child_960_19 |
| `satin/limit_matrix/limits.rs` | `test_a_childs_961st_slot_stops_at_960` | ported | port_budget_child_961_961 |
| `satin/limit_matrix/limits.rs` | `test_a_child_kv_budget_of_7_stops_the_eighth_record` | ported | port_kv_child_98pct_fit, port_kv_child_98pct_stopped |
| `satin/limit_matrix/limits.rs` | `test_kv_weighs_more_than_the_data_size_cap` | ported | port_budget_kv_vs_data_below, port_budget_kv_vs_data_at, port_budget_kv_vs_data_over |
| `satin/limit_matrix/limits.rs` | `test_the_transactions_own_frame_stops_at_a_reduced_kv_limit` | ported | port_kv_eleventh_record_10, port_kv_eleventh_record_11 |
| `satin/limit_matrix/limits.rs` | `test_a_record_that_crosses_data_size_and_state_growth_names_data_size` | ported | port_budget_data_and_growth_child, port_budget_data_and_growth_top |
| `satin/limit_matrix/limits.rs` | `test_state_growth_at_depth_2_and_3_stops_at_940` | ported | port_budget_growth_depth3_940, port_budget_growth_depth2_940, port_budget_growth_depth3_941, port_budget_growth_depth2_941 |
| `satin/limit_matrix/limits.rs` | `test_a_cleared_net_does_not_raise_the_childs_remaining` | ported | port_budget_cleared_net_kept, port_budget_cleared_net_reverted |
| `satin/limit_matrix/limits.rs` | `test_hints_then_a_child_log_names_the_transaction_data_limit` | ported | port_budget_hints_then_child_log |
| `satin/limit_matrix/limits.rs` | `test_hints_then_a_top_frame_log_stops_the_transaction` | ported | port_budget_hints_then_top_log |
| `satin/limit_matrix/limits.rs` | `test_a_successful_childs_negative_net_leaves_the_transaction_at_410` | ported | port_budget_negative_net_data_kept, port_budget_negative_net_data_reverted |
| `satin/limit_matrix/limits.rs` | `test_a_negative_data_size_net_does_not_raise_what_a_frame_forwards` | ported | port_budget_negative_net_forward |
| `satin/limit_matrix/limits.rs` | `test_a_prefunded_creation_keeps_a_zero_net_until_the_1001st_slot` | ported | port_budget_prefunded_creation_1000, port_budget_prefunded_creation_1001 |
| `satin/limit_matrix/records.rs` | `test_a_slot_set_and_cleared_refills_its_state_gas` | covered | p6_salt_sstore_restore_m1 (0 -> 1 -> 0 in one frame) |
| `satin/limit_matrix/records.rs` | `test_create2_onto_a_prefunded_address_is_not_a_rise` | ported | port_rec_create2_prefunded_plain, port_rec_create2_prefunded_kv1 |
| `satin/limit_matrix/records.rs` | `test_create2_onto_an_address_funded_in_this_transaction_charges_one_leaf` | covered | fz mode 9 (value call to the CREATE2 address, then CREATE2 there, contract left standing when not called) |
| `satin/limit_matrix/records.rs` | `test_create2_onto_code_keeps_the_creator_nonce_and_not_the_target` | covered | bd_det_read_then_create2_collision, fz mode 6 (CREATE2 onto code, with and without an endowment) |
| `satin/limit_matrix/records.rs` | `test_an_unfunded_value_call_and_a_stuck_nonce_count_nothing` | ported | port_rec_stuck_nonce_create |
| `satin/limit_matrix/records.rs` | `test_a_second_nonzero_write_neither_rises_nor_falls` | ported | port_rec_nonzero_after_nonzero_fresh, port_rec_nonzero_original |
| `satin/limit_matrix/records.rs` | `test_slot_sequences_in_one_frame_rise_once` | ported | port_rec_slot_sequences_growth3, port_rec_slot_sequences_growth2 |
| `satin/limit_matrix/stop.rs` | `test_a_record_that_crosses_kv_names_the_frame_budget` | ported | port_stop_kv_with_authorization |
| `satin/limit_matrix/stop.rs` | `test_a_charge_that_crosses_detention_stops_before_the_write_is_counted` | ported | port_stop_detention_before_count |
| `satin/limit_matrix/stop.rs` | `test_kv_at_depth_2_and_3_stops_at_94` | ported | port_stop_kv_depth3 |
| `satin/limit_matrix/stop.rs` | `test_a_caller_that_catches_a_stop_is_stopped_by_its_own_next_record` | covered | s1_caller_crosses_after_resume (kind 3); the kind-1 variant under kv 100 is not a scenario |
| `satin/limit_matrix/stop.rs` | `test_a_creator_nonce_record_stops_the_creator_at_its_kv_budget` | ported | port_stop_creator_nonce_kv_nine, port_stop_creator_nonce_kv_eight |
| `satin/limit_matrix/stop.rs` | `test_a_precompile_answered_from_its_price_is_held_to_the_price_alone` | ported | port_stop_precompile_price_cap, port_stop_precompile_price_kv2 |
| `satin/limit_matrix/stop.rs` | `test_an_unpriced_precompile_out_of_its_allowance_is_held_to_the_allowance_alone` | not-portable | other: a node-installed precompile (with_dyn_precompiles) |
| `satin/limit_stop.rs` | `test_a_contract_reverting_with_a_stops_bytes_is_no_stop` | ported | port_stop_spoofed_revert_kind0, port_stop_spoofed_revert_kind1, port_stop_spoofed_revert_kind2, port_stop_spoofed_revert_kind3 |
| `satin/limit_stop.rs` | `test_a_frame_budget_re_raised_by_its_caller_is_no_stop` | not-portable | other: per-frame data-size cap (frame_data_size_limit); a tx data limit giving the child a budget would reach the same shape |
| `satin/limit_stop.rs` | `test_the_transactions_own_frame_stopped_by_its_budget_is_a_stop` | not-portable | other: per-frame data-size cap (frame_data_size_limit) |
| `satin/limit_stop.rs` | `test_a_creation_whose_deposit_crosses_its_budget_is_a_stop` | not-portable | other: per-frame data-size cap (frame_data_size_limit) |
| `satin/limit_stop.rs` | `test_a_stop_an_inspector_turns_into_a_success_is_no_stop` | not-portable | other: a custom inspector that turns a stop into a success |
| `satin/neutral.rs` | `test_neutral_transfer_matches_op_revm` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_sstore_matches_op_revm` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_log_matches_op_revm` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_creation_matches_op_revm` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_creation_from_a_nonce_at_its_maximum_matches_op_revm` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_creation_that_destroys_itself_matches_op_revm` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_nested_value_call_matches_op_revm` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_programs_match_ethereum` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_reads_of_volatile_data_detain_nothing` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_extcodesize_matches_ethereum` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_kzg_call_matches_ethereum` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/neutral.rs` | `test_neutral_creation_onto_an_address_an_earlier_transaction_loaded_counts_its_account` | not-portable | other: the neutral Osaka/Amsterdam configuration (execution-spec gate), compared with op-revm or revm mainnet |
| `satin/outcome.rs` | `test_outcome_reports_the_ledgers` | covered | ctl_sstore, ctl_sstore_res (gas_used, regular, state, history, reservoir_remaining in the record) |
| `satin/outcome.rs` | `test_outcome_reports_the_stop` | covered | s1_top_stopped (limit_exceeded in the record) |
| `satin/outcome.rs` | `test_spoofed_revert_data_is_not_a_stop` | ported | port_outcome_spoofed_revert_kind0, port_outcome_spoofed_revert_kind1, port_outcome_spoofed_revert_kind2, port_outcome_spoofed_revert_kind3 |
| `satin/outcome.rs` | `test_convenience_execution_methods_work` | not-portable | internal api (execute_transaction with and without an enabled inspector) |
| `satin/pool_validation.rs` | `test_the_helper_and_the_evm_agree_on_what_they_admit` | not-portable | internal api (validate_transaction_stateless pool helper); the EVM side is the edge_sweep_* cells |
| `satin/pool_validation.rs` | `test_what_the_intrinsic_gas_is_made_of` | not-portable | internal api (the pool helper's IntrinsicGas figures) |
| `satin/pool_validation.rs` | `test_the_helper_and_the_evm_agree_on_what_they_refuse` | not-portable | internal api (pool helper); some refusals (priority fee above the fee cap, an empty authorization list) are not expressible either |
| `satin/pool_validation.rs` | `test_the_helper_and_the_evm_refuse_another_shape_from_the_system_address` | not-portable | internal api (pool helper); the EVM side is edge_system_* and fz mode 5 |
| `satin/pool_validation.rs` | `test_the_chain_id_is_checked_whatever_the_configuration_says` | not-portable | internal api (a configuration with the chain-id check off, and the pool helper) |
| `satin/pool_validation.rs` | `test_the_helper_and_the_evm_refuse_a_body_over_the_limit_alike` | not-portable | internal api (pool helper); the EVM side is e16_body_records_over_data_limit, edge_body_over_limit |
| `satin/pool_validation.rs` | `test_validation_bounds_the_body_below_the_data_size_limit` | ported | port_valid_body_data_limit_3124812, port_valid_body_data_limit_3124813, port_valid_auth_list_25587, port_valid_auth_list_25588 |
| `satin/pool_validation.rs` | `test_the_execution_cap_is_held_alike` | not-portable | internal api (pool helper); the EVM side is the intrinsic.rs cap gap |
| `satin/pool_validation.rs` | `test_the_least_gas_limit_leaves_out_what_the_start_adds` | covered | edge_start_record_at_validation_minimum, edge_sweep_tx0_*_start, edge_sweep_tx0_funded_absent_v5_runtime0 |
| `satin/precompile_gas.rs` | `test_a_valid_kzg_evaluation_costs_a_flat_price` | covered | fz mode 7 from seed 3,001: KZG point evaluation with a valid proof (commitment at infinity) at gas limits from its 100,000 price to ample (depth 0; the c-kzg vector itself is not used) |
| `satin/precompile_gas.rs` | `test_the_flat_price_is_one_hundred_thousand` | covered | fz mode 7: KZG with a valid proof and with a zero input (fails, burns what it was given) |
| `satin/precompile_gas.rs` | `test_every_verification_failure_burns_the_forwarded_gas` | ported | port_precompile_kzg_fails_proof, port_precompile_kzg_fails_hash, port_precompile_kzg_fails_short191 |
| `satin/precompile_gas.rs` | `test_the_precompile_addresses_are_warm` | covered | q09_det_precompile_price_fits, d9_precompile_price_crosses (CALLs to precompiles charged warm) |
| `satin/precompile_gas.rs` | `test_forwarding_less_than_the_price_is_out_of_gas` | covered | fz mode 7: KZG with the gas limit drawn one below and at needs + 100,000 |
| `satin/pricing_table.rs` | `test_the_pricing_table_is_up_to_date` | not-portable | snapshot (rendered pricing-table.md) |
| `satin/pricing_table.rs` | `test_the_table_shows_where_satin_differs_from_amsterdam` | not-portable | snapshot (reads the rendered schedule table) |
| `satin/pricing_table.rs` | `test_the_table_shows_the_multiplier_on_the_state_ledger_alone` | not-portable | snapshot (reads the rendered SALT scaling rows; p6_salt_* compare the same ledgers per scenario) |
| `satin/reservoir_drain.rs` | `test_a_log_heavy_callee_charges_past_its_forward_and_leaves_two_percent_of_each_pool` | ported | port_drain_log_data_10000_400m, port_drain_log_data_10000_200m, … (24 scenarios, `port_drain_*`) |
| `satin/reservoir_drain.rs` | `test_the_frame_budget_binds_a_callee_once_the_reservoir_outgrows_it` | ported | port_drain_budget_binds_400000000, port_drain_budget_binds_800000000, … (6 scenarios, `port_drain_budget_binds_*`) |
| `satin/reservoir_drain.rs` | `test_without_a_reservoir_the_forward_bounds_every_charge` | ported | port_drain_log4_greedy_10000_400m, port_drain_log4_greedy_10000_200m, … (16 scenarios, `port_drain_log*`) |
| `satin/reservoir_drain.rs` | `test_the_reservoir_a_logging_callee_consumes_grows_with_its_forward` | ported | port_drain_log_data_10000_400m, port_drain_log_data_10000_200m, … (8 scenarios, `port_drain_log_data_*`) |
| `satin/reservoir_drain.rs` | `test_a_writing_callees_state_growth_budget_stops_it_before_its_share_runs_out` | ported | port_drain_growth_budget_10000, port_drain_growth_budget_100000, … (12 scenarios, `port_drain_*`) |
| `satin/reservoir_drain.rs` | `test_a_deploying_callee_charges_the_reservoir_hundreds_of_times_its_forward` | ported | port_drain_deploy_96k_100000, port_drain_deploy_96k_1000000, port_drain_deploy_512k_1000000 |
| `satin/reservoir_settlement.rs` | `test_a_callers_reservoir_after_a_child_is_what_it_kept_with_what_the_child_hands_back` | ported | port_drain_settlement_children |
| `satin/reservoir_settlement.rs` | `test_the_transactions_gas_holds_what_its_frame_hands_back_and_nothing_more` | covered | c1_value_tx_ok_res, c2_value_tx_revert_res, c3_value_tx_halt_res, s3_value_call_ok_child_res (the frame hands back what it left, or its whole share) |
| `satin/reservoir_share.rs` | `test_a_child_takes_the_reservoir_less_its_callers_two_percent_at_every_depth` | covered | ctl_call_chain_res (A->B->C above the cap; frames record each reservoir) |
| `satin/reservoir_share.rs` | `test_the_regular_gas_a_call_asks_for_does_not_bound_the_reservoir_it_forwards` | ported | port_drain_regular_ask_value0, port_drain_regular_ask_value1 |
| `satin/reservoir_share.rs` | `test_a_system_calls_outermost_frame_takes_the_whole_reservoir_and_its_child_the_share` | not-portable | internal api (system call entry point) |
| `satin/reservoir_share.rs` | `test_the_shares_at_the_specs_numbers` | covered | ctl_call_chain_res (gas 210M: 10M margin; frames 9,972,720 / 9,773,266 / 9,577,801) |
| `satin/reservoir_share.rs` | `test_the_neutral_configuration_forwards_the_whole_reservoir_and_no_limits_keeps_the_share` | not-portable | other: the neutral configuration (Amsterdam forwards the whole reservoir) |
| `satin/reuse_after_error.rs` | `test_an_error_post_execution_preempts_does_not_fail_the_next_transaction` | not-portable | db fault (a failing account read, then reuse of the EVM) |
| `satin/reuse_after_error.rs` | `test_a_failed_transact_one_keeps_the_transaction_before_it_and_none_of_its_own_writes` | not-portable | db fault (a failing read under transact_one) |
| `satin/reuse_after_error.rs` | `test_a_failed_deposit_through_transact_one_keeps_its_nonce_bump_and_mint` | not-portable | internal api (transact_one journal left to the caller); the failed deposit itself is edge_deposit_body_over_limit |
| `satin/reuse_after_error.rs` | `test_an_error_in_the_failed_deposit_recovery_leaves_the_evm_clean` | not-portable | db fault (in the failed-deposit recovery) |
| `satin/reuse_after_error.rs` | `test_a_transact_one_retry_after_a_failed_deposit_recovery_error_runs_as_on_a_fresh_evm` | not-portable | db fault (retry after a recovery error) |
| `satin/reuse_after_error.rs` | `test_every_entry_point_leaves_nothing_of_a_failed_call` | not-portable | db fault (every public entry point with an injected error) |
| `satin/salt.rs` | `test_the_sstore_set_charge_scales_with_the_slot_s_bucket` | covered | p6_salt_sstore_set_m1, p6_salt_sstore_set_m2, p6_salt_sstore_set_m8 |
| `satin/salt.rs` | `test_the_new_account_charge_of_a_call_scales_with_the_account_s_bucket` | covered | q04_det_value_new_account_and_stipend_log (m=2), q21_deposit_writes_salt2; fz modes 1-2 (value calls to EMPTY with SALT m 1/2/8 on its leaf) |
| `satin/salt.rs` | `test_the_selfdestruct_beneficiary_charge_scales_with_the_beneficiary_s_bucket` | covered | q18_det_selfdestruct_to_new_beneficiary (m=2), q17; fz modes 1-2 (SELFDESTRUCT to EMPTY with SALT on it) |
| `satin/salt.rs` | `test_the_create_charge_scales_with_the_created_address_s_bucket` | covered | q01_det_create_spill_salt2, q03_det_create_ctor_write_salt8, p6_destroy_in_constructor_m1/m2/m8 |
| `satin/salt.rs` | `test_the_top_level_recipient_charge_scales_with_the_recipient_s_bucket` | covered | p6_salt_first_value_call_m1, m2, m8 |
| `satin/salt.rs` | `test_the_create_transaction_target_charge_scales_with_its_bucket` | covered | p6_salt_code_deposit_m1, m2, m8, q38_create_tx_detained_salt2 |
| `satin/salt.rs` | `test_the_eip7702_authority_charges_scale_with_the_authority_s_bucket` | covered | e12_authority_salt_m4 |
| `satin/salt.rs` | `test_the_code_deposit_charge_scales_with_the_deployed_address_s_bucket` | covered | p6_salt_code_deposit_m1, m2, m8 |
| `satin/salt.rs` | `test_a_crowded_bucket_elsewhere_does_not_change_the_price` | covered | fz modes 1-4 (SALT multipliers on accounts and slots a run may not touch, e.g. A.create(1)) |
| `satin/salt.rs` | `test_a_bucket_is_read_once_per_transaction` | not-portable | internal api (the SALT environment's query count; the record has the set of buckets read, not the count) |
| `satin/salt.rs` | `test_an_absurd_capacity_saturates_rather_than_wrapping` | covered | fz mode 8 (bucket capacities up to u64::MAX on touched buckets) |
| `satin/salt.rs` | `test_a_system_call_prices_at_the_minimum_bucket` | not-portable | internal api (system call entry point) |
| `satin/salt.rs` | `test_a_transaction_from_the_system_address_prices_at_the_minimum_bucket` | ported | port_salt_protocol_caller_min_bucket |
| `satin/salt.rs` | `test_the_promoted_system_transaction_prices_at_the_minimum_bucket` | ported | port_salt_promoted_system_min_bucket |
| `satin/salt.rs` | `test_the_system_call_entry_point_prices_the_same_at_any_capacity` | not-portable | internal api (system call entry point) |
| `satin/salt.rs` | `test_a_user_transaction_in_the_same_block_still_pays_the_crowded_price` | covered | p6_salt_sstore_set_m8 (a user write into a crowded bucket) |
| `satin/salt.rs` | `test_a_deposit_transaction_pays_the_crowded_price` | covered | q21_deposit_writes_salt2, q20_deposit_new_caller_creates_salt |
| `satin/salt.rs` | `test_an_unpriceable_deposit_transaction_fails` | not-portable | db fault (a failing SALT bucket lookup) |
| `satin/salt.rs` | `test_the_system_exemption_does_not_leak_into_the_next_transaction` | not-portable | internal api (a system call followed by a user transaction on the same EVM) |
| `satin/salt.rs` | `test_the_minimum_bucket_matches_the_engine_without_a_salt_environment` | covered | ctl_sstore and every scenario without SALT entries (all buckets at the minimum) |
| `satin/salt.rs` | `test_writing_over_a_slot_charges_no_state_gas` | ported | port_salt_slot_over |
| `satin/salt.rs` | `test_clearing_a_slot_charges_no_state_gas` | ported | port_salt_slot_clear |
| `satin/salt.rs` | `test_a_slot_written_twice_pays_for_one_leaf` | ported | port_salt_slot_twice |
| `satin/salt.rs` | `test_a_transfer_to_an_account_that_exists_charges_no_state_gas` | covered | edge_start_record_paid (top-level value to an existing account), s3_value_call_ok_child_res (from a frame) |
| `satin/salt.rs` | `test_a_selfdestruct_that_adds_no_account_charges_no_state_gas` | covered | edge_value_returned_by_selfdestruct (balance to an existing account), p6_destroy_after_deploy_m1 (no balance) |
| `satin/salt.rs` | `test_the_create2_charge_scales_with_the_created_address_s_bucket` | covered | q30_create2_deposit_salt8_res |
| `satin/salt.rs` | `test_a_creation_that_writes_a_slot_pays_both_in_their_own_buckets` | covered | q03_det_create_ctor_write_salt8 (account m=8, slot m=2), q20_deposit_new_caller_creates_salt |
| `satin/salt.rs` | `test_the_multiplier_is_linear_over_a_wide_range` | covered | fz mode 8 (capacities 2,048 .. 2^40 entries with gas limits above the cap) |
| `satin/salt_delegation.rs` | `test_a_call_to_a_self_delegating_account_settles` | ported | port_saltdel_call_self |
| `satin/salt_delegation.rs` | `test_a_call_into_a_two_address_delegation_cycle_settles` | ported | port_saltdel_call_cycle |
| `satin/salt_delegation.rs` | `test_a_staticcall_to_a_self_delegating_account_settles` | ported | port_saltdel_staticcall_self |
| `satin/salt_delegation.rs` | `test_a_delegatecall_into_a_two_address_delegation_cycle_settles` | ported | port_saltdel_delegatecall_cycle |
| `satin/salt_delegation.rs` | `test_a_callcode_to_a_self_delegating_account_settles` | ported | port_saltdel_callcode_self |
| `satin/salt_delegation.rs` | `test_a_transaction_to_a_self_delegating_account_settles` | ported | port_saltdel_tx_to_self_delegating |
| `satin/salt_delegation.rs` | `test_a_create_from_a_delegated_frame_settles` | ported | port_saltdel_create_from_delegated |
| `satin/salt_delegation.rs` | `test_a_create2_from_a_delegated_frame_settles` | ported | port_saltdel_create2_from_delegated |
| `satin/salt_delegation.rs` | `test_a_value_call_to_a_delegated_account_charges_no_state_gas` | covered | edge_sweep_tx0_funded_delegated_v5_runs, edge_sweep_auth_*_delegated_v5_runs (value to an account holding a designator: no new account) |
| `satin/salt_delegation.rs` | `test_a_create_inside_a_delegated_frame_is_priced_at_the_authority_s_address` | ported | port_saltdel_create_priced_at_authority |
| `satin/salt_delegation.rs` | `test_a_callcode_to_an_empty_account_charges_no_state_gas` | covered | fz modes 1-2 (CALLCODE with value to EMPTY); p6_allowance_value_callcode (CALLCODE with value) |
| `satin/salt_delegation.rs` | `test_a_callcode_to_a_delegated_account_charges_no_state_gas` | ported | port_saltdel_callcode_no_state_gas |
| `satin/salt_delegation.rs` | `test_a_callcode_does_not_consult_the_pricing_hook` | not-portable | db fault (a broken SALT environment) |
| `satin/salt_deposit.rs` | `test_a_deposit_creation_is_priced_where_it_deploys` | ported | port_saltdep_where_deployed |
| `satin/salt_deposit.rs` | `test_crowding_the_envelope_nonce_s_address_changes_no_price` | ported | port_saltdep_envelope_nonce_crowded |
| `satin/salt_deposit.rs` | `test_an_unpriceable_deposit_creation_fails_with_its_cause` | not-portable | db fault (a failing bucket lookup at the deployed address); the invalid-capacity analogue is edge_salt_capacity_zero_create |
| `satin/salt_deposit.rs` | `test_a_failure_at_the_envelope_nonce_s_address_is_never_asked_for` | ported | port_saltdep_envelope_nonce_invalid |
| `satin/salt_deposit.rs` | `test_a_reverting_deposit_creation_gives_the_charge_back_where_it_was_made` | ported | port_saltdep_reverting_init |
| `satin/salt_deposit.rs` | `test_at_equal_nonces_a_deposit_and_an_ordinary_creation_agree` | covered | q20_deposit_new_caller_creates_salt, p6_salt_code_deposit_m2 (equal nonces: priced at create(0)) |
| `satin/salt_failure.rs` | `test_an_unpriceable_sstore_set_fails_the_transaction` | not-portable | db fault (a failing SALT bucket lookup); the invalid-capacity analogue is edge_salt_capacity_zero_slot |
| `satin/salt_failure.rs` | `test_an_unpriceable_new_account_call_fails_the_transaction` | not-portable | db fault (a failing SALT bucket lookup); analogue edge_salt_capacity_zero_call_new_account |
| `satin/salt_failure.rs` | `test_an_unpriceable_selfdestruct_beneficiary_fails_the_transaction` | not-portable | db fault (a failing SALT bucket lookup) |
| `satin/salt_failure.rs` | `test_an_unpriceable_create_fails_the_transaction` | not-portable | db fault (a failing SALT bucket lookup); analogue edge_salt_capacity_zero_create |
| `satin/salt_failure.rs` | `test_an_unpriceable_top_level_recipient_fails_the_transaction` | not-portable | db fault (a failing SALT bucket lookup); analogue edge_salt_capacity_zero_tx_new_account |
| `satin/salt_failure.rs` | `test_an_unpriceable_create_transaction_target_fails_the_transaction` | not-portable | db fault (a failing SALT bucket lookup) |
| `satin/salt_failure.rs` | `test_an_unpriceable_eip7702_authority_fails_the_transaction` | not-portable | db fault (a failing SALT bucket lookup) |
| `satin/salt_failure.rs` | `test_an_unpriceable_code_deposit_fails_the_transaction` | not-portable | db fault (a failing SALT bucket lookup) |
| `satin/salt_failure.rs` | `test_a_program_that_charges_no_state_gas_is_unaffected` | not-portable | db fault (a SALT environment that cannot answer) |
| `satin/salt_failure.rs` | `test_an_unpriceable_charge_in_a_reverting_frame_still_fails_the_transaction` | not-portable | db fault (a failing SALT bucket lookup inside a reverting frame) |
| `satin/salt_failure.rs` | `test_a_capacity_below_the_minimum_bucket_fails_the_transaction` | covered | edge_salt_capacity_zero_slot, _tx_new_account, _call_new_account, _create, _deposit_caller; fz mode 8 (capacities 0, 1, 255) |
| `satin/salt_failure.rs` | `test_the_minimum_capacity_itself_is_priced_not_refused` | covered | fz mode 8 (capacity 256 entries, the minimum bucket) |
| `satin/salt_failure.rs` | `test_a_system_call_is_unaffected_by_an_unreadable_bucket` | not-portable | internal api (system call) and db fault |
| `satin/salt_failure.rs` | `test_a_database_error_on_the_create_target_read_fails_the_transaction` | not-portable | db fault (the create target's account read) |
| `satin/salt_refund.rs` | `test_a_slot_written_and_restored_nets_nothing_at_every_multiplier` | covered | p6_salt_sstore_restore_m1, m2, m8 |
| `satin/salt_refund.rs` | `test_a_reverting_creation_gives_its_upfront_charge_back` | covered | p6_salt_failed_create_m1, m2, m8, q29_create_reverts_salt2_res |
| `satin/salt_refund.rs` | `test_a_failed_value_call_gives_its_new_account_charge_back` | covered | p6_salt_failed_value_call_m1, m2, m8 |
| `satin/salt_refund.rs` | `test_a_charge_in_a_reverting_inner_frame_comes_back_whole` | covered | q13_grandchild_stop_after_history_state, q14 (a crowded slot written two frames deep, rolled back with its frame) |
| `satin/salt_refund.rs` | `test_a_charge_an_inner_frame_keeps_scales_with_its_bucket` | covered | fz modes 1-2 (B writing slots with SALT m 1/2/8 on B's slots); q21_deposit_writes_salt2 |
| `satin/salt_refund.rs` | `test_a_selfdestruct_whose_caller_reverts_gives_its_charge_back` | covered | fz mode 9 (destruction to EMPTY inside a wrapper B that reverts) |
| `satin/salt_refund.rs` | `test_a_transaction_that_reverts_pays_no_state_gas` | covered | c2_value_tx_revert, c2_value_tx_revert_res |
| `satin/salt_refund.rs` | `test_an_applied_authorization_keeps_its_charge_through_a_revert` | covered | e13_above_cap_first_frame_reverts |
| `satin/salt_refund.rs` | `test_a_refused_code_deposit_pays_no_state_gas` | ported | port_saltref_refused_code_deposit |
| `satin/salt_refund.rs` | `test_a_kept_charge_and_a_restored_one_settle_independently` | ported | port_saltref_kept_and_restored |
| `satin/salt_refund.rs` | `test_a_synthetic_call_failure_refunds_the_crowded_upfront_charge` | not-portable | internal api (settles a synthetic frame failure through revm's frame return by hand) |
| `satin/salt_refund.rs` | `test_a_synthetic_creation_failure_refunds_the_crowded_upfront_charge` | not-portable | internal api (settles a synthetic frame failure through revm's frame return by hand) |
| `satin/salt_refund.rs` | `test_a_creation_that_cannot_pay_its_code_deposit_keeps_no_state_gas` | ported | port_saltref_code_deposit_unpayable |
| `satin/salt_refund.rs` | `test_a_created_account_selfdestructed_in_the_same_transaction_gives_its_state_gas_back` | covered | p6_destroy_in_constructor_m1, m2, m8 |
| `satin/salt_refund.rs` | `test_a_contract_destroyed_after_its_creation_gives_back_its_leaf_slot_and_code` | ported | port_saltref_destroyed_after_creation |
| `satin/schedule.rs` | `test_an_empty_call_costs_fifteen_thousand` | covered | p6_stop, edge_sweep_tx0_funded_existing_v0_runs |
| `satin/schedule.rs` | `test_a_value_transfer_to_an_existing_account_costs_twenty_one_thousand` | covered | edge_start_record_paid, edge_sweep_tx0_funded_existing_v5_runs |
| `satin/schedule.rs` | `test_a_value_transfer_that_creates_the_recipient_draws_the_account_state_gas` | covered | c4_value_tx_to_empty_res, edge_sweep_tx0_funded_absent_v5_runs |
| `satin/schedule.rs` | `test_a_self_transfer_costs_twelve_thousand` | covered | fz mode 11 (value to the sender itself), e09_sender_authority (tx to the sender) |
| `satin/schedule.rs` | `test_a_create_transaction_costs_twenty_four_thousand_plus_the_account_state_gas` | covered | fz mode 11 (creation with empty init code), edge_sweep_tx0_funded_create_v0_runs |
| `satin/schedule.rs` | `test_init_code_costs_its_calldata_tokens_and_an_eip3860_word` | ported | port_sched_init_code_32, port_sched_init_code_33, … (6 scenarios, `port_sched_init_code_*`) |
| `satin/schedule.rs` | `test_a_reverted_creation_draws_no_state_gas` | covered | q29_create_reverts_salt2_res, w4_det_create_init_reverts_loop, fz mode 6 (creation tx whose init reverts) |
| `satin/schedule.rs` | `test_a_create_frame_rejected_for_its_first_byte_draws_no_state_gas` | ported | port_sched_create_rejected_first_byte |
| `satin/schedule.rs` | `test_the_amsterdam_opcodes_run` | covered | p6_dupn, p6_swapn, p6_exchange, p6_slotnum |
| `satin/schedule.rs` | `test_clz_runs_on_the_base_spec` | ported | port_sched_clz |
| `satin/schedule.rs` | `test_slotnum_pushes_the_blocks_slot_number` | covered | p6_slotnum (SLOTNUM stored) |
| `satin/spec_numbers.rs` | `test_satin_md_matches_the_constants` | not-portable | internal api (checks docs/spec/upgrades/satin.md against the constants) |
| `satin/start_ledger.rs` | `test_the_depth_guards_answer_settles_on_its_start` | not-portable | other: the neutral configuration (EIP-150 forwards reach depth 1,024); internal start-ledger settlements |
| `satin/start_ledger.rs` | `test_a_fatal_error_with_starts_open_discards_the_ledger_and_the_evm_runs_on` | not-portable | db fault (a failing storage read two starts deep) |
| `satin/start_ledger.rs` | `test_an_evm_run_inside_a_callback_keeps_a_ledger_of_its_own` | not-portable | other: a custom inspector that runs an EVM inside its callback |
| `satin/start_ledger.rs` | `test_an_inspectors_answer_to_the_transactions_frame_settles_as_unlimited_pools` | not-portable | other: a custom inspector that answers the transaction's own frame |
| `satin/start_ledger.rs` | `test_end_callback_rewrites_settle_by_the_new_status_and_the_flags` | not-portable | other: a custom inspector that rewrites results in call_end |
| `satin/start_ledger.rs` | `test_a_creation_that_takes_no_address_settles_as_one` | not-portable | other: a custom inspector that clears a creation's address |
| `satin/start_ledger.rs` | `test_a_write_in_a_start_an_inspector_made_itself_runs_in_a_system_call` | not-portable | other: a custom inspector that starts a call of its own (and a system call) |
| `satin/state.rs` | `test_evm_state_zero_storage` | covered | p7_sload_skip_cold_sees_total_noread, fz modes 1-2 (SLOAD of a never-written slot: in the read set) |
| `satin/state.rs` | `test_evm_state_include_failed_tx_reads` | covered | c2_value_tx_revert, c3_value_tx_halt (read set of a failed transaction) |
| `satin/state.rs` | `test_state_clear_with_noop_call` | covered | fz mode 11 (a call without value to an existing empty account, EMPTY2) |
| `satin/state.rs` | `test_write_through_a_delegator_leaves_the_delegate_storage_alone` | ported | port_state_delegator_write |
| `satin/state_growth.rs` | `test_a_delegate_that_writes_a_slot_back_gives_its_state_gas_back` | covered | p6_child_refills_ancestor_ok, p6_child_refills_ancestor_ok_res (a DELEGATECALL child writes the caller's slot back) |
| `satin/state_growth.rs` | `test_an_authorization_adds_state_only_when_it_applies` | covered | e01-e07 (new, existing, wrong nonce, other chain, invalid signature, code); the nonce-that-cannot-be-bumped row is not a scenario |
| `satin/state_growth.rs` | `test_an_authority_an_earlier_transaction_funded_is_no_new_account` | ported | port_growth_authority_funded_t1, port_growth_authority_funded_t2 |
| `satin/state_growth.rs` | `test_an_authority_an_earlier_transaction_touched_is_counted_as_it_is_charged` | ported | port_growth_authority_touched_t1, port_growth_authority_touched_t2 |
| `satin/state_growth.rs` | `test_an_authority_the_transaction_cannot_pay_for_is_not_applied` | covered | e17_authority_charges_out_of_gas, edge_sweep_auth_*_newauthority (out of gas before the first frame, authorization taken back) |
| `satin/state_growth.rs` | `test_an_authority_that_is_the_recipient_pays_for_one_account` | covered | e15_value_to_authority |
| `satin/state_growth.rs` | `test_authorities_whose_records_would_cross_the_data_size_limit_reject_the_transaction` | covered | e16_body_records_over_data_limit, e16b_body_records_fit (new authorities; existing ones not) |
| `satin/state_growth.rs` | `test_a_stop_keeps_what_was_applied_before_the_first_frame` | covered | e19_delegate_writes_growth_stop, port_hold_caller_one_over_limit (stop with an applied authority kept) |
| `satin/state_growth.rs` | `test_a_stopped_deposit_keeps_the_caller_it_created` | covered | edge_first_frame_data_budget_before_growth, edge_first_frame_over_growth_contract (deposit's created caller outlives the stop) |
| `satin/state_growth.rs` | `test_a_destruction_to_itself_grows_no_state` | covered | fz modes 1-2 (A, with a balance, ending in SELFDESTRUCT to A); fz mode 9 (ADDRESS SELFDESTRUCT) |
| `satin/state_growth.rs` | `test_a_state_charge_the_frame_cannot_pay_runs_out_of_gas_whatever_the_limit` | ported | port_growth_cannot_pay_sstore_60000, port_growth_cannot_pay_sstore_100000, … (25 scenarios, `port_growth_cannot_pay_*`) |
| `satin/state_growth.rs` | `test_the_state_growth_limit_is_the_specs` | not-portable | internal api (asserts the spec's state-growth limit constants) |
| `satin/state_growth.rs` | `test_the_1001st_fresh_slot_stops_the_transactions_own_frame` | ported | port_growth_fresh_slots_1000, port_growth_fresh_slots_1000_big_gas, port_growth_fresh_slots_1001, port_growth_fresh_slots_1001_big_gas |
| `satin/state_growth.rs` | `test_a_child_gets_98_percent_of_what_its_caller_has_left` | ported | port_budget_child_960_19, port_budget_child_961_961 |
| `satin/state_growth.rs` | `test_the_state_growth_budgets_at_depth` | ported | port_budget_growth_depth3_940, port_budget_growth_depth2_940, port_budget_growth_depth3_941, port_budget_growth_depth2_941 |
| `satin/state_growth.rs` | `test_a_negative_net_does_not_enlarge_the_next_childs_budget` | ported | port_budget_cleared_net_kept, port_budget_cleared_net_reverted |
| `satin/state_growth.rs` | `test_a_slot_written_back_to_zero_falls_and_makes_room` | ported | port_growth_back_to_zero_limit1, port_growth_back_to_zero_limit2, port_growth_child_clears_callers_slot |
| `satin/state_growth.rs` | `test_each_site_adds_one_new_account_or_slot` | covered | c4_value_tx_to_empty_res, q04, s3_create_res, edge_sweep_tx0_funded_create_v0_runs, q18, e01, p6_deposit_new_caller_m1 (one unit each); p6_allowance_value_callcode, edge_value_returned_by_selfdestruct (none) |
| `satin/state_growth.rs` | `test_a_start_that_makes_no_new_account_is_held_to_nothing` | covered | bd_create_tx_collision_no_growth, fz mode 6 (CREATE/CREATE2 onto occupied or balance-only addresses under growth limits 0-1), i7_unfunded_value_call, w1_det_create_out_of_funds_loop |
| `satin/state_growth.rs` | `test_a_creation_is_budgeted_after_its_creators_counts` | ported | port_growth_creation_budget_979, port_growth_creation_budget_979_after_writes, … (6 scenarios, `port_growth_creation_*`) |
| `satin/state_growth.rs` | `test_a_slot_crossing_several_limits_stops_on_the_first_in_order` | ported | port_growth_first_in_order_data_and_growth, port_growth_first_in_order_kv_and_growth, … (6 scenarios, `port_growth_first_in_order_*`) |
| `satin/state_growth.rs` | `test_an_authorization_list_is_held_to_the_state_growth_it_would_add` | covered | e11_list_skipped_growth, e10_list_skipped_kv, e11b_list_fits_growth, e03_two_entries_one_authority; the value-to-new-account-with-none-left row is the limit_matrix/keyless gap |
| `satin/state_growth.rs` | `test_a_deposits_new_caller_is_counted_before_its_first_frame` | ported | port_growth_deposit_new_caller_one_slot, port_growth_deposit_new_caller_two_slots, port_growth_deposit_new_caller_three_slots |
| `satin/state_growth.rs` | `test_a_deposits_failed_first_answer_is_held_to_no_limit` | ported | port_growth_deposit_failed_answer |
| `satin/state_growth.rs` | `test_a_start_over_the_state_growth_is_stopped_before_it_runs` | covered | s2_value_to_empty_stopped, s2_value_to_empty_stopped_res, s2_value_to_precompile_stopped, q27_det_start_stop_new_accounts, q11 (precompile price stop kind 2) |
| `satin/static_callee.rs` | `test_create2_static_oversized_initcode_reports_static_rejection` | not-portable | other: an inner frame's halt reason is not in the record (static rejection and the other halts both burn the forward) |
| `satin/static_callee.rs` | `test_create2_static_hugely_oversized_initcode_halt_reason` | not-portable | other: an inner frame's halt reason is not in the record (static rejection and the other halts both burn the forward) |
| `satin/static_callee.rs` | `test_create2_static_zero_length_initcode_reports_static_rejection` | not-portable | other: an inner frame's halt reason is not in the record (static rejection and the other halts both burn the forward) |
| `satin/static_callee.rs` | `test_create2_static_zero_length_low_gas_halt_reason` | not-portable | other: an inner frame's halt reason is not in the record (static rejection and the other halts both burn the forward) |
| `satin/static_callee.rs` | `test_create_static_low_gas_halt_reason` | not-portable | other: an inner frame's halt reason is not in the record (static rejection and the other halts both burn the forward) |
| `satin/static_callee.rs` | `test_create2_static_missing_operands_halt_reason` | not-portable | other: an inner frame's halt reason is not in the record (static rejection and the other halts both burn the forward) |
| `satin/stops.rs` | `test_every_limit_stops_the_frame_that_crosses_it_at_every_depth_and_tier` | ported | port_stopmx_data_depth0_below, port_stopmx_data_depth3_below, … (16 scenarios, `port_stopmx_*`) |
| `satin/stops.rs` | `test_an_inspector_that_rewrites_a_stop_into_a_halt_gets_the_halt` | not-portable | other: a custom inspector that rewrites every result into a halt |
| `satin/stops.rs` | `test_a_revived_creation_keeps_what_it_counted_before_its_stop` | not-portable | other: a custom inspector that revives stopped creations |
| `satin/stops.rs` | `test_a_frame_budget_crossed_three_calls_down_reverts_that_frame_alone` | ported | port_stopmx_budget_depth3_below, port_stopmx_budget_depth3_above |
| `satin/stops.rs` | `test_a_halt_three_calls_down_burns_its_frames_gas_and_its_caller_resumes` | ported | port_stopmx_halt_depth3_loop_200000, port_stopmx_halt_depth3_loop_200050, … (6 scenarios, `port_stopmx_halt_depth3_*`) |
| `satin/stops.rs` | `test_the_transactions_own_frame_that_halts_burns_its_regular_gas` | covered | c3_value_tx_halt, c3_value_tx_halt_res; fz mode 7 (a top-level precompile priced above the gas the transaction has) |
| `satin/stops.rs` | `test_the_transactions_own_value_transfer_that_halts_gives_its_record_back_as_history_alone` | covered | c3_value_tx_halt, c3_value_tx_halt_res |
| `satin/stops.rs` | `test_volatile_data_access_oog_does_not_consume_all_gas` | ported | port_det_writes_after_read_60000000, port_det_writes_after_read_372448000 |
| `satin/stops.rs` | `test_nested_call_block_env_access_child_oog` | covered | d2_child_crosses_caller_resumes (B crosses after its read; A resumes and its SSTORE crosses again) |
| `satin/stops.rs` | `test_parent_block_env_access_oog_after_nested_call` | ported | port_vol_two_frames_down, port_vol_two_frames_down_middle_loops |
| `satin/stops.rs` | `test_an_oracle_read_holds_the_transaction_to_its_cap` | covered | q54_oracle_answers_then_loop, q56_det_oracle_read_crosses (an Oracle read holds the transaction to the Oracle cap; at lowered caps, not the default 20M) |
| `satin/stops.rs` | `test_oracle_volatile_data_access_oog_does_not_consume_all_gas` | ported | port_vol_oracle_reads_call, port_vol_oracle_reads_staticcall, port_vol_oracle_reads_direct |
| `satin/stops.rs` | `test_parent_runs_out_of_gas_after_oracle_access` | ported | port_vol_oracle_reads_call, port_vol_oracle_reads_staticcall, port_vol_oracle_reads_direct |
| `satin/stops.rs` | `test_both_volatile_data_access_oog_does_not_consume_all_gas` | ported | port_det_most_restrictive_block_then_oracle, port_det_most_restrictive_oracle_then_block |
| `satin/stops.rs` | `test_volatile_access_post_access_cap_enforced` | covered | d1_top_frame_crosses, edge_modexp_over_bound_detained_* (compute past the cap after a TIMESTAMP read) |
| `satin/stops.rs` | `test_non_binding_detention_reports_a_normal_out_of_gas` | ported | port_vol_own_fields_nothing |
| `satin/stops.rs` | `test_detained_gas_is_restored` | covered | d3_gas_reads_full_gas, p7_gas_reads_total_read (detained frames that succeed pay only for what ran) |
| `satin/stops.rs` | `test_data_limit_exceed_in_nested_call` | ported | port_stopmx_nested_data |
| `satin/stops.rs` | `test_state_revert_when_exceeding_limit` | ported | port_stopmx_value_start_cross |
| `satin/stops.rs` | `test_detention_plus_intrinsic_data_size_overflow` | ported | port_stopmx_detained_body_over |
| `satin/stops.rs` | `test_a_call_revived_past_a_limit_hands_its_caller_nothing` | not-portable | other: a custom inspector that revives a stopped call (and a per-frame cap) |
| `satin/stops.rs` | `test_an_answer_three_calls_down_is_the_stop_its_caller_catches_at_either_tier` | not-portable | other: a node-installed precompile (with_dyn_precompiles) and a rewriting inspector |
| `satin/stops.rs` | `test_an_inspectors_answer_past_the_allowance_is_taken_as_given` | not-portable | other: a custom inspector that answers a frame |
| `satin/stops.rs` | `test_a_start_three_calls_down_crossing_the_limit_is_stopped_at_its_start` | ported | port_stop_start_depth3_below_cap, port_stop_start_depth3_above_cap |
| `satin/stops.rs` | `test_a_stopped_answer_gives_its_caller_back_its_share` | ported | port_stop_answer_gives_back_data_below_cap, port_stop_answer_gives_back_kv_below_cap, … (6 scenarios, `port_stop_answer_gives_back_*`) |
| `satin/strategic_stop.rs` | `test_cap_crossed_at_depth_three_stops_each_frame_that_crosses_it` | ported | port_strat_depth3 |
| `satin/strategic_stop.rs` | `test_stop_refills_the_reservoir_at_any_gas_limit` | covered | s1_top_stopped, s1_top_stopped_res (stopped transaction: unspent gas and the whole reservoir back) |
| `satin/strategic_stop.rs` | `test_true_out_of_gas_still_halts_and_burns` | ported | port_strat_true_oog |
| `satin/strategic_stop.rs` | `test_child_out_of_gas_under_a_cap_is_no_stop` | ported | port_strat_child_oog |
| `satin/strategic_stop.rs` | `test_cap_crossed_before_the_first_frame_reverts_without_running` | ported | port_strat_before_first_frame |
| `satin/strategic_stop.rs` | `test_frame_budget_reverts_the_frame_alone` | not-portable | other: per-frame data-size cap (frame_data_size_limit = 100) |
| `satin/strategic_stop.rs` | `test_frame_budget_of_the_outermost_frame_reverts_the_transaction` | not-portable | other: per-frame data-size cap (frame_data_size_limit = 100) |
| `satin/strategic_stop.rs` | `test_the_stop_does_not_outlive_its_transaction` | covered | blk_limits_reset_t1..t3, blk_hint_limit_per_transaction_t1..t3 (the next transaction counts from zero); the system-call entry point is not portable |
| `satin/strategic_stop.rs` | `test_create_transaction_stopped_before_its_first_frame_bumps_the_nonce` | ported | port_strat_create_tx_stopped |
| `satin/strategic_stop.rs` | `test_usage_equal_to_the_limit_does_not_stop` | not-portable | other: per-frame data-size cap (frame_data_size_limit = 120); the transaction-limit case is the data_size_counts gap |
| `satin/strategic_stop.rs` | `test_stop_refills_the_state_gas_drawn_from_the_reservoir` | covered | s1_top_stopped_res (state gas of the slots before the stop refilled, reservoir back) |
| `satin/strategic_stop.rs` | `test_outcome_reports_history_gas` | covered | s1_top_stopped, s1_top_stopped_res (history of the body kept, the frames' taken back) |
| `satin/synthetic_frame_gas.rs` | `test_depth_guard_returns_call_too_deep_with_the_inherited_reservoir` | not-portable | internal api (frame_init past the call-stack limit; depth 1,025 is unreachable under the 98/100 forward) |
| `satin/synthetic_frame_gas.rs` | `test_depth_guard_answers_a_creation_and_every_call_scheme` | not-portable | internal api (frame_init past the call-stack limit) |
| `satin/synthetic_frame_gas.rs` | `test_depth_boundary_allows_call_at_limit` | not-portable | internal api (frame_init at depth 1,024) |
| `satin/synthetic_frame_gas.rs` | `test_an_inspectors_answer_past_the_depth_limit_is_taken_as_given` | not-portable | other: a custom inspector answering a call past the depth limit |
| `satin/synthetic_frame_gas.rs` | `test_stopped_first_frame_keeps_the_reservoir_at_any_gas_limit` | ported | port_synth_stopped_first_frame_10000000, port_synth_stopped_first_frame_300000000 |
| `satin/synthetic_frame_gas.rs` | `test_an_intercepted_answer_keeps_the_reservoir` | ported | port_synth_intercepted_remaining_1000000, port_synth_intercepted_remaining_300000000, … (8 scenarios, `port_synth_intercepted_*`) |
| `satin/synthetic_frame_gas.rs` | `test_an_intercepted_refusal_keeps_the_reservoir` | ported | port_synth_refused_remaining_1000000, port_synth_refused_remaining_300000000, … (8 scenarios, `port_synth_refused_*`) |
| `satin/synthetic_frame_gas.rs` | `test_a_forwarded_hint_keeps_the_reservoir` | ported | port_synth_direct_hint_300m |
| `satin/synthetic_frame_gas.rs` | `test_the_keyless_overhead_comes_out_of_regular_gas` | covered | k01_ok, k19_above_cap, k12_malformed_call (overhead charged, refusal answered) |
| `satin/synthetic_frame_gas.rs` | `test_the_keyless_value_refusal_keeps_the_reservoir` | ported | port_synth_keyless_value_refused_300m |
| `satin/synthetic_frame_gas.rs` | `test_a_keyless_call_below_the_overhead_runs_out_of_gas` | not-portable | internal api (frame_init with a forward below the overhead; not reachable by a transaction) |
| `satin/system_call.rs` | `test_a_system_call_above_30m_carries_the_excess_as_reservoir` | not-portable | internal api (system call entry point) |
| `satin/system_call.rs` | `test_a_system_call_s_state_draws_the_reservoir_first_then_spills` | not-portable | internal api (system call entry point) |
| `satin/system_call.rs` | `test_reservoir_remaining_is_what_the_writes_left` | not-portable | internal api (system call entry point) |
| `satin/system_call.rs` | `test_a_failing_system_call_settles_like_op_revm` | not-portable | internal api (system call entry point) |
| `satin/system_call.rs` | `test_a_system_call_with_a_reservoir_stays_exempt` | not-portable | internal api (system call entry point) |
| `satin/system_call.rs` | `test_the_pre_block_apply_pending_changes_is_priced_at_the_minimum_bucket` | not-portable | block executor (pre-block applyPendingChanges call) |
| `satin/system_call.rs` | `test_the_default_pre_block_calls_keep_the_30m_regular_budget_in_a_crowded_region` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `satin/system_call.rs` | `test_a_system_address_transaction_is_split_by_the_execution_cap` | covered | fz mode 5 (Mega System Transactions at 1M, 30M, 2^32 and 2^32+7M gas), edge_system_legacy_oracle_promoted |
| `satin/tool_writes.rs` | `test_a_precompiles_writes_beyond_its_starts_accounts_count_nothing` | not-portable | other: a node-installed precompile writing through EvmInternals |
| `satin/tool_writes.rs` | `test_a_precompiles_writes_to_the_starts_own_accounts_are_read_with_the_start` | not-portable | other: a node-installed precompile writing through EvmInternals |
| `satin/tool_writes.rs` | `test_a_value_call_answered_at_frame_start_counts_nothing` | not-portable | other: a custom inspector answering calls in frame_start |
| `satin/transfer_logs.rs` | `test_each_site_counts_its_transfer_log` | covered | c1_value_tx_ok(_res), s3_value_call_ok_child_res, q17_det_created_destroyed_to_new_beneficiary (endowment), q18, edge_value_returned_by_selfdestruct (logs and data size in the record) |
| `satin/transfer_logs.rs` | `test_each_site_stops_one_byte_short_of_its_frame_budget` | not-portable | other: per-frame data-size cap (frame_data_size_limit) |
| `satin/transfer_logs.rs` | `test_each_site_stops_one_byte_short_of_the_transaction_limit` | ported | port_tlog_site_tx_value_0, port_tlog_site_tx_value_1, … (14 scenarios, `port_tlog_*`) |
| `satin/transfer_logs.rs` | `test_an_answered_value_call_whose_start_crosses_is_taken_back_before_its_caller_resumes` | ported | port_tlog_start_crosses_no_code_0, port_tlog_start_crosses_no_code_1, … (6 scenarios, `port_tlog_start_crosses_*`) |
| `satin/transfer_logs.rs` | `test_a_failure_takes_each_sites_transfer_log_back` | covered | c2_value_tx_revert, c3_value_tx_halt, s3_value_call_reverting_child_res, q32_nested_value_calls_allowance_middle_reverts |
| `satin/transfer_logs.rs` | `test_no_transfer_log_where_no_value_moves_to_another_account` | covered | p6_allowance_value_callcode (CALLCODE), fz mode 11 (value to the sender), fz mode 9 (destruction to itself), i6_value_call_refused (refused by an interceptor) |
| `satin/transfer_logs.rs` | `test_a_journal_switched_off_neither_logs_nor_counts_a_transfer_log` | not-portable | internal api (turns the journal's EIP-7708 switch off) |
| `satin/transfer_logs.rs` | `test_a_value_call_its_caller_cannot_fund_runs_as_without_a_limit` | ported | port_tlog_caller_cannot_fund_unfunded, port_tlog_caller_cannot_fund_funded |
| `satin/transfer_logs.rs` | `test_a_value_call_its_caller_cannot_fund_is_charged_nothing` | ported | port_tlog_charge_nothing_unfunded_400000, port_tlog_charge_nothing_unfunded_450000, … (6 scenarios, `port_tlog_charge_nothing_*`) |
| `satin/transfer_logs.rs` | `test_a_start_an_inspector_rewrites_is_refused_on_the_input_it_leaves` | not-portable | other: a custom inspector that raises a call's value |
| `satin/transfer_logs.rs` | `test_a_creation_onto_an_occupied_address_fails_on_the_collision` | ported | port_tlog_create_collision_start_short, port_tlog_create_collision_body_short, port_tlog_create_collision_ample |
| `satin/transfer_logs.rs` | `test_a_deposits_value_is_logged_and_its_mint_is_not` | covered | edge_deposit_value_new_recipient_ok, q21_deposit_writes_salt2 (deposit value logged, mint not) |
| `satin/transfer_logs.rs` | `test_a_deposit_its_depositor_cannot_fund_fails_as_without_a_limit` | covered | edge_sweep_dep_*_unfunded_runs (deposit value nothing funds: failed deposit); the limit-one-short row is not a scenario |
| `satin/transfer_logs.rs` | `test_a_value_call_its_recipient_cannot_hold_keeps_nothing` | ported | port_tlog_recipient_overflow_plain, port_tlog_recipient_overflow_short |
| `satin/transfer_logs.rs` | `test_a_failed_precompiles_start_kept_nothing_and_is_held_to_nothing` | ported | port_tlog_failed_precompile_plain, port_tlog_failed_precompile_short |
| `satin/transfer_logs.rs` | `test_a_failed_precompile_answer_at_a_frame_budget_boundary_burns_its_forward` | not-portable | other: per-frame data-size cap (frame_data_size_limit) |
| `satin/transfer_logs.rs` | `test_a_nested_creation_whose_nonce_cannot_be_bumped_keeps_nothing` | ported | port_tlog_stuck_nonce_create_plain, port_tlog_stuck_nonce_create_short |
| `satin/transfer_logs.rs` | `test_a_creation_whose_nonce_cannot_be_bumped_keeps_nothing` | covered | edge_deposit_create_nonce_max(_value), port_create_max_nonce_check_off, port_create_max_nonce_deposit |
| `satin/transfer_logs.rs` | `test_a_system_transactions_value_is_logged_and_no_limit_stops_it` | ported | port_tlog_system_tx_value |
| `satin/transfer_logs.rs` | `test_transfer_logs_keep_execution_order` | ported | port_tlog_order_log_then_selfdestruct, port_tlog_order_create_then_log |
| `satin/volatile_access.rs` | `test_the_transactions_own_fields_record_nothing` | ported | port_vol_own_fields_nothing |
| `satin/volatile_access.rs` | `test_detention_starts_afresh_for_every_transaction` | covered | blk_detention_not_carried_t1..t4 |
| `satin/volatile_access.rs` | `test_a_read_two_frames_down_caps_every_caller` | ported | port_vol_two_frames_down, port_vol_two_frames_down_middle_loops |
| `satin/volatile_access.rs` | `test_a_read_in_a_frame_that_reverts_still_caps` | ported | port_vol_read_in_reverted_frame |
| `satin/volatile_access.rs` | `test_detention_leaves_a_data_size_stop_to_the_data_size_limit` | covered | q33_det_deposit_refused_data_size (detained, stopped on data size), q15_det_grandchild_kv_stop (detained, KV stop) |
| `satin/volatile_access.rs` | `test_calling_the_oracle_without_reading_its_storage_detains_nothing` | ported | port_vol_oracle_no_read |
| `satin/volatile_access.rs` | `test_the_oracle_reading_its_own_storage_is_held_to_the_cap` | ported | port_vol_oracle_reads_call, port_vol_oracle_reads_staticcall, port_vol_oracle_reads_direct |
| `satin/volatile_access.rs` | `test_the_oracles_code_on_another_accounts_storage_reads_nothing_volatile` | ported | port_vol_oracle_code_elsewhere_delegatecall, port_vol_oracle_code_elsewhere_callcode |
| `satin/volatile_access.rs` | `test_a_hint_detains_nothing` | ported | port_vol_hint_detains_nothing |
| `satin/volatile_access.rs` | `test_the_system_address_is_not_detained` | ported | port_vol_system_not_detained, port_vol_deposit_detained |
| `satin/volatile_access.rs` | `test_a_users_deposit_is_detained` | covered | q23_deposit_detained_crosses (a user's deposit detained by a TIMESTAMP read) |
| `satin/volatile_access.rs` | `test_the_beneficiarys_own_frame_is_detained` | covered | t01_recipient_is_beneficiary, u5_full_stack_selfbalance (the beneficiary's own SELFBALANCE) |
| `satin/volatile_access.rs` | `test_a_selfdestruct_to_the_beneficiary_moves_the_balance_and_detains` | covered | edge_sd_beneficiary_history_paid, edge_sd_beneficiary_kv_stop (SELFDESTRUCT to the beneficiary) |
| `satin/volatile_access.rs` | `test_a_refusal_follows_what_the_host_loads` | covered | bd_call_delegated_*, t05_call_follows_delegation_refused, bd_selfdestruct_own_static_* |
| `satin/volatile_access.rs` | `test_the_beneficiary_destroying_itself_is_refused` | covered | bd_selfdestruct_own_static_at, bd_selfdestruct_own_static_p1 |
| `satin/volatile_access.rs` | `test_a_stack_underflow_is_not_a_refusal` | covered | bd_call_target_empty_stack, bd_selfdestruct_to_empty_stack, a25_blockhash_empty_stack_underflows |
| `satin/volatile_access.rs` | `test_a_database_failure_surfaces_and_a_stack_underflow_comes_first` | not-portable | db fault (a failing account load) |
| `satin/volatile_access.rs` | `test_a_call_to_the_access_control_contract_costs_its_overhead` | covered | a05_is_disabled, a01_disable_then_timestamp_refused (calls to MegaAccessControl answered on the forwarded gas) |
| `satin/volatile_access.rs` | `test_an_applied_authority_that_is_the_beneficiary_detains` | covered | e14_beneficiary_authority_detained |
| `satin/volatile_access.rs` | `test_a_transaction_to_a_delegator_of_the_beneficiary_detains` | covered | t03_recipient_delegates_to_beneficiary, t04_call_follows_delegation_to_beneficiary |
| `satin/volatile_access.rs` | `test_a_cap_below_the_intrinsic_gas_stops_nothing_before_the_first_frame` | ported | port_vol_cap_below_intrinsic_completes, port_vol_cap_below_intrinsic_loops |
| `satin/withheld_gas.rs` | `test_gas_answers_the_whole_regular_gas_after_a_read` | covered | d3_gas_reads_full_gas, p7_gas_reads_total_read vs _noread |
| `satin/withheld_gas.rs` | `test_a_bounded_callee_writes_what_it_writes_without_the_read` | ported | port_wh_bounded_callee_below_read, port_wh_bounded_callee_below_noread, port_wh_bounded_callee_above_read, port_wh_bounded_callee_above_noread |
| `satin/withheld_gas.rs` | `test_the_sstore_sentry_reads_the_whole_regular_gas` | covered | p7_sstore_sentry_sees_total_read vs _noread |
| `satin/withheld_gas.rs` | `test_the_settlement_counts_the_withheld_gas_as_unspent` | ported | port_wh_settlement_unspent_read, port_wh_settlement_unspent_noread |
| `satin/withheld_gas.rs` | `test_an_out_of_gas_nothing_could_pay_halts_as_without_the_read` | covered | p7_real_oog_halts_read vs _noread (16 MiB memory after a read); the operand-above-usize row is not a scenario |
| `satin/withheld_gas.rs` | `test_a_creation_that_cannot_pay_its_deposit_halts_as_without_the_read` | covered | q61_det_failed_deposit_then_refused_deposit (a detained creation whose deposit state gas exceeds its frame) |
| `satin/withheld_gas.rs` | `test_a_callees_out_of_gas_is_caught_as_without_the_read` | covered | d5_halt_burns_no_compute, p7_real_oog_halts_read (the wrapper catches the child's halt) |
| `satin/withheld_gas.rs` | `test_what_a_halting_callee_burns_is_not_compute` | ported | port_wh_halt_burn_invalid, port_wh_halt_burn_memory_16mib, … (10 scenarios, `port_wh_halt_burn_*`) |
| `satin/withheld_gas.rs` | `test_an_exp_that_cannot_pay_its_exponent_burns_what_its_frame_had` | ported | port_wh_unpayable_exp |
| `satin/withheld_gas.rs` | `test_a_returndatacopy_that_cannot_pay_its_words_burns_what_its_frame_had` | ported | port_wh_unpayable_returndatacopy |
| `satin/withheld_gas.rs` | `test_a_halt_on_a_static_charge_burns_what_its_frame_had` | ported | port_wh_unpayable_static_selfdestruct |
| `satin/withheld_gas.rs` | `test_static_gas_halts_leave_the_compute_where_it_was` | ported | port_wh_static_halt_3400_below, port_wh_static_halt_chain_3400_below, … (8 scenarios, `port_wh_static_halt_*`) |
| `satin/withheld_gas.rs` | `test_a_callee_computing_past_the_cap_is_stopped` | covered | d2_child_crosses_caller_resumes (the caller cannot pay the store of the failed call's status) |
| `satin/withheld_gas.rs` | `test_the_stop_reports_its_compute_whatever_a_callee_burned` | covered | d5_halt_burns_no_compute (A reads, B halts burning its forward, A loops to the stop) |
| `satin/withheld_gas.rs` | `test_a_halt_burns_what_its_frame_had_when_it_halted` | ported | port_wh_halt_burn_at_invalid |
| `satin/withheld_gas.rs` | `test_a_callee_within_the_cap_bills_the_same_as_without_the_read` | covered | p7_*_read vs p7_*_noread pairs (gas_reads_total, forward_from_total, grandchild_crossing_returns_all); the inspector-answered row is not portable |
| `satin/withheld_gas.rs` | `test_a_call_that_reads_the_beneficiary_costs_what_a_warm_account_costs` | ported | port_wh_call_beneficiary_zero_beneficiary, port_wh_call_beneficiary_zero_warm_empty, … (8 scenarios, `port_wh_call_beneficiary_*`) |
| `satin/withheld_gas.rs` | `test_a_precompile_runs_on_the_allowance` | not-portable | other: a node-installed precompile (with_dyn_precompiles) |
| `satin/withheld_gas.rs` | `test_a_precompile_is_held_to_what_the_limit_leaves` | not-portable | other: a node-replaced modexp (with_dyn_precompiles) |
| `satin/withheld_gas.rs` | `test_a_precompile_priced_past_the_cap_computes_nothing` | ported | port_wh_precompile_past_cap_58m_below, port_wh_precompile_past_cap_58m_above, port_wh_precompile_past_cap_125m_below, port_wh_precompile_past_cap_125m_above |
| `satin/withheld_gas.rs` | `test_the_modexp_is_held_from_its_price` | covered | p_modexp_big_exponent_detained vs _plain |
| `satin/withheld_gas.rs` | `test_the_kzg_point_evaluation_is_priced_and_keeps_no_clamp` | ported | port_wh_kzg_priced |
| `satin/withheld_gas.rs` | `test_a_priced_and_an_unpriced_precompile_part_as_a_failed_call_and_a_crossing` | not-portable | other: a precompile installed through with_dyn_precompiles / EvmTr::all_mut |
| `satin/withheld_gas.rs` | `test_a_precompile_whose_input_fails_past_its_gas_check_stops_past_the_allowance` | covered | p_bn_pairing_odd_length_detained vs _plain (pairing charged for whole pairs, length checked after, under a cap) |
| `satin/withheld_gas.rs` | `test_the_bn254_pairing_is_held_from_its_price` | covered | p_bn_pairing_one_pair_detained vs _plain |
| `satin/withheld_gas.rs` | `test_the_bls12_g1_msm_is_held_from_its_price` | covered | p_bls_g1_msm_four_detained vs _plain |
| `satin/withheld_gas.rs` | `test_the_bls12_g2_msm_is_held_from_its_price` | covered | p_bls_g2_msm_two_detained vs _plain |
| `satin/withheld_gas.rs` | `test_the_bls12_pairing_is_held_from_its_price` | covered | p_bls_pairing_one_detained vs _plain |
| `satin/withheld_gas.rs` | `test_a_keyless_calls_charges_are_held_to_what_the_limit_leaves` | covered | edge_keyless_value_detained_sender/_recipient/_at_overhead, k25_signer_is_beneficiary(_tight) |
| `satin/withheld_gas.rs` | `test_a_refused_keyless_calls_spilled_state_gas_is_not_compute` | ported | port_wh_keyless_refused_50000_below, port_wh_keyless_refused_50000_above, … (6 scenarios, `port_wh_keyless_refused_*`) |
| `satin/withheld_gas.rs` | `test_an_inspectors_answer_to_the_transactions_frame_past_the_cap_is_taken_as_given` | not-portable | other: a custom inspector that answers the transaction's own frame |
| `satin/write_records.rs` | `test_two_value_calls_record_the_sender_once` | covered | q04_det_value_new_account_and_stipend_log, q27_det_start_stop_new_accounts (two value calls from one frame) |
| `satin/write_records.rs` | `test_value_calls_to_one_recipient_record_it_once` | ported | port_rec_three_value_calls |
| `satin/write_records.rs` | `test_one_transfer_records_its_sender_and_its_recipient` | covered | s3_value_call_ok_child_res |
| `satin/write_records.rs` | `test_a_transfer_and_its_exact_return_take_both_records_back` | ported | port_rec_return_exact |
| `satin/write_records.rs` | `test_a_partial_return_keeps_both_records` | ported | port_rec_return_partial |
| `satin/write_records.rs` | `test_a_child_that_reverts_after_writing_keeps_nothing` | ported | port_rec_child_reverts, port_rec_return_reverts |
| `satin/write_records.rs` | `test_a_creation_that_fails_leaves_the_creators_record_on_the_caller` | covered | q29_create_reverts_salt2_res, w4_det_create_init_reverts_loop, w5_det_create_init_halts_loop (creator's nonce record stands) |
| `satin/write_records.rs` | `test_nested_creations_record_each_account_once` | ported | port_rec_nested_creations |
| `satin/write_records.rs` | `test_a_value_call_to_self_records_nothing` | ported | port_rec_value_call_to_self |
| `satin/write_records.rs` | `test_a_transfer_to_the_sender_and_to_a_fee_account` | ported | port_rec_transfer_to_sender, port_rec_transfer_to_base_fee_vault, port_rec_transfer_to_l1_vault |
| `satin/write_records.rs` | `test_a_destruction_to_a_fresh_and_to_a_changed_beneficiary` | covered | q18_det_selfdestruct_to_new_beneficiary (fresh), fz mode 9 (heir changed earlier), edge_value_returned_by_selfdestruct (balance back to the loaded state) |
| `satin/write_records.rs` | `test_a_creation_records_its_account_only_when_it_changes_it` | covered | edge_transfer_then_create_destroyed, edge_selfdestruct_balance_then_create_destroyed, fz mode 9 |
| `satin/write_records.rs` | `test_create_then_call_record_the_frame_account_once` | covered | edge_transfer_then_create_destroyed, q17 (a creation and a transfer from one frame) |
| `satin/write_records.rs` | `test_two_creates_record_the_creator_once` | covered | edge_create2_after_create2_destroyed, q61_det_failed_deposit_then_refused_deposit |
| `satin/write_records.rs` | `test_failed_child_discards_its_transfer_records` | covered | s3_value_call_reverting_child_res, q32_nested_value_calls_allowance_middle_reverts |
| `satin/write_records.rs` | `test_failed_first_child_lets_the_next_transfer_record_the_sender` | ported | port_rec_failed_first_child |
| `satin/write_records.rs` | `test_a_delegates_record_of_its_callers_account_follows_the_delegate` | ported | port_rec_delegate_follows_succeeds, port_rec_delegate_follows_reverts |
| `satin/write_records.rs` | `test_reverted_then_retried_create_records_the_creator_once` | ported | port_rec_retried_create |
| `satin/write_records.rs` | `test_creation_without_funds_records_nothing` | covered | w1_det_create_out_of_funds_loop |
| `satin/write_records.rs` | `test_top_level_self_transfer_no_double_count` | covered | fz mode 11 (value to the sender itself) |
| `satin/write_records.rs` | `test_top_level_transfer_counts_the_recipient` | covered | c1_value_tx_ok, edge_start_record_paid |
| `satin/write_records.rs` | `test_nested_self_transfer_after_recorded_sender_counts_nothing` | ported | port_rec_self_after_recorded |
| `satin/write_records.rs` | `test_sstore_first_change_records_and_write_back_refunds` | covered | p6_salt_sstore_restore_m1, ctl_sstore |
| `satin/write_records.rs` | `test_write_back_in_a_child_refunds_the_caller_record` | covered | p6_child_refills_ancestor_ok, p6_child_refills_ancestor_revert |
| `satin/write_records.rs` | `test_selfdestruct_records_the_beneficiary_when_value_moves` | covered | edge_value_returned_by_selfdestruct (to the sender), q18 (fresh), fz mode 9 (to itself), p6_destroy_after_deploy_m1 (no balance) |
| `satin/write_records.rs` | `test_top_level_create_records_the_created_account` | covered | edge_sweep_tx0_funded_create_v0_runs, q38_create_tx_detained_salt2; fz mode 6 (creation tx whose init reverts) |
| `satin/write_records.rs` | `test_execution_writes_to_the_fee_accounts_are_records` | ported | port_rec_fee_accounts_gas_price, port_rec_fee_accounts_beneficiary |
| `satin/write_records.rs` | `test_completed_sstore_commits_its_record` | not-portable | internal api (reads the usage in an inspector right after the SSTORE) |
| `satin/write_records.rs` | `test_sstore_out_of_gas_after_the_write_counts_nothing` | ported | port_rec_sstore_oog_100000, port_rec_sstore_oog_120000, … (5 scenarios, `port_rec_sstore_oog_1*`) |
| `satin/write_records.rs` | `test_a_tools_log_in_step_counts_nothing` | not-portable | other: a custom inspector appending a log through the Host |
| `satin/write_records.rs` | `test_applied_authorities_record_once_each` | covered | e03_two_entries_one_authority, e04_wrong_nonce_skipped |
| `satin/write_records.rs` | `test_value_to_an_applied_authority_records_it_once` | covered | e15_value_to_authority |
| `satin/write_records.rs` | `test_frames_running_as_the_sender_do_not_record_it` | ported | port_rec_frames_as_sender |
| `satin/write_records.rs` | `test_creation_failing_before_the_nonce_bump_records_nothing` | not-portable | other: a custom inspector raising a creation's endowment |
| `satin/write_records.rs` | `test_colliding_creation_keeps_the_creator_record` | covered | x3_create_collision_default_caps, fz mode 6 |
| `satin/write_records.rs` | `test_creation_stopped_at_init_bumps_the_creator_nonce` | not-portable | other: per-frame data-size cap (frame_data_size_limit = 80) |
| `satin/write_records.rs` | `test_authority_records_are_held_to_the_limit_at_validation` | ported | port_rec_authority_limit_fits, port_rec_authority_limit_one_short, port_rec_authority_limit_twice_fits, port_rec_authority_limit_twice_short |
| `satin/write_records.rs` | `test_runtime_out_of_gas_after_authorities_keeps_no_record` | covered | e17_authority_charges_out_of_gas |
| `satin/write_records.rs` | `test_an_inspected_runtime_out_of_gas_unwinds_as_a_plain_one` | covered | e17_authority_charges_out_of_gas (plain and inspected runs compared) |
| `satin/write_records.rs` | `test_before_its_own_frame_a_transaction_changed_only_its_sender_and_authorities` | covered | edge_sweep_* (every envelope and recipient at its first frame) |
| `satin/write_records.rs` | `test_value_receiving_child_records_itself_once` | ported | port_rec_value_passed_on |
| `satin/write_records.rs` | `test_zero_value_child_starts_unrecorded` | ported | port_rec_zero_value_child |
| `satin/write_records.rs` | `test_a_call_moving_no_value_to_an_empty_account_counts_nothing` | covered | fz modes 1-2 (zero-value CALL/STATICCALL/DELEGATECALL/CALLCODE to EMPTY and EMPTY2); fz mode 11 (an existing empty account) |
| `satin/write_records.rs` | `test_a_call_moving_no_value_to_a_delegated_account_counts_what_the_delegate_writes` | covered | edge_sweep_tx0_funded_delegated_v0_runs (top level), t04_call_follows_delegation_to_beneficiary (from a contract) |
| `satin/write_records.rs` | `test_delegatecall_child_inherits_the_frame_account_record` | ported | port_rec_delegatecall_inherits |
| `satin/write_records.rs` | `test_created_frame_delegatecall_inherits_the_created_account_record` | ported | port_rec_created_frame_delegatecall |
| `satin/write_records.rs` | `test_top_level_self_call_counts_the_sender_as_recorded` | ported | port_rec_top_level_self_call |
| `satin/write_records.rs` | `test_inspector_answered_call_counts_nothing_of_its_own` | not-portable | other: a custom inspector answering calls |
| `satin/write_records.rs` | `test_every_kept_write_pays_one_record_of_history` | covered | history = kv x 3,520 + other bytes holds in every scenario's record (ctl_sstore, s3_*, i6_value_call_refused, h01_hint_forwarded, p6_history_*) |
| `satin/write_records.rs` | `test_an_inspector_answered_value_call_keeps_its_history_charged` | not-portable | other: a custom inspector answering value calls |
| `satin/write_records.rs` | `test_the_kv_count_never_weighs_more_than_the_data_size` | not-portable | internal api (a per-instruction invariant read by an inspector) |
| `system/access_control.rs` | `test_inner_call_timestamp_reverts` | covered | a02_child_refused_parent_reenables |
| `system/access_control.rs` | `test_caller_frame_is_restricted` | covered | a01_disable_then_timestamp_refused |
| `system/access_control.rs` | `test_inner_call_without_volatile_access_succeeds` | covered | a14_grandchild_refused (B, below the switched-off A, reads nothing and returns normally) |
| `system/access_control.rs` | `test_volatile_opcodes_all_revert_in_inner_call` | covered | bd_timestamp/number/coinbase/prevrandao/gaslimit/basefee/blobbasefee/slotnum/blockhash_static_at (refused in a probe frame below A); the BLOBHASH-not-refused row is not a scenario |
| `system/access_control.rs` | `test_revert_data_contains_error_with_access_type` | covered | bd_*_static_at outputs (VolatileDataAccessDisabled(accessType) revert data) |
| `system/access_control.rs` | `test_a_slotnum_refusal_names_access_type_12` | covered | a11b_slotnum_refused, bd_slotnum_static_at |
| `system/access_control.rs` | `test_nested_call_volatile_access_reverts` | covered | a14_grandchild_refused |
| `system/access_control.rs` | `test_staticcall_also_restricted` | covered | fz mode 10 (probe frames reached by STATICCALL with access off) |
| `system/access_control.rs` | `test_delegatecall_also_restricted` | ported | port_acl_delegatecall_restricted |
| `system/access_control.rs` | `test_balance_non_beneficiary_not_restricted` | ported | port_acl_non_beneficiary_balance |
| `system/access_control.rs` | `test_extcodesize_non_beneficiary_not_restricted` | ported | port_acl_non_beneficiary_extcodesize |
| `system/access_control.rs` | `test_balance_beneficiary_restricted` | covered | a06_beneficiary_balance_refused, bd_balance_static_* |
| `system/access_control.rs` | `test_call_beneficiary_restricted` | covered | a22_call/callcode/delegatecall/staticcall_beneficiary_refused |
| `system/access_control.rs` | `test_call_non_beneficiary_not_restricted` | covered | a02_child_refused_parent_reenables, a14_grandchild_refused (calls of other accounts with access off proceed) |
| `system/access_control.rs` | `test_call_beneficiary_not_restricted_without_disable` | covered | q53_beneficiary_call_detains |
| `system/access_control.rs` | `test_selfdestruct_beneficiary_restricted` | covered | a16_selfdestruct_to_beneficiary_refused, bd_selfdestruct_to_* |
| `system/access_control.rs` | `test_selfdestruct_non_beneficiary_not_restricted` | ported | port_acl_non_beneficiary_selfdestruct |
| `system/access_control.rs` | `test_selfdestruct_beneficiary_not_restricted_without_disable` | covered | edge_sd_beneficiary_history_paid, edge_sd_beneficiary_kv_stop |
| `system/access_control.rs` | `test_a_beneficiary_frame_with_access_off_creates` | ported | port_acl_beneficiary_off_creates, port_acl_beneficiary_off_create2 |
| `system/access_control.rs` | `test_a_value_call_from_a_beneficiary_frame_with_access_off_moves_the_value` | ported | port_acl_beneficiary_off_value_call |
| `system/access_control.rs` | `test_a_beneficiary_frame_with_access_off_is_refused_its_own_account` | ported | port_acl_beneficiary_off_own_selfbalance, port_acl_beneficiary_off_own_balance_self, port_acl_beneficiary_off_own_extcodesize_self |
| `system/access_control.rs` | `test_a_creation_at_the_beneficiary_is_no_read` | ported | port_acl_creation_at_beneficiary_on, port_acl_creation_at_beneficiary_off |
| `system/access_control.rs` | `test_blocked_volatile_access_does_not_set_bitmap` | ported | port_acl_blocked_sets_no_bitmap |
| `system/access_control.rs` | `test_oracle_sload_reverts_when_volatile_access_disabled` | covered | a07_oracle_read_refused, bd_oracle_sload_static_* |
| `system/access_control.rs` | `test_create_reverts_when_volatile_access_disabled` | ported | port_acl_create_reads_while_disabled |
| `system/access_control.rs` | `test_parent_accesses_volatile_then_child_restricted` | ported | port_acl_parent_read_child_restricted |
| `system/access_control.rs` | `test_sibling_call_not_restricted` | ported | port_acl_sibling_returns |
| `system/access_control.rs` | `test_disable_in_reverted_child_does_not_affect_sibling` | ported | port_acl_sibling_reverts |
| `system/access_control.rs` | `test_disable_in_halted_child_does_not_affect_sibling` | ported | port_acl_sibling_halts, port_acl_sibling_runs_out_of_gas |
| `system/access_control.rs` | `test_the_switch_does_not_carry_into_the_next_transaction` | covered | blk_access_switch_not_carried_t1..t4 |
| `system/access_control.rs` | `test_enable_after_disable_succeeds` | covered | a02_child_refused_parent_reenables |
| `system/access_control.rs` | `test_enable_by_child_reverts_when_parent_disabled` | covered | a03_disabled_by_parent |
| `system/access_control.rs` | `test_enable_when_not_disabled_is_noop` | covered | fz mode 3 (enableVolatileDataAccess called with access on) |
| `system/access_control.rs` | `test_staticcall_disable_volatile_data_access_is_intercepted` | ported | port_acl_staticcall_disable_intercepted |
| `system/access_control.rs` | `test_the_query_answers_for_the_caller_at_every_depth` | ported | port_acl_query_every_depth |
| `system/access_control.rs` | `test_delegatecall_to_access_control_not_intercepted` | covered | a10_delegatecall_not_intercepted (the CALLCODE row is not a scenario) |
| `system/access_control.rs` | `test_the_switch_is_the_same_below_and_above_the_execution_cap` | ported | port_acl_cap_below, port_acl_cap_above |
| `system/control.rs` | `test_the_three_methods_are_intercepted` | covered | a01_disable_then_timestamp_refused, a02_child_refused_parent_reenables, a05_is_disabled |
| `system/control.rs` | `test_a_direct_transaction_is_intercepted` | covered | a08_direct_disable, edge_sweep_tx0_funded_system_v0_runs |
| `system/control.rs` | `test_a_static_call_is_intercepted` | ported | port_ctl_staticcall_0, port_ctl_staticcall_1, port_ctl_staticcall_2 |
| `system/control.rs` | `test_a_value_bearing_call_is_refused` | covered | a09_value_refused, edge_sweep_tx0_funded_system_v5_runs |
| `system/control.rs` | `test_a_value_bearing_call_to_an_unknown_selector_falls_through` | ported | port_ctl_value_unknown_selector |
| `system/control.rs` | `test_an_unknown_selector_falls_through_to_not_intercepted` | covered | a13_unknown_selector_falls_through (the input-shorter-than-a-selector row is not a scenario) |
| `system/control.rs` | `test_a_selector_with_trailing_bytes_is_intercepted` | ported | port_ctl_trailing_access_0, port_ctl_trailing_access_28, port_ctl_trailing_access_100 |
| `system/control.rs` | `test_callcode_and_delegatecall_are_not_intercepted` | covered | a10_delegatecall_not_intercepted, i4_limit_callcode_not_intercepted (the scheme guard) |
| `system/control.rs` | `test_the_address_is_what_the_interceptor_matches` | covered | a01-a05 (MegaAccessControl's address holds no code in these scenarios: answered by address) |
| `system/dispatch.rs` | `test_an_intercepted_call_costs_what_a_plain_call_costs` | covered | i1_limit_call, a05_is_disabled (the dispatch charges nothing beyond the CALL) |
| `system/dispatch.rs` | `test_the_forwarded_gas_of_an_intercepted_call_comes_back` | covered | i1_limit_call (forward 500,000), rev_child_return_interceptor (forward 100,000) |
| `system/dispatch.rs` | `test_admission_is_the_selector_alone` | ported | port_ctl_trailing_limit_0, port_ctl_trailing_limit_28, … (5 scenarios, `port_ctl_*`) |
| `system/dispatch.rs` | `test_the_address_admits_the_dispatch` | ported | port_ctl_ordinary_contract_21, port_ctl_ordinary_contract_27 |
| `system/dispatch.rs` | `test_an_inspector_sees_an_intercepted_call` | covered | i1_limit_call (the frames field records the intercepted call) |
| `system/dispatch.rs` | `test_a_database_error_reading_the_system_address_fails_the_transaction` | not-portable | db fault (the system address's account read) |
| `system/dispatch.rs` | `test_a_database_error_reading_the_registry_fails_the_transaction` | not-portable | db fault (the registry's account or slot read) |
| `system/dispatch.rs` | `test_a_value_bearing_transaction_is_refused_before_the_value_moves` | covered | edge_sweep_tx0_funded_system_v5_runs (value tx to MegaAccessControl refused) |
| `system/dispatch.rs` | `test_a_nested_static_call_is_intercepted` | ported | port_ctl_nested_static |
| `system/dispatch.rs` | `test_a_transaction_that_touches_no_system_contract_is_untouched` | covered | ctl_sstore and every plain scenario |
| `system/dispatch.rs` | `test_the_contracts_without_an_interceptor_run_their_bytecode` | covered | fz mode 5 (calls to the SequencerRegistry holding its trusted code), rev_oracle_write_through_timestamp_wrapper_unnamed (High-Precision Timestamp code) |
| `system/keyless/charges.rs` | `test_every_charge_of_a_deployment_is_accounted_for` | covered | k01_ok (below the cap), k19_above_cap (above it) |
| `system/keyless/charges.rs` | `test_an_empty_signer_is_charged_its_account_once` | covered | k01_ok (signer absent) vs k09_value_funded_signer, k16b_signer_funded_nonce_zero |
| `system/keyless/charges.rs` | `test_the_created_account_is_charged_only_when_the_address_is_empty` | covered | rev_keyless_balance_only_destination vs rev_keyless_empty_destination |
| `system/keyless/charges.rs` | `test_a_refused_call_keeps_the_overhead_alone` | covered | k12_malformed_call, k13_call_value_refused, k10_nonzero_tx_nonce |
| `system/keyless/charges.rs` | `test_a_failed_deployment_gives_the_created_account_back` | covered | k02_revert, k03_halt |
| `system/keyless/charges.rs` | `test_the_call_pays_the_create_opcodes_regular_gas` | ported | port_kdeploy_init_max, port_kdeploy_init_over, port_kdeploy_regular_gas_empty_init, port_kdeploy_regular_gas_ret1 |
| `system/keyless/charges.rs` | `test_gas_used_counts_the_creations_spend_before_its_refund` | ported | port_kdeploy_refund_before_gas_used |
| `system/keyless/charges.rs` | `test_the_upfront_charges_scale_with_their_buckets` | covered | k23_salt_m8 (signer m=8, deploy address m=2) |
| `system/keyless/charges.rs` | `test_a_failed_deployment_gives_the_created_account_back_at_its_price` | ported | port_kdeploy_failed_gives_back_at_price |
| `system/keyless/charges.rs` | `test_a_signer_that_sends_its_own_deployment_makes_no_record_of_its_own` | covered | edge_keyless_signer_is_sender_ok/_revert/_halt/_value/_gas_price, edge_keyless_signer_at_nonce_1_other_sender |
| `system/keyless/charges.rs` | `test_the_creation_takes_the_reservoir_share_of_what_the_call_has_left` | covered | k19_above_cap (the frames field records the creation's reservoir share) |
| `system/keyless/deploy.rs` | `test_keyless_deploy_create2_factory` | ported | port_kdeploy_create2_factory_below, port_kdeploy_create2_factory_above |
| `system/keyless/deploy.rs` | `test_keyless_deploy_eip1820` | ported | port_kdeploy_registry_sized |
| `system/keyless/deploy.rs` | `test_the_deploy_address_is_the_signers_first_creation_address` | covered | k01_ok, k1_keyless_deploy |
| `system/keyless/deploy.rs` | `test_a_deployment_survives_a_commit` | covered | blk_keyless_twice_t1/t2 |
| `system/keyless/deploy.rs` | `test_keyless_deploy_execution_reverted` | covered | k02_revert |
| `system/keyless/deploy.rs` | `test_keyless_deploy_execution_halted_invalid_opcode` | covered | k03_halt |
| `system/keyless/deploy.rs` | `test_a_constructor_out_of_gas_is_execution_halted` | ported | port_kdeploy_constructor_oog_below, port_kdeploy_constructor_oog_above |
| `system/keyless/deploy.rs` | `test_a_halted_deployment_is_paid_for_and_spends_the_nonce` | covered | k03_halt |
| `system/keyless/deploy.rs` | `test_keyless_deploy_empty_initcode` | covered | k04_empty_code |
| `system/keyless/deploy.rs` | `test_an_empty_code_deployment_leaves_the_address_deployable` | ported | port_kdeploy_empty_then_deploys_t1, port_kdeploy_empty_then_deploys_t2 |
| `system/keyless/deploy.rs` | `test_a_deploy_address_with_a_nonce_and_no_code_collides` | covered | bd_keyless_deploy_address_with_nonce |
| `system/keyless/deploy.rs` | `test_a_constructors_own_creation_may_return_empty_code` | ported | port_kdeploy_constructor_child_empty |
| `system/keyless/deploy.rs` | `test_keyless_deploy_init_code_selfdestructs` | ported | port_kdeploy_selfdestructs_to_beneficiary |
| `system/keyless/deploy.rs` | `test_a_constructor_destroyed_through_delegatecall_deploys_no_code` | ported | port_kdeploy_destroyed_via_delegatecall |
| `system/keyless/deploy.rs` | `test_code_starting_with_ef_is_refused` | ported | port_kdeploy_code_ef |
| `system/keyless/deploy.rs` | `test_code_over_the_size_limit_is_refused` | ported | port_kdeploy_code_over_limit |
| `system/keyless/deploy.rs` | `test_keyless_deploy_with_value_transfer` | covered | k09_value_funded_signer |
| `system/keyless/deploy.rs` | `test_keyless_deploy_modifies_other_contract_state` | ported | port_kdeploy_modifies_other_state |
| `system/keyless/deploy.rs` | `test_keyless_deploy_creates_child_contract` | covered | k17_child_creation |
| `system/keyless/deploy.rs` | `test_keyless_deploy_reads_existing_contract` | ported | port_kdeploy_reads_existing |
| `system/keyless/deploy.rs` | `test_failed_initcode_keeps_no_storage` | ported | port_kdeploy_failed_init_no_storage |
| `system/keyless/deploy.rs` | `test_keyless_deploy_emits_logs` | covered | k05_writes_logs |
| `system/keyless/deploy.rs` | `test_an_empty_code_deployment_keeps_no_log` | ported | port_kdeploy_empty_code_no_log |
| `system/keyless/deploy.rs` | `test_an_empty_code_deployment_adds_no_log` | covered | k04_empty_code |
| `system/keyless/deploy.rs` | `test_a_failed_deployment_keeps_no_log` | ported | port_kdeploy_failed_no_log |
| `system/keyless/deploy.rs` | `test_origin_and_gasprice_follow_the_outer_transaction` | covered | k1_keyless_deploy (init stores ORIGIN, GASPRICE and CALLER); a non-zero relayer gas price is not a scenario |
| `system/keyless/deploy.rs` | `test_beneficiary_receives_fees_on_success` | covered | edge_keyless_signer_is_sender_gas_price (priced keyless deployment, fees to the beneficiary) |
| `system/keyless/deploy.rs` | `test_beneficiary_receives_fees_on_execution_failure` | ported | port_kdeploy_beneficiary_fees_on_failure |
| `system/keyless/deploy.rs` | `test_keyless_deploy_not_intercepted_for_inner_calls` | covered | k18_from_a_contract_not_dispatched |
| `system/keyless/deploy.rs` | `test_keyless_deploy_nested_inside_a_keyless_deployment_is_not_intercepted` | ported | port_kdeploy_nested_keyless |
| `system/keyless/deploy.rs` | `test_a_deployment_is_priced_by_the_transactions_salt_env` | covered | k23_salt_m8 |
| `system/keyless/deploy.rs` | `test_a_constructor_reaches_the_oracle_service` | ported | port_kdeploy_constructor_hint |
| `system/keyless/deploy.rs` | `test_a_deployment_is_counted_where_it_writes` | covered | k05_writes_logs (signer nonce, created account, slot, code counted) |
| `system/keyless/deploy.rs` | `test_a_retry_does_not_pay_for_the_signer_again` | covered | blk_keyless_twice_t2 (the signer's account exists for the retry) |
| `system/keyless/deploy.rs` | `test_a_failed_keyless_transaction_leaves_nothing_for_the_next` | not-portable | db fault (an error inside a keyless deployment, then EVM reuse) |
| `system/keyless/deploy.rs` | `test_a_deployment_carrying_value_logs_the_move` | covered | k09_value_funded_signer (transfer log of the endowment), edge_keyless_signer_is_sender_value |
| `system/keyless/deploy.rs` | `test_a_creation_is_stopped_on_its_transfer_log_before_the_value_moves` | ported | port_kdeploy_value_transfer_log_holds_records_not_log, port_kdeploy_value_transfer_log_ample |
| `system/keyless/deploy.rs` | `test_a_destruction_an_earlier_transaction_made_is_not_the_deployments` | ported | port_kdeploy_earlier_destruction_t1, port_kdeploy_earlier_destruction_t2, port_kdeploy_earlier_destruction_t3 |
| `system/keyless/detention.rs` | `test_a_signer_that_is_the_beneficiary_detains_its_deployment` | covered | k25_signer_is_beneficiary, k25b_signer_is_beneficiary_tight |
| `system/keyless/detention.rs` | `test_a_constructor_that_reads_volatile_data_is_held_to_the_cap` | covered | k06_volatile_detained, k06b_volatile_crossing (constructor reads NUMBER under a cap) |
| `system/keyless/detention.rs` | `test_a_sender_that_is_the_beneficiary_holds_the_whole_deployment_to_the_cap` | ported | port_kdet_sender_is_beneficiary_20000, port_kdet_sender_is_beneficiary_60000, … (6 scenarios, `port_kdet_sender_is_beneficiary_*`) |
| `system/keyless/detention.rs` | `test_a_sender_that_is_the_beneficiary_is_held_before_the_overhead` | covered | edge_keyless_value_detained_sender, edge_keyless_value_detained_deposit (cap below the overhead) |
| `system/keyless/detention.rs` | `test_a_deployment_in_a_subtree_with_volatile_access_off_is_refused_its_reads` | not-portable | internal api (with_volatile_access_disabled_from on the keyless call's own frame; no transaction can switch it off above that frame) |
| `system/keyless/detention.rs` | `test_a_constructor_that_switches_volatile_access_off_is_refused_its_own_read` | ported | port_kdet_ctor_disable_then_timestamp_plain, port_kdet_ctor_disable_then_timestamp_capped, port_kdet_ctor_disable_then_timestamp_as_beneficiary |
| `system/keyless/detention.rs` | `test_remaining_compute_gas_answers_a_constructor_its_allowance` | ported | port_kdet_ctor_remaining_gas_plain, port_kdet_ctor_remaining_gas_capped, … (6 scenarios, `port_kdet_ctor_remaining_gas_*`) |
| `system/keyless/detention.rs` | `test_remaining_compute_gas_in_a_constructor_is_the_cap_less_the_deployments_charges` | ported | port_kdet_ctor_remaining_gas_after_read_plain, port_kdet_ctor_remaining_gas_after_read_capped, port_kdet_ctor_remaining_gas_after_read_as_beneficiary |
| `system/keyless/detention.rs` | `test_a_constructor_reads_the_oracle_as_any_frame_does` | ported | port_kdet_ctor_oracle_read_plain, port_kdet_ctor_oracle_read_capped, port_kdet_ctor_oracle_read_as_beneficiary |
| `system/keyless/detention.rs` | `test_a_constructors_reads_are_the_transactions` | ported | port_kdet_ctor_reads_block_plain, port_kdet_ctor_reads_block_capped, port_kdet_ctor_reads_block_as_beneficiary |
| `system/keyless/detention.rs` | `test_a_failed_deployment_keeps_the_reads_its_creation_made` | ported | port_kdet_ctor_timestamp_empty_plain, port_kdet_ctor_timestamp_empty_capped, port_kdet_ctor_timestamp_empty_as_beneficiary |
| `system/keyless/detention.rs` | `test_a_deployment_another_limit_stops_keeps_the_reads_its_creation_made` | ported | port_kdet_ctor_timestamp_then_fill_no_room_plain, port_kdet_ctor_timestamp_then_fill_no_room_capped, port_kdet_ctor_timestamp_then_fill_no_room_as_beneficiary |
| `system/keyless/differential.rs` | `test_keyless_differential_record` | not-portable | snapshot (digest of a four-mode inspector record of keyless scenarios) |
| `system/keyless/dispatch.rs` | `test_a_keyless_deploy_transaction_pays_the_overhead` | covered | k12_malformed_call |
| `system/keyless/dispatch.rs` | `test_a_value_bearing_call_is_refused` | covered | k13_call_value_refused, k2_keyless_with_value_refused |
| `system/keyless/dispatch.rs` | `test_a_call_from_a_contract_is_not_dispatched` | covered | k18_from_a_contract_not_dispatched |
| `system/keyless/dispatch.rs` | `test_an_unknown_selector_is_not_dispatched` | ported | port_kframe_not_dispatched_unknown, port_kframe_not_dispatched_three_bytes |
| `system/keyless/dispatch.rs` | `test_a_call_that_cannot_pay_the_overhead_runs_out_of_gas` | covered | k20_overhead_out_of_gas |
| `system/keyless/dispatch.rs` | `test_the_overhead_does_not_depend_on_the_payload` | covered | k12_malformed_call, k24_not_preeip155, edge_keyless_offset_u64_max, edge_keyless_offset_past_input (refused payloads, one overhead) |
| `system/keyless/dispatch.rs` | `test_a_truncated_payload_is_still_dispatched` | covered | k12_malformed_call (calldata truncated to 40 bytes) |
| `system/keyless/frame.rs` | `test_the_call_is_a_frame_on_revms_stack` | not-portable | internal api (drives revm's frame loop by hand) |
| `system/keyless/frame.rs` | `test_the_inspector_sees_one_call_frame_in_which_nothing_runs` | covered | k01_ok, k12_malformed_call (the frames field: one call frame, the creation its child) |
| `system/keyless/frame.rs` | `test_the_contracts_bytecode_never_runs_for_a_dispatched_call` | ported | port_kframe_bytecode_not_run_dispatched, port_kframe_bytecode_not_run_refused, port_kframe_bytecode_runs_other_selector, port_kframe_contract_call_not_dispatched |
| `system/keyless/frame.rs` | `test_a_call_is_dispatched_where_the_state_holds_no_contract_code` | covered | k01_ok (no code at the KeylessDeploy address) |
| `system/keyless/frame.rs` | `test_a_call_where_the_state_holds_no_contract_code_commits_nothing_for_it` | covered | k01_ok (the absent contract account is not in the changed accounts) |
| `system/keyless/frame.rs` | `test_a_call_carrying_value_is_refused_before_its_start_is_counted` | ported | port_kframe_value_refused_holds_body, port_kframe_value_refused_ample |
| `system/keyless/frame.rs` | `test_the_call_touches_the_contract_as_any_call_does` | ported | port_kframe_touches_contract |
| `system/keyless/interface.rs` | `test_the_interface_declares_exactly_the_documented_errors` | not-portable | internal api (IKeylessDeploy errors against the spec page) |
| `system/keyless/inspector.rs` | `test_the_inspector_sees_the_call_and_the_creation_it_starts` | covered | k01_ok, k05_writes_logs (the frames field: the call, then the creation one level below) |
| `system/keyless/inspector.rs` | `test_a_refused_call_is_seen_as_a_call` | covered | k12_malformed_call, k13_call_value_refused (frames: one call, no creation) |
| `system/keyless/inspector.rs` | `test_a_failed_deployment_ends_as_a_revert_in_the_trace` | covered | k02_revert (frames: the creation reverts, the call succeeds) |
| `system/keyless/inspector.rs` | `test_a_creation_an_inspector_answers_keeps_what_its_start_was_charged` | not-portable | other: a custom inspector that answers the creation |
| `system/keyless/inspector.rs` | `test_an_answered_creation_gives_the_signers_account_back_at_its_price` | not-portable | other: a custom inspector that answers every creation with a revert |
| `system/keyless/inspector.rs` | `test_a_rejected_transaction_is_seen_as_nothing` | ported | port_kframe_rejected_tx |
| `system/keyless/inspector.rs` | `test_a_tracer_sees_the_call_and_the_creation_it_starts` | not-portable | tracer output (parity and geth call traces) |
| `system/keyless/inspector.rs` | `test_a_revived_keyless_creation_answers_address_mismatch` | not-portable | other: a custom inspector reviving failed creations |
| `system/keyless/inspector.rs` | `test_a_revived_keyless_creation_a_limit_stopped_answers_address_mismatch` | not-portable | other: a custom inspector reviving a stopped creation |
| `system/keyless/inspector.rs` | `test_an_inspector_sees_an_empty_code_deployment_end_as_a_revert` | covered | k04_empty_code (frames: the creation ends as a revert) |
| `system/keyless/inspector.rs` | `test_an_inspector_that_would_halt_the_calls_frame_is_never_handed_it` | not-portable | other: a custom inspector that would halt the call's frame |
| `system/keyless/limits.rs` | `test_a_deployment_crossing_its_data_size_budget_reverts_alone` | ported | port_klim_data_log_room, port_klim_data_no_room, port_klim_data_start_only_one_record |
| `system/keyless/limits.rs` | `test_the_creations_start_crossing_the_limit_stops_the_creation` | ported | port_klim_start_crossing |
| `system/keyless/limits.rs` | `test_the_signers_record_can_put_the_call_over_its_budget` | ported | port_klim_signer_record_budget_0, port_klim_signer_record_budget_20, port_klim_signer_record_budget_40, port_klim_signer_record_budget_80 |
| `system/keyless/limits.rs` | `test_a_hint_the_creation_sent_leaves_the_signers_record_over_the_transaction_limit` | ported | port_klim_hint_then_fail_0, port_klim_hint_then_fail_40, port_klim_hint_then_fail_400 |
| `system/keyless/limits.rs` | `test_a_failure_from_nonce_one_leaves_the_call_within_its_budget` | ported | port_klim_980_n1_above, port_klim_980_n1_below, … (6 scenarios) |
| `system/keyless/limits.rs` | `test_a_new_signer_and_its_deploy_address_are_state_growth` | ported | port_klim_growth_8, port_klim_growth_9, port_klim_growth_8_existing, port_klim_growth_9_existing |
| `system/keyless/limits.rs` | `test_a_deployment_within_every_limit_deploys` | covered | k01_ok, k05_writes_logs (default limits) |
| `system/keyless/limits.rs` | `test_a_refusal_after_the_charges_gives_the_reservoir_back` | ported | port_klim_refusal_gives_back_250000000, port_klim_refusal_gives_back_300000000, port_klim_refusal_gives_back_400000000 |
| `system/keyless/limits.rs` | `test_a_refusal_after_the_creations_charges_gives_the_reservoir_back` | ported | port_klim_refusal_gives_back_250000000, port_klim_refusal_gives_back_300000000, port_klim_refusal_gives_back_400000000 |
| `system/keyless/limits.rs` | `test_the_unspent_forward_comes_back_on_success_and_revert_alike` | covered | k01_ok, k02_revert (below the cap), k19_above_cap |
| `system/keyless/limits.rs` | `test_every_limit_stops_a_deployment_from_either_nonce_at_either_tier` | ported | port_klim_every_data_n0_below, port_klim_every_data_n0_above, … (16 scenarios, `port_klim_every_*`) |
| `system/keyless/limits.rs` | `test_a_deployment_is_counted_but_not_stopped_under_the_exemption` | not-portable | internal api (keyless deployment run as a system call) |
| `system/keyless/precedence.rs` | `test_a_call_two_rules_refuse_reports_the_first_the_legacy_engine_checked` | ported | port_kprec_two_rules_nonce_and_address_5m, port_kprec_two_rules_nonce_and_address_300m, … (6 scenarios, `port_kprec_two_rules_*`) |
| `system/keyless/precedence.rs` | `test_a_value_bearing_call_that_does_not_decode_is_refused_for_the_value` | ported | port_kprec_value_not_decoding, port_kprec_value_bad_tx |
| `system/keyless/precedence.rs` | `test_the_signers_account_and_the_forward_come_before_the_address_and_the_balance` | ported | port_kprec_forward_first_below_occupied, port_kprec_forward_first_below_plain |
| `system/keyless/precedence.rs` | `test_the_forward_comes_before_the_address_and_the_balance_above_the_cap` | ported | port_kprec_forward_first_above_occupied, port_kprec_forward_first_above_plain |
| `system/keyless/precedence.rs` | `test_the_creations_charges_can_refuse_a_call_no_rule_refuses` | ported | port_kprec_creation_charges_140000, port_kprec_creation_charges_160000, … (6 scenarios, `port_kprec_creation_charges_*`) |
| `system/keyless/precedence.rs` | `test_an_out_of_gas_at_the_creations_charges_consumes_the_regular_gas_alone` | ported | port_kprec_oog_creation_absent_30000, port_kprec_oog_creation_one_wei_30000, … (12 scenarios, `port_kprec_oog_creation_*`) |
| `system/keyless/rules.rs` | `test_keyless_deploy_gas_limit_too_low` | covered | k11_override_below_tx_gas |
| `system/keyless/rules.rs` | `test_keyless_deploy_gas_limit_exactly_equal` | ported | port_krule_override_equals_signed |
| `system/keyless/rules.rs` | `test_a_forward_capped_below_the_signed_gas_limit_is_refused` | ported | port_krule_forward_150000, port_krule_forward_200000, port_krule_forward_250000 |
| `system/keyless/rules.rs` | `test_a_forward_at_the_signed_gas_limit_deploys` | ported | port_krule_forward_300000, port_krule_forward_400000 |
| `system/keyless/rules.rs` | `test_the_signers_account_can_take_the_forward_below_the_signed_gas_limit` | ported | port_krule_forward_absent_signer_150000, port_krule_forward_absent_signer_200000, … (5 scenarios, `port_krule_forward_absent_signer_*`) |
| `system/keyless/rules.rs` | `test_keyless_deploy_invalid_signature` | ported | port_krule_r_over_order |
| `system/keyless/rules.rs` | `test_keyless_deploy_rejects_corrupted_signature` | ported | port_krule_r_zero |
| `system/keyless/rules.rs` | `test_keyless_deploy_modified_s_changes_signer` | ported | port_krule_modified_s |
| `system/keyless/rules.rs` | `test_keyless_deploy_malformed_encoding` | covered | k12_malformed_call, edge_keyless_offset_u64_max |
| `system/keyless/rules.rs` | `test_keyless_deploy_rejects_eip2718_typed_envelope` | ported | port_krule_typed_envelope |
| `system/keyless/rules.rs` | `test_keyless_deploy_not_contract_creation` | ported | port_krule_not_creation |
| `system/keyless/rules.rs` | `test_keyless_deploy_not_pre_eip155` | covered | k24_not_preeip155 |
| `system/keyless/rules.rs` | `test_keyless_deploy_non_zero_tx_nonce` | covered | k10_nonzero_tx_nonce |
| `system/keyless/rules.rs` | `test_init_code_over_the_limit_is_refused` | ported | port_krule_init_over_5m |
| `system/keyless/rules.rs` | `test_init_code_at_the_limit_is_admitted` | ported | port_krule_init_max_5m |
| `system/keyless/rules.rs` | `test_keyless_deploy_signer_nonce_too_high` | covered | k15_signer_nonce_too_high |
| `system/keyless/rules.rs` | `test_failing_attempts_by_anybody_leave_the_address_deployable` | ported | port_krule_failing_attempts_t1, port_krule_failing_attempts_t2, port_krule_failing_attempts_t3 |
| `system/keyless/rules.rs` | `test_a_resubmission_after_failures_and_a_deployment_finds_the_address_taken` | covered | blk_keyless_twice_t2 (ContractAlreadyExists on the resubmission) |
| `system/keyless/rules.rs` | `test_a_success_from_nonce_one_leaves_the_nonce_at_one` | covered | edge_keyless_signer_at_nonce_1_other_sender, k16_signer_nonce_one_taken_back |
| `system/keyless/rules.rs` | `test_a_failure_from_nonce_one_keeps_no_record_of_the_signer` | ported | port_krule_nonce_one_revert |
| `system/keyless/rules.rs` | `test_every_way_a_deployment_from_nonce_one_fails_leaves_the_nonce_at_one` | ported | port_krule_nonce_one_revert, port_krule_nonce_one_halt, port_krule_nonce_one_empty, port_krule_nonce_one_stopped_at_start |
| `system/keyless/rules.rs` | `test_a_signer_that_sends_its_failing_deployment_ends_at_nonce_one` | covered | edge_keyless_signer_is_sender_revert, edge_keyless_signer_is_sender_halt |
| `system/keyless/rules.rs` | `test_a_signer_with_code_is_refused` | covered | k16c_signer_has_code, port_keyless_signer_code_3607_on |
| `system/keyless/rules.rs` | `test_disabling_eip3607_disables_the_signer_code_rule` | ported | port_keyless_signer_code_3607_on, port_keyless_signer_code_3607_off |
| `system/keyless/rules.rs` | `test_a_disabled_eip3607_reads_no_signer_code` | covered | port_keyless_signer_code_3607_off (the read set holds no signer code) |
| `system/keyless/rules.rs` | `test_a_delegated_signer_deploys` | covered | port_keyless_signer_delegated |
| `system/keyless/rules.rs` | `test_nonces_a_delegated_signer_spends_in_its_deployment_stay_spent` | ported | port_krule_delegated_signer_creates_twice |
| `system/keyless/rules.rs` | `test_a_delegated_signer_whose_creation_reverts_keeps_every_bump` | ported | port_krule_delegated_signer_reverting_create |
| `system/keyless/rules.rs` | `test_keyless_deploy_contract_already_exists` | covered | k14_contract_already_exists |
| `system/keyless/rules.rs` | `test_keyless_deploy_twice_fails_second_time` | covered | blk_keyless_twice_t1/t2 |
| `system/keyless/rules.rs` | `test_keyless_deploy_insufficient_balance_zero` | covered | k08_value_unfunded_signer |
| `system/keyless/rules.rs` | `test_keyless_deploy_insufficient_balance_partial` | ported | port_krule_balance_partial |
| `system/keyless/rules.rs` | `test_keyless_deploy_balance_exactly_sufficient` | ported | port_krule_balance_exact |
| `system/keyless/rules.rs` | `test_a_signer_with_nothing_deploys_what_carries_no_value` | covered | k01_ok (absent signer deploys) |
| `system/keyless/rules.rs` | `test_a_call_below_the_overhead_reads_nothing` | covered | k20_overhead_out_of_gas (read set without the signer) |
| `system/keyless/rules.rs` | `test_a_call_that_cannot_pay_the_signers_account_runs_out_of_gas` | ported | port_krule_cannot_pay_signer_account_60000, port_krule_cannot_pay_signer_account_80000, … (5 scenarios, `port_krule_cannot_pay_signer_account_*`) |
| `system/keyless/rules.rs` | `test_a_call_refused_after_its_charges_keeps_their_regular_gas_alone` | ported | port_krule_refused_after_charges_150000, port_krule_refused_after_charges_170000, … (5 scenarios, `port_krule_refused_after_charges_*`) |
| `system/keyless/rules.rs` | `test_the_signers_account_is_charged_exactly_when_its_nonce_is_spent` | covered | k13_call_value_refused, k10_nonzero_tx_nonce (refused: no charge) vs k02_revert, k03_halt (failed at run time: charged) |
| `system/keyless/rules.rs` | `test_a_failed_read_of_the_deploy_address_fails_the_transaction` | not-portable | db fault (the deploy address's read) |
| `system/keyless/rules.rs` | `test_the_occupancy_read_needs_no_code` | not-portable | db fault (a database missing the deploy address's bytecode) |
| `system/keyless/rules.rs` | `test_a_failed_read_of_the_signer_fails_the_transaction` | not-portable | db fault (the signer's read) |
| `system/keyless/rules.rs` | `test_a_failed_salt_lookup_fails_the_transaction` | not-portable | db fault (a failing SALT lookup) |
| `system/limit_control.rs` | `test_remaining_compute_gas_is_intercepted` | covered | i1_limit_call, i2_limit_staticcall |
| `system/limit_control.rs` | `test_a_value_bearing_call_is_refused` | covered | i6_value_call_refused |
| `system/limit_control.rs` | `test_an_unknown_selector_falls_through_to_not_intercepted` | covered | i5_unknown_selector_falls_through (the input-shorter-than-a-selector row is not a scenario) |
| `system/limit_control.rs` | `test_a_selector_with_trailing_bytes_is_intercepted` | ported | port_ctl_trailing_limit_0, port_ctl_trailing_limit_28, port_ctl_trailing_limit_100 |
| `system/limit_control.rs` | `test_callcode_and_delegatecall_are_not_intercepted` | covered | i3_limit_delegatecall_not_intercepted, i4_limit_callcode_not_intercepted |
| `system/oracle.rs` | `test_send_hint_reaches_the_oracle_service` | covered | h08_direct_hint (hints field) |
| `system/oracle.rs` | `test_the_hint_payload_is_counted_on_the_transaction` | ported | port_orcl_payload_counted_0, port_orcl_payload_counted_7, port_orcl_payload_counted_64 |
| `system/oracle.rs` | `test_a_malformed_payload_is_paid_for_and_not_forwarded` | covered | h09_hint_undecodable_counted |
| `system/oracle.rs` | `test_a_static_call_forwards_the_hint` | covered | h07_hint_staticcall |
| `system/oracle.rs` | `test_a_value_bearing_call_forwards_nothing` | covered | h03_hint_with_value_not_intercepted |
| `system/oracle.rs` | `test_a_call_without_gas_forwards_nothing` | covered | fz mode 3 (sendHint calls forwarding 0 gas) |
| `system/oracle.rs` | `test_a_call_with_one_gas_forwards_the_hint_it_cannot_deliver` | ported | port_orcl_one_gas_hint |
| `system/oracle.rs` | `test_callcode_and_delegatecall_forward_nothing` | covered | fz mode 3 (DELEGATECALL of sendHint forwards nothing); the CALLCODE row is not a scenario |
| `system/oracle.rs` | `test_an_unknown_selector_runs_the_bytecode` | ported | port_orcl_unknown_selector |
| `system/oracle.rs` | `test_reading_a_slot_runs_the_bytecode` | ported | port_orcl_get_slot_runs_bytecode |
| `system/oracle.rs` | `test_a_reverting_frame_does_not_take_the_hint_bytes_back` | covered | h05_hint_in_reverted_child |
| `system/oracle.rs` | `test_a_hint_that_crosses_the_data_size_limit_is_not_forwarded` | ported | port_orcl_limit_crossing |
| `system/oracle.rs` | `test_a_hint_at_the_data_size_limit_is_forwarded` | ported | port_orcl_limit_at_limit |
| `system/oracle.rs` | `test_a_contracts_hint_reaches_the_service_from_the_contract` | covered | h01_hint_forwarded (hint from A) |
| `system/oracle.rs` | `test_the_oracles_code_elsewhere_forwards_no_hint` | ported | port_orcl_code_elsewhere |
| `system/oracle.rs` | `test_hints_reach_the_service_in_the_order_they_were_sent` | covered | h10_hints_fill_limit_then_write (two hints, in order) |
| `system/oracle.rs` | `test_a_malformed_or_padded_hint_is_counted_whole` | ported | port_orcl_malformed_plain, port_orcl_malformed_half, port_orcl_padded_plain, port_orcl_padded_half |
| `system/oracle.rs` | `test_consecutive_hints_add_up_on_the_transaction` | covered | h10_hints_fill_limit_then_write |
| `system/oracle.rs` | `test_data_size_overflow_blocks_forwarding_and_stops_the_call` | covered | h02_hint_over_data_limit |
| `system/oracle.rs` | `test_a_frame_that_switched_access_off_sends_no_hint` | covered | h04_hint_dropped_when_disabled |
| `system/oracle.rs` | `test_a_frame_with_access_on_sends_its_hint` | covered | h01_hint_forwarded (access on); the off-then-on row is not a scenario |
| `system/oracle.rs` | `test_a_child_of_a_frame_that_switched_access_off_sends_no_hint` | ported | port_orcl_child_after_disable |
| `system/oracle.rs` | `test_a_withheld_hint_is_not_counted` | ported | port_orcl_withheld_hint_not_counted |
| `system/oracle_storage.rs` | `test_oracle_storage_sload_uses_oracle_env` | covered | q54_oracle_answers_then_loop, h06_hint_then_oracle_read (the service's value over the chain's) |
| `system/oracle_storage.rs` | `test_oracle_storage_sload_fallback_to_database` | covered | q55_oracle_no_answer_loaded_value |
| `system/oracle_storage.rs` | `test_oracle_storage_sload_direct_call` | ported | port_ostore_direct_call |
| `system/oracle_storage.rs` | `test_oracle_sload_determinism_between_oracle_env_and_state` | covered | q54_oracle_answers_then_loop vs q55_oracle_no_answer_loaded_value (same cost either source) |
| `system/oracle_storage.rs` | `test_every_oracle_read_is_cold` | covered | q54, q55 (the Oracle reads slot 5 twice, both cold); the same code elsewhere (warm second read) is not a scenario |
| `system/oracle_storage.rs` | `test_an_oracle_read_is_detained_under_the_oracles_cap` | covered | q54_oracle_answers_then_loop (Oracle cap 60,000) |
| `system/oracle_storage.rs` | `test_a_hint_reaches_the_service_before_a_later_read` | covered | h06_hint_then_oracle_read |
| `system/oracle_storage.rs` | `test_a_read_outside_the_oracles_frame_is_an_ordinary_read` | ported | port_ostore_delegatecall_ordinary_read |
| `system/oracle_storage.rs` | `test_a_read_the_frame_cannot_pay_for_asks_nothing` | covered | fz mode 12 (the Oracle's SLOAD with frame gas around its cold access, access on) |
| `system/oracle_storage.rs` | `test_a_refused_read_asks_nothing` | covered | a07_oracle_read_refused, bd_oracle_sload_static_* |
| `system/oracle_storage.rs` | `test_the_reads_a_transaction_made_are_on_its_outcome` | covered | q54, q55, h06 (oracle_reads in the read set) |
| `system/oracle_storage.rs` | `test_two_answers_for_one_slot_replay_only_from_the_record` | not-portable | other: an oracle service whose answer for a slot changes after a hint (scenario answers are fixed) |
| `system/oracle_storage.rs` | `test_a_detained_frame_that_may_not_spend_the_cold_access_asks_and_is_stopped` | covered | q56_det_oracle_read_crosses |
| `system/oracle_storage.rs` | `test_a_write_after_a_read_costs_the_same_whichever_source_answered` | ported | port_ostore_read_then_write_answer_below, port_ostore_read_then_write_noanswer_below, port_ostore_read_then_write_answer_above, port_ostore_read_then_write_noanswer_above |
| `system/oracle_storage.rs` | `test_a_multicall_read_then_write_costs_the_same_whichever_source_answered` | ported | port_ostore_multicall_answer_below, port_ostore_multicall_noanswer_below, port_ostore_multicall_answer_above, port_ostore_multicall_noanswer_above |
| `system/oracle_storage.rs` | `test_the_read_slot_is_in_the_transactions_state_whichever_source_answered` | covered | q54, q55 (the Oracle slot in the read set either way) |
| `system/oracle_storage.rs` | `test_the_services_value_wins_over_the_frames_own_write` | ported | port_ostore_service_wins_answer, port_ostore_service_wins_none |
| `system/oracle_storage.rs` | `test_reads_in_a_nested_oracle_frame_are_cold_and_ask` | ported | port_ostore_nested_frame_cold |
| `system/predeploys.rs` | `test_the_factory_code_hashes_to_its_pinned_hash` | not-portable | internal api (pinned predeploy code hash) |
| `system/predeploys.rs` | `test_the_spec_list_is_the_seven_predeploys_in_order` | not-portable | internal api (the predeploy list constant) |
| `system/predeploys.rs` | `test_the_expected_code_is_read_as_it_is` | not-portable | internal api (the system-contract code check, a block-level helper) |
| `system/predeploys.rs` | `test_anything_else_is_unexpected_code` | not-portable | internal api (the system-contract code check) |
| `system/predeploys.rs` | `test_a_database_error_is_the_source_of_the_check_error` | not-portable | db fault (in the system-contract code check) |
| `system/remaining_compute_gas.rs` | `test_direct_tx_remaining_compute_gas` | ported | port_rcg_direct_1000000, port_rcg_direct_300000000 |
| `system/remaining_compute_gas.rs` | `test_remaining_compute_gas_staticcall` | covered | i2_limit_staticcall |
| `system/remaining_compute_gas.rs` | `test_remaining_compute_gas_decreases_after_compute_work` | ported | port_rcg_work_then_query_0, port_rcg_work_then_query_100, port_rcg_work_then_query_1000 |
| `system/remaining_compute_gas.rs` | `test_remaining_compute_gas_sequential_queries_decrease` | ported | port_rcg_sequential_queries |
| `system/remaining_compute_gas.rs` | `test_remaining_compute_gas_exact_value_matches_tracker` | ported | port_rcg_direct_padded_1000000, port_rcg_direct_padded_300000000 |
| `system/remaining_compute_gas.rs` | `test_remaining_compute_gas_inner_call_returns_frame_remaining` | covered | i1_limit_call (a 500,000 forward; the answer is the caller's own gas) |
| `system/remaining_compute_gas.rs` | `test_remaining_compute_gas_two_level_nesting_returns_inner_frame` | ported | port_rcg_two_level |
| `system/remaining_compute_gas.rs` | `test_remaining_compute_gas_persistent_after_inner_revert` | ported | port_rcg_after_inner_revert |
| `system/remaining_compute_gas.rs` | `test_remaining_compute_gas_clamped_by_detention_limit` | covered | i9_limit_call_detained, q39_det_limit_control_after_state |
| `system/remaining_compute_gas.rs` | `test_a_detained_transactions_own_call_hears_the_cap` | ported | port_rcg_direct_from_beneficiary_1000000, port_rcg_direct_from_beneficiary_300000000 |
| `system/remaining_compute_gas.rs` | `test_a_contracts_query_below_and_above_the_execution_cap` | ported | port_rcg_contract_query_below, port_rcg_contract_query_above, port_rcg_detained_contract_query_below, port_rcg_detained_contract_query_above |
| `system/remaining_compute_gas.rs` | `test_a_detained_caller_with_less_than_the_allowance_hears_its_own_gas` | ported | port_rcg_detained_less_than_allowance |
| `system/remaining_compute_gas.rs` | `test_a_childs_read_binds_its_callers_answer` | ported | port_rcg_child_read_binds_caller |
| `system/remaining_compute_gas.rs` | `test_the_answer_is_what_the_caller_resumes_with` | ported | port_rcg_answer_resume_undetained, port_rcg_answer_resume_after_read, … (5 scenarios, `port_rcg_answer_resume_*`) |
| `system/remaining_compute_gas.rs` | `test_a_value_called_callee_hears_its_stipend_whether_or_not_detained` | ported | port_rcg_value_callee_stipend_noread_0, port_rcg_value_callee_stipend_noread_10000, … (6 scenarios, `port_rcg_value_callee_stipend_*`) |
| `system/remaining_compute_gas.rs` | `test_a_caller_can_spend_the_answer_and_no_more` | ported | port_rcg_spend_the_answer_under, port_rcg_spend_the_answer_exact_ish, port_rcg_spend_the_answer_over |
| `system/system_tx.rs` | `test_a_system_transaction_writes_through_the_oracle` | covered | edge_system_legacy_oracle_promoted, rev_oracle_write_through_system_contract (real Oracle setSlot) |
| `system/system_tx.rs` | `test_a_system_transaction_is_promoted_to_a_deposit` | covered | edge_system_legacy_oracle_promoted (executed as a deposit: no fee, no history); the stamped source hash itself is internal |
| `system/system_tx.rs` | `test_a_system_transaction_pays_no_fee` | covered | fz mode 5 (priced Mega System Transactions pay no fee); the L1/operator-fee row is not a scenario |
| `system/system_tx.rs` | `test_a_system_transaction_moves_only_its_value` | covered | fz mode 5 (Mega System Transactions carrying 1 wei) |
| `system/system_tx.rs` | `test_another_shape_from_the_live_system_address_is_invalid` | covered | edge_system_legacy_off_whitelist, edge_system_creation, edge_system_eip2930_oracle, edge_system_eip1559_oracle, edge_system_eip7702_oracle; fz mode 5 (funded and unfunded) |
| `system/system_tx.rs` | `test_every_transaction_reads_the_registry` | covered | every scenario's read set (the registry account and its slot 0); the db-fault rows are not portable |
| `system/system_tx.rs` | `test_every_transaction_warms_the_registry_and_its_slot` | covered | rev_oracle_write_refused_for_other_caller, rev_oracle_write_through_system_contract (the real Oracle reads the registry slot warm) |
| `system/system_tx.rs` | `test_committing_a_transaction_writes_nothing_of_the_registry` | covered | every scenario (the registry is never in the changed accounts) |
| `system/system_tx.rs` | `test_a_user_transaction_is_not_held_to_the_whitelist` | covered | edge_eip2930_ordinary_call, edge_eip1559_ordinary_call |
| `system/system_tx.rs` | `test_a_user_deposit_is_not_a_system_transaction` | covered | q21_deposit_writes_salt2, q22_deposit_first_frame_halts |
| `system/system_tx.rs` | `test_a_deposit_from_another_caller_is_untouched` | not-portable | other: the MEGA_SYSTEM_TRANSACTION_SOURCE_HASH source hash on a deposit |
| `system/system_tx.rs` | `test_a_replayed_system_transaction_is_refused` | covered | port_system_nonce_stale_checks_on |
| `system/system_tx.rs` | `test_two_system_transactions_in_a_block_both_go_through` | ported | port_systx_two_in_block_t1, port_systx_two_in_block_t2 |
| `system/system_tx.rs` | `test_a_system_transaction_is_bound_to_the_chain` | covered | edge_system_legacy_oracle_without_chain_id; fz mode 5 (chain id 2) |
| `system/system_tx.rs` | `test_a_system_address_with_code_is_refused` | covered | edge_system_sender_with_code |
| `system/system_tx.rs` | `test_the_chain_id_is_checked_whatever_the_configuration_says` | not-portable | internal api (a configuration with the chain-id check off) |
| `system/system_tx.rs` | `test_the_configuration_switches_hold_for_system_transactions` | ported | port_system_nonce_stale_check_off, port_system_code_3607_off, port_system_code_and_stale_nonce_both_off (listed difference, pending fix) |
| `system/system_tx.rs` | `test_validating_a_system_transaction_charges_its_caller_as_any_senders` | covered | edge_system_legacy_oracle_promoted |
| `system/system_tx.rs` | `test_a_system_transaction_spends_no_history_gas` | covered | edge_system_legacy_oracle_promoted |
| `system/system_tx.rs` | `test_a_deposit_creates_its_caller_and_pays_for_it_once` | covered | p6_deposit_new_caller_m1, rev_deposit_caller_absent_ample |
| `system/system_tx.rs` | `test_a_deposit_to_its_own_caller_pays_once` | covered | fz mode 11 (a deposit from an absent caller to itself) |
| `system/system_tx.rs` | `test_a_deposit_that_creates_two_accounts_pays_for_both` | covered | edge_sweep_dep_absent_absent_value_runs, edge_deposit_new_caller_value_new_recipient_runtime_oog |
| `system/system_tx.rs` | `test_a_deposit_that_cannot_pay_for_its_caller_is_a_failed_deposit` | covered | edge_sweep_dep_absent_*_caller, edge_sweep_dep_empty_*_caller (one gas short of the created caller) |
| `system/system_tx.rs` | `test_a_deposit_over_the_data_size_limit_that_cannot_pay_for_its_caller_is_a_failed_deposit` | ported | port_systx_deposit_over_limit_21000, port_systx_deposit_over_limit_40000, port_systx_deposit_over_limit_60000, port_systx_deposit_over_limit_100000 |
| `system/system_tx.rs` | `test_the_oracle_accepts_the_system_address_the_registry_names` | covered | rev_oracle_write_through_system_contract, edge_system_legacy_oracle_promoted |
| `system/system_tx.rs` | `test_the_callee_sees_the_system_address_as_its_caller` | ported | port_systx_callee_sees_system |
| `system/system_tx.rs` | `test_the_created_caller_is_not_a_write_record` | covered | p6_deposit_new_caller_m1 (kv excludes the created caller, growth counts it) |
| `system/system_tx.rs` | `test_a_deposit_that_creates_a_contract_pays_for_its_caller_too` | covered | q20_deposit_new_caller_creates_salt |
| `system/system_tx.rs` | `test_a_transaction_that_arrives_promoted_is_a_failed_deposit` | not-portable | other: a transaction already carrying the system source hash (MEGA_SYSTEM_TRANSACTION_SOURCE_HASH) |
| `block/activation.rs` | `test_a_node_context_refuses_a_user_transaction_where_a_fork_activates` | not-portable | block executor (activation-block rule, predeploy check) |
| `block/activation.rs` | `test_the_activation_rule_follows_the_optimism_fork_order` | not-portable | block executor (activation-block rule, predeploy check) |
| `block/activation.rs` | `test_a_node_context_checks_the_predeploys_where_satin_activates` | not-portable | block executor (activation-block rule, predeploy check) |
| `block/block_hashes.rs` | `test_accessed_block_hashes_track_and_clear_per_transaction` | not-portable | block executor (block-hash read record for witnesses) |
| `block/block_hashes.rs` | `test_two_blocks_over_the_same_evm_report_only_their_own_reads` | not-portable | block executor (block-hash read record for witnesses) |
| `block/block_hashes.rs` | `test_a_cached_hash_the_block_never_read_is_not_reported` | not-portable | block executor (block-hash read record for witnesses) |
| `block/bucket_ids.rs` | `test_accessed_bucket_ids_accumulate_over_the_block` | not-portable | block executor (SALT bucket read record for witnesses) |
| `block/bucket_ids.rs` | `test_two_blocks_over_the_same_evm_report_only_their_own_buckets` | not-portable | block executor (SALT bucket read record for witnesses) |
| `block/bucket_ids.rs` | `test_the_pre_block_calls_and_a_system_transaction_add_no_bucket` | not-portable | block executor (SALT bucket read record for witnesses) |
| `block/bucket_ids.rs` | `test_a_failed_bucket_read_commits_nothing_for_a_deposit_or_a_user_transaction` | not-portable | block executor (SALT bucket read record for witnesses) |
| `block/bucket_ids.rs` | `test_a_transaction_without_state_charges_asks_about_no_bucket` | not-portable | block executor (SALT bucket read record for witnesses) |
| `block/building_policy.rs` | `test_a_hint_that_crosses_the_limit_adds_nothing_to_the_block_s_data_size` | not-portable | block executor (builder policy and block counters) |
| `block/building_policy.rs` | `test_a_builder_and_a_validator_that_differ_on_building_policy_agree_on_the_block` | not-portable | block executor (builder policy and block counters) |
| `block/counters.rs` | `test_a_block_sums_each_ledger_of_its_transactions` | not-portable | block executor (block counters) |
| `block/counters.rs` | `test_the_result_carries_the_counters_and_the_footprint` | not-portable | block executor (block counters) |
| `block/counters.rs` | `test_the_state_and_history_ledgers_refuse_nothing` | not-portable | block executor (block counters) |
| `block/counters.rs` | `test_a_block_reports_the_bytes_its_history_gas_does_not_cover` | not-portable | block executor (block counters) |
| `block/counters.rs` | `test_a_floor_bound_transaction_adds_its_floor_to_the_block` | not-portable | block executor (block counters) |
| `block/counters.rs` | `test_an_out_of_gas_before_the_first_frame_bills_the_whole_gas_limit_to_the_block` | not-portable | block executor (block counters) |
| `block/counters.rs` | `test_the_pre_block_calls_count_towards_no_block_budget` | not-portable | block executor (block counters) |
| `block/deposits.rs` | `test_no_block_limit_refuses_a_deposit_and_every_deposit_counts` | not-portable | block executor (block data-size limit and deposits) |
| `block/detention.rs` | `test_a_stopped_transaction_is_included_with_a_failed_receipt_that_bills_what_ran` | not-portable | block executor (receipt of a stopped transaction); the transaction itself is d1_top_frame_crosses |
| `block/eips.rs` | `test_the_pre_block_calls_record_the_parent_hash_and_the_beacon_root` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_the_pre_block_calls_are_held_to_no_limit` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_the_pre_block_calls_count_towards_no_counter_or_budget` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_the_block_hashes_call_waits_for_prague` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_the_beacon_root_call_waits_for_cancun` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_the_genesis_block_makes_no_pre_block_call` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_the_genesis_block_refuses_a_non_zero_beacon_root` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_a_block_without_a_parent_beacon_root_is_refused` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_the_successful_pre_block_calls_commit_normally` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_a_block_is_refused_when_its_block_hashes_call_halts` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_a_block_is_refused_when_its_beacon_root_call_halts` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_the_pre_block_budget_does_not_follow_the_block` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_a_database_error_in_the_block_hashes_call_is_internal` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_a_database_error_in_the_beacon_root_call_is_internal` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/eips.rs` | `test_a_pre_block_call_commits_the_state_op_revm_produces` | not-portable | block executor (EIP-2935/EIP-4788 pre-block calls) |
| `block/external_envs.rs` | `test_a_blocks_environments_come_from_the_factory` | not-portable | block executor (ExternalEnvFactory per block) |
| `block/factory.rs` | `test_trait_path_applies_the_specs_runtime_limits` | not-portable | block executor (factory and executor construction) |
| `block/factory.rs` | `test_inspector_and_plain_paths_apply_same_runtime_limits` | not-portable | block executor (factory and executor construction) |
| `block/factory.rs` | `test_direct_construction_applies_the_specs_runtime_limits` | not-portable | block executor (factory and executor construction) |
| `block/factory.rs` | `test_direct_construction_stops_a_transaction_over_the_blocks_data_size_limit` | not-portable | block executor (factory and executor construction) |
| `block/inspector.rs` | `test_an_inspector_reaches_block_execution` | not-portable | block executor (inspectors over block execution) |
| `block/inspector.rs` | `test_an_observer_sees_the_transactions_of_a_block_and_nothing_else` | not-portable | block executor (inspectors over block execution) |
| `block/inspector.rs` | `test_inspector_works_with_block_executor` | not-portable | block executor (inspectors over block execution) |
| `block/inspector.rs` | `test_inspector_early_return_with_additional_limits` | not-portable | block executor (inspectors over block execution) |
| `block/l1_attributes.rs` | `test_an_l1_attributes_deposit_within_an_origin_fits_the_satin_limits` | ported | port_l1attr_sequence_only |
| `block/l1_attributes.rs` | `test_an_l1_attributes_deposit_that_starts_an_origin_fits_the_satin_limits` | ported | port_l1attr_new_origin |
| `block/limits.rs` | `test_block_custom_data_limit` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_the_block_kv_budget_packs_the_transaction_that_reaches_it` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_the_block_state_growth_budget_is_the_protocols_1000` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_a_block_full_on_several_budgets_names_them_in_order` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_block_no_state_commit_on_limit_exceeded` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_a_deposit_over_the_transaction_data_size_limit_is_still_included` | not-portable | block executor (inclusion); the failed deposit itself is edge_deposit_body_over_limit |
| `block/limits.rs` | `test_block_tx_size_limit_default_unlimited` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_block_tx_size_limit_exceeded_first_transaction` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_block_tx_size_limit_exceeded_mid_block` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_block_tx_size_limit_with_varying_sizes` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_the_trait_commit_trips_on_an_outcome_the_block_has_no_room_for` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_commit_time_pre_execution_check_parallel_simulation` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_run_transaction_rejects_understated_cached_da_size` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_run_transaction_succeeds_with_accurate_cache` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_deposit_transaction_exempt_from_single_tx_da_limit` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_regular_transaction_rejected_by_single_tx_da_limit` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_no_building_policy_refuses_a_deposit` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_commit_time_da_footprint_check_parallel_simulation` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/limits.rs` | `test_a_database_error_in_the_authorization_list_simulation_is_internal_only_when_fatal` | not-portable | block executor (block limits, packing, commit-time checks) |
| `block/predeploys.rs` | `test_the_activation_block_reads_the_seven_predeploys_once` | not-portable | block executor (activation-block predeploy check) |
| `block/predeploys.rs` | `test_no_other_block_reads_a_predeploy` | not-portable | block executor (activation-block predeploy check) |
| `block/predeploys.rs` | `test_an_activation_block_without_the_expected_code_is_refused` | not-portable | block executor (activation-block predeploy check) |
| `block/predeploys.rs` | `test_a_create2_round_trip_through_the_factory` | ported | port_predeploy_create2_factory |
| `block/receipt_goldens.rs` | `test_a_blocks_receipts_pin_their_bytes_bloom_and_cumulative_gas` | not-portable | snapshot (receipt bytes and receipts root) |
| `block/receipts.rs` | `test_the_receipts_are_readable_during_the_block_and_leave_with_its_result` | not-portable | block executor (receipts) |
| `block/rules.rs` | `test_activation_block_rejects_a_mega_system_transaction` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_da_footprint_is_inert_without_a_scalar` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_da_footprint_within_the_block_budget_is_reported_as_blob_gas` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_a_mega_system_transaction_costs_its_da_footprint` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_da_footprint_over_the_block_budget_refuses_the_transaction` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_da_footprint_that_exactly_fills_the_block_budget_is_admitted` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_deposits_do_not_count_towards_the_da_footprint` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_l1_block_info_is_read_by_the_first_transaction_that_prices_against_it` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_l1_block_info_of_this_block_is_not_overwritten` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_the_l1_info_deposit_prices_the_transactions_after_it` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_operator_fee_is_charged_on_the_karst_formula` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/rules.rs` | `test_block_gas_limit_comes_from_the_block_environment` | not-portable | block executor (Karst block rules: DA footprint, L1 info, operator fee) |
| `block/salt.rs` | `test_the_pre_block_calls_commit_at_any_capacity` | not-portable | block executor (pre-block calls in a crowded region) |
| `block/salt.rs` | `test_a_user_transaction_in_the_crowded_block_pays_the_crowded_price` | not-portable | block executor (the block around it); the transaction-level price is p6_salt_sstore_set_m8 |
| `block/schedule.rs` | `test_canonical_schedules_match_the_chain_activation_table` | not-portable | block executor (hardfork schedule loading) |
| `block/schedule.rs` | `test_a_schedule_activating_satin_alone_loads_and_runs` | not-portable | block executor (hardfork schedule loading) |
| `block/schedule.rs` | `test_a_base_fork_after_satin_is_refused_at_load` | not-portable | block executor (hardfork schedule loading) |
| `block/schedule.rs` | `test_a_base_fork_not_scheduled_by_timestamp_is_refused_at_load` | not-portable | block executor (hardfork schedule loading) |
| `block/schedule.rs` | `test_a_base_fork_at_satin_time_is_accepted` | not-portable | block executor (hardfork schedule loading) |
| `block/schedule.rs` | `test_engine_built_schedules_keep_their_base_forks_at_genesis` | not-portable | block executor (hardfork schedule loading) |
| `block/schedule.rs` | `test_base_forks_are_not_checked_without_satin` | not-portable | block executor (hardfork schedule loading) |
| `block/schedule.rs` | `test_forks_scheduled_by_block_are_not_checked` | not-portable | block executor (hardfork schedule loading) |
| `block/sequencer_registry.rs` | `test_a_due_change_is_applied_under_a_crowded_bucket` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_a_block_with_nothing_due_makes_no_call` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_a_change_not_yet_due_makes_no_call` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_a_due_sequencer_change_is_applied` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_the_slot_is_in_the_system_transactions_state_not_the_pre_block_witness` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_system_tx_uses_resolved_system_address` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_a_rotation_governs_the_blocks_from_its_activation_block` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_an_evm_outside_block_execution_promotes_the_rotated_address` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_a_deposit_from_the_live_system_address_is_a_failed_deposit` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_a_typed_transaction_from_the_live_system_address_is_invalid` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_sequencer_change_does_not_affect_system_address` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_dual_change_in_same_block` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_admin_handoff_via_block_executor` | ported | port_registry_admin_handoff_t1, port_registry_admin_handoff_t2 |
| `block/sequencer_registry.rs` | `test_a_rotation_with_a_valid_proof_activates_at_its_block` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_a_sequencer_rotation_takes_effect_across_blocks` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequencer_registry.rs` | `test_a_schedule_without_a_proof_reverts` | ported | port_registry_schedule_without_proof |
| `block/sequencer_registry.rs` | `test_a_non_fatal_error_on_the_due_read_is_an_internal_error` | not-portable | block executor (pre-block applyPendingChanges and the live system address across blocks) |
| `block/sequences.rs` | `test_the_block_hash_ring_wraps_and_keeps_the_slot_beside_it` | not-portable | block executor (consecutive blocks, the block-hash ring) |
| `block/sequences.rs` | `test_the_block_after_an_activation_block_packs_a_user_transaction` | not-portable | block executor (consecutive blocks, the block-hash ring) |
| `block/stale.rs` | `test_a_second_outcome_at_the_same_nonce_is_refused` | not-portable | block executor (delayed commits of executed outcomes) |
| `block/stale.rs` | `test_an_outcome_whose_senders_balance_moved_is_refused` | not-portable | block executor (delayed commits of executed outcomes) |
| `block/stale.rs` | `test_a_second_system_transaction_at_the_same_nonce_is_refused` | not-portable | block executor (delayed commits of executed outcomes) |
| `block/stale.rs` | `test_the_system_transaction_first_flow_commits` | not-portable | block executor (delayed commits of executed outcomes) |
| `block/stale.rs` | `test_the_trait_commit_trips_on_a_stale_outcome` | not-portable | block executor (delayed commits of executed outcomes) |
| `block/state_gas.rs` | `test_the_blocks_state_gas_counts_what_the_reservoir_paid` | not-portable | block executor (block state-gas counter) |
| `block/system_tx_budgets.rs` | `test_a_mega_system_transaction_is_admitted_at_a_full_budget` | not-portable | block executor (block packing budgets for Mega System Transactions) |
| `block/system_tx_budgets.rs` | `test_a_mega_system_transaction_is_admitted_under_a_tightened_building_policy` | not-portable | block executor (block packing budgets for Mega System Transactions) |
| `block/system_tx_budgets.rs` | `test_a_delayed_commit_of_a_mega_system_transaction_ignores_the_spent_budget` | not-portable | block executor (block packing budgets for Mega System Transactions) |
| `block/system_tx_budgets.rs` | `test_a_mega_system_transaction_is_still_refused_by_the_header_gas_limit` | not-portable | block executor (block packing budgets for Mega System Transactions) |
| `block/system_tx_budgets.rs` | `test_a_mega_system_transaction_is_still_refused_by_the_da_footprint` | not-portable | block executor (block packing budgets for Mega System Transactions) |
| `block/system_tx_budgets.rs` | `test_an_unshaped_transaction_from_the_system_address_is_no_system_transaction` | not-portable | block executor (block packing budgets for Mega System Transactions) |
| `block/transfer_logs.rs` | `test_a_blocks_receipts_carry_its_transfer_logs_in_execution_order` | ported | port_tlog_block_t1, port_tlog_block_t2, port_tlog_block_t3, port_tlog_block_t4 |
| `block/transfer_logs.rs` | `test_a_deposit_receipt_logs_its_value_and_not_its_mint` | covered | edge_deposit_value_new_recipient_ok, q21_deposit_writes_salt2 (the deposit transaction; the receipt is the block executor) |
| `block/transfer_logs.rs` | `test_a_system_transaction_receipt_logs_no_transfer` | covered | edge_system_legacy_oracle_promoted (no transfer log; the receipt is the block executor) |
| `block/transfer_logs.rs` | `test_a_value_left_on_a_destroyed_account_is_burned_without_a_log` | covered | fz mode 9 (endowed creation destroying itself with ADDRESS SELFDESTRUCT: the balance burned, no log) |
| `block/witness/basics.rs` | `test_a_block_of_writes_replays_from_its_witness` | not-portable | block executor (stateless witness) |
| `block/witness/basics.rs` | `test_an_empty_block_replays_from_its_witness` | not-portable | block executor (stateless witness) |
| `block/witness/basics.rs` | `test_a_refused_transaction_reads_nothing_and_is_not_replayed` | not-portable | block executor (stateless witness) |
| `block/witness/basics.rs` | `test_a_dropped_candidate_is_in_no_state_and_no_replay` | not-portable | block executor (stateless witness) |
| `block/witness/basics.rs` | `test_a_failed_salt_lookup_fails_its_transaction_and_is_not_exported` | not-portable | block executor (stateless witness) |
| `block/witness/basics.rs` | `test_a_capacity_below_the_minimum_fails_its_transaction_and_is_not_exported` | not-portable | block executor (stateless witness) |
| `block/witness/basics.rs` | `test_the_reads_of_a_frame_that_failed_are_in_the_witness` | not-portable | block executor (stateless witness) |
| `block/witness/basics.rs` | `test_the_reads_of_a_deposit_that_failed_are_in_the_witness` | not-portable | block executor (stateless witness) |
| `block/witness/history.rs` | `test_block_hash_reads_replay` | not-portable | block executor (stateless witness) |
| `block/witness/keyless.rs` | `test_a_keyless_deployment_replays` | not-portable | block executor (stateless witness) |
| `block/witness/keyless.rs` | `test_a_value_bearing_keyless_call_reads_no_account` | not-portable | block executor (stateless witness) |
| `block/witness/keyless.rs` | `test_a_signer_nonce_refusal_reads_the_signer_alone` | not-portable | block executor (stateless witness) |
| `block/witness/keyless.rs` | `test_an_occupied_deploy_address_refusal_reads_both_accounts` | not-portable | block executor (stateless witness) |
| `block/witness/keyless.rs` | `test_an_insufficient_balance_refusal_replays` | not-portable | block executor (stateless witness) |
| `block/witness/keyless.rs` | `test_a_reverting_constructor_replays` | not-portable | block executor (stateless witness) |
| `block/witness/keyless.rs` | `test_a_resubmission_in_the_same_block_is_refused` | not-portable | block executor (stateless witness) |
| `block/witness/keyless.rs` | `test_a_malformed_keyless_call_reads_nothing` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_an_oracle_read_replays_with_its_answer_and_not_without` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_an_oracle_read_the_chain_holds_replays_without_a_service` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_an_unanswered_oracle_read_replays` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_a_failed_deposits_oracle_read_is_recorded_and_replayed` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_the_replayed_answers_are_the_included_transactions_own` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_a_dropped_read_of_the_same_slot_leaves_the_included_read_its_own_answer` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_a_validator_without_a_service_needs_the_included_update` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_a_hint_and_a_read_replay` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_the_high_precision_timestamp_replays` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_a_pre_block_oracle_read_takes_chain_storage_and_replays` | not-portable | block executor (stateless witness) |
| `block/witness/oracle.rs` | `test_a_pre_block_oracle_read_leaves_the_blocks_transactions_unchanged` | not-portable | block executor (stateless witness) |
| `block/witness/predeploys.rs` | `test_a_deployment_through_the_factory_replays` | not-portable | block executor (stateless witness) |
| `block/witness/predeploys.rs` | `test_an_activation_block_replays` | not-portable | block executor (stateless witness) |
| `block/witness/skips.rs` | `test_a_cold_sload_short_of_gas_reads_no_slot` | not-portable | block executor (stateless witness) |
| `block/witness/skips.rs` | `test_a_cold_sload_with_gas_reads_the_slot` | not-portable | block executor (stateless witness) |
| `block/witness/skips.rs` | `test_a_cold_account_load_short_of_gas_reads_no_account` | not-portable | block executor (stateless witness) |
| `block/witness/skips.rs` | `test_an_oracle_read_short_of_gas_asks_nothing` | not-portable | block executor (stateless witness) |
| `block/witness/skips.rs` | `test_a_refused_block_hash_read_reads_nothing` | not-portable | block executor (stateless witness) |
| `block/witness/skips.rs` | `test_a_refused_oracle_read_reads_nothing` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_a_delegation_replays` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_a_replaced_delegation_replays_from_the_code_the_chain_held` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_a_creation_replays` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_selfdestructs_replay` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_a_transfer_creating_its_recipient_replays` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_a_deposit_only_block_replays_and_reads_no_l1_slot` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_a_user_transaction_is_priced_against_the_l1_info_the_pre_block_state_carries` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_the_overhead_is_in_the_entry_when_the_scalars_are_empty` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_the_l1_attributes_deposit_first_prices_the_transactions_after_it` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_a_deposit_emptying_the_scalars_leaves_the_overhead_in_the_witness` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_the_overhead_a_block_reads_decides_no_result` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_a_deposit_creating_the_l1_block_contract_adds_no_slot_read` | not-portable | block executor (stateless witness) |
| `block/witness/state.rs` | `test_a_database_that_cannot_serve_the_l1_entry_fails_the_block_as_internal` | not-portable | block executor (stateless witness) |
| `block/witness/stops.rs` | `test_a_data_size_stop_replays` | not-portable | block executor (stateless witness) |
| `block/witness/stops.rs` | `test_a_compute_stop_replays` | not-portable | block executor (stateless witness) |
| `block/witness/stops.rs` | `test_a_precompile_price_crossing_replays` | not-portable | block executor (stateless witness) |
| `block/witness/stops.rs` | `test_an_answer_crossing_replays` | not-portable | block executor (stateless witness) |
| `block/witness/stops.rs` | `test_a_block_budget_refusal_replays` | not-portable | block executor (stateless witness) |
| `block/witness/stops.rs` | `test_a_stop_above_the_execution_cap_replays` | not-portable | block executor (stateless witness) |
| `block/witness/stops.rs` | `test_a_skipped_authorization_list_replays` | not-portable | block executor (stateless witness) |
| `block/witness/system.rs` | `test_a_rotation_and_a_system_transaction_replay` | not-portable | block executor (stateless witness) |
| `block/witness/system.rs` | `test_a_system_transaction_reads_the_live_address_from_the_witness` | not-portable | block executor (stateless witness) |
| `tracers/scenarios.rs` | `test_eth_transfer_emits_7708_log` | covered | c4_value_tx_to_empty_res, edge_start_record_paid (the outcome and its transfer log; the tracer snapshots are not portable) |
| `tracers/scenarios.rs` | `test_sstore_new_slot_charges_state_gas` | covered | ctl_sstore (outcome; tracer snapshots not portable) |
| `tracers/scenarios.rs` | `test_create_deploys_runtime_code` | covered | p6_salt_code_deposit_m1 (creation tx depositing runtime; tracer snapshots not portable) |
| `tracers/scenarios.rs` | `test_nested_call_inner_reverts` | covered | s3_value_call_reverting_child_res, p6_history_child_reverts (callee writes and logs then reverts, caller logs; tracer snapshots not portable) |
| `tracers/scenarios.rs` | `test_keyless_deploy_succeeds` | covered | k01_ok (tracer snapshots not portable) |
| `tracers/scenarios.rs` | `test_keyless_deploy_refused` | covered | k08_value_unfunded_signer (tracer snapshots not portable) |
| `tracers/scenarios.rs` | `test_data_size_limit_stops_the_first_frame_at_its_start` | ported | port_trace_first_frame_start |
| `tracers/scenarios.rs` | `test_data_size_limit_stop_is_caught_by_the_parent` | ported | port_trace_child_log_tx_limit |
| `tracers/scenarios.rs` | `test_frame_budget_reverts_the_child_alone` | ported | port_trace_child_log_budget |
| `tracers/scenarios.rs` | `test_timestamp_detention_stop` | covered | d1_top_frame_crosses (tracer snapshots not portable) |
| `tracers/scenarios.rs` | `test_detention_stop_crosses_again_in_the_parent` | covered | d2_child_crosses_caller_resumes (tracer snapshots not portable) |
| `tracers/scenarios.rs` | `test_reservoir_pays_state_gas` | covered | ctl_sstore_res |
| `tracers/scenarios.rs` | `test_identity_precompile` | covered | fz mode 7 (a transaction to the identity precompile) |
| `tracers/scenarios.rs` | `test_precompile_child_frame` | ported | port_trace_precompile_child |
| `tracers/scenarios.rs` | `test_deposit_transaction` | covered | edge_sweep_dep_funded_contract_value_runs, edge_deposit_value_new_recipient_ok (deposit with mint and value; no history) |
| `fuzz/blocks.rs` | `test_property_block_counters_and_budgets` | not-portable | block executor (random blocks through MegaBlockExecutor under random ProtocolLimits and policy) |
| `fuzz/blocks.rs` | `test_property_block_determinism` | not-portable | block executor (the same block twice) |
| `fuzz/differential.rs` | `test_differential_neutral_mega_evm_matches_op_revm` | not-portable | other: the neutral configuration against op-revm on random programs |
| `fuzz/differential.rs` | `test_differential_neutral_mega_evm_matches_ethereum` | not-portable | other: the neutral configuration against revm's mainnet EVM |
| `fuzz/harness.rs` | `test_the_mode_the_environment_chooses` | not-portable | internal api (the proptest harness's environment handling) |
| `fuzz/harness.rs` | `test_coverage_is_required_only_on_the_default_run` | not-portable | internal api (the proptest harness's environment handling) |
| `fuzz/harness.rs` | `test_a_case_count_that_is_not_a_number_is_refused` | not-portable | internal api (the proptest harness's environment handling) |
| `fuzz/harness.rs` | `test_a_case_count_of_zero_is_refused` | not-portable | internal api (the proptest harness's environment handling) |
| `fuzz/harness.rs` | `test_a_blank_padded_case_count_of_zero_is_refused` | not-portable | internal api (the proptest harness's environment handling) |
| `fuzz/harness.rs` | `test_a_seed_that_is_not_a_number_is_refused` | not-portable | internal api (the proptest harness's environment handling) |
| `fuzz/keyless.rs` | `test_property_a_keyless_deployment_follows_its_rules` | covered | fz mode 3 (keyless deployments with the signer absent, funded, at nonce 1, the sender, the beneficiary), fz mode 6 (onto occupied addresses), fz mode 14 (keyless pairs) |
| `fuzz/prices.rs` | `test_property_schedule_is_monotone_in_the_byte_prices` | not-portable | internal api (satin_gas_params_at at varied byte prices) |
| `fuzz/prices.rs` | `test_property_price_record` | not-portable | other: the satin-price-override measurement feature |
| `fuzz/properties.rs` | `test_property_determinism` | not-portable | other: a property of one engine run twice on the same input (the gates compare two engines) |
| `fuzz/properties.rs` | `test_property_a_reused_evm_runs_a_transaction_as_a_fresh_one_does` | covered | fz mode 15 and the blk_* families (transactions run one after another on one EVM) |
| `fuzz/properties.rs` | `test_property_inspector_transparency` | covered | every scenario's inspected run (inspected_equal); the revm-inspectors tracer arm is not portable |
| `fuzz/properties.rs` | `test_property_gas_ledgers` | covered | fz modes 1-4 (random compositions compared on gas_used, regular, state, history, reservoir_remaining) |
| `fuzz/properties.rs` | `test_property_kv_weighs_no_more_than_data_size` | not-portable | internal api (a per-instruction invariant read by an inspector) |
| `fuzz/properties.rs` | `test_property_the_record_count_is_the_net_change` | covered | fz modes 1-4 and 9 (kv compared with the reference on random programs) |
| `fuzz/properties.rs` | `test_property_state_growth_is_the_net_count_of_new_state` | covered | fz modes 1-4 and 9 (growth compared with the reference on random programs) |
| `fuzz/properties.rs` | `test_property_a_stop_keeps_only_what_survives` | covered | fz modes 1-4 (random limits and caps; limit_exceeded, state and usage compared) |
| `fuzz/regressions.rs` | `test_an_out_of_gas_before_the_first_frame_pays_the_operator_fee_on_the_whole_gas_limit` | not-portable | other: a non-zero operator fee scalar and constant (l1_fee carries the L1 base fee and base-fee scalar only) |
| `fuzz/regressions.rs` | `test_a_delegated_sender_that_funds_then_creates_reports_the_net_change` | ported | port_trace_delegated_sender_funds_then_creates |
| `fuzz/regressions.rs` | `test_a_program_that_funds_the_account_it_then_creates_keeps_one_record_for_it` | covered | fz mode 9 (value to the CREATE2 address, then CREATE2 there: one record for it) |
