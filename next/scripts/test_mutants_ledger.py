#!/usr/bin/env python3
"""Tests of next/scripts/mutants-ledger.py. Plain python3, no pytest:

    python3 -I next/scripts/test_mutants_ledger.py
"""
from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
LEDGER = HERE / "mutants-ledger.py"
_spec = importlib.util.spec_from_file_location("mutants_ledger", LEDGER)
ml = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(ml)


def run(*args: str) -> tuple[int, str, str]:
    p = subprocess.run([sys.executable, "-I", str(LEDGER), *args], capture_output=True, text=True)
    return p.returncode, p.stdout, p.stderr


def write(d: Path, name: str, lines) -> None:
    d.mkdir(parents=True, exist_ok=True)
    (d / name).write_text("".join(f"{x}\n" for x in lines))


def lines(p: Path) -> list[str]:
    return p.read_text().splitlines() if p.exists() else []


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True,
                          text=True).stdout


DIFF = """\
diff --git a/m/src/a.rs b/m/src/a.rs
index 1..2 100644
--- a/m/src/a.rs
+++ b/m/src/a.rs
@@ -2,0 +3,2 @@ fn top
+// one
+// two
@@ -10,3 +12,3 @@ fn mid
 keep
-old
+new
 keep
@@ -30,2 +32,0 @@ fn low
-gone
-gone
diff --git a/m/src/dead.rs b/m/src/dead.rs
deleted file mode 100644
--- a/m/src/dead.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-x
-y
diff --git a/m/src/new.rs b/m/src/new.rs
new file mode 100644
--- /dev/null
+++ b/m/src/new.rs
@@ -0,0 +1 @@
+z
"""


class DiffTests(unittest.TestCase):
    def setUp(self) -> None:
        self.diffs = ml.parse_diff(DIFF)

    def test_lines_above_every_hunk_keep_their_number(self) -> None:
        self.assertEqual(self.diffs["m/src/a.rs"].map_line(1), 1)
        self.assertEqual(self.diffs["m/src/a.rs"].map_line(2), 2)

    def test_an_insertion_shifts_the_lines_after_it(self) -> None:
        self.assertEqual(self.diffs["m/src/a.rs"].map_line(3), 5)
        self.assertEqual(self.diffs["m/src/a.rs"].map_line(9), 11)

    def test_context_lines_map_and_changed_lines_do_not(self) -> None:
        fd = self.diffs["m/src/a.rs"]
        self.assertEqual(fd.map_line(10), 12)
        self.assertIsNone(fd.map_line(11))
        self.assertEqual(fd.map_line(12), 14)

    def test_a_deletion_shifts_the_lines_after_it(self) -> None:
        fd = self.diffs["m/src/a.rs"]
        self.assertIsNone(fd.map_line(30))
        self.assertIsNone(fd.map_line(31))
        self.assertEqual(fd.map_line(32), 32)
        self.assertEqual(fd.map_line(29), 31)

    def test_a_deleted_file_maps_nothing_and_a_new_file_is_not_an_old_path(self) -> None:
        self.assertIsNone(self.diffs["m/src/dead.rs"].map_line(1))
        self.assertNotIn("m/src/new.rs", self.diffs)

    def test_translate_moves_the_name_or_drops_it(self) -> None:
        self.assertEqual(ml.translate("m/src/a.rs:9:5: replace + with -", self.diffs),
                         "m/src/a.rs:11:5: replace + with -")
        self.assertIsNone(ml.translate("m/src/a.rs:11:5: replace + with -", self.diffs))
        self.assertEqual(ml.translate("m/src/other.rs:9:5: x", self.diffs), "m/src/other.rs:9:5: x")

    def test_a_real_git_diff_translates_like_the_new_tree_numbers_lines(self) -> None:
        with tempfile.TemporaryDirectory() as t:
            repo = Path(t)
            git(repo, "init", "-q")
            git(repo, "config", "user.email", "t@example.invalid")
            git(repo, "config", "user.name", "t")
            src = repo / "m" / "src"
            src.mkdir(parents=True)
            old = [f"line {i}" for i in range(1, 41)]
            (src / "a.rs").write_text("\n".join(old) + "\n")
            git(repo, "add", ".")
            git(repo, "commit", "-q", "-m", "base")
            new = old[:4] + ["inserted a", "inserted b"] + old[4:19] + ["changed 20"] + old[20:33] + old[35:]
            (src / "a.rs").write_text("\n".join(new) + "\n")
            text = git(repo, "-c", "diff.noprefix=false", "diff", "--no-color", "--no-ext-diff",
                       "--no-renames", "--src-prefix=a/", "--dst-prefix=b/", "HEAD")
            diffs = ml.parse_diff(text)
            for i, content in enumerate(old, start=1):
                mapped = ml.translate(f"m/src/a.rs:{i}:1: x", diffs)
                if content in new:
                    self.assertEqual(mapped, f"m/src/a.rs:{new.index(content) + 1}:1: x", content)
                else:
                    self.assertIsNone(mapped, content)


class GlobTests(unittest.TestCase):
    def test_a_glob_with_a_slash_matches_the_path(self) -> None:
        self.assertTrue(ml.glob_matches("mega-evm2/src/tx.rs", "mega-evm2/src/tx.rs"))
        self.assertTrue(ml.glob_matches("mega-evm2/src/*", "mega-evm2/src/block/pre_block.rs"))
        self.assertFalse(ml.glob_matches("op-evm2/src/*", "mega-evm2/src/tx.rs"))

    def test_a_glob_without_a_slash_matches_the_file_name(self) -> None:
        self.assertTrue(ml.glob_matches("tx.rs", "mega-evm2/src/tx.rs"))
        self.assertFalse(ml.glob_matches("src", "mega-evm2/src/tx.rs"))

    def test_all_matches_everything(self) -> None:
        self.assertTrue(ml.glob_matches("all", "op-evm2/src/lib.rs"))


A = "m/src/a.rs:9:5: replace + with -"
A2 = "m/src/a.rs:11:5: replace + with -"  # A after the diff above
B = "m/src/a.rs:11:5: replace * with /"  # on the changed line
C = "m/src/c.rs:4:1: replace c with ()"
U = "m/src/c.rs:8:1: replace d -> u8 with 0"
M = "m/src/c.rs:12:1: replace e with ()"


class PrepareFinalizeTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.out = self.tmp / "out"
        self.results = self.out / "mutants.out"
        self.stage = self.out / "run"

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def prepare(self, *extra: str) -> tuple[int, str, str]:
        return run("prepare", "--results", str(self.results), "--stage", str(self.stage), *extra)

    def seed(self) -> list[str]:
        return lines(self.stage / "mutants.out" / "previously_caught.txt")

    def test_nothing_earlier_seeds_an_empty_list(self) -> None:
        rc, out, _ = self.prepare()
        self.assertEqual(rc, 0)
        self.assertEqual(self.seed(), [])
        self.assertIn("0 caught and 0 unviable", out)

    def test_caught_unviable_and_earlier_carried_ones_are_carried_but_not_missed_or_timeouts(self) -> None:
        write(self.results, "caught.txt", [C])
        write(self.results, "unviable.txt", [U])
        write(self.results, "missed.txt", [M])
        write(self.results, "timeout.txt", [A])
        write(self.results, "previously_caught.txt", [B])
        rc, out, _ = self.prepare()
        self.assertEqual(rc, 0)
        self.assertEqual(self.seed(), sorted([B, C, U]))
        self.assertEqual(lines(self.stage / "carry" / "previously_unviable.txt"), [U])
        self.assertIn("2 caught and 1 unviable", out)

    def test_the_diff_moves_names_and_drops_changed_lines(self) -> None:
        write(self.results, "caught.txt", [A, B, C])
        (self.tmp / "d.diff").write_text(DIFF)
        rc, out, _ = self.prepare("--diff", str(self.tmp / "d.diff"))
        self.assertEqual(rc, 0)
        self.assertEqual(self.seed(), sorted([A2, C]))
        self.assertIn("dropped 1 in changed code", out)

    def test_changed_and_no_longer_generated_mutants_are_dropped(self) -> None:
        write(self.results, "caught.txt", [C, M])
        write(self.results, "unviable.txt", [U])
        write(self.tmp, "universe.txt", [C, U])
        write(self.tmp, "changed.txt", [U])
        rc, out, _ = self.prepare("--universe", str(self.tmp / "universe.txt"),
                                  "--changed", str(self.tmp / "changed.txt"))
        self.assertEqual(rc, 0)
        self.assertEqual(self.seed(), [C])
        self.assertEqual(lines(self.stage / "carry" / "previously_unviable.txt"), [])
        self.assertIn("dropped 1 in changed code, 1 no longer generated", out)

    def test_a_refresh_glob_drops_the_carried_results_of_its_files(self) -> None:
        write(self.results, "caught.txt", [A, C])
        rc, out, _ = self.prepare("--refresh", "c.rs")
        self.assertEqual(rc, 0)
        self.assertEqual(self.seed(), [A])
        self.assertIn("1 by a refresh (c.rs)", out)
        rc, _, _ = self.prepare("--refresh", "all")
        self.assertEqual(self.seed(), [])

    def test_a_refresh_request_applies_once(self) -> None:
        req = self.tmp / "refresh.txt"
        req.write_text("# header\n2026-11-13 m/src/c.rs   # deleted tests\n")
        write(self.results, "caught.txt", [A, C])
        self.prepare("--refresh-file", str(req))
        self.assertEqual(self.seed(), [A])
        self.assertEqual(lines(self.stage / "carry" / "refreshed.txt"), ["2026-11-13 m/src/c.rs"])
        # The run's results become the next starting point, with the request recorded.
        (self.stage / "mutants.out" / "caught.txt").write_text(f"{C}\n")
        rc, _, _ = run("finalize", "--stage", str(self.stage), "--out", str(self.out),
                       "--refresh-file", str(req))
        self.assertEqual(rc, 0)
        self.prepare("--refresh-file", str(req))
        self.assertEqual(self.seed(), sorted([A, C]))
        # A new line is a new request.
        req.write_text(req.read_text() + "2026-12-01 c.rs\n")
        self.prepare("--refresh-file", str(req))
        self.assertEqual(self.seed(), [A])

    def test_finalize_promotes_the_run_and_keeps_the_previous_results(self) -> None:
        write(self.results, "caught.txt", [C])
        write(self.results, "unviable.txt", [U])
        self.prepare()
        run_dir = self.stage / "mutants.out"
        # What cargo-mutants does: rotate the seed and write a new directory.
        (run_dir).rename(self.stage / "mutants.out.old")
        write(run_dir, "previously_caught.txt", [C, U])
        write(run_dir, "caught.txt", [A])
        rc, _, _ = run("finalize", "--stage", str(self.stage), "--out", str(self.out),
                       "--base", "abc123")
        self.assertEqual(rc, 0)
        self.assertFalse(self.stage.exists())
        self.assertEqual(lines(self.results / "caught.txt"), [A])
        self.assertEqual(lines(self.results / "previously_unviable.txt"), [U])
        self.assertEqual(lines(self.results / "carry-base.txt"), ["abc123"])
        self.assertEqual(lines(self.out / "mutants.out.old" / "caught.txt"), [C])

    def test_finalize_without_a_run_directory_changes_nothing(self) -> None:
        write(self.results, "caught.txt", [C])
        self.stage.mkdir(parents=True)
        rc, _, _ = run("finalize", "--stage", str(self.stage), "--out", str(self.out))
        self.assertEqual(rc, 0)
        self.assertEqual(lines(self.results / "caught.txt"), [C])
        self.assertFalse((self.out / "mutants.out.old").exists())


class TallyTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self._tmp.name) / "mutants.out"

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def tally(self, *extra: str) -> str:
        rc, out, _ = run("tally", "--results", str(self.dir), *extra)
        self.assertEqual(rc, 0)
        return out.strip()

    def test_nothing_ran(self) -> None:
        rc, out, _ = run("tally")
        self.assertEqual(out.strip(),
                         "MUTANTS-RESULT caught=0 missed=0 timeout=0 unviable=0 tested=0 skipped=0")

    def test_a_carried_over_run_counts_this_run_and_the_carried_ones(self) -> None:
        write(self.dir, "caught.txt", [A])
        write(self.dir, "missed.txt", [M])
        write(self.dir, "unviable.txt", [B])
        write(self.dir, "previously_caught.txt", [C, U])
        write(self.dir, "previously_unviable.txt", [U])
        self.assertEqual(self.tally(),
                         "MUTANTS-RESULT caught=2 missed=1 timeout=0 unviable=2 tested=3 skipped=2")

    def test_carried_only_treats_the_whole_directory_as_carried(self) -> None:
        write(self.dir, "caught.txt", [A])
        write(self.dir, "missed.txt", [M])
        write(self.dir, "unviable.txt", [B])
        write(self.dir, "previously_caught.txt", [C, U])
        write(self.dir, "previously_unviable.txt", [U])
        self.assertEqual(self.tally("--carried-only"),
                         "MUTANTS-RESULT caught=2 missed=0 timeout=0 unviable=2 tested=0 skipped=4")


class MergeTests(unittest.TestCase):
    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def shard(self, name: str, base: str, **lists) -> Path:
        d = self.tmp / name
        for k, v in lists.items():
            if k == "mutants":
                d.mkdir(parents=True, exist_ok=True)
                (d / "mutants.json").write_text(json.dumps([{"name": n} for n in v]))
            else:
                write(d, f"{k}.txt", v)
        write(d, "carry-base.txt", [base])
        return d

    def test_shards_of_this_commit_are_unioned(self) -> None:
        s0 = self.shard("s0", "h", caught=[A], missed=[M], previously_caught=[C, U],
                        previously_unviable=[U], mutants=[A, M])
        s1 = self.shard("s1", "h", caught=[M], unviable=[B], previously_caught=[C, U],
                        previously_unviable=[U], refreshed=["2026-11-13 x"], mutants=[B, M])
        old = self.shard("old", "g", caught=["m/src/z.rs:1:1: stale"])
        into = self.tmp / "merged"
        rc, out, err = run("merge", "--into", str(into), "--base", "h", str(s0), str(s1), str(old))
        self.assertEqual(rc, 0, err)
        self.assertIn("skipped", err)
        self.assertEqual(lines(into / "caught.txt"), sorted([A, M]))
        self.assertEqual(lines(into / "missed.txt"), [])
        self.assertEqual(lines(into / "unviable.txt"), [B])
        self.assertEqual(lines(into / "previously_caught.txt"), sorted([C, U]))
        self.assertEqual(lines(into / "previously_unviable.txt"), [U])
        self.assertEqual(lines(into / "refreshed.txt"), ["2026-11-13 x"])
        self.assertEqual(lines(into / "carry-base.txt"), ["h"])
        self.assertEqual(sorted(m["name"] for m in json.loads((into / "mutants.json").read_text())),
                         sorted([A, B, M]))
        self.assertIn("caught=3", out)

    def test_no_shard_of_this_commit_fails(self) -> None:
        old = self.shard("old", "g", caught=[A])
        rc, _, err = run("merge", "--into", str(self.tmp / "m"), "--base", "h", str(old))
        self.assertEqual(rc, 1)
        self.assertIn("no shard results", err)


class TimingTests(unittest.TestCase):
    def test_timing_compares_the_mutants_both_runs_tested(self) -> None:
        with tempfile.TemporaryDirectory() as t:
            def outcomes(d: Path, rows) -> None:
                d.mkdir()
                data = {"outcomes": [{"scenario": "Baseline", "summary": "Success", "phase_results": []}]}
                for name, summary, b, s in rows:
                    phases = [{"phase": "Build", "duration": b}]
                    if s is not None:
                        phases.append({"phase": "Test", "duration": s})
                    data["outcomes"].append({"scenario": {"Mutant": {"name": name}},
                                             "summary": summary, "phase_results": phases})
                (d / "outcomes.json").write_text(json.dumps(data))
            a, b = Path(t) / "a", Path(t) / "b"
            outcomes(a, [(A, "CaughtMutant", 100, 10), (M, "MissedMutant", 90, 80), (C, "Unviable", 3, None)])
            outcomes(b, [(A, "CaughtMutant", 40, 12), (M, "MissedMutant", 30, 85)])
            rc, out, _ = run("timing", "--results", str(a), "--compare", str(b))
            self.assertEqual(rc, 0)
            self.assertIn("2 mutants in both runs, 2 with the same outcome", out)
            self.assertIn("build mean   40.0 s", out)


if __name__ == "__main__":
    unittest.main(verbosity=2)
