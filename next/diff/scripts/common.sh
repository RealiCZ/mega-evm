# shellcheck shell=bash
# Sourced by gate.sh, controls.sh, faults.sh, check-reference.sh, regen-reference.sh and
# make-sample.sh.

# The fuzz batches of the full gate, as name:mode:first..=last:count. `gate.sh full` runs the
# engine on them and `regen-reference.sh` generates their reference outputs, so both read this list.
# Modes 1 to 4 are random compositions; modes 5 to 10 each aim at one class of edge inputs (the
# generator's `fz/classes.rs`).
FULL_FUZZ_BATCHES=(
  fq:3:1..=3000:3000 fqbig:3:3001..=13000:10000 fqh:4:1..=2000:2000
  fqhbig:4:2001..=7000:5000 big:1:401..=3400:3000 harsh:2:1..=2000:2000
  sys:5:1..=3000:3000 collide:6:1..=3000:3000 pre0:7:1..=3000:3000
  limits:8:1..=3000:3000 destroy:9:1..=3000:3000 refuse:10:1..=3000:3000
)

# The fuzz batches of the sample, in the same format: each holds the first seeds of the full
# batch of its mode that starts at seed 1, from which `make-sample.sh` cuts it. Its reference
# outputs are `data/synthetic/fuzz-<name>.json.zst`.
SAMPLE_FUZZ_BATCHES=(
  mode4:4:1..=2000:2000
  sys:5:1..=300:300 collide:6:1..=300:300 pre0:7:1..=300:300
  limits:8:1..=300:300 destroy:9:1..=300:300 refuse:10:1..=300:300
)

# The counts of the main summary: the hand-written scenarios plus the default fuzz seeds, five of
# them known differences (`known-differences.toml`). This is the one place that states them.
# Adding a scenario or a known difference means changing it here (and the data under
# `next/diff/data`, see `data/README.md`); `gate-lib.sh`, `check-reference.sh` and
# `regen-reference.sh` read it.
MAIN_EXPECT="total=1055 match=1050 known=5"
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
