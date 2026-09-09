#!/usr/bin/env python3
"""Observe large anonymous mappings in a same-binary CPU/GPU diagnostic pair.

This observes residency, not a new performance gate. Large anonymous VMAs are
candidates for index mappings, not automatically attributed to index arrays.
"""
import argparse
import json
from pathlib import Path
import re

from host_observation import HostObservation
from measurement_common import executable_identity, fresh_dir, load_base_argv, run_stage, verify_lock
from measurement_runner import _argv, _env, _sidecar, warm_index

HEADER = re.compile(r"^[0-9a-f]+-[0-9a-f]+\s")
FIELDS = {"Size", "Rss", "AnonHugePages", "THPeligible", "KernelPageSize", "MMUPageSize"}


def large_anonymous_mappings(text, minimum_bytes=1024**3):
    result, current = [], None
    for line in text.splitlines():
        if HEADER.match(line):
            parts = line.split(maxsplit=5)
            low, high = (int(v, 16) for v in parts[0].split("-"))
            name = parts[5] if len(parts) == 6 else ""
            current = None
            if high - low >= minimum_bytes and parts[4] == "0":
                current = {"range": parts[0], "permissions": parts[1], "name": name,
                           "extent_bytes": high - low, "field_units": "kB except THPeligible"}
                result.append(current)
        elif current is not None and ":" in line:
            key, value = line.split(":", 1)
            if key in FIELDS:
                current[key] = int(value.split()[0])
            elif key == "VmFlags":
                current[key] = value.strip()
    return result


class ResidencyObservation(HostObservation):
    def _write(self, row):
        if row.get("phase") == "during":
            pid = row["process"]["pid"]
            row["large_anonymous_mappings"] = large_anonymous_mappings(
                (self.proc_root / str(pid) / "smaps").read_text()
            )
        super()._write(row)


def run(args):
    lock = verify_lock(args.lock)
    try:
        root = fresh_dir(args.output)
        binary = executable_identity(args.integrated)
        if binary["sha256"] != args.expected_sha256:
            raise ValueError("binary does not match accepted identity")
        base = load_base_argv(args.base)
        i = base.index("--readFilesIn")
        mate1, mate2 = Path(base[i + 1]), Path(base[i + 2])
        (root / "protocol.json").write_text(json.dumps({
            "binary": binary, "base_argv": base, "order": ["bypass", "gpu"],
            "strict": 0, "thp": 1, "drop_index_cache": 1,
            "precondition": "untimed full index-file read before each invocation",
            "purpose": "residency diagnosis, not timing acceptance",
            "large_mapping_threshold_bytes": 1024**3, "sample_interval_s": 1.0,
        }, indent=2) + "\n")
        for name, gpu in (("bypass", False), ("gpu", True)):
            directory = root / name
            directory.mkdir()
            warm_index(base, directory / "index-prewarm.json")
            observer = ResidencyObservation(directory, args.integrated)
            observer.start()
            print("START", name, flush=True)
            try:
                run_stage(root, name, _argv(base, args.integrated, mate1, mate2, directory),
                          _env(directory, gpu, "1"), args.timeout_s)
            finally:
                observed = observer.finish()
                (directory / "observation-summary.json").write_text(json.dumps(observed, indent=2) + "\n")
            if gpu:
                _sidecar(directory / "integrate-stats.jsonl")
            samples = [json.loads(line) for line in observer.path.read_text().splitlines()]
            if not any(row.get("large_anonymous_mappings") for row in samples):
                raise RuntimeError("no large target mappings observed")
            print("OBSERVED", name, observed, flush=True)
    finally:
        if lock is not None:
            lock.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--integrated", type=Path, required=True)
    parser.add_argument("--expected-sha256", required=True)
    parser.add_argument("--base", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout-s", type=int, default=1200)
    parser.add_argument("--lock", type=Path, default=Path.home() / ".cache/uni-rnaseq-resource.lock")
    run(parser.parse_args())
