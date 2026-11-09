#!/usr/bin/env python3
"""Exercise the public summary CLI with independently calculated timing samples."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class SummaryTests(unittest.TestCase):
    def run_summary(self, reference_change=None):
        with tempfile.TemporaryDirectory(dir=os.environ.get("CARGO_TARGET_DIR", ".")) as tmp:
            rows = [
                {"set": "sample", "block": 1, "txs": 3, "included": 2, "refused": 1,
                 "gas": 30, "ms": [1.0, 9.0, 3.0]},
                {"set": "sample", "block": 2, "txs": 2, "included": 2, "refused": 0,
                 "gas": 70, "ms": [9.0, 1.0, 3.0]},
            ]
            paths = [Path(tmp) / side for side in ("engine", "reference")]
            paths[0].write_text("\n".join(map(json.dumps, rows)))
            if reference_change:
                reference_change(rows)
            paths[1].write_text("\n".join(map(json.dumps, rows)))
            return subprocess.run(
                [sys.executable, str(Path(__file__).with_name("bench-summary.py")), *map(str, paths)],
                capture_output=True, text=True, check=False,
            )

    def test_total_uses_paired_rounds_and_counts_refusals(self):
        result = self.run_summary()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("total_median_ms=10.000000", result.stdout)
        self.assertIn("block_median_ms=3.000000", result.stdout)
        self.assertIn("gas_per_second=10000.000", result.stdout)
        self.assertEqual(result.stdout.splitlines()[-1],
                         "BENCH blocks=2 txs=5 engine_ms=10.000000 reference_ms=10.000000")

    def test_rejects_different_block_work(self):
        result = self.run_summary(lambda rows: rows[1].update(txs=3, refused=1))
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("BENCH ", result.stdout)

    def test_each_side_reports_its_own_outcomes(self):
        # The block gate holds the outcomes; a difference it accepted is timed as it is.
        result = self.run_summary(lambda rows: rows[1].update(gas=170, included=1, refused=1))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("engine: blocks=2 txs=5 included=4 refused=1", result.stdout)
        self.assertIn("reference: blocks=2 txs=5 included=3 refused=2", result.stdout)
        self.assertIn("receipt_gas=100 ", result.stdout)
        self.assertIn("receipt_gas=200 ", result.stdout)

    def test_rejects_missing_repetitions(self):
        result = self.run_summary(lambda rows: rows[0].update(ms=[1.0]))
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn("BENCH ", result.stdout)


if __name__ == "__main__":
    unittest.main()
