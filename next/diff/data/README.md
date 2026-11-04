# Gate sample and full-corpus layout

`gate.sh sample` runs every gate on the files in this directory (about 2.5 MiB, zstd-compressed).
`gate.sh full` runs them on the full corpus under `MEGA_EVM2_CORPUS_DIR`.
`make-sample.sh` rebuilds this directory from the full corpus.

## Layout of this directory

| Path                              | Content                                                                                          |
| --------------------------------- | ------------------------------------------------------------------------------------------------ |
| `synthetic/main.json.zst`         | Reference outputs of the 709 shared scenarios (hand-written ones plus 400 default fuzz seeds)    |
| `synthetic/fuzz-mode4.json.zst`   | Reference outputs of fuzz mode 4, seeds 1..=2000 (`fqh0001`..`fqh2000`)                          |
| `real-blocks/<set>/<n>.json.zst`  | Recorded block `n` of corpus `set` (`d4` or `p9`): header, transactions, receipts, sparse state  |
| `real-blocks/<set>/manifest.json` | The sample's blocks of that set, in the full corpus's manifest format                            |
| `replay/<set>/<n>.jsonl.zst`      | Reference output of the engine replay gate: one line per pre-block call and per transaction      |
| `block/<set>/<n>.jsonl.zst`       | Reference output of the block executor gate: per transaction, then block counters and post-state |
| `expected.env`                    | Summary counts `gate.sh sample` expects, counted from the reference files                        |

Scenario names (such as `p6_dupn`), group keys (`p5`, `p6`, `p7`, `q2`, `fz`, `q8`, `cap`) and fuzz batch names (`fq`, `fqbig`, `fqh`, `fqhbig`, `big`, `harsh`) are stable data identifiers: the reference outputs and `known-differences.toml` are keyed on them, so they keep their names.

Expected counts of the sample: 709 main scenarios (708 match, 1 known difference), 2,000 fuzz scenarios;
20 blocks with 499 matching transactions, 40 pre-block calls and 52 refusals in the engine replay;
479 included and 72 refused transactions in the block executor.

`MAIN_EXPECT` in `scripts/common.sh` is the one place the scripts state the main count.
When a scenario is added, change it there, update the counts in this file, and regenerate the reference with `regen-reference.sh` (it needs `MEGA_EVM2_CORPUS_DIR`; the new corpus archive has to replace the old one by hand before `gate.sh full` passes).

## The `cap_` scenarios

Satin does not apply the EIP-8037 total transaction gas cap (2^32 − 1); a transaction is bounded by the block's gas limit alone (10,000,000,000 in the synthetic block).
Nine scenarios (group `cap`) call a contract that stores one slot, as a legacy transaction, an OP deposit and an EIP-7702 transaction, each at gas limit 2^32 − 1, 2^32 and the block's gas limit.
The reference accepts and executes all nine, and the engine matches it.
No scenario sends a system-address transaction.
The promotion to a deposit happens in the transaction handler (`prepare_legacy` in `mega-evm2/src/tx.rs`), so plain `transact` reaches it, but it needs a live system address: the `SequencerRegistry` with its trusted code and the address in its slot.
The scenario fixtures give the registry a one-byte placeholder code, so neither side has a live system address and no scenario takes that path.
The recorded blocks cover it instead.
Every transaction of the full corpus with a gas limit of 2^32 or more is such a transaction: 2,367 legacy calls from the system address to the Oracle in 230 of the 263 blocks, with gas price 0 and a gas limit just under the block's 10,000,000,000, all successful on chain.
No other transaction (deposits and user transactions of every type) reaches 2^32.
Both sides run them as promoted deposits, whose path checks intrinsic and floor gas but no total cap, and the replay and block gates compare them, 133 of them in 13 blocks of the sample.
The `system_tx` negative control runs one of those blocks (`p9:26400004`): with the promotion off, its ten system transactions are refused as ordinary transactions whose fee cap is below the base fee.
`mega-evm2/tests/tx_gas_cap.rs` (`test_system_address_tx_above_total_cap_executes`) also sends one at gas limit 2^32 through plain `transact`.
EIP-2930 and EIP-1559 are not expressed: the shared transaction format has no access list or fee-cap fields, and the two sides build their transactions separately, so adding them means extending both builders.

## Selection of the sample blocks

Corpus `d4` holds blocks sampled across the whole history of the chain, so it covers the early specs;
corpus `p9` is 200 consecutive blocks from the current spec.
The sample takes 14 blocks of `d4` and 6 of `p9`.
It leaves out the huge `d4` block 6780842 (21,153 transactions, 24 MiB).
Because that block is the only one with block data-size refusals, the sample has none of those; the full run has 7.

| Block (set:number)                          | Why                                                                                                                                                    |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `d4:2045117`                                | Smallest block with an EIP-7702 transaction (and a deposit)                                                                                            |
| `d4:14924732`, `d4:18138664`                | EIP-7702 transaction together with system-address transactions; `18138664` also has block gas-limit refusals                                           |
| `d4:19010529`, `d4:28102900`                | EIP-7702 transaction, system-address transactions and block gas-limit refusals                                                                         |
| `d4:27670576`, `d4:27985540`                | EIP-7702 (`27985540` has two) with system-address transactions and halts                                                                               |
| `d4:7487457`, `d4:8064067`                  | Keyless deployment transactions (the other two of the 10 transactions the earlier harness had to take from the reference), plus nonce-too-low refusals |
| `d4:3224158`, `d4:10108963`                 | Early-spec blocks whose transactions are mostly refused as nonce-too-low (`10108963` has 22, and two deposits)                                         |
| `d4:12747297`, `d4:22016777`, `d4:28107033` | System-address transactions, halts and reverts; two deposits in `22016777` and `28107033`                                                              |
| `p9:26400000`                               | Current spec; block gas-limit refusals and 9 halts                                                                                                     |
| `p9:26400004`                               | Current spec; plain block with system-address transactions                                                                                             |
| `p9:26400020`, `p9:26400077`                | Current spec; nonce-too-high refusals (all 3 of them are in these two blocks), block gas-limit and nonce-too-low refusals                              |
| `p9:26400051`                               | Current spec; three block gas-limit refusals                                                                                                           |
| `p9:26400193`                               | Current spec; a revert, system-address transactions                                                                                                    |

Together the sample contains all 10 transactions the earlier harness had to take from the reference
(8 EIP-7702 transactions in seven blocks, 2 keyless deployments in two blocks),
every block has at least one deposit, 12 blocks have system-address transactions,
and the refusals cover nonce-too-low, nonce-too-high and block gas limit.

## Layout of the full corpus (`MEGA_EVM2_CORPUS_DIR`)

```text
real-blocks/
  d4/manifest.json, d4/<n>.json     63 blocks across the chain's history (the manifest lists them)
  p9/manifest.json, p9/<n>.json     200 consecutive blocks from the current spec
  rot/                              5 variant blocks for the registry rotation (not part of the gates)
  *.perf.json                       measurement inputs (not used by the gates)
reference/
  reference-outputs.tar.zst         reference outputs, extracted by `gate.sh full` (made by `regen-reference.sh`;
                                    when absent the original `q11-oracles.tar.zst` is read instead):
    satin-oracle.json               synthetic main (707 scenarios)
    oracles/{fq,fqbig,fqh,fqhbig,big,harsh}.json
                                    the six fuzz batches, 25,000 scenarios in total
    raw/satin/<set>/<n>.jsonl       engine replay reference
    raw/satin-block/<set>/<n>.jsonl block executor reference
```

The six fuzz batches (mode, seeds): `fq` 3 / 1..=3000, `fqbig` 3 / 3001..=13000, `fqh` 4 / 1..=2000, `fqhbig` 4 / 2001..=7000, `big` 1 / 401..=3400, `harsh` 2 / 1..=2000.
The full run expects 263 blocks, 28,000 matching transactions, 526 pre-block calls and 186 refusals in the engine replay,
and 27,879 included and 307 refused transactions in the block executor.

## Regenerating the reference outputs

The reference outputs are produced by the Satin implementation under review (`next/reference`, pinned in `next/reference/Cargo.toml`), not by this repository's engine.
`scripts/check-reference.sh sample` regenerates the sample's outputs and compares them with this directory.

`scripts/regen-reference.sh [--check] <out-dir>` does the same for the whole corpus:
it runs `next/reference` over the main scenarios, the six fuzz batches of `gate.sh full` and every block of `$MEGA_EVM2_CORPUS_DIR/real-blocks`,
and packs `<out-dir>/reference-outputs.tar.zst` in the layout above, which `gate.sh full` prefers over `q11-oracles.tar.zst`.

- `--check` writes nothing into the repository.
  It compares the regenerated outputs with the corpus's expected archive and the full counts with `expected.env`, and prints `REFERENCE full identical`.
  The expected archive is picked the way `gate.sh` picks it: `reference/reference-outputs.tar.zst` when present, else `reference/q11-oracles.tar.zst`.
  Only `micros` is ignored.
- Without `--check` it also re-cuts this sample with `make-sample.sh` from the new archive and rewrites `expected.env` from the regenerated files.

`regen-reference.sh` writes only a new directory of its own, `<out-dir>/stage.XXXXXX` (removed when the run succeeds; a failed run keeps it, with its logs), the archive path `<out-dir>/reference-outputs.tar.zst`, cargo's directory `<target>/mega-evm2-reference`, and, without `--check`, this directory.
The new archive is renamed over the archive path, so a link there is replaced and never written through.

**What the script refuses.** Before it builds or writes anything, `regen-reference.sh` exits with status 2 when `<out-dir>` is the corpus or lies inside it (a symlink to it counts), or when the archive path is a corpus archive or a link into the corpus.
A `--check` pointed at `<corpus>/reference` would replace the expected archive and then compare the new outputs with themselves.
Nothing else is guarded: the gate scripts only read the corpus, and the results that count are CI's, which downloads a fresh corpus.
A local corpus can be re-downloaded from the storage named by `MEGA_EVM2_CORPUS_URL`, or made read-only with `chmod -R a-w <corpus>`.
`--check` takes the expected outputs out of the corpus archive before it generates anything; replace the corpus archive by hand once the regenerated one is reviewed.
`scripts/check-regen-refusal.sh`, run by CI, checks the refusals on a scratch corpus, in both modes.

**Bumping the reference pin** (the `mega-evm` revision in `next/reference/Cargo.toml`) requires running `regen-reference.sh <out-dir>` and reviewing the resulting `git diff` of this directory:
every changed file is a behavior change of the reference, and must be explained by the change that moved the pin.
Replace the corpus archive with the new `reference-outputs.tar.zst` in the same change.
