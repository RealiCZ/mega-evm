#!/usr/bin/env bash
# Run the pinned Mega EEST reviewed slice and wrong-expectation control on this checkout.
# MEGA_EEST_REPO may override the clone source (including a local repository).
# CARGO_TARGET_DIR selects the build cache; the suite always gets a fresh temporary clone.
set -euo pipefail

next=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
root=$(cd "$next/.." && pwd -P)
repo_override=${MEGA_EEST_REPO:-}
# shellcheck source=../mega-eest.env
source "$next/mega-eest.env"
repo=${repo_override:-$MEGA_EEST_REPO}
if [[ ! "$MEGA_EEST_REV" =~ ^[0-9a-f]{40}$ ]]; then
  echo "mega-eest-gate: MEGA_EEST_REV must be a full commit SHA" >&2
  exit 1
fi

if [ ! -f "$root/../evm2/crates/evm2/Cargo.toml" ]; then
  echo "mega-eest-gate: check out the fork with next/scripts/fetch-evm2-fork.sh first" >&2
  exit 1
fi
fork=$(cd "$root/../evm2" && pwd -P)
implementation=$(git -C "$root" rev-parse HEAD)
fork_revision=$(git -C "$fork" rev-parse HEAD)
target=${CARGO_TARGET_DIR:-$next/target}/mega-eest
mkdir -p "$target"
target=$(cd "$target" && pwd -P)
work=$(mktemp -d "$target/run.XXXXXX")
trap 'rm -rf "$work"' EXIT
suite=$work/suite

git clone --quiet --no-checkout -- "$repo" "$suite"
git -C "$suite" -c advice.detachedHead=false checkout --quiet --detach "$MEGA_EEST_REV"
test "$(git -C "$suite" rev-parse HEAD)" = "$MEGA_EEST_REV"

# Reproduce the suite's vendor layout without fetching its pinned implementation.
# Cargo reads both dependencies through these links; build output stays under target/.
mkdir -p "$suite/vendor"
ln -s "$root" "$suite/vendor/mega-evm"
ln -s "$fork" "$suite/vendor/evm2"
(
  # Select this checkout's toolchain for the consumer and adapter alike.
  cd "$next"
  CARGO_TARGET_DIR="$target/format" cargo build --quiet --locked \
    --manifest-path "$suite/Cargo.toml" -p mega-fixture-format --bin mega-fixture
  CARGO_TARGET_DIR="$target/adapter-evm2" cargo build --quiet --locked \
    --manifest-path "$suite/adapters/evm2/Cargo.toml" --bin adapter-evm2
) >&2
python3 -I "$suite/tools/make_slice.py" --check >&2

cli=$target/format/debug/mega-fixture
adapter=$target/adapter-evm2/debug/adapter-evm2
slice_status=0
"$cli" consume --adapter "$adapter" --specs "$suite/specs" \
  --report "$work/slice.json" "$suite/slice/cases" > "$work/slice.log" || slice_status=$?
negative_status=0
"$cli" consume --adapter "$adapter" --specs "$suite/specs" \
  --report "$work/negative.json" "$suite/slice/negative" > "$work/negative.log" || negative_status=$?

# Check actual report identities and the complete indexed case sets, so an empty or
# partial run cannot pass. A negative run must fail at its deliberately wrong field.
if ! python3 -I - "$work" "$implementation" "$fork_revision" "$MEGA_EEST_REV" \
    "$slice_status" "$negative_status" <<'PY'
import collections
import json
import pathlib
import sys

work = pathlib.Path(sys.argv[1])
implementation, fork_revision, suite_revision = sys.argv[2:5]
slice_status, negative_status = map(int, sys.argv[5:7])


def load(path):
    return json.loads(path.read_text(encoding="utf-8"))


def checked_cases(report, entries):
    identity = report["adapter"]["implementation"]
    if identity["commit"] != implementation or identity["evm2Commit"] != fork_revision:
        raise ValueError("adapter was not built from this checkout and its adjacent fork")
    cases = report["cases"]
    expected_ids = collections.Counter(entry["caseId"] for entry in entries)
    if not entries or collections.Counter(case["caseId"] for case in cases) != expected_ids:
        raise ValueError("report does not contain the complete indexed case set")
    return cases


index = load(work / "suite/slice/index.json")
cases = checked_cases(load(work / "slice.json"), index["cases"])
negative = checked_cases(load(work / "negative.json"), index["negative"])
counts = collections.Counter(case["status"] for case in cases)
wrong_at = {entry["caseId"]: entry["wrongAt"] for entry in index["negative"]}
negative_ok = negative_status == 1 and all(
    case["status"] == "fail" and case.get("path") == wrong_at[case["caseId"]]
    for case in negative
)
print(f"MEGA-EEST implementation={implementation} suite={suite_revision}")
print(f"MEGA-EEST adapter=evm2 cases={len(cases)} pass={counts['pass']} "
      f"fail={counts['fail']} unsupported={counts['unsupported']} error={counts['error']}")
print(f"NEGATIVE wrong-expected verdict={'fail' if negative_ok else 'unexpected'}")
sys.exit(0 if slice_status == 0 and counts["pass"] == len(cases) and negative_ok else 1)
PY
then
  cat "$work/slice.log" "$work/negative.log" >&2
  exit 1
fi
