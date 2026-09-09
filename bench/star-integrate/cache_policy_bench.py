#!/usr/bin/env python3
"""Run cache-policy screens without global cache eviction or output deletion."""

import argparse
import json
from pathlib import Path

from host_observation import HostObservation
from measurement_common import (
    clean_env,
    executable_identity,
    fresh_dir,
    identity,
    load_base_argv,
    run_stage,
    star_argv,
    verify_lock,
)

REQUIRED_ZERO = (
    "batch_faults",
    "rejected",
    "shift_mismatch",
    "flag_mismatch",
    "step_count_mismatch",
    "live_bytes_at_finish",
    "live_requests_at_finish",
)


def policy_env(drop, sidecar):
    return clean_env(
        {
            "STAR_INTEGRATE": "1",
            "STAR_INTEGRATE_STRICT": "0",
            "STAR_INTEGRATE_THP": "1",
            "STAR_INTEGRATE_DROP_INDEX_CACHE": "1" if drop else "0",
            "STAR_INTEGRATE_SIDECAR": sidecar,
        }
    )


def sidecar_stats(path):
    if not path.is_file():
        raise RuntimeError("missing required integration sidecar: " + str(path))
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    if len(rows) != 1 or not isinstance(rows[0], dict):
        raise RuntimeError("expected one final integration sidecar row")
    row = rows[0]
    if (
        not isinstance(row.get("gpu_consumed"), (int, float))
        or row["gpu_consumed"] <= 0
    ):
        raise RuntimeError("cache arm did not positively consume GPU output")
    missing = [name for name in REQUIRED_ZERO if name not in row]
    bad = [name for name in REQUIRED_ZERO if name in row and row[name] != 0]
    if missing or bad:
        raise RuntimeError(
            "unclean or incomplete integration sidecar: " + ", ".join(missing + bad)
        )
    return row


def arm_order(repeat):
    arms = [("evict-off", False), ("evict-on", True)]
    return arms if repeat % 2 else list(reversed(arms))


def gnu_time_fields(text):
    values = text.strip().split("\t")
    if len(values) != 5:
        raise RuntimeError("measured arm is missing the expected GNU time fields")
    names = ("wall_s", "user_s", "system_s", "max_rss_kib", "exit_status")
    return dict(zip(names, values))


def execute_arm(root, label, base, args, drop, warm):
    directory = root / label
    directory.mkdir()
    sidecar = directory / "integrate-stats.jsonl"
    argv = star_argv(base, args.integrated, args.mate1, args.mate2, directory)
    observer = None if warm else HostObservation(directory, args.integrated)
    if observer:
        observer.start()
    try:
        run_stage(root, label, argv, policy_env(drop, sidecar), args.timeout_s)
    finally:
        observation = observer.finish() if observer else None
    stats = sidecar_stats(sidecar)
    if observation is not None:
        (directory / "host-observation-summary.json").write_text(
            json.dumps(observation, indent=2) + "\n"
        )
    return stats


def main(args):
    if not args.integrated.is_file() or not args.base_argv.is_file():
        raise ValueError("integrated binary and base argv must exist")
    if args.repeats <= 0 or args.timeout_s <= 0:
        raise ValueError("repeats and timeout must be positive")
    if args.mode != "causal":
        raise ValueError(
            "consecutive mode is disabled: it does not isolate same-policy invocations; use causal"
        )
    output = fresh_dir(args.output)
    lock = verify_lock(args.lock)
    try:
        base = load_base_argv(args.base_argv)
        (output / "policy.json").write_text(
            json.dumps(
                {
                    "mode": args.mode,
                    "warm_before_each_causal_arm": True,
                    "no_global_drop_caches": True,
                    "strict": False,
                    "thp": "1",
                    "arms": ["evict-off", "evict-on"],
                    "alternating_order": "AB on odd repeats, BA on even repeats",
                    "consecutive_mode": "disabled",
                    "repeats": args.repeats,
                    "integrated": executable_identity(args.integrated),
                    "mate1": identity(args.mate1, True),
                    "mate2": identity(args.mate2, True),
                },
                indent=2,
            )
            + "\n"
        )
        summary = output / "accepted-arms.jsonl"
        for repeat in range(1, args.repeats + 1):
            for name, drop in arm_order(repeat):
                execute_arm(
                    output, f"warm-r{repeat}-{name}", base, args, False, warm=True
                )
                stats = execute_arm(
                    output, f"r{repeat}-{name}", base, args, drop, warm=False
                )
                timing_raw = (
                    output / "stages" / f"r{repeat}-{name}.time.tsv"
                ).read_text()
                with summary.open("a") as stream:
                    stream.write(
                        json.dumps(
                            {
                                "repeat": repeat,
                                "arm": name,
                                "drop_index_cache": int(drop),
                                "gnu_time_raw": timing_raw.strip(),
                                "gnu_time": gnu_time_fields(timing_raw),
                                "sidecar": stats,
                                "accepted": True,
                            }
                        )
                        + "\n"
                    )
    finally:
        if lock is not None:
            lock.close()


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--integrated", type=Path, required=True)
    p.add_argument("--base-argv", type=Path, required=True)
    p.add_argument("--mate1", type=Path, required=True)
    p.add_argument("--mate2", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--timeout-s", type=int, required=True)
    p.add_argument("--mode", choices=("causal", "consecutive"), required=True)
    p.add_argument("--repeats", type=int, default=3)
    p.add_argument(
        "--lock", type=Path, default=Path.home() / ".cache/uni-rnaseq-resource.lock"
    )
    main(p.parse_args())
