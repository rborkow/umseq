import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent


class PerfAndTimingScripts(unittest.TestCase):
    def test_index_prewarm_reads_all_three_files_and_records_bytes(self):
        sys.path.insert(0, str(ROOT))
        import measurement_runner as runner

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in ("Genome", "SA", "SAindex"):
                (root / name).write_bytes(b"index-fixture")
            record = root / "readback.json"
            runner.warm_index(["STAR", "--genomeDir", str(root)], record)
            rows = json.loads(record.read_text())
            self.assertEqual([row["bytes_read"] for row in rows], [13, 13, 13])
            (root / "SA").unlink()
            with self.assertRaises(FileNotFoundError):
                runner.warm_index(["STAR", "--genomeDir", str(root)], root / "bad.json")
            self.assertFalse((root / "bad.json").exists())

    def test_profile_times_star_inside_perf_not_the_profiler(self):
        sys.path.insert(0, str(ROOT))
        import measurement_runner as runner

        output = Path("/tmp/profile-command-test")
        argv = ["/test/STAR", "--runThreadN", "20"]
        command = runner.profile_command(argv, output)
        payload = command[command.index("--") + 1 :]
        self.assertEqual(Path(payload[0]).name, "time")
        self.assertEqual(
            payload[1:],
            ["-f", "%e\t%U\t%S\t%M\t%x", "-o", str(output / "star.time.tsv"), *argv],
        )
        self.assertEqual(command[command.index("-e") + 1], "cpu-clock")
        self.assertEqual(command[command.index("-F") + 1], "499")

    def test_environment_requires_selected_policy_and_scrubs_it(self):
        sys.path.insert(0, str(ROOT))
        import measurement_runner as runner

        env = runner._env(Path("run"), True, "1")
        self.assertEqual(env["STAR_INTEGRATE_THP"], "1")
        self.assertEqual(env["STAR_INTEGRATE_DROP_INDEX_CACHE"], "1")
        self.assertEqual(env["STAR_INTEGRATE_STRICT"], "0")
        self.assertEqual(env["STAR_INTEGRATE_SIDECAR"], "run/integrate-stats.jsonl")

    def test_sidecar_rejects_missing_live_charge(self):
        sys.path.insert(0, str(ROOT))
        import measurement_runner as runner

        with tempfile.TemporaryDirectory() as tmp:
            sidecar = Path(tmp) / "stats"
            clean = {
                "gpu_consumed": 1,
                "batch_faults": 0,
                "rejected": 0,
                "shift_mismatch": 0,
                "flag_mismatch": 0,
                "step_count_mismatch": 0,
                "live_bytes_at_finish": 0,
                "live_requests_at_finish": 0,
            }
            sidecar.write_text(json.dumps(clean) + "\n")
            runner._sidecar(sidecar)
            del clean["live_bytes_at_finish"]
            sidecar.write_text(json.dumps(clean) + "\n")
            with self.assertRaises(RuntimeError):
                runner._sidecar(sidecar)

    def test_scripts_have_no_fixed_host_or_recursive_delete(self):
        for name in ("perf_differential.sh", "run_timing_host.sh"):
            text = (ROOT / name).read_text()
            self.assertNotIn("rm -rf", text)
            self.assertNotIn("host8", text)
            self.assertNotIn("host5", text)
            self.assertIn("measurement_runner.py", text)
            self.assertIn("measurement_runner.py", text)
        runner = (ROOT / "measurement_runner.py").read_text()
        self.assertIn('"STAR_INTEGRATE_STRICT": "0"', runner)
        self.assertIn('"cpu-clock"', runner)
        self.assertIn("perf script", runner)

    def test_scripts_parse(self):
        for name in ("perf_differential.sh", "run_timing_host.sh"):
            subprocess.run(["bash", "-n", str(ROOT / name)], check=True)

    def test_fake_process_protocol_success_failure_timeout_and_signal(self):
        sys.path.insert(0, str(ROOT))
        import measurement_common as common
        import measurement_runner as runner

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            ok = root / "ok.py"
            ok.write_text("#!/usr/bin/env python3\n")
            ok.chmod(0o755)
            common.run_stage(
                root, "ok", [sys.executable, str(ok)], common.clean_env(), 2
            )
            bad = root / "bad.py"
            bad.write_text("#!/usr/bin/env python3\nraise SystemExit(7)\n")
            bad.chmod(0o755)
            with self.assertRaises(RuntimeError):
                common.run_stage(
                    root, "bad", [sys.executable, str(bad)], common.clean_env(), 2
                )
            sleeper = root / "sleep.py"
            sleeper.write_text("#!/usr/bin/env python3\nimport time; time.sleep(3)\n")
            sleeper.chmod(0o755)
            with self.assertRaises(RuntimeError):
                common.run_stage(
                    root,
                    "timeout",
                    [sys.executable, str(sleeper)],
                    common.clean_env(),
                    1,
                )
            self.assertLessEqual(int((root / "stages/timeout.exit").read_text()), 137)
            sidecar = root / "missing.jsonl"
            with self.assertRaises(RuntimeError):
                runner._sidecar(sidecar)
            sidecar.write_text("not-json\n")
            with self.assertRaises(json.JSONDecodeError):
                runner._sidecar(sidecar)
            sidecar.write_text(
                json.dumps({"gpu_consumed": 1, "batch_faults": 1}) + "\n"
            )
            with self.assertRaises(RuntimeError):
                runner._sidecar(sidecar)

    def test_runner_entry_rejects_nonpositive_parameters(self):
        sys.path.insert(0, str(ROOT))
        import measurement_runner as runner

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for name in ("stock", "integrated", "base"):
                (root / name).write_text("[]" if name == "base" else "fixture")
            args = type(
                "Args",
                (),
                {
                    "stock": root / "stock",
                    "integrated": root / "integrated",
                    "base": root / "base",
                    "repeats": 0,
                    "timeout_s": 0,
                    "output": root / "out",
                },
            )()
            with self.assertRaises(ValueError):
                runner.run(args, False)

    def test_runner_entry_executes_fake_binary_with_explicit_policy(self):
        import measurement_runner as runner

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / "STAR"
            os.symlink(Path(sys.executable).resolve(), binary)
            mate1, mate2 = root / "mate1", root / "mate2"
            mate1.write_text("one")
            mate2.write_text("two")
            for name in ("Genome", "SA", "SAindex"):
                (root / name).write_bytes(b"index-fixture")
            sidecar_row = {
                "gpu_consumed": 1,
                "batch_faults": 0,
                "rejected": 0,
                "shift_mismatch": 0,
                "flag_mismatch": 0,
                "step_count_mismatch": 0,
                "live_bytes_at_finish": 0,
                "live_requests_at_finish": 0,
            }
            code = (
                "import json,os,pathlib; p=os.getenv('STAR_INTEGRATE_SIDECAR'); "
                f"pathlib.Path(p).write_text({(json.dumps(sidecar_row) + chr(10))!r}) if p else None"
            )
            base = root / "base.json"
            base.write_text(
                json.dumps(
                    [
                        "old",
                        "-c",
                        code,
                        "--genomeDir",
                        str(root),
                        "--readFilesIn",
                        "x",
                        "y",
                        "--outFileNamePrefix",
                        "old/",
                    ]
                )
            )
            args = type(
                "Args",
                (),
                {
                    "stock": binary,
                    "integrated": binary,
                    "base": base,
                    "mate1": mate1,
                    "mate2": mate2,
                    "repeats": 1,
                    "timeout_s": 2,
                    "output": root / "out",
                    "profile_read_limit": None,
                    "drop_index_cache": "1",
                    "lock": root / "lock",
                },
            )()
            runner.run(args, False)
            rows = (args.output / "raw.tsv").read_text().splitlines()
            self.assertEqual(len(rows), 4)
            provenance = json.loads((args.output / "provenance.json").read_text())
            self.assertEqual(provenance["drop_index_cache"], "1")
            self.assertTrue((args.output / "r1-gpu/index-prewarm.json").is_file())
            self.assertTrue((args.output / "warm-r1-gpu/index-prewarm.json").is_file())

            # A CPU-only GPU warmup must stop before measured-arm prewarming.
            sidecar_row["gpu_consumed"] = 0
            payload = json.loads(base.read_text())
            payload[2] = (
                "import json,os,pathlib; p=os.getenv('STAR_INTEGRATE_SIDECAR'); "
                f"pathlib.Path(p).write_text({(json.dumps(sidecar_row) + chr(10))!r}) if p else None"
            )
            base.write_text(json.dumps(payload))
            args.output = root / "failed-warmup"
            with self.assertRaisesRegex(RuntimeError, "positively consume GPU"):
                runner.run(args, False)
            self.assertFalse((args.output / "r1-gpu/index-prewarm.json").exists())


if __name__ == "__main__":
    unittest.main()
