#!/usr/bin/env python3
"""TEMPORARY (validation only): per-mutant build/test times of a cargo-mutants run.

    python3 scripts/mutation_bench_summary.py <mutants.out dir> [label]

"cold" is the first mutant built in a freshly copied scratch directory other
than the baseline's (cargo-mutants' copy resets mtimes, so that build also
recompiles stale dependencies); "warm" is every later mutant in its directory.
"""
import json
import re
import statistics
import sys
from collections import defaultdict
from datetime import datetime
from pathlib import Path

out = Path(sys.argv[1])
label = sys.argv[2] if len(sys.argv) > 2 else str(out)
d = json.loads((out / "outcomes.json").read_text())

# Which build directory tested which mutant, in order (from cargo-mutants' debug log).
dirs, order = [], defaultdict(list)
for line in (out / "debug.log").read_text().splitlines():
    m = re.search(r'Copied source tree .* temp_dir="([^"]+)"', line)
    if m:
        dirs.append(m.group(1))
    m = re.search(r'build_dir="([^"]+)".*Apply mutant self=Mutant \{ name: "((?:[^"\\]|\\.)*)"', line)
    if m:
        order[m.group(1)].append(m.group(2).replace('\\"', '"'))
baseline_dir = dirs[0] if dirs else None
cold = {names[0] for dir_, names in order.items() if dir_ != baseline_dir and names}

rows = defaultdict(list)
for o in d["outcomes"]:
    ph = {p["phase"]: p["duration"] for p in o["phase_results"]}
    if o["scenario"] == "Baseline":
        kind = "baseline"
    else:
        name = o["scenario"]["Mutant"]["name"]
        kind = f'{o["summary"]} ({"cold" if name in cold else "warm"})'
    rows[kind].append((ph.get("Build", 0.0), ph.get("Test", 0.0)))


def stat(xs):
    return f"{statistics.mean(xs):.1f} / {statistics.median(xs):.1f}"


print(f"### bench: {label}\n")
print("| outcome | n | build s (mean / median) | test s (mean / median) |")
print("|---|---|---|---|")
for k, v in sorted(rows.items()):
    print(f"| {k} | {len(v)} | {stat([b for b, _ in v])} | {stat([t for _, t in v])} |")
mutants = [(b, t) for k, v in rows.items() if k != "baseline" for b, t in v]
print(f"| all mutants | {len(mutants)} | {stat([b for b, _ in mutants])} | {stat([t for _, t in mutants])} |")
if d.get("end_time"):
    t = lambda x: datetime.fromisoformat(x.replace("Z", "+00:00"))
    print(f"\ncargo-mutants wall clock: {(t(d['end_time']) - t(d['start_time'])).total_seconds():.0f} s, "
          f"{len(order)} build dirs")
print()
