# shellcheck shell=bash
# Sourced by gate.sh, controls.sh and faults.sh (after common.sh): the inputs of the gates and the
# runs of the four gates, so the official run, the negative controls and the fault injection
# accept and reject on the same code.
#
# A work directory holds the inputs of one run, in the same layout for both modes:
#   blocks/<set>/{manifest.json,<n>.json}   recorded blocks (full: a link to the corpus's)
#   replay/<set>/<n>.jsonl                  reference, per transaction
#   block/<set>/<n>.jsonl                   reference, block level
#   main.json                               reference, hand-written and default fuzz scenarios
#   fuzz/<name>.json                        reference, fuzz batches
#   expect.env                              what the summaries must be (written by `gate_inputs`):
#     EXPECT_MAIN    the counts of the main summary, `total=<n> match=<n> known=<n>`
#     FUZZ_BATCHES   the fuzz batches, space-separated `name:mode:first..=last:count`
#     EXPECT_REPLAY  the counts of the replay summary before `differ=0 applied=0`
#     EXPECT_BLOCK   the counts of the block summary before `differ=0 applied=0`

gate_lib_diff=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
gate_lib_next=$(cd "$gate_lib_diff/.." && pwd)
# shellcheck source=stamp.sh
source "$gate_lib_diff/scripts/stamp.sh"

# The counts of the main summary: the hand-written scenarios plus the default fuzz seeds, one
# known difference.
MAIN_EXPECT="total=707 match=706 known=1"

# gate_inputs sample|full <work>: fills <work> with the inputs of the sample (`next/diff/data`)
# or of the full corpus ($MEGA_EVM2_CORPUS_DIR) and writes <work>/expect.env. The extracted files
# are reused while the sample (or the corpus archive) and this file are unchanged.
gate_inputs() {
  local mode=$1 work=$2 stamp corpus archive batches
  mkdir -p "$work"
  if [ "$mode" = sample ]; then
    bash "$gate_lib_diff/scripts/check-stamp.sh" > /dev/null
    # Every file of the sample, and this file (it defines the extracted layout).
    stamp=$(stamp_dir "$gate_lib_diff/data")-$(stamp_file "$gate_lib_diff/scripts/gate-lib.sh")
    if [ "$(cat "$work/.stamp" 2>/dev/null)" != "$stamp" ]; then
      rm -rf "$work/blocks" "$work/replay" "$work/block" "$work/fuzz" "$work/main.json"
      mkdir -p "$work/fuzz"
      gate_unz "$gate_lib_diff/data/synthetic/main.json.zst" "$work/main.json"
      gate_unz "$gate_lib_diff/data/synthetic/fuzz-mode4.json.zst" "$work/fuzz/mode4.json"
      local kind src dst f
      for kind in real-blocks:blocks replay:replay block:block; do
        src=${kind%%:*}
        dst=${kind##*:}
        (cd "$gate_lib_diff/data/$src" && find . -type f) | while read -r f; do
          f=${f#./}
          mkdir -p "$work/$dst/$(dirname "$f")"
          case "$f" in
            *.zst) gate_unz "$gate_lib_diff/data/$src/$f" "$work/$dst/${f%.zst}" ;;
            *) cp "$gate_lib_diff/data/$src/$f" "$work/$dst/$f" ;;
          esac
        done
      done
      echo "$stamp" > "$work/.stamp"
    fi
    batches="mode4:4:1..=2000:2000"
  else
    : "${MEGA_EVM2_CORPUS_DIR:?set MEGA_EVM2_CORPUS_DIR to the full corpus}"
    corpus=$(cd "$MEGA_EVM2_CORPUS_DIR" && pwd)
    # A regenerated archive (`regen-reference.sh`) wins over the original one.
    archive=$corpus/reference/reference-outputs.tar.zst
    [ -f "$archive" ] || archive=$corpus/reference/q11-oracles.tar.zst
    stamp=$(stamp_file "$archive")-$(stamp_file "$gate_lib_diff/scripts/gate-lib.sh")
    if [ "$(cat "$work/.stamp" 2>/dev/null)" != "$stamp" ]; then
      rm -rf "$work/extract" "$work/replay" "$work/block" "$work/fuzz" "$work/main.json"
      mkdir -p "$work/extract" "$work/fuzz"
      tar --zstd -xf "$archive" -C "$work/extract" satin-oracle.json oracles raw/satin raw/satin-block
      mv "$work/extract/satin-oracle.json" "$work/main.json"
      mv "$work/extract/raw/satin" "$work/replay"
      mv "$work/extract/raw/satin-block" "$work/block"
      local name
      for name in fq fqbig fqh fqhbig big harsh; do mv "$work/extract/oracles/$name.json" "$work/fuzz/$name.json"; done
      rm -rf "$work/extract"
      echo "$stamp" > "$work/.stamp"
    fi
    # The corpus directory may move between runs of the same archive: link it on every run.
    rm -rf "$work/blocks"
    ln -s "$corpus/real-blocks" "$work/blocks"
    batches="${FULL_FUZZ_BATCHES[*]}"
  fi
  (
    # shellcheck disable=SC1091
    source "$gate_lib_diff/data/expected.env"
    replay=$EXPECT_REPLAY block=$EXPECT_BLOCK
    [ "$mode" = full ] && replay=$EXPECT_FULL_REPLAY block=$EXPECT_FULL_BLOCK
    printf '%s="%s"\n' EXPECT_MAIN "$MAIN_EXPECT" FUZZ_BATCHES "$batches" \
      EXPECT_REPLAY "$replay" EXPECT_BLOCK "$block"
  ) > "$work/expect.env"
}

gate_unz() { zstd -q -d -f -o "$2" "$1"; }

# gate_env <work> <gate> [<batch>]: sets the array GATE_ENV to the environment of a run of <gate>
# (main, fuzz, replay or block) on the inputs under <work>; a fuzz run names its batch
# (`name:mode:first..=last:count`). Expected counts are not part of it.
gate_env() {
  local work=$1 name fmode range
  case "$2" in
    main) GATE_ENV=(MEGA_EVM2_ORACLE="$work/main.json" MEGA_EVM2_GATE_NAME=main) ;;
    fuzz)
      IFS=: read -r name fmode range _ <<< "$3"
      GATE_ENV=(MEGA_EVM2_ORACLE="$work/fuzz/$name.json" MEGA_EVM2_GATE_NAME=fuzz MEGA_EVM2_FUZZ_ONLY=1
        MEGA_EVM2_FUZZ_MODE="$fmode" MEGA_EVM2_FUZZ="$range")
      ;;
    replay | block)
      GATE_ENV=(MEGA_EVM2_BLOCK_DIR="$work/blocks" MEGA_EVM2_REPLAY_REF="$work/replay"
        MEGA_EVM2_BLOCK_REF="$work/block" MEGA_EVM2_EXPECT_APPLIED=0)
      ;;
    *) echo "gate_env: unknown gate $2" >&2; return 2 ;;
  esac
}

# gate_test <log> <test target> [VAR=value...]: runs one gate test of `mega-evm2-diff` with the
# given environment, output to <log>; returns the test's exit status.
gate_test() {
  local log=$1 test=$2
  shift 2
  (cd "$gate_lib_next" && env RUST_MIN_STACK=8388608 "$@" \
    cargo test --release --locked -p mega-evm2-diff --test "$test" -- --nocapture) > "$log" 2>&1
}

# gate_summary <log> <test target> [VAR=value...]: runs the test and prints its single `GATE`
# line. A failing test or a log without exactly one `GATE` line returns 1, after the tail of the
# log on stderr.
gate_summary() {
  local log=$1 line
  if ! gate_test "$@"; then
    echo "gate $(basename "$log" .log) failed; tail of $log:" >&2
    tail -n 25 "$log" >&2
    return 1
  fi
  line=$(grep '^GATE ' "$log" || true)
  if [ "$(echo "$line" | grep -c '^GATE ')" != 1 ]; then
    echo "gate $(basename "$log" .log) printed no single GATE line (see $log)" >&2
    return 1
  fi
  echo "$line"
}

# gate_run <work> <logs> <gate> [VAR=value...]: runs <gate> (main, fuzz, replay or block) on the
# inputs under <work> and prints its summary line (nothing when its test failed); returns 0 only
# when the summary is the one <work>/expect.env expects. The assignments are added to the
# environment of every test of the gate.
gate_run() {
  local work=$1 logs=$2 gate=$3 line
  shift 3
  mkdir -p "$logs"
  # shellcheck disable=SC1091
  source "$work/expect.env"
  case "$gate" in
    main)
      local count=${EXPECT_MAIN#total=}
      gate_env "$work" main
      line=$(gate_summary "$logs/main.log" synthetic "${GATE_ENV[@]}" MEGA_EVM2_EXPECT="${count%% *}" "$@") || return 1
      echo "$line"
      [ "$line" = "GATE main $EXPECT_MAIN differ=0" ]
      ;;
    fuzz)
      # The batches summed into one line; a failed batch counts as one difference.
      local spec total=0 matched=0 differ=0 expected=0 t m k d
      for spec in $FUZZ_BATCHES; do
        gate_env "$work" fuzz "$spec"
        expected=$((expected + ${spec##*:}))
        if line=$(gate_summary "$logs/fuzz-${spec%%:*}.log" synthetic "${GATE_ENV[@]}" MEGA_EVM2_EXPECT="${spec##*:}" "$@"); then
          t=$(sed -E 's/.* total=([0-9]+) .*/\1/' <<< "$line")
          m=$(sed -E 's/.* match=([0-9]+) .*/\1/' <<< "$line")
          k=$(sed -E 's/.* known=([0-9]+) .*/\1/' <<< "$line")
          d=$(sed -E 's/.* differ=([0-9]+).*/\1/' <<< "$line")
          total=$((total + t))
          matched=$((matched + m))
          differ=$((differ + d + k))
        else
          differ=$((differ + 1))
        fi
      done
      echo "GATE fuzz total=$total match=$matched differ=$differ"
      [ "$total" = "$expected" ] && [ "$matched" = "$total" ] && [ "$differ" = 0 ]
      ;;
    replay | block)
      local expect=$EXPECT_REPLAY
      [ "$gate" = block ] && expect=$EXPECT_BLOCK
      gate_env "$work" "$gate"
      line=$(gate_summary "$logs/$gate.log" "$gate" "${GATE_ENV[@]}" "$@") || return 1
      echo "$line"
      [ "$line" = "GATE $gate $expect differ=0 applied=0" ]
      ;;
    *) echo "gate_run: unknown gate $gate" >&2; return 2 ;;
  esac
}
