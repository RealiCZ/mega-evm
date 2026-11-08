#!/bin/bash
# Rule coverage of the scenarios: how many rule ids of the Satin specification have at least one
# scenario that names them, and which have none.
#
# Usage: rule-coverage.sh <rules.md>
#   <rules.md>  the specification's rule index (`docs/spec/satin/rules.md`): every rule id is the
#               first cell of a table row, written `SATIN-<AREA>-<number>` between backticks.
#
# A scenario names the rules it exercises in `Scenario::rules` (`diff/shared/src`); the script
# reads the ids from the quoted `"SATIN-..."` strings of those sources, so an id written in a
# comment does not count. Retired ids (rows without a link) are left out.
#
# Prints:
#   RULES total=<n> covered=<n> uncovered=<n>
#   UNCOVERED <id>        one line per rule id no scenario names
#   UNKNOWN <id>          one line per id a scenario names that the index does not list
# Exit status 0, also when rules are uncovered; 2 when the index cannot be read.
set -euo pipefail

[ $# = 1 ] && [ -f "$1" ] || { echo "usage: rule-coverage.sh <rules.md>" >&2; exit 2; }
here=$(cd "$(dirname "$0")/.." && pwd)
python3 -I - "$1" "$here/shared/src" << 'EOF_PY'
import os, re, sys

index, src = sys.argv[1:3]
rules = []
with open(index, encoding="utf-8") as f:
    for line in f:
        m = re.match(r"\| \[`(SATIN-[A-Z0-9]+-[0-9]+)`\]\(", line)
        if m:
            rules.append(m.group(1))
if not rules:
    sys.exit("rule-coverage.sh: no rule id in " + index)
named = set()
for root, _, files in os.walk(src):
    for name in files:
        if name.endswith(".rs"):
            with open(os.path.join(root, name), encoding="utf-8") as f:
                named.update(re.findall(r'"(SATIN-[A-Z0-9]+-[0-9]+)"', f.read()))
known = set(rules)
covered = [r for r in rules if r in named]
uncovered = [r for r in rules if r not in named]
print(f"RULES total={len(rules)} covered={len(covered)} uncovered={len(uncovered)}")
for r in uncovered:
    print("UNCOVERED", r)
for r in sorted(named - known):
    print("UNKNOWN", r)
EOF_PY
