#!/usr/bin/env python3
"""Regression tests for the cost-curve model and generated artifacts."""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "cost_curve.py"


def run_model(out_dir):
    interpreters = [sys.executable, "/Users/rborkows/projects/ft/venv/bin/python"]
    last = None
    for interpreter in dict.fromkeys(interpreters):
        try:
            return subprocess.run(
                [interpreter, str(SCRIPT), "--out", str(out_dir)],
                cwd=ROOT, check=True, capture_output=True, text=True,
            )
        except (FileNotFoundError, subprocess.CalledProcessError) as exc:
            last = exc
            if isinstance(exc, subprocess.CalledProcessError) and "No module named" not in exc.stderr:
                raise
    raise unittest.SkipTest(f"no interpreter with numpy/matplotlib available: {last}")


class CostCurveTests(unittest.TestCase):
    def test_model_and_figures(self):
        with tempfile.TemporaryDirectory() as tmp:
            run_model(tmp)
            model = json.loads((Path(tmp) / "cost-model.json").read_text())

            self.assertEqual(
                {k: model["scenarios_cpu_min"][k] for k in (
                    "nf-core stock (measured)",
                    "umbam CPU replaces BAM chain",
                    "umbam CPU+GPU",
                )},
                {
                    "nf-core stock (measured)": 222.89999999999998,
                    "umbam CPU replaces BAM chain": 148.48666666666665,
                    "umbam CPU+GPU": 147.0533333333333,
                },
            )
            self.assertEqual(len(model["scenarios_cpu_min"]), 4)
            projected = model["projection"]
            self.assertEqual(projected["label"], "umbam + GPU seed search (PROJECTED, 1.07–1.15×)")
            self.assertEqual(projected["comparator"], "umbam CPU replaces BAM chain")
            self.assertEqual(projected["factor_bounds"], [1.07, 1.15])
            self.assertEqual(projected["cpu_min_bounds"], [148.48666666666665 / 1.15, 148.48666666666665 / 1.07])
            self.assertEqual(projected["capacity_bounds"], [1.07, 1.15])
            self.assertEqual(projected["baseline_cpu_min"], 148.48666666666665)
            self.assertEqual(projected["capacity_per_month_bounds_80pct"], [
                projected["baseline_capacity_per_month_80pct"] * 1.07,
                projected["baseline_capacity_per_month_80pct"] * 1.15,
            ])
            self.assertTrue(projected["projected"])
            self.assertLess(projected["factor_bounds"][0], projected["factor_bounds"][1])

            self.assertEqual(model["chart"]["projected_style"], "dashed")
            self.assertIn("PROJECTED", model["chart"]["projected_label"])
            self.assertTrue(model["chart"]["projection_envelope"])
            for name in ("cost-curve.png", "cpu-minutes.png", "umbam-stages.png", "bamchain-wall.png"):
                artifact = Path(tmp) / name
                self.assertTrue(artifact.is_file() and artifact.stat().st_size > 0, artifact)


if __name__ == "__main__":
    unittest.main()
