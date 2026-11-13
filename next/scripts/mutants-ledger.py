#!/usr/bin/env python3
"""Carry cargo-mutants results over from one run of next/scripts/mutants.sh to the next.

A results directory is cargo-mutants' `mutants.out/`: caught.txt, missed.txt, timeout.txt and
unviable.txt hold the mutants this run tested, one name (`file:line:col: change`) per line, and
mutants.json the mutants it set out to test. With `--iterate`, cargo-mutants skips every mutant
named in caught.txt, unviable.txt or previously_caught.txt of the directory it writes into, and
writes the skipped names to the new directory's previously_caught.txt. This helper adds three
files to that directory:

  previously_unviable.txt  the unviable part of previously_caught.txt (cargo-mutants mixes the
                           caught and the unviable there), so the carried-over caught set can be
                           scored apart from the unviable one;
  carry-base.txt           the commit the names refer to (absent when the engine sources had
                           uncommitted changes), from which the next run finds the code that changed;
  refreshed.txt            the refresh requests (next/diff/mutants/refresh.txt) already applied.

Subcommands:

  prepare   the starting point of a carried-over run: the names of the previous results,
            moved to their current lines through the diff since carry-base.txt, less those in
            changed code, those no longer generated and those a refresh drops. Written as the
            seed `<stage>/mutants.out/previously_caught.txt`, which cargo-mutants reads.
  finalize  makes the run's `<stage>/mutants.out` the new `<out>/mutants.out` (the previous one
            becomes mutants.out.old) and adds the three files above.
  tally     prints the `MUTANTS-RESULT ...` line.
  merge     unions the results of the shards of one run into one directory.
  timing    build and test seconds per outcome of one run, or of the mutants two runs share.
"""
from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import shutil
import statistics
import sys
from pathlib import Path

LISTS = ("caught", "missed", "timeout", "unviable")
PREV_CAUGHT = "previously_caught.txt"
PREV_UNVIABLE = "previously_unviable.txt"
CARRY_BASE = "carry-base.txt"
REFRESHED = "refreshed.txt"

NAME_RE = re.compile(r"^(?P<file>[^:]+):(?P<line>\d+):(?P<col>\d+): (?P<rest>.*)$")
HUNK_RE = re.compile(r"^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")


def read_lines(path: Path) -> list[str]:
    if not path.is_file():
        return []
    return [ln.strip() for ln in path.read_text().splitlines() if ln.strip()]


def read_set(path: Path) -> set[str]:
    return set(read_lines(path))


def write_lines(path: Path, items) -> None:
    items = sorted(set(items))
    path.write_text("".join(f"{i}\n" for i in items))


def name_file(name: str) -> str:
    return name.split(":", 1)[0]


# ---- the diff since the recorded commit -------------------------------------------------------


class FileDiff:
    """The line mapping of one file of a unified diff, old side to new side."""

    def __init__(self) -> None:
        self.deleted = False
        # (old_start, old_len, new_start, new_len, {old line: new line or None})
        self.hunks: list[tuple[int, int, int, int, dict[int, int | None]]] = []

    def map_line(self, old: int) -> int | None:
        if self.deleted:
            return None
        delta = 0
        for old_start, old_len, _new_start, new_len, explicit in self.hunks:
            if old_len > 0 and old_start <= old < old_start + old_len:
                return explicit.get(old)
            # A hunk with no old lines inserts after line old_start; a hunk with old lines
            # ends at old_start + old_len - 1. Lines after either are shifted by it.
            end = old_start if old_len == 0 else old_start + old_len - 1
            if old > end:
                delta += new_len - old_len
        return old + delta


def parse_diff(text: str) -> dict[str, FileDiff]:
    """Parse `git diff --no-renames` output into a line mapping per old path."""
    files: dict[str, FileDiff] = {}
    current: FileDiff | None = None
    old_line = new_line = 0
    explicit: dict[int, int | None] = {}
    in_hunk = False
    old_path: str | None = None
    for raw in text.splitlines():
        if raw.startswith("diff --git "):
            current, in_hunk, old_path = None, False, None
            continue
        if not in_hunk and raw.startswith("--- "):
            p = raw[4:].strip()
            old_path = None if p == "/dev/null" else p[2:] if p.startswith("a/") else p
            continue
        if not in_hunk and raw.startswith("+++ "):
            p = raw[4:].strip()
            if old_path is not None:
                current = files.setdefault(old_path, FileDiff())
                current.deleted = p == "/dev/null"
            continue
        m = HUNK_RE.match(raw)
        if m:
            in_hunk = current is not None
            if not in_hunk:
                continue
            old_start, old_len = int(m.group(1)), int(m.group(2) or "1")
            new_start, new_len = int(m.group(3)), int(m.group(4) or "1")
            explicit = {}
            current.hunks.append((old_start, old_len, new_start, new_len, explicit))
            old_line, new_line = old_start, new_start
            continue
        if not in_hunk or current is None:
            continue
        if raw.startswith(" "):
            explicit[old_line] = new_line
            old_line += 1
            new_line += 1
        elif raw.startswith("-"):
            explicit[old_line] = None
            old_line += 1
        elif raw.startswith("+"):
            new_line += 1
        elif raw.startswith("\\"):
            pass
        else:
            in_hunk = False
    return files


def translate(name: str, diffs: dict[str, FileDiff]) -> str | None:
    """The name of the same mutant on the new side of the diff, or None when its line changed."""
    m = NAME_RE.match(name)
    if not m:
        return None
    fd = diffs.get(m.group("file"))
    if fd is None:
        return name
    line = fd.map_line(int(m.group("line")))
    if line is None:
        return None
    return f"{m.group('file')}:{line}:{m.group('col')}: {m.group('rest')}"


# ---- refresh requests ------------------------------------------------------------------------


def request_lines(path: Path | None) -> list[str]:
    """The request lines of a refresh file: every non-blank line with its comment removed."""
    if path is None or not path.is_file():
        return []
    out = []
    for raw in path.read_text().splitlines():
        line = raw.split("#", 1)[0].strip()
        if line:
            out.append(" ".join(line.split()))
    return out


def request_glob(line: str) -> str:
    """A request is `<date> <glob>`; the glob is its last field."""
    return line.split()[-1]


def glob_matches(glob: str, path: str) -> bool:
    """cargo-mutants' rule for `--file`: a glob with a slash matches the whole path, one
    without matches the file name. `all` and `*` match everything."""
    if glob in ("all", "*", "**"):
        return True
    if "/" in glob:
        return fnmatch.fnmatchcase(path, glob)
    return fnmatch.fnmatchcase(path.rsplit("/", 1)[-1], glob)


# ---- prepare / finalize ----------------------------------------------------------------------


def carried_sets(results: Path) -> tuple[set[str], set[str]]:
    """(every carried-over name, the unviable ones) of a results directory."""
    caught = read_set(results / "caught.txt")
    unviable = read_set(results / "unviable.txt")
    prev = read_set(results / PREV_CAUGHT)
    prev_unviable = read_set(results / PREV_UNVIABLE)
    everything = caught | unviable | prev
    return everything, (unviable | prev_unviable) & everything


def cmd_prepare(args: argparse.Namespace) -> int:
    results, stage = Path(args.results), Path(args.stage)
    everything, unviable = carried_sets(results)

    dropped = {"changed": 0, "gone": 0, "refresh": 0}
    if args.diff:
        diffs = parse_diff(Path(args.diff).read_text(errors="replace"))
        moved, moved_unviable = set(), set()
        for n in everything:
            t = translate(n, diffs)
            if t is None:
                dropped["changed"] += 1
                continue
            moved.add(t)
            if n in unviable:
                moved_unviable.add(t)
        everything, unviable = moved, moved_unviable
    changed = read_set(Path(args.changed)) if args.changed else set()
    universe = read_set(Path(args.universe)) if args.universe else None

    requests = request_lines(Path(args.refresh_file)) if args.refresh_file else []
    applied = read_set(results / REFRESHED)
    globs = [g for g in (args.refresh or []) if g]
    new_requests = [r for r in requests if r not in applied]
    globs += [request_glob(r) for r in new_requests]

    kept = set()
    for n in everything:
        if n in changed:
            dropped["changed"] += 1
        elif universe is not None and n not in universe:
            dropped["gone"] += 1
        elif any(glob_matches(g, name_file(n)) for g in globs):
            dropped["refresh"] += 1
        else:
            kept.add(n)
    kept_unviable = unviable & kept

    seed = stage / "mutants.out"
    seed.mkdir(parents=True, exist_ok=True)
    write_lines(seed / PREV_CAUGHT, kept)
    carry = stage / "carry"
    carry.mkdir(parents=True, exist_ok=True)
    write_lines(carry / PREV_UNVIABLE, kept_unviable)
    write_lines(carry / REFRESHED, applied | set(requests))

    print(
        f"carry-over: {len(kept) - len(kept_unviable)} caught and {len(kept_unviable)} unviable "
        f"mutants carried over; dropped {dropped['changed']} in changed code, "
        f"{dropped['gone']} no longer generated, {dropped['refresh']} by a refresh"
        + (f" ({', '.join(globs)})" if globs else "")
    )
    return 0


def cmd_finalize(args: argparse.Namespace) -> int:
    stage, out = Path(args.stage), Path(args.out)
    run = stage / "mutants.out"
    if not run.is_dir():
        shutil.rmtree(stage, ignore_errors=True)
        return 0
    carry = stage / "carry"
    prev = read_set(run / PREV_CAUGHT)
    write_lines(run / PREV_UNVIABLE, read_set(carry / PREV_UNVIABLE) & prev)
    requests = request_lines(Path(args.refresh_file)) if args.refresh_file else []
    write_lines(run / REFRESHED, read_set(carry / REFRESHED) | set(requests))
    if args.base:
        (run / CARRY_BASE).write_text(args.base + "\n")
    current, old = out / "mutants.out", out / "mutants.out.old"
    if current.exists():
        shutil.rmtree(old, ignore_errors=True)
        os.replace(current, old)
    os.replace(run, current)
    shutil.rmtree(stage, ignore_errors=True)
    return 0


# ---- tally -----------------------------------------------------------------------------------


def tally(results: Path, carried_only: bool = False) -> dict[str, int]:
    zero = {k: 0 for k in ("caught", "missed", "timeout", "unviable", "tested", "skipped")}
    if not results.is_dir():
        return zero
    if carried_only:
        everything, unviable = carried_sets(results)
        new = {k: set() for k in LISTS}
    else:
        everything = read_set(results / PREV_CAUGHT)
        unviable = read_set(results / PREV_UNVIABLE) & everything
        new = {k: read_set(results / f"{k}.txt") for k in LISTS}
    carried_caught = everything - unviable
    return {
        "caught": len(new["caught"]) + len(carried_caught),
        "missed": len(new["missed"]),
        "timeout": len(new["timeout"]),
        "unviable": len(new["unviable"]) + len(unviable),
        "tested": sum(len(v) for v in new.values()),
        "skipped": len(everything),
    }


def cmd_tally(args: argparse.Namespace) -> int:
    t = tally(Path(args.results), args.carried_only) if args.results else tally(Path("/nonexistent"))
    print("MUTANTS-RESULT " + " ".join(f"{k}={v}" for k, v in t.items()))
    return 0


# ---- merge -----------------------------------------------------------------------------------


def cmd_merge(args: argparse.Namespace) -> int:
    into = Path(args.into)
    if into.exists() and any(into.iterdir()):
        print(f"{into} is not empty", file=sys.stderr)
        return 2
    # Only the results of this commit: a shard that ended before cargo-mutants wrote anything
    # still holds the directory it started from, whose names refer to an older commit.
    srcs = []
    for s in map(Path, args.sources):
        base = "".join(read_lines(s / CARRY_BASE))
        if args.base and base != args.base:
            print(f"skipped {s}: its results are of commit {base or '(none recorded)'}, "
                  f"not {args.base}", file=sys.stderr)
            continue
        srcs.append(s)
    if not srcs:
        print("no shard results of this commit to merge", file=sys.stderr)
        return 1
    into.mkdir(parents=True, exist_ok=True)
    lists = {k: set() for k in LISTS}
    prev, prev_unviable, refreshed, bases = set(), set(), set(), set()
    planned: dict[str, dict] = {}
    for s in srcs:
        for k in LISTS:
            lists[k] |= read_set(s / f"{k}.txt")
        prev |= read_set(s / PREV_CAUGHT)
        prev_unviable |= read_set(s / PREV_UNVIABLE)
        refreshed |= read_set(s / REFRESHED)
        bases |= read_set(s / CARRY_BASE) or {""}
        mj = s / "mutants.json"
        if mj.is_file():
            for m in json.loads(mj.read_text() or "[]"):
                planned.setdefault(m.get("name", ""), m)
    # Shards test disjoint mutants; a name caught anywhere is not a survivor.
    lists["missed"] -= lists["caught"]
    lists["timeout"] -= lists["caught"] | lists["missed"]
    for k in LISTS:
        write_lines(into / f"{k}.txt", lists[k])
    write_lines(into / PREV_CAUGHT, prev)
    write_lines(into / PREV_UNVIABLE, prev_unviable & prev)
    write_lines(into / REFRESHED, refreshed)
    if len(bases) == 1 and "" not in bases:
        (into / CARRY_BASE).write_text(next(iter(bases)) + "\n")
    else:
        print(f"the shards record different commits ({sorted(bases)}); the next run carries "
              "over by name only", file=sys.stderr)
    (into / "mutants.json").write_text(json.dumps(list(planned.values()), indent=1))
    t = tally(into)
    print(f"merged {len(srcs)} shard results into {into}: "
          + " ".join(f"{k}={v}" for k, v in t.items()))
    return 0


# ---- timing ----------------------------------------------------------------------------------


def outcome_times(results: Path) -> dict[str, tuple[str, float, float]]:
    """name -> (summary, build seconds, test seconds) from outcomes.json."""
    data = json.loads((results / "outcomes.json").read_text())
    out = {}
    for o in data.get("outcomes", []):
        sc = o.get("scenario")
        if not isinstance(sc, dict) or "Mutant" not in sc:
            continue
        build = test = 0.0
        for ph in o.get("phase_results", []):
            if ph.get("phase") == "Build":
                build += ph.get("duration", 0.0)
            elif ph.get("phase") == "Test":
                test += ph.get("duration", 0.0)
        out[sc["Mutant"]["name"]] = (o.get("summary", "?"), build, test)
    return out


SUMMARIES = (("CaughtMutant", "caught"), ("MissedMutant", "missed"),
             ("Unviable", "unviable"), ("Timeout", "timeout"))


def timing_rows(times: dict[str, tuple[str, float, float]], names) -> list[str]:
    rows = []
    for key, label in SUMMARIES:
        xs = [times[n] for n in names if times[n][0] == key]
        if not xs:
            continue
        b = [x[1] for x in xs]
        t = [x[2] for x in xs]
        rows.append(f"{label:9} n={len(xs):4}  build mean {statistics.mean(b):6.1f} s median "
                    f"{statistics.median(b):6.1f} s   test mean {statistics.mean(t):6.1f} s median "
                    f"{statistics.median(t):6.1f} s")
    return rows


def cmd_timing(args: argparse.Namespace) -> int:
    a = outcome_times(Path(args.results))
    if not args.compare:
        print("\n".join(timing_rows(a, a.keys())))
        return 0
    b = outcome_times(Path(args.compare))
    shared = [n for n in a if n in b]
    same = [n for n in shared if a[n][0] == b[n][0]]
    print(f"{len(shared)} mutants in both runs, {len(same)} with the same outcome")
    print(f"--- {args.results}")
    print("\n".join(timing_rows(a, same)))
    print(f"--- {args.compare}")
    print("\n".join(timing_rows(b, same)))
    return 0


def main() -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)

    pp = sub.add_parser("prepare", help="the seed of a carried-over run")
    pp.add_argument("--results", required=True, help="the previous mutants.out (may be absent)")
    pp.add_argument("--stage", required=True, help="the directory cargo-mutants will write into")
    pp.add_argument("--universe", help="every mutant name the run's filters generate (cargo mutants --list)")
    pp.add_argument("--diff", help="git diff --no-renames from carry-base.txt to the tree")
    pp.add_argument("--changed", help="the names --in-diff of that diff selects (cargo mutants --list --in-diff)")
    pp.add_argument("--refresh", action="append", help="drop the carried results of files matching this glob (`all`)")
    pp.add_argument("--refresh-file", help="refresh requests, applied once")
    pp.set_defaults(func=cmd_prepare)

    pf = sub.add_parser("finalize", help="make the run's results the new mutants.out")
    pf.add_argument("--stage", required=True)
    pf.add_argument("--out", required=True, help="the directory holding mutants.out")
    pf.add_argument("--base", help="the commit the names refer to")
    pf.add_argument("--refresh-file", help="refresh requests: a fresh run has applied them all")
    pf.set_defaults(func=cmd_finalize)

    pt = sub.add_parser("tally", help="print the MUTANTS-RESULT line")
    pt.add_argument("--results", help="a mutants.out; absent means nothing ran")
    pt.add_argument("--carried-only", action="store_true",
                    help="this run tested nothing: every result in the directory is carried over")
    pt.set_defaults(func=cmd_tally)

    pm = sub.add_parser("merge", help="union shard results")
    pm.add_argument("--into", required=True)
    pm.add_argument("--base", help="merge only results recorded at this commit")
    pm.add_argument("sources", nargs="*")
    pm.set_defaults(func=cmd_merge)

    pg = sub.add_parser("timing", help="build and test seconds per outcome")
    pg.add_argument("--results", required=True)
    pg.add_argument("--compare", help="a second run: compare the mutants both tested")
    pg.set_defaults(func=cmd_timing)

    args = p.parse_args()
    return args.func(args)


if __name__ == "__main__":
    raise SystemExit(main())
