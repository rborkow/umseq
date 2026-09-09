#!/usr/bin/env python3
"""Regression tests for the cost-curve model and generated artifacts."""
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "cost_curve.py"


def run_model(out_dir):
    interpreters = [
        sys.executable,
        "/Users/rborkows/projects/ft/venv/bin/python",
        "/Users/rborkows/miniconda3/bin/python",
    ]
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
            self.assertEqual(len(model["scenarios_cpu_min"]), 5)
            projected_names = (
                "umbam CPU + STAR advised bypass (PROJECTED)",
                "umbam CPU + STAR GPU (PROJECTED)",
            )
            projected = model["projection_metadata"]
            self.assertEqual(set(projected), set(projected_names))

            measurement = model["star_stage_measurement"]
            self.assertEqual(measurement["source_artifact"], "bench/evidence/integrate-1-host/timing-round9-raw.tsv")
            self.assertEqual(measurement["source_input_size"], "20M-pair SAM")
            self.assertEqual(measurement["stage"], "STAR align")
            self.assertEqual(measurement["unit"], "CPU-s")
            self.assertEqual(measurement["threads"], 20)
            self.assertEqual(measurement["means_cpu_s"], {
                "stock": 753.9133333333333,
                "bypass": 661.5466666666667,
                "gpu": 618.6533333333333,
            })
            ratios = measurement["stage_ratios"]
            self.assertEqual(ratios["advised_bypass_over_stock"], 661.5466666666667 / 753.9133333333333)
            self.assertEqual(ratios["gpu_over_stock"], 618.6533333333333 / 753.9133333333333)
            self.assertEqual(ratios["gpu_over_advised_bypass"], 618.6533333333333 / 661.5466666666667)

            baseline = model["scenarios_cpu_min"]["umbam CPU replaces BAM chain"]
            stock_star = 64.6
            bypass_projected = model["scenarios_cpu_min"][projected_names[0]]
            gpu_projected = model["scenarios_cpu_min"][projected_names[1]]
            self.assertEqual(bypass_projected, baseline - stock_star + stock_star * ratios["advised_bypass_over_stock"])
            self.assertEqual(gpu_projected, baseline - stock_star + stock_star * ratios["gpu_over_stock"])
            self.assertAlmostEqual(bypass_projected - baseline, stock_star * (ratios["advised_bypass_over_stock"] - 1), places=12)
            self.assertAlmostEqual(gpu_projected - bypass_projected, stock_star * (ratios["gpu_over_stock"] - ratios["advised_bypass_over_stock"]), places=12)
            self.assertNotEqual(model["scenarios_cpu_min"]["umbam CPU+GPU"], gpu_projected)

            required = {"source_artifact", "source_input_size", "stage", "unit", "threads",
                        "stock_mean_cpu_s", "advised_bypass_mean_cpu_s", "gpu_mean_cpu_s",
                        "stage_ratios", "comparator", "provisional_evidence_status", "pipeline_label"}
            for name in projected_names:
                self.assertTrue(projected[name]["projected"])
                self.assertIn("PROJECTED", projected[name]["pipeline_label"])
                self.assertTrue(required <= projected[name].keys())
                self.assertIn("PROVISIONAL", projected[name]["provisional_evidence_status"])

            self.assertEqual(model["chart"]["projected_style"], "dashed")
            self.assertIn("PROJECTED", model["chart"]["projected_label"])
            self.assertFalse(model["chart"]["projection_envelope"])
            for name in ("cost-curve.png", "cpu-minutes.png", "umbam-stages.png", "bamchain-wall.png"):
                artifact = Path(tmp) / name
                self.assertTrue(artifact.is_file() and artifact.stat().st_size > 0, artifact)


if __name__ == "__main__":
    unittest.main()
