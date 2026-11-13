#!/usr/bin/env python3
"""Tests of scripts/mutation_gate.py. Plain python3, no pytest:

    python3 -I scripts/test_mutation_gate.py
"""
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

GATE = Path(__file__).resolve().parent / "mutation_gate.py"

A = "src/a.rs:10:5: replace foo -> bool with true"
B = "src/a.rs:20:9: replace + with - in bar"
C = "src/b.rs:3:1: replace baz with ()"
D = "src/b.rs:7:13: replace == with != in qux"
U = "src/b.rs:9:5: replace qux -> u8 with 0"


def run(*args: str) -> tuple[int, str, str]:
    p = subprocess.run([sys.executable, "-I", str(GATE), *args], capture_output=True, text=True)
    return p.returncode, p.stdout, p.stderr


class Results:
    """A mutants.out directory in a temporary directory."""

    def __init__(self, tmp: Path, **files: list[str] | None) -> None:
        self.dir = tmp / "mutants.out"
        self.dir.mkdir()
        for name in ("caught", "missed", "timeout", "unviable"):
            files.setdefault(name, [])
        for name, lines in files.items():
            if lines is None:
                continue
            if name == "mutants":
                (self.dir / "mutants.json").write_text(json.dumps([{"name": n} for n in lines]))
            else:
                (self.dir / f"{name}.txt").write_text("".join(f"{x}\n" for x in lines))

    def report(self, *extra: str) -> tuple[int, str, str]:
        return run("report", "--results", str(self.dir), *extra)


class ReportTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def suppressions(self, body: str) -> str:
        p = self.tmp / "suppressions.toml"
        p.write_text(body)
        return str(p)

    def test_absent_results_pass_as_nothing_mutated(self) -> None:
        rc, out, _ = run("report", "--results", str(self.tmp / "none"))
        self.assertEqual(rc, 0)
        self.assertIn("nothing was mutated", out)

    def test_results_without_the_lists_fail_closed(self) -> None:
        (self.tmp / "mutants.out").mkdir()
        rc, _, err = run("report", "--results", str(self.tmp / "mutants.out"))
        self.assertEqual(rc, 2)
        self.assertIn("refusing", err)

    def test_all_caught_passes(self) -> None:
        rc, out, _ = Results(self.tmp, caught=[A, B]).report()
        self.assertEqual(rc, 0)
        self.assertIn("**Diff mutation score: 100.0%** (2/2 viable mutants killed)", out)

    def test_a_survivor_fails(self) -> None:
        rc, out, _ = Results(self.tmp, caught=[A], missed=[B]).report()
        self.assertEqual(rc, 1)
        self.assertIn(f"- `{B}`", out)

    def test_a_line_suppressed_survivor_passes(self) -> None:
        supp = self.suppressions(
            f'[[suppress]]\nkind = "line"\nfile = "src/a.rs"\nmutant = "{B.split(": ", 1)[1]}"\n'
        )
        rc, out, _ = Results(self.tmp, caught=[A], missed=[B]).report("--suppressions", supp)
        self.assertEqual(rc, 0)
        self.assertIn("(1/1 viable mutants killed)", out)

    def test_a_timeout_fails(self) -> None:
        rc, out, _ = Results(self.tmp, caught=[A], timeout=[B]).report()
        self.assertEqual(rc, 1)
        self.assertIn("Timed-out mutants", out)

    def test_nothing_viable_says_nothing_to_test(self) -> None:
        rc, out, _ = Results(self.tmp, unviable=[U]).report()
        self.assertEqual(rc, 0)
        self.assertIn("Nothing to test", out)

    def test_carried_over_caught_and_unviable_are_scored(self) -> None:
        # This run caught C and missed D; an earlier run caught A and B and found U unviable.
        r = Results(self.tmp, caught=[C], missed=[D], previously_caught=[A, B, U],
                    previously_unviable=[U])
        rc, out, _ = r.report()
        self.assertEqual(rc, 1)
        self.assertIn("(3/4 viable mutants killed)", out)
        self.assertIn("- caught: 3 (2 carried over from earlier runs)", out)
        self.assertIn("- unviable: 1 ·", out)

    def test_a_carried_over_run_that_tested_nothing_is_not_nothing_to_test(self) -> None:
        rc, out, _ = Results(self.tmp, previously_caught=[A, B]).report()
        self.assertEqual(rc, 0)
        self.assertNotIn("Nothing to test", out)
        self.assertIn("(2/2 viable mutants killed)", out)

    def test_carried_over_without_the_unviable_list_counts_all_as_caught(self) -> None:
        rc, out, _ = Results(self.tmp, previously_caught=[A, U]).report()
        self.assertEqual(rc, 0)
        self.assertIn("- caught: 2 (2 carried over from earlier runs)", out)

    def test_unreached_mutants_are_reported_and_gate_only_when_required(self) -> None:
        r = Results(self.tmp, caught=[A], mutants=[A, B, C])
        rc, out, _ = r.report()
        self.assertEqual(rc, 0)
        self.assertIn("- not reached in this run: **2**", out)
        rc, out, err = r.report("--require-complete")
        self.assertEqual(rc, 2)
        self.assertIn("INCOMPLETE", out)
        self.assertIn("never tested", err)

    def test_a_run_stopped_before_any_outcome_is_not_nothing_to_test(self) -> None:
        rc, out, _ = Results(self.tmp, mutants=[A, B]).report("--require-complete")
        self.assertEqual(rc, 2)
        self.assertNotIn("Nothing to test", out)
        self.assertIn("- not reached in this run: **2**", out)

    def test_a_complete_plan_adds_no_line(self) -> None:
        rc, out, _ = Results(self.tmp, caught=[A], unviable=[U], mutants=[A, U]).report(
            "--require-complete")
        self.assertEqual(rc, 0)
        self.assertNotIn("not reached", out)

    def test_scope_labels_the_score(self) -> None:
        _, out, _ = Results(self.tmp, caught=[A]).report("--scope", "Full-engine")
        self.assertIn("**Full-engine mutation score: 100.0%**", out)

    def test_summary_is_appended(self) -> None:
        summary = self.tmp / "summary.md"
        summary.write_text("before\n")
        Results(self.tmp, caught=[A]).report("--summary", str(summary))
        text = summary.read_text()
        self.assertTrue(text.startswith("before\n## 🧬 Mutation testing"))


class SuppressionTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def test_exclude_re_prints_one_pair_per_function_suppression(self) -> None:
        p = self.tmp / "s.toml"
        p.write_text('[[suppress]]\nkind = "function"\nfile = "a"\npattern = "a\\\\.rs:1:"\n'
                     '[[suppress]]\nkind = "line"\nfile = "b"\nmutant = "x"\n')
        rc, out, _ = run("exclude-re", "--suppressions", str(p))
        self.assertEqual(rc, 0)
        self.assertEqual(out.splitlines(), ["--exclude-re", "a\\.rs:1:"])

    def test_orphans_flags_a_suppression_matching_nothing(self) -> None:
        p = self.tmp / "s.toml"
        p.write_text('[[suppress]]\nkind = "function"\nfile = "a"\npattern = "nowhere"\n')
        u = self.tmp / "universe.txt"
        u.write_text(f"{A}\n")
        rc, out, _ = run("orphans", "--suppressions", str(p), "--universe", str(u))
        self.assertEqual(rc, 1)
        self.assertIn("stale", out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
