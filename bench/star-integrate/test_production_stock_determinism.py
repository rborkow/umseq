import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run_production_gate as gate


def template():
    return ["{STAR}", "--genomeDir", "g", "--readFilesIn", "{R1}", "{R2}", "--runThreadN", "16",
            "--outFileNamePrefix", "{OUT}", "--twopassMode", "Basic", "--quantMode", "TranscriptomeSAM",
            "--outSAMtype", "BAM", "Unsorted", "--readFilesCommand", "zcat", "--runRNGseed", "0",
            "--outFilterMultimapNmax", "20", "--alignSJDBoverhangMin", "1", "--outSAMstrandField", "intronMotif",
            "--quantTranscriptomeSAMoutput", "BanSingleEnd"]


class StockDeterminismTests(unittest.TestCase):
    def test_stock_twice_runs_two_stock_arms_then_cmp(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); exe = root / "STAR"; r1 = root / "r1"; r2 = root / "r2"; argv = root / "argv.json"
            for path in (exe, r1, r2): path.write_text("x")
            argv.write_text(json.dumps(template()))
            args = SimpleNamespace(stock=exe, mate1=r1, mate2=r2, argv_template=argv, timeout_s=1,
                output=root / "out", lock=root / "lock", hash_inputs=False, compare="cmp", genome_dir=None, sjdb_gtf=None)
            with patch.object(gate, "run_stage") as run, patch.object(gate, "require_outputs") as outputs, patch.object(gate, "compare_cmp") as cmp:
                gate.stock_twice(args)
            self.assertEqual([call.args[1] for call in run.call_args_list], ["stock-1", "stock-2"])
            self.assertEqual(outputs.call_count, 2); cmp.assert_called_once()
            self.assertEqual(json.loads((args.output / "gate-summary.json").read_text())["status"], "PASS_STOCK_TWICE_CMP")

    def test_stock_twice_does_not_compare_after_a_failed_stock_arm(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp); exe = root / "STAR"; r1 = root / "r1"; r2 = root / "r2"; argv = root / "argv.json"
            for path in (exe, r1, r2): path.write_text("x")
            argv.write_text(json.dumps(template()))
            args = SimpleNamespace(stock=exe, mate1=r1, mate2=r2, argv_template=argv, timeout_s=1, output=root / "out", lock=root / "lock", hash_inputs=False, genome_dir=None, sjdb_gtf=None)
            with patch.object(gate, "run_stage", side_effect=RuntimeError("stock-1 failed (exit 1)")), patch.object(gate, "compare_cmp") as cmp:
                with self.assertRaisesRegex(RuntimeError, "stock-1 failed"): gate.stock_twice(args)
            cmp.assert_not_called()


if __name__ == "__main__":
    unittest.main()
