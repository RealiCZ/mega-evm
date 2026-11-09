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
#     EXPECT_MAIN    the counts of the main summary, `total=<n> match=<n> known=<n> class=<n>`
#     FUZZ_BATCHES   the fuzz batches, space-separated `name:mode:first..=last:count`
#     FUZZ_CLASS     the seeds of all fuzz batches the class rule accepts
#     EXPECT_REPLAY  the counts of the replay summary before `differ=0`, the transactions a rerun
#                    class takes moved from `match` to `class` (`rerun_class`)
#     EXPECT_BLOCK   the counts of the block summary before `differ=0`
#     EXPECT_BLOCK_RERUN  the blocks the block gate takes by running them again (`common.sh`)
#
# Every script extracts its inputs into its own work directory, fresh on every run, and keeps its
# logs in a sibling directory cleared at the start of the run.

gate_lib_diff=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
gate_lib_next=$(cd "$gate_lib_diff/.." && pwd)

# gate_inputs sample|full <work>: extracts the inputs of the sample (`next/diff/data`) or of the
# full corpus ($MEGA_EVM2_CORPUS_DIR) into <work> and writes <work>/expect.env. <work> is emptied
# first, so an interrupted extraction or a stale input never survives into a run.
gate_inputs() {
  local mode=$1 work=$2 corpus archive batches fuzz_class blocks=""
  fresh_dir "$work" > /dev/null
  if [ "$mode" = sample ]; then
    batches="${SAMPLE_FUZZ_BATCHES[*]}"
    fuzz_class=$SAMPLE_FUZZ_CLASS
  else
    : "${MEGA_EVM2_CORPUS_DIR:?set MEGA_EVM2_CORPUS_DIR to the full corpus}"
    corpus=$(cd -P "$MEGA_EVM2_CORPUS_DIR" && pwd -P)
    # A regenerated archive (`regen-reference.sh`) wins over the original one.
    archive=$corpus/reference/reference-outputs.tar.zst
    [ -f "$archive" ] || archive=$corpus/reference/q11-oracles.tar.zst
    # The corpus directory may move between runs of the same archive: the link follows it.
    blocks=$corpus/real-blocks
    batches="${FULL_FUZZ_BATCHES[*]}"
    fuzz_class=$FULL_FUZZ_CLASS
  fi
  gate_extract "$mode" "$work" "${archive:-}"
  [ -z "$blocks" ] || ln -s "$blocks" "$work/blocks"
  (
    # shellcheck disable=SC1091
    source "$gate_lib_diff/data/expected.env"
    replay=$EXPECT_REPLAY block=$EXPECT_BLOCK rerun=$SAMPLE_REPLAY_RERUN_CLASS
    reruns=$SAMPLE_BLOCK_RERUN_CLASS
    if [ "$mode" = full ]; then
      replay=$EXPECT_FULL_REPLAY block=$EXPECT_FULL_BLOCK rerun=$FULL_REPLAY_RERUN_CLASS
      reruns=$FULL_BLOCK_RERUN_CLASS
    fi
    replay=$(rerun_class "$replay" "$rerun")
    printf '%s="%s"\n' EXPECT_MAIN "$MAIN_EXPECT" FUZZ_BATCHES "$batches" \
      FUZZ_CLASS "$fuzz_class" EXPECT_REPLAY "$replay" EXPECT_BLOCK "$block" \
      EXPECT_BLOCK_RERUN "$reruns"
  ) > "$work/expect.env"
}

# rerun_class <counts> <n>: the replay counts <counts> (`... match=<m> ... class=<c>`) with <n>
# transactions moved from `match` to `class`: the transactions of a class the replay gate checks by
# running them again (`common.sh`), which the reference's files alone cannot tell from a match.
rerun_class() {
  local m c
  m=$(sed -E 's/.* match=([0-9]+) .*/\1/' <<< "$1")
  c=$(sed -E 's/.* class=([0-9]+)$/\1/' <<< "$1")
  sed -E "s/ match=[0-9]+ / match=$((m - $2)) /; s/ class=[0-9]+\$/ class=$((c + $2))/" <<< "$1"
}

# gate_extract sample|full <work> [<archive>]: extracts the inputs into the empty directory <work>
# (the blocks too for the sample; the full corpus's blocks are a link).
gate_extract() {
  local mode=$1 work=$2 archive=${3:-}
  mkdir -p "$work"
  if [ "$mode" = sample ]; then
    mkdir -p "$work/fuzz"
    gate_unz "$gate_lib_diff/data/synthetic/main.json.zst" "$work/main.json"
    local kind src dst f spec name
    for spec in "${SAMPLE_FUZZ_BATCHES[@]}"; do
      name=${spec%%:*}
      gate_unz "$gate_lib_diff/data/synthetic/fuzz-$name.json.zst" "$work/fuzz/$name.json"
    done
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
  else
    mkdir -p "$work/extract" "$work/fuzz"
    tar --zstd -xf "$archive" -C "$work/extract" satin-oracle.json oracles raw/satin raw/satin-block
    mv "$work/extract/satin-oracle.json" "$work/main.json"
    mv "$work/extract/raw/satin" "$work/replay"
    mv "$work/extract/raw/satin-block" "$work/block"
    local spec name
    for spec in "${FULL_FUZZ_BATCHES[@]}"; do
      name=${spec%%:*}
      mv "$work/extract/oracles/$name.json" "$work/fuzz/$name.json"
    done
    rm -rf "$work/extract"
  fi
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
        MEGA_EVM2_BLOCK_REF="$work/block")
      ;;
    *) echo "gate_env: unknown gate $2" >&2; return 2 ;;
  esac
}

# gate_test <log> <test target> [VAR=value...]: runs one gate test of `mega-evm2-diff` with the
# given environment, output to <log>, its exit status to <log>.status; returns that status.
gate_test() {
  local log=$1 test=$2 status=0
  shift 2
  (cd "$gate_lib_next" && env RUST_MIN_STACK=8388608 "$@" \
    cargo test --release --locked -p mega-evm2-diff --test "$test" -- --nocapture) > "$log" 2>&1 ||
    status=$?
  echo "$status" > "$log.status"
  return "$status"
}

# gate_summary <log> <test target> [VAR=value...]: runs the test and prints its single `GATE`
# line (the block gate's `GATE block-satin` line, the engine's own figures, is not its summary). A
# failing test or a log without exactly one `GATE` line returns 1, after the tail of the log on
# stderr.
gate_summary() {
  local log=$1 line
  if ! gate_test "$@"; then
    echo "gate $(basename "$log" .log) failed; tail of $log:" >&2
    tail -n 25 "$log" >&2
    return 1
  fi
  line=$(grep '^GATE ' "$log" | grep -v '^GATE block-satin ' || true)
  if [ "$(echo "$line" | grep -c '^GATE ')" != 1 ]; then
    echo "gate $(basename "$log" .log) printed no single GATE line (see $log)" >&2
    return 1
  fi
  echo "$line"
}

# gate_run <work> <logs> <gate> [VAR=value...]: runs <gate> (main, fuzz, replay or block) on the
# inputs under <work> and prints its summary line (nothing when its test failed); returns 0 only
# when the summary is the one <work>/expect.env expects. The block gate also prints the line of
# the Satin-rule run's figures, `GATE block-satin blocks=<b> included=<i> refused=<r> pinned=<b>
# differ=0`, which must be there with every block pinned and none differing. The assignments are
# added to the environment of every test of the gate.
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
      # The batches summed into one line; a failed batch counts as one difference, and so does a
      # known difference (no fuzz seed has an entry).
      local spec total=0 matched=0 class=0 differ=0 expected=0 t m k c d
      for spec in $FUZZ_BATCHES; do
        gate_env "$work" fuzz "$spec"
        expected=$((expected + ${spec##*:}))
        if line=$(gate_summary "$logs/fuzz-${spec%%:*}.log" synthetic "${GATE_ENV[@]}" MEGA_EVM2_EXPECT="${spec##*:}" "$@"); then
          t=$(sed -E 's/.* total=([0-9]+) .*/\1/' <<< "$line")
          m=$(sed -E 's/.* match=([0-9]+) .*/\1/' <<< "$line")
          k=$(sed -E 's/.* known=([0-9]+) .*/\1/' <<< "$line")
          c=$(sed -E 's/.* class=([0-9]+) .*/\1/' <<< "$line")
          d=$(sed -E 's/.* differ=([0-9]+).*/\1/' <<< "$line")
          total=$((total + t))
          matched=$((matched + m))
          class=$((class + c))
          differ=$((differ + d + k))
        else
          differ=$((differ + 1))
        fi
      done
      echo "GATE fuzz total=$total match=$matched class=$class differ=$differ"
      [ "$total" = "$expected" ] && [ "$class" = "$FUZZ_CLASS" ] &&
        [ "$((matched + class))" = "$total" ] && [ "$differ" = 0 ]
      ;;
    replay | block)
      local expect=$EXPECT_REPLAY extra=()
      if [ "$gate" = block ]; then
        expect=$EXPECT_BLOCK
        extra=(MEGA_EVM2_EXPECT_RERUN="$EXPECT_BLOCK_RERUN")
      fi
      gate_env "$work" "$gate"
      line=$(gate_summary "$logs/$gate.log" "$gate" "${GATE_ENV[@]}" ${extra[@]+"${extra[@]}"} "$@") || return 1
      echo "$line"
      if [ "$gate" = block ]; then
        local satin blocks=${expect#blocks=}
        satin=$(grep '^GATE block-satin ' "$logs/$gate.log" || true)
        echo "${satin:-GATE block-satin MISSING}"
        grep -Eq "^GATE block-satin blocks=${blocks%% *} included=[0-9]+ refused=[0-9]+ pinned=${blocks%% *} differ=0\$" <<< "$satin" || return 1
      fi
      [ "$line" = "GATE $gate $expect differ=0" ]
      ;;
    *) echo "gate_run: unknown gate $gate" >&2; return 2 ;;
  esac
}
