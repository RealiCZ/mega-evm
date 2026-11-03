# shellcheck shell=bash
# Sourced by gate.sh, controls.sh, faults.sh, check-reference.sh and regen-reference.sh.
#
# The six fuzz batches of the full gate, as name:mode:first..=last:count. `gate.sh full` runs the
# engine on them and `regen-reference.sh` generates their reference outputs, so both read this list.
FULL_FUZZ_BATCHES=(
  fq:3:1..=3000:3000 fqbig:3:3001..=13000:10000 fqh:4:1..=2000:2000
  fqhbig:4:2001..=7000:5000 big:1:401..=3400:3000 harsh:2:1..=2000:2000
)

# The counts of the main summary: the hand-written scenarios plus the default fuzz seeds, one
# known difference. This is the one place that states them. Adding a scenario means changing it
# here (and the data under `next/diff/data`, see `data/README.md`); `gate-lib.sh`,
# `check-reference.sh` and `regen-reference.sh` read it.
MAIN_EXPECT="total=707 match=706 known=1"
# The scenario count alone, as `MEGA_EVM2_EXPECT` takes it.
MAIN_TOTAL=${MAIN_EXPECT#total=}
MAIN_TOTAL=${MAIN_TOTAL%% *}

# abs_dir <dir>: creates <dir> when missing and prints its absolute path. Call it before any `cd`,
# so a relative path means the same thing for the whole script and for the cargo it starts.
abs_dir() {
  mkdir -p "$1"
  (cd "$1" && pwd)
}

# run_dir <dir> <name>: creates and prints a new directory `<dir>/runs/<name>.XXXXXX` for one run's
# logs and scratch files, so runs that share <dir> (two scripts on one cargo target) keep their
# files apart. Run directories under `<dir>/runs` older than a day are removed first.
run_dir() {
  mkdir -p "$1/runs"
  find "$1/runs" -mindepth 1 -maxdepth 1 -type d -mmin +1440 -exec rm -rf {} + 2> /dev/null || true
  mktemp -d "$1/runs/$2.XXXXXX"
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
