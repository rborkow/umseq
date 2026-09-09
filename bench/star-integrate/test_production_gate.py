import json
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_production_gate as gate


class ProductionGateTests(unittest.TestCase):
    def template(self):
        return ["{STAR}", "--genomeDir", "g", "--readFilesIn", "{R1}", "{R2}", "--runThreadN", "16",
                "--outFileNamePrefix", "{OUT}", "--twopassMode", "Basic", "--quantMode", "TranscriptomeSAM",
                "--outSAMtype", "BAM", "Unsorted", "--readFilesCommand", "zcat", "--runRNGseed", "0",
                "--outFilterMultimapNmax", "20", "--alignSJDBoverhangMin", "1", "--outSAMstrandField", "intronMotif",
                "--quantTranscriptomeSAMoutput", "BanSingleEnd"]

    def test_template_is_explicit_and_thread_pinned(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "argv.json"; path.write_text(json.dumps(self.template()))
            self.assertEqual(gate.load_template(path), self.template())
            missing = [token for token in self.template() if token != "--quantMode"]
            path.write_text(json.dumps(missing))
            with self.assertRaisesRegex(ValueError, "quantMode"): gate.load_template(path)
            bad = self.template(); bad[bad.index("16")] = "8"; path.write_text(json.dumps(bad))
            with self.assertRaisesRegex(ValueError, "runThreadN 16"): gate.load_template(path)
            bad = self.template(); bad[bad.index("Basic")] = "None"; path.write_text(json.dumps(bad))
            with self.assertRaisesRegex(ValueError, "twopassMode"): gate.load_template(path)

    def test_render_does_not_shell_expand_and_replaces_all_paths(self):
        got = gate.render_argv(self.template(), "/x/STAR", "/r/1.fq.gz", "/r/2.fq.gz", "/out")
        self.assertEqual(got[0], "/x/STAR"); self.assertIn("/out/", got)
        self.assertNotIn("{R1}", got)

    def test_sidecar_rejects_every_accounting_failure(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "sidecar"
            good = {"gpu_consumed": 1, "batch_faults": 0, "rejected": 0, "shift_mismatch": 0,
                    "flag_mismatch": 0, "step_count_mismatch": 0}
            path.write_text(json.dumps(good) + "\n")
            self.assertEqual(gate.sidecar_stats(path)["gpu_consumed"], 1)
            for name in ("gpu_consumed", "batch_faults", "rejected", "shift_mismatch", "flag_mismatch", "step_count_mismatch"):
                bad = dict(good); bad[name] = 0 if name == "gpu_consumed" else 1
                path.write_text(json.dumps(bad) + "\n")
                with self.assertRaises(RuntimeError): gate.sidecar_stats(path)
            path.write_text(json.dumps(good) + "\n{}\n")
            with self.assertRaises(RuntimeError): gate.sidecar_stats(path)

    def test_outputs_and_log_timing_rejections(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            with self.assertRaisesRegex(RuntimeError, "Aligned.out.bam"):
                gate.require_outputs(root)
            log = root / "Log.final.out"
            log.write_text("Started job on | old\nNumber of input reads | 7\nFinished on | new\n")
            self.assertEqual(gate.normalized_log(log), "Number of input reads | 7\n")

    def test_cmp_failure_is_a_rejection_and_normalization_is_recorded(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); stock, integrated = root / "s", root / "i"; stock.mkdir(); integrated.mkdir()
            for directory in (stock, integrated):
                for name in gate.REQUIRED_OUTPUTS:
                    path = directory / name; path.parent.mkdir(parents=True, exist_ok=True); path.write_text("same\n")
            with patch.object(gate, "command_ok", side_effect=RuntimeError("cmp Aligned.out.bam failed (exit 1)")):
                with self.assertRaisesRegex(RuntimeError, "Aligned.out.bam"):
                    gate.compare_cmp(stock, integrated, root)

    def test_subprocess_nonzero_is_a_rejection(self):
        class Result: returncode = 3
        with patch.object(gate.subprocess, "run", return_value=Result()):
            with self.assertRaisesRegex(RuntimeError, "cmp failed"):
                gate.command_ok(["cmp"], "cmp")

    def test_production_env_scrubs_inherited_flags(self):
        env = gate.production_environment(Path("stats"))
        self.assertEqual(env["STAR_INTEGRATE_STRICT"], "1")
        self.assertEqual(env["STAR_INTEGRATE_SIDECAR"], "stats")


if __name__ == "__main__":
    unittest.main()
