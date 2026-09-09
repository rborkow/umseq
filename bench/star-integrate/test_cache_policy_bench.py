import argparse
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import cache_policy_bench as cache
import host_observation


class CachePolicy(unittest.TestCase):
    def test_policy_is_explicit_and_scrubs_inherited_flags(self):
        with patch.dict("os.environ", {"STAR_INTEGRATE_THP": "0"}, clear=False):
            env = cache.policy_env(True, Path("stats.jsonl"))
        self.assertEqual(env["STAR_INTEGRATE_DROP_INDEX_CACHE"], "1")
        self.assertEqual(env["STAR_INTEGRATE_THP"], "1")
        self.assertEqual(env["STAR_INTEGRATE_STRICT"], "0")
        self.assertEqual(env["STAR_INTEGRATE_SIDECAR"], "stats.jsonl")

    def test_sidecar_rejects_missing_unclean_and_zero_live_contract(self):
        with tempfile.TemporaryDirectory() as tmp:
            sidecar = Path(tmp) / "stats.jsonl"
            with self.assertRaises(RuntimeError):
                cache.sidecar_stats(sidecar)
            sidecar.write_text(json.dumps({"gpu_consumed": 1}) + "\n")
            with self.assertRaises(RuntimeError):
                cache.sidecar_stats(sidecar)
            row = {"gpu_consumed": 1, **{key: 0 for key in cache.REQUIRED_ZERO}}
            sidecar.write_text(json.dumps(row) + "\n")
            self.assertEqual(cache.sidecar_stats(sidecar), row)
            sidecar.write_text(json.dumps(row) + "\n{}\n")
            with self.assertRaises(RuntimeError):
                cache.sidecar_stats(sidecar)

    def test_alternates_ab_ba(self):
        self.assertEqual(
            [name for name, _ in cache.arm_order(1)], ["evict-off", "evict-on"]
        )
        self.assertEqual(
            [name for name, _ in cache.arm_order(2)], ["evict-on", "evict-off"]
        )
        self.assertEqual(cache.gnu_time_fields("1\t2\t3\t4\t0\n")["max_rss_kib"], "4")
        with self.assertRaises(RuntimeError):
            cache.gnu_time_fields("not GNU time")

    def test_causal_entry_path_warms_outside_measured_rows_and_keeps_early_results(
        self,
    ):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / "STAR"
            binary.write_text("fixture")
            base = root / "base.json"
            base.write_text(
                json.dumps(
                    ["old", "--readFilesIn", "a", "b", "--outFileNamePrefix", "old/"]
                )
            )
            mate1, mate2 = root / "a", root / "b"
            mate1.write_text("a")
            mate2.write_text("b")
            args = argparse.Namespace(
                integrated=binary,
                base_argv=base,
                mate1=mate1,
                mate2=mate2,
                output=root / "out",
                timeout_s=2,
                repeats=2,
                mode="causal",
                lock=root / "lock",
            )
            labels = []
            row = {"gpu_consumed": 1, **{key: 0 for key in cache.REQUIRED_ZERO}}

            class Observed:
                def __init__(self, *_):
                    pass

                def start(self):
                    pass

                def finish(self):
                    return {"available": True, "target_sampled": True, "pid": 44}

            def fake_stage(root_arg, label, _argv, env, _timeout):
                labels.append((label, env["STAR_INTEGRATE_DROP_INDEX_CACHE"]))
                Path(env["STAR_INTEGRATE_SIDECAR"]).write_text(json.dumps(row) + "\n")
                stage = Path(root_arg) / "stages"
                stage.mkdir(exist_ok=True)
                (stage / f"{label}.time.tsv").write_text("1\t2\t3\t4\t0\n")

            with patch.object(cache, "HostObservation", Observed), patch.object(
                cache, "run_stage", fake_stage
            ):
                cache.main(args)
            measured = [label for label, _ in labels if not label.startswith("warm-")]
            self.assertEqual(
                measured, ["r1-evict-off", "r1-evict-on", "r2-evict-on", "r2-evict-off"]
            )
            self.assertEqual(len(labels), 8)
            accepted = [
                json.loads(line)
                for line in (args.output / "accepted-arms.jsonl")
                .read_text()
                .splitlines()
            ]
            self.assertEqual(
                [item["arm"] for item in accepted],
                ["evict-off", "evict-on", "evict-on", "evict-off"],
            )
            self.assertNotIn("warm", "".join(item["arm"] for item in accepted))

    def test_bad_parameters_and_consecutive_mode_fail_before_runs(self):
        args = argparse.Namespace(
            integrated=Path("missing"),
            base_argv=Path("missing"),
            mate1=Path("a"),
            mate2=Path("b"),
            output=Path("out"),
            timeout_s=0,
            repeats=0,
            mode="causal",
            lock=Path("lock"),
        )
        with self.assertRaises(ValueError):
            cache.main(args)
        self.assertEqual(
            host_observation.parse_key_values(
                "VmRSS:\t12 kB\nVmSwap:\t0 kB\n", ("VmRSS", "VmSwap")
            ),
            {"VmRSS": 12, "VmSwap": 0},
        )

    def test_later_arm_failure_preserves_accepted_early_result(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / "STAR"
            binary.write_text("fixture")
            base = root / "base.json"
            base.write_text(
                json.dumps(
                    ["old", "--readFilesIn", "a", "b", "--outFileNamePrefix", "old/"]
                )
            )
            mate1, mate2 = root / "a", root / "b"
            mate1.write_text("a")
            mate2.write_text("b")
            args = argparse.Namespace(
                integrated=binary,
                base_argv=base,
                mate1=mate1,
                mate2=mate2,
                output=root / "out",
                timeout_s=2,
                repeats=1,
                mode="causal",
                lock=root / "lock",
            )
            row = {"gpu_consumed": 1, **{key: 0 for key in cache.REQUIRED_ZERO}}

            class Observed:
                def __init__(self, *_):
                    pass

                def start(self):
                    pass

                def finish(self):
                    return {"available": True, "target_sampled": True, "pid": 44}

            def fake_stage(root_arg, label, _argv, env, _timeout):
                if label == "r1-evict-on":
                    raise RuntimeError("later arm failed")
                Path(env["STAR_INTEGRATE_SIDECAR"]).write_text(json.dumps(row) + "\n")
                stage = Path(root_arg) / "stages"
                stage.mkdir(exist_ok=True)
                (stage / f"{label}.time.tsv").write_text("1\t2\t3\t4\t0\n")

            with patch.object(cache, "HostObservation", Observed), patch.object(
                cache, "run_stage", fake_stage
            ):
                with self.assertRaisesRegex(RuntimeError, "later arm failed"):
                    cache.main(args)
            accepted = (args.output / "accepted-arms.jsonl").read_text().splitlines()
            self.assertEqual(len(accepted), 1)
            self.assertEqual(json.loads(accepted[0])["arm"], "evict-off")

    def test_procfs_unavailable_is_labeled_not_zero(self):
        with tempfile.TemporaryDirectory() as tmp:
            observer = host_observation.HostObservation(
                Path(tmp), sys.executable, proc_root=Path(tmp) / "no-proc"
            )
            observer.start()
            self.assertEqual(
                observer.finish(), {"available": False, "target_sampled": False}
            )
            text = (Path(tmp) / "host-observation.jsonl").read_text()
            self.assertIn("procfs unavailable", text)
            self.assertNotIn("VmSwap", text)

    def test_discovered_pid_without_a_successful_sample_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            observer = host_observation.HostObservation(Path(tmp), sys.executable)
            observer._unavailable = False
            observer.pid = 123  # Discovery succeeded, but reading procfs did not.
            with patch.object(host_observation, "host_snapshot", return_value={}):
                with self.assertRaisesRegex(RuntimeError, "never sampled"):
                    observer.finish()

    def test_observer_verifies_actual_target_pid_and_writes_raw_rows(self):
        if not Path("/proc/self/exe").exists():
            with tempfile.TemporaryDirectory() as tmp:
                observer = host_observation.HostObservation(
                    Path(tmp), sys.executable, proc_root=Path(tmp) / "no-proc"
                )
                observer.start()
                self.assertFalse(observer.finish()["available"])
            return
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            target = root / "target"
            shutil.copy2("/bin/sleep", target)
            observer = host_observation.HostObservation(root, target, interval_s=0.01)
            observer.start()
            process = subprocess.Popen([str(target), "0.1"])
            process.wait()
            result = observer.finish()
            self.assertTrue(result["target_sampled"])
            rows = [
                json.loads(line)
                for line in (root / "host-observation.jsonl").read_text().splitlines()
            ]
            during = [row for row in rows if row["phase"] == "during"]
            self.assertTrue(during)
            self.assertEqual(during[0]["target_exe"], str(target.resolve()))

    def test_sampler_failure_rejects_arm_after_retaining_stage_evidence(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            binary = root / "STAR"
            binary.write_text("fixture")
            mates = root / "mates"
            mates.write_text("fixture")
            args = argparse.Namespace(
                integrated=binary, mate1=mates, mate2=mates, timeout_s=2
            )
            row = {"gpu_consumed": 1, **{key: 0 for key in cache.REQUIRED_ZERO}}

            class FailingObserver:
                def __init__(self, *_):
                    pass

                def start(self):
                    pass

                def finish(self):
                    raise RuntimeError("target process was never sampled")

            def fake_stage(root_arg, label, _argv, env, _timeout):
                Path(env["STAR_INTEGRATE_SIDECAR"]).write_text(json.dumps(row) + "\n")
                stage = Path(root_arg) / "stages"
                stage.mkdir(exist_ok=True)
                (stage / f"{label}.time.tsv").write_text("1\t2\t3\t4\t0\n")

            run_root = root / "out"
            run_root.mkdir()
            with patch.object(cache, "HostObservation", FailingObserver), patch.object(
                cache, "run_stage", fake_stage
            ):
                with self.assertRaisesRegex(RuntimeError, "never sampled"):
                    cache.execute_arm(
                        run_root,
                        "r1-evict-off",
                        ["old", "--readFilesIn", "a", "b", "--outFileNamePrefix", "x/"],
                        args,
                        False,
                        warm=False,
                    )
            self.assertTrue((run_root / "stages" / "r1-evict-off.time.tsv").is_file())


if __name__ == "__main__":
    unittest.main()
