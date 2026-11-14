#!/usr/bin/env python3
"""Tests of next/scripts/mutants-results.py. Plain python3, no pytest:

    python3 -I next/scripts/test_mutants_results.py
"""
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HELPER = Path(__file__).resolve().parent / "mutants-results.py"
COMMIT = "a" * 40
TOOL = "cargo-mutants 27.1.0"

# Eight mutants, four shards of two.
M = [f"mega-evm2/src/x.rs:{i}:5: replace f{i} -> bool with true" for i in range(1, 9)]
SLICES = [M[0:2], M[2:4], M[4:6], M[6:8]]


def run(*args: str) -> tuple[int, str, str]:
    p = subprocess.run([sys.executable, "-I", str(HELPER), *args], capture_output=True, text=True)
    return p.returncode, p.stdout, p.stderr


def write(path: Path, items) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(f"{i}\n" for i in items))
    return path


def line(out: str, prefix: str) -> str:
    return next(ln for ln in out.splitlines() if ln.startswith(prefix))


class Base(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def results(self, name: str, **lists: list[str]) -> Path:
        d = self.tmp / name
        d.mkdir(parents=True)
        for kind, items in lists.items():
            write(d / f"{kind}.txt", items)
        return d


class StatusTests(Base):
    def status(self, results: Path, *extra: str) -> tuple[int, str, str, dict]:
        rc, out, err = run("status", "--results", str(results), "--selection", "smoke",
                           "--tool", TOOL, "--commit", COMMIT, *extra)
        record = json.loads((results / "status.json").read_text())
        return rc, out, err, record

    def test_every_listed_mutant_with_an_outcome_is_complete(self) -> None:
        inv = write(self.tmp / "inv.txt", M[:3])
        r = self.results("r", caught=M[:2], unviable=M[2:3], missed=[])
        rc, out, _, record = self.status(r, "--inventory", str(inv))
        self.assertEqual(rc, 0)
        self.assertEqual(line(out, "MUTANTS-STATUS"), "MUTANTS-STATUS status=complete expected=3 outcomes=3")
        self.assertEqual(line(out, "MUTANTS-RESULT"),
                         "MUTANTS-RESULT caught=2 missed=0 timeout=0 unviable=1 tested=3 skipped=0")
        self.assertEqual(record["status"], "complete")
        self.assertEqual(record["commit"], COMMIT)
        self.assertEqual((r / "inventory.txt").read_text().splitlines(), sorted(M[:3]))

    def test_some_outcomes_are_partial(self) -> None:
        inv = write(self.tmp / "inv.txt", M[:3])
        r = self.results("r", caught=M[:1])
        rc, out, _, _ = self.status(r, "--inventory", str(inv), "--stopped", "deadline")
        self.assertEqual(rc, 0)
        self.assertIn("MUTANTS-STATUS status=partial expected=3 outcomes=1", out)

    def test_no_outcome_is_not_started(self) -> None:
        inv = write(self.tmp / "inv.txt", M[:3])
        rc, out, _, record = self.status(self.tmp / "absent", "--inventory", str(inv))
        self.assertEqual(rc, 0)
        self.assertIn("MUTANTS-STATUS status=not-started expected=3 outcomes=0", out)
        self.assertEqual(record["expected"], 3)

    def test_no_inventory_is_not_started_with_an_unknown_count(self) -> None:
        rc, out, _, record = self.status(self.tmp / "absent", "--stopped", "deadline")
        self.assertEqual(rc, 0)
        self.assertIn("MUTANTS-STATUS status=not-started expected=unknown outcomes=0", out)
        self.assertIsNone(record["expected"])
        self.assertEqual(record["stopped"], "deadline")

    def test_an_empty_inventory_is_complete(self) -> None:
        inv = write(self.tmp / "inv.txt", [])
        rc, out, _, _ = self.status(self.tmp / "absent", "--inventory", str(inv))
        self.assertEqual(rc, 0)
        self.assertIn("MUTANTS-STATUS status=complete expected=0 outcomes=0", out)

    def test_a_failed_baseline_wins(self) -> None:
        inv = write(self.tmp / "inv.txt", M[:1])
        r = self.results("r", caught=M[:1])
        _, out, _, _ = self.status(r, "--inventory", str(inv), "--baseline-failed")
        self.assertIn("MUTANTS-STATUS status=baseline-failed expected=1 outcomes=1", out)

    def test_an_outcome_outside_the_inventory_is_not_complete_and_fails(self) -> None:
        inv = write(self.tmp / "inv.txt", M[:1])
        r = self.results("r", caught=M[:2])
        rc, out, err, _ = self.status(r, "--inventory", str(inv))
        self.assertEqual(rc, 1)
        self.assertIn("status=partial", out)
        self.assertIn("not in the inventory", err)


class MergeTests(Base):
    def setUp(self) -> None:
        super().setUp()
        write(self.tmp / "inventory.txt", M)
        for k, s in enumerate(SLICES):
            write(self.tmp / "slices" / f"{k}.txt", s)

    def shard(self, k: int, *, commit: str = COMMIT, selection: str | None = None,
              inventory: list[str] | None = None, status: str = "complete", **lists) -> Path:
        d = self.tmp / "shards" / str(k)
        d.mkdir(parents=True)
        lists.setdefault("caught", SLICES[k])
        for kind, items in lists.items():
            write(d / f"{kind}.txt", items)
        write(d / "inventory.txt", SLICES[k] if inventory is None else inventory)
        (d / "status.json").write_text(json.dumps({
            "status": status, "commit": commit, "dirty": False, "tool": TOOL,
            "selection": selection or f"shard {k}/4"}))
        return d

    def merge(self, dirs: dict[int, Path]) -> tuple[int, str, dict]:
        args = ["merge", "--into", str(self.tmp / "merged"), "--inventory", str(self.tmp / "inventory.txt"),
                "--slices", str(self.tmp / "slices"), "--shards", "4", "--commit", COMMIT, "--tool", TOOL]
        for k in range(4):
            args += ["--shard", f"{k}={dirs.get(k, self.tmp / 'shards' / f'none-{k}')}"]
        rc, out, err = run(*args)
        self.assertEqual(rc, 0, err)
        return rc, out, json.loads((self.tmp / "merged" / "status.json").read_text())

    def test_every_shard_complete_merges_complete(self) -> None:
        _, out, record = self.merge({k: self.shard(k) for k in range(4)})
        self.assertIn("MUTANTS-STATUS status=complete expected=8 outcomes=8", out)
        self.assertIn("MUTANTS-RESULT caught=8 missed=0 timeout=0 unviable=0 tested=8 skipped=0", out)
        self.assertEqual(record["status"], "complete")
        merged = (self.tmp / "merged" / "caught.txt").read_text().splitlines()
        self.assertEqual(merged, sorted(M))

    def test_a_missing_shard_is_partial_and_its_mutants_count_as_missing(self) -> None:
        dirs = {k: self.shard(k) for k in range(3)}
        _, out, record = self.merge(dirs)
        self.assertIn("MUTANTS-STATUS status=partial expected=8 outcomes=6", out)
        self.assertIn("2 missing", out)
        self.assertIn("| 3/4 | missing | 2 | 0 | **no** | no results", out)
        self.assertFalse(record["shards"][3]["accepted"])

    def test_a_shard_stopped_before_listing_is_accepted_but_partial(self) -> None:
        dirs = {k: self.shard(k) for k in range(3)}
        d = self.tmp / "shards" / "3"
        d.mkdir(parents=True)
        (d / "status.json").write_text(json.dumps({
            "status": "not-started", "commit": COMMIT, "tool": TOOL, "selection": "shard 3/4",
            "stopped": "deadline", "phase": "listing the mutants"}))
        dirs[3] = d
        _, out, record = self.merge(dirs)
        self.assertIn("status=partial expected=8 outcomes=6", out)
        self.assertTrue(record["shards"][3]["accepted"])

    def test_a_shard_of_another_commit_is_rejected(self) -> None:
        dirs = {k: self.shard(k) for k in range(3)}
        dirs[3] = self.shard(3, commit="b" * 40)
        _, out, _ = self.merge(dirs)
        self.assertIn("status=partial expected=8 outcomes=6", out)
        self.assertIn("commit is", out)

    def test_a_shard_with_another_selection_is_rejected(self) -> None:
        dirs = {k: self.shard(k) for k in range(3)}
        dirs[3] = self.shard(3, selection="shard 3/8")
        _, out, _ = self.merge(dirs)
        self.assertIn("status=partial expected=8 outcomes=6", out)
        self.assertIn("selection is", out)

    def test_a_resliced_shard_cannot_be_merged_silently(self) -> None:
        # A re-run whose selection changed under it: its inventory is not its slice, even though
        # its (empty) outcomes lie within the slice.
        dirs = {k: self.shard(k) for k in range(3)}
        dirs[3] = self.shard(3, inventory=[], caught=[])
        _, out, record = self.merge(dirs)
        self.assertIn("status=partial expected=8 outcomes=6", out)
        self.assertIn("is not its slice", record["shards"][3]["note"])

    def test_outcomes_outside_the_slice_reject_the_shard_and_never_double_count(self) -> None:
        # Shard 3 claims shard 2's mutants too: they are counted once, from shard 2.
        dirs = {k: self.shard(k) for k in range(3)}
        dirs[3] = self.shard(3, caught=SLICES[2] + SLICES[3])
        _, out, record = self.merge(dirs)
        self.assertIn("status=partial expected=8 outcomes=6", out)
        self.assertIn("outside its slice", record["shards"][3]["note"])

    def test_a_mutant_with_two_outcomes_rejects_the_shard(self) -> None:
        dirs = {k: self.shard(k) for k in range(3)}
        dirs[3] = self.shard(3, caught=SLICES[3], missed=SLICES[3][:1])
        _, _, record = self.merge(dirs)
        self.assertEqual(record["status"], "partial")
        self.assertIn("more than one outcome", record["shards"][3]["note"])

    def test_a_partial_shard_is_merged_with_its_outcomes(self) -> None:
        dirs = {k: self.shard(k) for k in range(3)}
        dirs[3] = self.shard(3, status="partial", caught=SLICES[3][:1])
        _, out, record = self.merge(dirs)
        self.assertIn("status=partial expected=8 outcomes=7", out)
        self.assertTrue(record["shards"][3]["accepted"])

    def test_a_shard_whose_baseline_failed_marks_the_audit(self) -> None:
        dirs = {k: self.shard(k) for k in range(3)}
        dirs[3] = self.shard(3, status="baseline-failed", caught=[])
        _, out, _ = self.merge(dirs)
        self.assertIn("MUTANTS-STATUS status=baseline-failed expected=8 outcomes=6", out)

    def test_slices_that_do_not_partition_the_inventory_stop_the_merge(self) -> None:
        write(self.tmp / "slices" / "3.txt", SLICES[2])
        rc, _, err = run("merge", "--into", str(self.tmp / "merged"),
                         "--inventory", str(self.tmp / "inventory.txt"),
                         "--slices", str(self.tmp / "slices"), "--shards", "4",
                         "--commit", COMMIT, "--tool", TOOL)
        self.assertEqual(rc, 2)
        self.assertIn("do not partition", err)


if __name__ == "__main__":
    unittest.main(verbosity=2)
