#!/usr/bin/env python3
"""Validated host-only timing/profiling runner used by both shell entry points."""

import argparse
import json
from pathlib import Path
import subprocess

from measurement_common import (
    clean_env,
    executable_identity,
    fresh_dir,
    load_base_argv,
    replace_option,
    run_stage,
    star_argv,
    verify_lock,
)


def _argv(base, binary, mate1, mate2, out, read_limit=None):
    value = star_argv(base, binary, mate1, mate2, out)
    if read_limit is not None:
        if "--readMapNumber" in value:
            value = replace_option(value, "--readMapNumber", (read_limit,))
        else:
            value.extend(["--readMapNumber", str(read_limit)])
    return value


def _sidecar(path):
    if not path.is_file():
        raise RuntimeError("missing required integration sidecar: " + str(path))
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    if len(rows) != 1:
        raise RuntimeError("expected one final integration sidecar row")
    row = rows[0]
    if not isinstance(row, dict) or row.get("gpu_consumed", 0) <= 0:
        raise RuntimeError("performance arm did not positively consume GPU output")
    required = (
        "batch_faults",
        "rejected",
        "shift_mismatch",
        "flag_mismatch",
        "step_count_mismatch",
        "live_bytes_at_finish",
        "live_requests_at_finish",
    )
    bad = [key for key in required if key not in row or row[key] != 0]
    if bad:
        raise RuntimeError("unclean integration sidecar counters: " + ", ".join(bad))
    return row


def _env(directory, gpu, drop_index_cache):
    extra = {
        "STAR_INTEGRATE_STRICT": "0",
        "STAR_INTEGRATE_THP": "1",
        "STAR_INTEGRATE_DROP_INDEX_CACHE": drop_index_cache,
    }
    if gpu:
        extra.update(
            {
                "STAR_INTEGRATE": "1",
                "STAR_INTEGRATE_SIDECAR": directory / "integrate-stats.jsonl",
            }
        )
    return clean_env(extra)


def warm_index(base, record):
    positions = [i for i, value in enumerate(base) if value == "--genomeDir"]
    if len(positions) != 1 or positions[0] + 1 >= len(base):
        raise ValueError("expected exactly one --genomeDir")
    directory = Path(base[positions[0] + 1])
    rows = []
    for name in ("Genome", "SA", "SAindex"):
        path = directory / name
        before = path.stat()
        count = 0
        with path.open("rb") as stream:
            while block := stream.read(8 * 1024 * 1024):
                count += len(block)
        after = path.stat()
        if count != before.st_size or (before.st_size, before.st_mtime_ns) != (
            after.st_size, after.st_mtime_ns
        ):
            raise RuntimeError("index changed while warming: " + str(path))
        rows.append({"path": str(path), "bytes_read": count, "mtime_ns": after.st_mtime_ns})
    record.write_text(json.dumps(rows, indent=2) + "\n")


def profile_command(argv, directory):
    # Inner time accounts for STAR and its children, not perf recording/extraction.
    # run_stage additionally retains wrapper-inclusive diagnostic time separately.
    return [
        "perf",
        "record",
        "-q",
        "-e",
        "cpu-clock",
        "-F",
        "499",
        "-g",
        "-o",
        str(directory / "perf.data"),
        "--",
        "/usr/bin/time",
        "-f",
        "%e\t%U\t%S\t%M\t%x",
        "-o",
        str(directory / "star.time.tsv"),
        *argv,
    ]


def run(args, profile):
    for path in (args.stock, args.integrated, args.base):
        if not path.is_file():
            raise ValueError("required file missing: " + str(path))
    if args.repeats <= 0 or args.timeout_s <= 0:
        raise ValueError("repeats and timeout must be positive")
    root = fresh_dir(args.output)
    lock = verify_lock(args.lock)
    try:
        base = load_base_argv(args.base)
        if args.mate1 is None or args.mate2 is None:
            positions = [i for i, value in enumerate(base) if value == "--readFilesIn"]
            if len(positions) != 1 or positions[0] + 2 >= len(base):
                raise ValueError(
                    "base argv must contain exactly one --readFilesIn pair"
                )
            args.mate1, args.mate2 = Path(base[positions[0] + 1]), Path(
                base[positions[0] + 2]
            )
        for path in (args.mate1, args.mate2):
            if not path.is_file():
                raise ValueError("required mate file missing: " + str(path))
        (root / "provenance.json").write_text(
            json.dumps(
                {
                    "stock": executable_identity(args.stock),
                    "integrated": executable_identity(args.integrated),
                    "base_argv": base,
                    "repeats": args.repeats,
                    "profile_event": "cpu-clock" if profile else None,
                    "profile_read_limit": args.profile_read_limit,
                    "strict": 0,
                    "thp": 1,
                    "drop_index_cache": args.drop_index_cache,
                    "index_precondition": "full untimed read of Genome/SA/SAindex before every invocation",
                    "warmup": "same-policy workload, followed by index precondition again",
                },
                indent=2,
            )
            + "\n"
        )
        arms = {
            "stock": (args.stock, False),
            "bypass": (args.integrated, False),
            "gpu": (args.integrated, True),
        }
        orders = (
            ("stock", "bypass", "gpu"),
            ("gpu", "stock", "bypass"),
            ("bypass", "gpu", "stock"),
        )
        rows = []
        for repeat in range(1, args.repeats + 1):
            for name in orders[(repeat - 1) % len(orders)]:
                binary, gpu = arms[name]
                directory = root / f"r{repeat}-{name}"
                directory.mkdir()
                argv = _argv(
                    base,
                    binary,
                    args.mate1,
                    args.mate2,
                    directory,
                    args.profile_read_limit if profile else None,
                )
                warm = root / f"warm-r{repeat}-{name}"
                warm.mkdir()
                warm_index(base, warm / "index-prewarm.json")
                run_stage(
                    root,
                    f"warm-r{repeat}-{name}",
                    _argv(
                        base,
                        binary,
                        args.mate1,
                        args.mate2,
                        warm,
                        args.profile_read_limit if profile else None,
                    ),
                    _env(warm, gpu, args.drop_index_cache),
                    args.timeout_s,
                )
                if gpu:
                    _sidecar(warm / "integrate-stats.jsonl")
                warm_index(base, directory / "index-prewarm.json")
                if profile:
                    perf_data = directory / "perf.data"
                    command = profile_command(argv, directory)
                    run_stage(
                        root,
                        f"r{repeat}-{name}",
                        command,
                        _env(directory, gpu, args.drop_index_cache),
                        args.timeout_s,
                    )
                    script = subprocess.run(
                        [
                            "perf",
                            "script",
                            "-i",
                            str(perf_data),
                            "--fields",
                            "tid,ip,sym,dso",
                        ],
                        check=False,
                        capture_output=True,
                        text=True,
                    )
                    (directory / "leaf-stacks.txt").write_text(script.stdout)
                    if script.returncode or not script.stdout.strip():
                        raise RuntimeError(
                            f"perf script extraction failed; raw data retained in {perf_data}"
                        )
                    rows.append(
                        (
                            repeat,
                            name,
                            (directory / "star.time.tsv").read_text().strip(),
                        )
                    )
                else:
                    run_stage(
                        root,
                        f"r{repeat}-{name}",
                        argv,
                        _env(directory, gpu, args.drop_index_cache),
                        args.timeout_s,
                    )
                    rows.append(
                        (
                            repeat,
                            name,
                            (root / "stages" / f"r{repeat}-{name}.time.tsv")
                            .read_text()
                            .strip(),
                        )
                    )
                if gpu:
                    _sidecar(directory / "integrate-stats.jsonl")
        target = root / ("profile-rows.tsv" if profile else "raw.tsv")
        target.write_text(
            "repeat\tarm\tvalue\n"
            + "\n".join("\t".join(map(str, row)) for row in rows)
            + "\n"
        )
    finally:
        if lock is not None:
            lock.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stock", type=Path, required=True)
    parser.add_argument("--integrated", type=Path, required=True)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--mate1", type=Path)
    parser.add_argument("--mate2", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout-s", type=int, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--profile", action="store_true")
    parser.add_argument("--profile-read-limit", type=int)
    parser.add_argument("--drop-index-cache", choices=("0", "1"), required=True)
    parser.add_argument(
        "--lock", type=Path, default=Path.home() / ".cache/uni-rnaseq-resource.lock"
    )
    args = parser.parse_args()
    if args.profile and args.profile_read_limit is None:
        parser.error("--profile-read-limit is required for profiling")
    run(args, args.profile)


if __name__ == "__main__":
    main()
