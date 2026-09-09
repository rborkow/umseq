import json
import hashlib
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import measurement_common as common
import run_full_depth as runner


class FullDepthRunner(unittest.TestCase):
    def test_strict_environment_has_explicit_cache_policy(self):
        for policy in ("0", "1"):
            env = runner.strict_environment(Path("stats.jsonl"), policy)
            self.assertEqual(env["STAR_INTEGRATE_DROP_INDEX_CACHE"], policy)
            self.assertEqual(env["STAR_INTEGRATE_THP"], "1")
            self.assertEqual(env["STAR_INTEGRATE_STRICT"], "1")
            self.assertEqual(env["STAR_INTEGRATE"], "1")
        with self.assertRaises(ValueError):
            runner.strict_environment(Path("stats.jsonl"), "invalid")

    def test_collapse_is_explicit_opt_in_and_retained_in_stage_env(self):
        self.assertIn("STAR_INTEGRATE_COLLAPSE_INDEX", common.STAR_FLAGS)
        self.assertEqual(runner.strict_environment(Path("s"), "1")["STAR_INTEGRATE_COLLAPSE_INDEX"], "0")
        for collapse in ("0", "1"):
            env = runner.strict_environment(Path("s"), "1", collapse)
            self.assertEqual(env["STAR_INTEGRATE_COLLAPSE_INDEX"], collapse)
            self.assertEqual(env["STAR_INTEGRATE_STRICT"], "1")
        with self.assertRaises(ValueError):
            runner.strict_environment(Path("s"), "1", "invalid")

    def test_structured_argv_replaces_both_mates_and_prefix(self):
        base = ["old-star", "--readFilesIn", "old1", "old2", "--outFileNamePrefix", "old/", "--runMode", "alignReads"]
        got = common.star_argv(base, "/bin/STAR", "/reads/1.gz", "/reads/2.gz", "/out")
        self.assertEqual(got[:6], ["/bin/STAR", "--readFilesIn", "/reads/1.gz", "/reads/2.gz", "--outFileNamePrefix", "/out/"])

    def test_ambiguous_or_capped_full_depth_argv_is_rejected(self):
        with self.assertRaises(ValueError):
            common.replace_option(["--readFilesIn", "a", "b", "--readFilesIn", "c", "d"], "--readFilesIn", ("x", "y"))

    def test_sidecar_requires_one_gpu_consuming_clean_row(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "stats"
            p.write_text(json.dumps({"gpu_consumed": 0, "batch_faults": 0, "rejected": 0, "shift_mismatch": 0, "flag_mismatch": 0, "step_count_mismatch": 0}) + "\n")
            with self.assertRaises(RuntimeError): runner.sidecar_stats(p)
            p.write_text("{}\n{}\n")
            with self.assertRaises(RuntimeError): runner.sidecar_stats(p)

    def test_existing_output_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(ValueError): common.fresh_dir(Path(tmp))

    def test_clean_environment_removes_inherited_arm_flags(self):
        old = dict(common.os.environ)
        try:
            common.os.environ["STAR_INTEGRATE"] = "1"
            common.os.environ["STAR_INTEGRATE_DROP_INDEX_CACHE"] = "1"
            env = common.clean_env({"STAR_INTEGRATE_STRICT": "1"})
            self.assertNotIn("STAR_INTEGRATE", env)
            self.assertNotIn("STAR_INTEGRATE_DROP_INDEX_CACHE", env)
            self.assertEqual(env["STAR_INTEGRATE_STRICT"], "1")
        finally:
            common.os.environ.clear(); common.os.environ.update(old)

    def test_comparator_rejects_unsupported_whitespace_and_preserves_argv(self):
        with self.assertRaises(ValueError):
            runner.comparator_command(Path("check.py"), Path("s"), Path("c"),
                                      ["STAR", "--x", "a b"], ["STAR"], 2, Path("o"))
        command = runner.comparator_command(Path("check.py"), Path("s"), Path("c"),
                                             ["STAR", "--outFileNamePrefix", "s/"],
                                             ["STAR", "--outFileNamePrefix", "c/"], 2, Path("o"))
        self.assertIn("--expected-pairs", command)
        self.assertEqual(command[command.index("--expected-pairs") + 1], "2")

    def test_stock_reuse_real_preflight_protocol_and_rejections(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            stock = root / "stock"
            stock.mkdir()
            exe = root / "STAR"
            exe.write_text("stock")
            exe.chmod(0o755)
            mate1, mate2 = root / "1.gz", root / "2.gz"
            mate1.write_bytes(b"1")
            mate2.write_bytes(b"2")
            base = ["STAR", "--readFilesIn", "x", "y", "--outFileNamePrefix", "x/"]
            argv = common.star_argv(base, exe, mate1, mate2, stock)
            (stock / "argv.json").write_text(json.dumps(argv))
            (stock / "status").write_text("PASS_STOCK_ONLY_NOT_PARITY\n")
            (stock / "exit.code").write_text("0\n")
            (stock / "Aligned.out.sam").write_text("test-only SAM fixture\n")
            (stock / "SJ.out.tab").write_text("test-only junction fixture\n")
            (stock / "Log.final.out").write_text("Number of input reads |\t2\n")
            manifest = {
                "status": "PASS_INPUTS_ONLY", "expected_pairs": 2,
                "stock": common.executable_identity(exe),
                "inputs": [{"path": str(p), "bytes": p.stat().st_size,
                            "mtime_ns": p.stat().st_mtime_ns,
                            "md5": hashlib.md5(p.read_bytes()).hexdigest(),
                            "pairs": 2, "verified": True} for p in (mate1, mate2)],
            }
            preflight = stock / "preflight.json"
            preflight.write_text(json.dumps(manifest))
            self.assertEqual(runner.stock_reuse(stock, exe, base, mate1, mate2, 2)[0], stock)
            with self.assertRaisesRegex(ValueError, "pair count"):
                runner.stock_reuse(stock, exe, base, mate1, mate2, 3)
            with self.assertRaisesRegex(ValueError, "argv"):
                runner.stock_reuse(stock, exe, base + ["--runRNGseed", "1"], mate1, mate2, 2)
            preflight.unlink()
            with self.assertRaisesRegex(ValueError, "preflight"):
                runner.stock_reuse(stock, exe, base, mate1, mate2, 2)
            preflight.write_text(json.dumps(manifest))
            exe.write_text("changed executable")
            with self.assertRaisesRegex(ValueError, "executable identity"):
                runner.stock_reuse(stock, exe, base, mate1, mate2, 2)
            exe.write_text("stock")
            mate2.write_bytes(b"changed input")
            with self.assertRaisesRegex(ValueError, "input identity"):
                runner.stock_reuse(stock, exe, base, mate1, mate2, 2)


if __name__ == "__main__":
    unittest.main()
