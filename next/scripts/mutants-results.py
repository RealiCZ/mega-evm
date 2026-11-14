#!/usr/bin/env python3
"""The status of a run of next/scripts/mutants.sh, and the merge of the shards of a full audit.

A results directory is cargo-mutants' `mutants.out/`: caught.txt, missed.txt, timeout.txt and
unviable.txt name the mutants that have an outcome, one name (`file:line:col: change`) per line.
mutants.sh adds two files:

  inventory.txt  the mutants the run set out to test (`cargo mutants --list` of its selection,
                 taken before anything is built);
  status.json    the run's status against that inventory, and its identity: the commit, the
                 selection (`shard 3/8`, `pr <base>`, ...) and the cargo-mutants version.

The status is `complete` when every mutant of the inventory has an outcome (also when the
inventory is empty), `partial` when some have, `not-started` when none has or the inventory was
never listed, and `baseline-failed` when the oracle fails on the unmutated tree.

Subcommands:

  status  writes status.json (and inventory.txt) into a run's results directory and prints the
          MUTANTS-STATUS and MUTANTS-RESULT lines.
  merge   unions the results of the shards of one full audit, against an inventory and shard
          slices listed independently of the shards. A shard is accepted only when its identity
          (commit, selection, cargo-mutants version) is the audit's, its inventory is exactly its
          slice, and its outcomes lie within that slice, one outcome per mutant. The mutants of a
          missing or rejected shard count as missing, so the merged status is complete only when
          every mutant of the inventory has an outcome.
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

LISTS = ("caught", "missed", "timeout", "unviable")
STATUS = "status.json"
INVENTORY = "inventory.txt"


def read_lines(path: Path) -> list[str]:
    if not path.is_file():
        return []
    return [ln.strip() for ln in path.read_text().splitlines() if ln.strip()]


def write_lines(path: Path, items) -> None:
    path.write_text("".join(f"{i}\n" for i in sorted(set(items))))


def outcome_lists(results: Path) -> dict[str, list[str]]:
    return {name: read_lines(results / f"{name}.txt") for name in LISTS}


def contradictions(lists: dict[str, list[str]]) -> list[str]:
    """Mutants with more than one outcome."""
    seen: dict[str, str] = {}
    both = []
    for name, items in lists.items():
        for m in items:
            if m in seen and seen[m] != name:
                both.append(m)
            seen[m] = name
    return sorted(set(both))


def classify(expected: list[str] | None, outcomes: set[str], baseline_failed: bool) -> str:
    if baseline_failed:
        return "baseline-failed"
    if expected is None:
        return "not-started"
    if outcomes == set(expected):
        return "complete"
    if not outcomes:
        return "not-started"
    return "partial"


def lines(status: str, expected: list[str] | None, lists: dict[str, list[str]]) -> list[str]:
    outcomes = set().union(*lists.values())
    exp = "unknown" if expected is None else str(len(set(expected)))
    counts = " ".join(f"{name}={len(set(lists[name]))}" for name in LISTS)
    return [
        f"MUTANTS-STATUS status={status} expected={exp} outcomes={len(outcomes)}",
        f"MUTANTS-RESULT {counts} tested={len(outcomes)} skipped=0",
    ]


def cmd_status(args: argparse.Namespace) -> int:
    results = Path(args.results)
    results.mkdir(parents=True, exist_ok=True)
    lists = outcome_lists(results)
    expected = None
    if args.inventory:
        expected = read_lines(Path(args.inventory))
        write_lines(results / INVENTORY, expected)
    outcomes = set().union(*lists.values())
    status = classify(expected, outcomes, args.baseline_failed)
    outside = sorted(outcomes - set(expected)) if expected is not None else []
    if outside:
        # Outcomes the inventory does not list: the run did not test what it listed.
        print(f"ERROR: {len(outside)} outcomes are not in the inventory, for example {outside[0]}",
              file=sys.stderr)
        if status == "complete":
            status = "partial"
    both = contradictions(lists)
    if both:
        print(f"ERROR: {len(both)} mutants have more than one outcome, for example {both[0]}",
              file=sys.stderr)
    record = {
        "status": status,
        "expected": None if expected is None else len(set(expected)),
        "outcomes": len(outcomes),
        "counts": {name: len(set(lists[name])) for name in LISTS},
        "commit": args.commit,
        "dirty": args.dirty,
        "selection": args.selection,
        "tool": args.tool,
        "phase": args.phase,
        "stopped": args.stopped,
    }
    (results / STATUS).write_text(json.dumps(record, indent=2) + "\n")
    for line in lines(status, expected, lists):
        print(line)
    return 1 if (outside or both) else 0


def load_status(results: Path) -> dict | None:
    path = results / STATUS
    if not path.is_file():
        return None
    try:
        data = json.loads(path.read_text())
    except json.JSONDecodeError:
        return None
    return data if isinstance(data, dict) else None


def check_shard(k: int, n: int, results: Path | None, slice_: list[str], args) -> dict:
    """The verdict on shard k: accepted (with its lists) or not, and why."""
    verdict = {"shard": f"{k}/{n}", "expected": len(slice_), "outcomes": 0, "status": "missing",
               "accepted": False, "note": ""}
    if results is None or not results.is_dir():
        verdict["note"] = "no results (the shard's job never uploaded them)"
        return verdict
    status = load_status(results)
    if status is None:
        verdict["note"] = f"no readable {STATUS}"
        return verdict
    verdict["status"] = str(status.get("status"))
    want = {"commit": args.commit, "selection": f"shard {k}/{n}", "tool": args.tool}
    for key, value in want.items():
        if status.get(key) != value:
            verdict["note"] = f"{key} is {status.get(key)!r}, not {value!r}"
            return verdict
    if status.get("dirty"):
        verdict["note"] = "made from a tree with uncommitted changes"
        return verdict
    lists = outcome_lists(results)
    outcomes = set().union(*lists.values())
    verdict["outcomes"] = len(outcomes)
    if (results / INVENTORY).is_file():
        listed = read_lines(results / INVENTORY)
        if sorted(listed) != sorted(slice_):
            verdict["note"] = (f"its inventory ({len(listed)} mutants) is not its slice of the audit's "
                               f"inventory ({len(slice_)})")
            return verdict
    elif outcomes:
        verdict["note"] = f"outcomes without an {INVENTORY}"
        return verdict
    outside = outcomes - set(slice_)
    if outside:
        verdict["note"] = f"{len(outside)} outcomes outside its slice, for example {sorted(outside)[0]}"
        return verdict
    both = contradictions(lists)
    if both:
        verdict["note"] = f"{len(both)} mutants with more than one outcome, for example {both[0]}"
        return verdict
    verdict["accepted"] = True
    verdict["lists"] = lists
    if status.get("stopped"):
        verdict["note"] = f"stopped by the {status['stopped']} during {status.get('phase')}"
    return verdict


def cmd_merge(args: argparse.Namespace) -> int:
    inventory = read_lines(Path(args.inventory))
    if not inventory:
        print(f"ERROR: the audit's inventory {args.inventory} is empty", file=sys.stderr)
        return 2
    n = args.shards
    slices = [read_lines(Path(args.slices) / f"{k}.txt") for k in range(n)]
    flat = [m for s in slices for m in s]
    if len(flat) != len(set(flat)) or set(flat) != set(inventory) or len(set(inventory)) != len(inventory):
        print("ERROR: the shard slices do not partition the audit's inventory", file=sys.stderr)
        return 2
    dirs: dict[int, Path] = {}
    for spec in args.shard:
        k, _, path = spec.partition("=")
        dirs[int(k)] = Path(path)
    verdicts = [check_shard(k, n, dirs.get(k), slices[k], args) for k in range(n)]

    merged = {name: [] for name in LISTS}
    for v in verdicts:
        for name, items in v.pop("lists", {}).items():
            merged[name] += items
    outcomes = set().union(*merged.values())
    baseline = any(v["accepted"] and v["status"] == "baseline-failed" for v in verdicts)
    status = classify(inventory, outcomes, baseline)

    into = Path(args.into)
    into.mkdir(parents=True, exist_ok=True)
    for name in LISTS:
        write_lines(into / f"{name}.txt", merged[name])
    write_lines(into / INVENTORY, inventory)
    record = {
        "status": status,
        "expected": len(inventory),
        "outcomes": len(outcomes),
        "counts": {name: len(set(merged[name])) for name in LISTS},
        "commit": args.commit,
        "dirty": False,
        "selection": f"full ({n} shards)",
        "tool": args.tool,
        "shards": verdicts,
    }
    (into / STATUS).write_text(json.dumps(record, indent=2) + "\n")

    table = ["| shard | status | expected | outcomes | merged | note |", "|---|---|---|---|---|---|"]
    for v in verdicts:
        table.append(f"| {v['shard']} | {v['status']} | {v['expected']} | {v['outcomes']} | "
                     f"{'yes' if v['accepted'] else '**no**'} | {v['note']} |")
    missing = len(set(inventory) - outcomes)
    md = [f"### Full audit: {status}", "",
          f"{len(outcomes)} of {len(inventory)} mutants have an outcome; {missing} missing.", "",
          *table, ""]
    print("\n".join(md))
    if args.summary:
        with open(args.summary, "a") as fh:
            fh.write("\n".join(md) + "\n")
    for line in lines(status, inventory, merged):
        print(line)
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)

    ps = sub.add_parser("status", help="write status.json and print the status lines")
    ps.add_argument("--results", required=True)
    ps.add_argument("--inventory", help="the run's inventory; absent when it was never listed")
    ps.add_argument("--baseline-failed", action="store_true")
    ps.add_argument("--stopped", choices=("deadline", "signal"))
    ps.add_argument("--phase", default="")
    ps.add_argument("--selection", default="")
    ps.add_argument("--tool", default="")
    ps.add_argument("--commit")
    ps.add_argument("--dirty", action="store_true")
    ps.set_defaults(func=cmd_status)

    pm = sub.add_parser("merge", help="merge the shards of a full audit")
    pm.add_argument("--into", required=True)
    pm.add_argument("--inventory", required=True, help="the full inventory, listed by the merge")
    pm.add_argument("--slices", required=True, help="directory of <k>.txt, the slice of shard k")
    pm.add_argument("--shards", type=int, required=True)
    pm.add_argument("--shard", action="append", default=[], metavar="K=DIR",
                    help="the results directory of shard K (repeat)")
    pm.add_argument("--commit", required=True)
    pm.add_argument("--tool", required=True)
    pm.add_argument("--summary")
    pm.set_defaults(func=cmd_merge)

    args = p.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
