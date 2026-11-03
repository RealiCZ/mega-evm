# Gate sample and full-corpus layout

`gate.sh sample` runs every gate on the files in this directory (about 2.5 MiB, zstd-compressed).
`gate.sh full` runs them on the full corpus under `MEGA_EVM2_CORPUS_DIR`.
`make-sample.sh` rebuilds this directory from the full corpus.

## Layout of this directory

| Path                              | Content                                                                                          |
| --------------------------------- | ------------------------------------------------------------------------------------------------ |
| `synthetic/main.json.zst`         | Reference outputs of the 698 shared scenarios (hand-written ones plus 400 default fuzz seeds)    |
| `synthetic/fuzz-mode4.json.zst`   | Reference outputs of fuzz mode 4, seeds 1..=2000 (`fqh0001`..`fqh2000`)                          |
| `real-blocks/<set>/<n>.json.zst`  | Recorded block `n` of corpus `set` (`d4` or `p9`): header, transactions, receipts, sparse state  |
| `real-blocks/<set>/manifest.json` | The sample's blocks of that set, in the full corpus's manifest format                            |
| `replay/<set>/<n>.jsonl.zst`      | Reference output of the engine replay gate: one line per pre-block call and per transaction      |
| `block/<set>/<n>.jsonl.zst`       | Reference output of the block executor gate: per transaction, then block counters and post-state |
| `expected.env`                    | Summary counts `gate.sh sample` expects, counted from the reference files                        |

Scenario names (such as `p6_dupn`), group keys (`p5`, `p6`, `p7`, `q2`, `fz`, `q8`) and fuzz batch names (`fq`, `fqbig`, `fqh`, `fqhbig`, `big`, `harsh`) are stable data identifiers: the reference outputs and `known-differences.toml` are keyed on them, so they keep their names.

Expected counts of the sample: 698 main scenarios (697 match, 1 known difference), 2,000 fuzz scenarios;
20 blocks with 499 matching transactions, 40 pre-block calls and 52 refusals in the engine replay;
479 included and 72 refused transactions in the block executor.

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
    satin-oracle.json               synthetic main (698 scenarios)
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
  It compares the regenerated outputs with `reference/q11-oracles.tar.zst` of the corpus (only `micros` is ignored) and the full counts with `expected.env`, and prints `REFERENCE full identical`.
- Without `--check` it also re-cuts this sample with `make-sample.sh` from the new archive and rewrites `expected.env` from the regenerated files.

**Bumping the reference pin** (the `mega-evm` revision in `next/reference/Cargo.toml`) requires running `regen-reference.sh <out-dir>` and reviewing the resulting `git diff` of this directory:
every changed file is a behavior change of the reference, and must be explained by the change that moved the pin.
Replace the corpus archive with the new `reference-outputs.tar.zst` in the same change.
