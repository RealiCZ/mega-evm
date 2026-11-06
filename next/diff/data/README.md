# Gate sample and full-corpus layout

`gate.sh sample` runs every gate on the files in this directory (about 2.4 MiB, zstd-compressed).
`gate.sh full` runs them on the full corpus under `MEGA_EVM2_CORPUS_DIR`.
`make-sample.sh` cuts this directory from the full corpus; `regen-reference.sh` calls it.

## Layout of this directory

| Path                              | Content                                                                                          |
| --------------------------------- | ------------------------------------------------------------------------------------------------ |
| `synthetic/main.json.zst`         | Reference outputs of the shared scenarios (the hand-written ones and the default fuzz seeds)     |
| `synthetic/fuzz-<batch>.json.zst` | Reference outputs of a sample fuzz batch (below)                                                 |
| `real-blocks/<set>/<n>.json.zst`  | Recorded block `n` of corpus `set` (`d4` or `p9`): header, transactions, receipts, sparse state  |
| `real-blocks/<set>/manifest.json` | The sample's blocks of that set, in the full corpus's manifest format                            |
| `replay/<set>/<n>.jsonl.zst`      | Reference output of the engine replay gate: one line per pre-block call and per transaction      |
| `block/<set>/<n>.jsonl.zst`       | Reference output of the block executor gate: per transaction, then block counters and post-state |
| `expected.env`                    | The replay and block counts `gate.sh` expects, sample and full                                   |

The sample's fuzz batches are `SAMPLE_FUZZ_BATCHES` in `scripts/common.sh`: `mode4` (mode 4, seeds 1..=2000, `fqh0001`..`fqh2000`) and the first 300 seeds of each class mode, `sys` (5), `collide` (6), `pre0` (7), `limits` (8), `destroy` (9), `refuse` (10), `frame` (11), `readfail` (12) and `stack` (13); each is cut from the full batch of its mode that starts at seed 1.
The main gate's expected count is `MAIN_EXPECT` in `scripts/common.sh`, the one place the scripts state it; `expected.env` holds the others, counted by `count_expected.py` from the reference files and `known-differences.toml`, independently of the gates.
Scenario names (such as `p6_dupn`), group keys (`p5`, `p6`, `p7`, `q2`, `fz`, `q8`, `cap`, `edge`, `bd`) and fuzz batch names (`fq`, `fqbig`, `fqh`, `fqhbig`, `big`, `harsh`, `sys`, `collide`, `pre0`, `limits`, `destroy`, `refuse`, `frame`, `readfail`, `stack`, and the sample's `mode4`) are stable data identifiers: the reference outputs and `known-differences.toml` are keyed on them.

## Selection of the sample blocks

Corpus `d4` holds blocks sampled across the whole history of the chain, so it covers the early specs; corpus `p9` is 200 consecutive blocks of the current spec.
The sample takes 14 blocks of `d4` and 6 of `p9`.
It leaves out the huge `d4` block 6780842 (21,153 transactions, 23 MiB); it is the only block with block data-size refusals, so the sample has none of those.

| Block (set:number)                          | Why                                                                                                                            |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `d4:2045117`                                | Smallest block with an EIP-7702 transaction (and a deposit)                                                                    |
| `d4:14924732`, `d4:18138664`                | EIP-7702 transaction together with system-address transactions; `18138664` also has block gas-limit refusals                   |
| `d4:19010529`, `d4:28102900`                | EIP-7702 transaction, system-address transactions and block gas-limit refusals                                                 |
| `d4:27670576`, `d4:27985540`                | EIP-7702 (`27985540` has two) with system-address transactions; a revert in `27670576`, halts and reverts in `27985540`        |
| `d4:7487457`, `d4:8064067`                  | The corpus's two keyless deployment transactions, plus nonce-too-low refusals                                                  |
| `d4:3224158`, `d4:10108963`                 | Early-spec blocks whose system-address transactions are refused as nonce-too-low (10 and 22); `10108963` also has two deposits |
| `d4:12747297`, `d4:22016777`, `d4:28107033` | System-address transactions, halts and reverts; two deposits in `22016777` and `28107033`                                      |
| `p9:26400000`                               | Current spec; block gas-limit refusals and 9 halts                                                                             |
| `p9:26400004`                               | Current spec; plain block with system-address transactions                                                                     |
| `p9:26400020`, `p9:26400077`                | Current spec; the sample's three nonce-too-high refusals, and block gas-limit refusals                                         |
| `p9:26400051`                               | Current spec; three block gas-limit refusals                                                                                   |
| `p9:26400193`                               | Current spec; a revert, system-address transactions                                                                            |

Together the sample holds every EIP-7702 transaction of the full corpus (8, in seven blocks) and both keyless deployments, a deposit in every block, and refusals for nonce too low, nonce too high and block gas limit.
Transactions from the system address are in 19 of the 20 blocks; 133 of them, in 13 blocks, have a gas limit of 2^32 or more.
In the full corpus those are the only transactions at or above 2^32: 2,367 legacy calls from the system address to the Oracle, in 230 of the 263 blocks, which both sides run as promoted deposits.

## Layout of the full corpus (`MEGA_EVM2_CORPUS_DIR`)

```text
real-blocks/
  d4/manifest.json, d4/<n>.json     63 blocks across the chain's history (the manifest lists them)
  p9/manifest.json, p9/<n>.json     200 consecutive blocks of the current spec
  <set>/<n>.perf.json               measurement inputs (not used by the gates)
  rot/                              5 variant blocks for the registry rotation (not part of the gates)
reference/
  reference-outputs.tar.zst         reference outputs, extracted by `gate.sh full` (made by `regen-reference.sh`;
                                    when absent the original `q11-oracles.tar.zst` is read instead):
    satin-oracle.json               synthetic main
    oracles/<batch>.json            the fifteen fuzz batches, 52,000 scenarios in total
    raw/satin/<set>/<n>.jsonl       engine replay reference
    raw/satin-block/<set>/<n>.jsonl block executor reference
```

The fifteen fuzz batches (mode, seeds), `FULL_FUZZ_BATCHES` in `scripts/common.sh`: `fq` 3 / 1..=3000, `fqbig` 3 / 3001..=13000, `fqh` 4 / 1..=2000, `fqhbig` 4 / 2001..=7000, `big` 1 / 401..=3400, `harsh` 2 / 1..=2000, and seeds 1..=3000 of each class mode: `sys` 5 (the live system address as sender, every envelope), `collide` 6 (creations onto occupied addresses), `pre0` 7 (depth-0 precompile calls with the sender or recipient as beneficiary), `limits` 8 (limits and SALT capacities from 0 to `u64::MAX`), `destroy` 9 (a creation address funded, created and destroyed), `refuse` 10 (refusals with the frame gas around the charges before the load), `frame` 11 (every envelope and recipient with the gas limit around each charge before and at the first frame), `readfail` 12 (an instruction that reads volatile data, then fails) and `stack` 13 (short stacks under detention).
Modes 1 to 4 are random compositions, modes 5 to 10 are described in `shared/src/fz/classes.rs`, modes 11 to 13 in `shared/src/fz/regions.rs`.

## Regenerating the reference outputs

The reference outputs come from the Satin implementation under review (`next/reference`, pinned in `next/reference/Cargo.toml`), not from this repository's engine.

From `next/`:

```bash
bash diff/scripts/check-reference.sh sample                                         # the sample's outputs, compared with this directory
MEGA_EVM2_CORPUS_DIR=<corpus> bash diff/scripts/regen-reference.sh --check <out-dir>  # the whole corpus, compared with its archive
MEGA_EVM2_CORPUS_DIR=<corpus> bash diff/scripts/regen-reference.sh <out-dir>          # also re-cuts this sample and rewrites expected.env
```

`regen-reference.sh` packs `<out-dir>/reference-outputs.tar.zst` in the layout above.
With `--check` it writes nothing into the repository and prints `REFERENCE full identical` when every regenerated file equals the corpus archive's (only `micros` is ignored).
`<out-dir>` must lie outside the corpus: the script refuses, before it builds anything, an output directory in the corpus or an archive path that is or links into it, so a `--check` cannot replace the archive it compares with; `scripts/check-regen-refusal.sh` tests that refusal in CI.
Nothing else is guarded: the gates only read the corpus, and CI downloads a fresh one.

When a scenario is added, update `MAIN_EXPECT` and regenerate.
When the reference pin moves, regenerate and review the resulting `git diff` of this directory: every changed file is a behavior change of the reference and must be explained by the change that moved the pin.
Either way, replace the corpus archive by hand with the new `reference-outputs.tar.zst` in the same change: `gate.sh full` reads its expected outputs from it.
