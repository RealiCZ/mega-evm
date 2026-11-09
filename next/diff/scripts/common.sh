# shellcheck shell=bash
# Sourced by gate.sh, controls.sh, faults.sh, check-reference.sh, regen-reference.sh and
# make-sample.sh.

# The fuzz batches of the full gate, as name:mode:first..=last:count. `gate.sh full` runs the
# engine on them and `regen-reference.sh` generates their reference outputs, so both read this list.
# Modes 1 to 4 are random compositions (mode 3's `fqsig` seeds also make a keyless deployment's
# signer the sender; mode 7's `pre0kzg` seeds price KZG at 100,000); modes 5 to 10 each aim at one class of edge inputs (the generator's
# `fz/classes.rs`), modes 11 to 13 at a region no earlier mode drew (`fz/regions.rs`), mode 14 at
# two mechanisms in one transaction, every pair 50 times (`fz/pairs.rs`).
FULL_FUZZ_BATCHES=(
  fq:3:1..=3000:3000 fqbig:3:3001..=13000:10000 fqsig:3:13001..=14000:1000 fqh:4:1..=2000:2000
  fqhbig:4:2001..=7000:5000 big:1:401..=3400:3000 harsh:2:1..=2000:2000
  sys:5:1..=3000:3000 collide:6:1..=3000:3000 pre0:7:1..=3000:3000 pre0kzg:7:3001..=4000:1000
  limits:8:1..=3000:3000 destroy:9:1..=3000:3000 refuse:10:1..=3000:3000
  frame:11:1..=3000:3000 readfail:12:1..=3000:3000 stack:13:1..=3000:3000
  pairs:14:1..=3300:3300 blocks:15:1..=3000:9014
)

# The fuzz batches of the sample, in the same format: each holds the first seeds of the full
# batch of its mode that starts at seed 1, from which `make-sample.sh` cuts it. Its reference
# outputs are `data/synthetic/fuzz-<name>.json.zst`.
SAMPLE_FUZZ_BATCHES=(
  mode4:4:1..=2000:2000
  sys:5:1..=300:300 collide:6:1..=300:300 pre0:7:1..=300:300
  limits:8:1..=300:300 destroy:9:1..=300:300 refuse:10:1..=300:300
  frame:11:1..=300:300 readfail:12:1..=300:300 stack:13:1..=300:300
  pairs:14:1..=330:330 blocks:15:1..=150:452
)

# The counts of the main summary: the hand-written scenarios plus the default fuzz seeds, five of
# them listed differences and the rest of the differing ones accepted by a class
# (`known-differences.toml`). This is the one place that states them. Adding a scenario or a known
# difference means changing it here (and the data under `next/diff/data`, see `data/README.md`);
# `gate-lib.sh`, `check-reference.sh` and `regen-reference.sh` read it.
MAIN_EXPECT="total=2915 match=1124 known=5 class=1786"
# The fuzz seeds the accepted classes take, summed over the batches of the sample and of the full
# gate (mode 11's first-frame window, mode 14's pairs and mode 15's blocks, `halted_first_frame`;
# mode 14's pairs, `modexp_formula_price`; mode 14's pairs and mode 4's seeds,
# `auth_preflight_reads`; every mode's seeds that give state or history gas back or create state,
# `reservoir_give_back`; every mode's deposits, `deposit_history`; the Oracle reads, refused reads,
# keyless calls and hints of the generators, the classes of those decisions); every other seed must
# match.
SAMPLE_FUZZ_CLASS=2191
FULL_FUZZ_CLASS=24883
# The replay transactions the classes the replay gate runs again for take (`gate::REPLAY_RERUN_CLASSES`:
# `reservoir_give_back`, and `deposit_history` for every recorded deposit), in the sample and in the
# full gate. `count_expected.py` counts them among the applied transactions (`match`), as it counts
# from the reference's files alone; `gate_inputs` moves them to `class`.
SAMPLE_REPLAY_RERUN_CLASS=205
FULL_REPLAY_RERUN_CLASS=9118
# The blocks the block executor gate runs again, whole, under those rules and accepts as a class's
# (`tests/block.rs`): every recorded block, each opening with a deposit. Their rows count in the
# summary as the rerun's verdicts, so their number is pinned here, apart from it.
SAMPLE_BLOCK_RERUN_CLASS=20
FULL_BLOCK_RERUN_CLASS=263
# The scenario count alone, as `MEGA_EVM2_EXPECT` takes it.
MAIN_TOTAL=${MAIN_EXPECT#total=}
MAIN_TOTAL=${MAIN_TOTAL%% *}

# abs_dir <dir>: creates <dir> when missing and prints its absolute path. Call it before any `cd`,
# so a relative path means the same thing for the whole script and for the cargo it starts.
abs_dir() {
  mkdir -p "$1"
  (cd "$1" && pwd)
}

# rename_exact <from> <to>: rename(2), which replaces <to> itself even when it is a link to a
# directory (where `mv` would move <from> into that directory, writing through the link).
rename_exact() { python3 -I -c 'import os, sys; os.replace(sys.argv[1], sys.argv[2])' "$1" "$2"; }

# fresh_dir <dir>: empties <dir> (a script's logs and scratch files), creates it and prints it.
fresh_dir() {
  rm -rf "${1:?}"
  mkdir -p "$1"
  echo "$1"
}

# refuse_overrides: exits 2 when the environment carries any `MEGA_EVM2_*` variable except
# `MEGA_EVM2_CORPUS_DIR`. Those variables select or replace what a gate runs on (blocks, sets,
# scenarios, fuzz seeds, reference and output paths, expected counts, engine switches); a script
# that prints the official `GATE` lines must set them itself. A direct `cargo test` with them stays
# available for debugging.
refuse_overrides() {
  local name
  while IFS='=' read -r name _; do
    case "$name" in
      MEGA_EVM2_CORPUS_DIR) ;;
      MEGA_EVM2_*)
        echo "$(basename "$0"): $name is set; unset it. The official runner decides what is gated, an external selection or override would make its result mean something else." >&2
        exit 2
        ;;
    esac
  done < <(env)
}
