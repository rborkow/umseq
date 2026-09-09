#!/usr/bin/env python3
"""Read-only full-input verification, before launching a full-depth STAR gate.

Writes only a fresh evidence directory. Must run under the shared resource lock.
This is input/provenance evidence, NOT STAR parity or a performance measurement.
"""
import argparse
import fcntl
import gzip
import hashlib
import json
import os
from pathlib import Path
import sys
import time


def digest(path, algorithm):
    result = hashlib.new(algorithm)
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def fastq_line_count(path):
    """Count complete four-line records, also verifying gzip CRC at EOF.

    This checks the four-line FASTQ convention, not sequence/quality semantics.
    """
    lines = 0
    last = b""
    with gzip.open(path, "rb") as stream:
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            lines += block.count(b"\n")
            last = block[-1:]
    if last != b"\n" or lines == 0 or lines % 4:
        raise ValueError(f"not complete four-line FASTQ: {path}: {lines} lines")
    return lines // 4


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--gate", type=Path, required=True)
    parser.add_argument("--sample-dir", type=Path, required=True)
    parser.add_argument("--sample", required=True)
    parser.add_argument("--expected-pairs", type=int, required=True)
    parser.add_argument("--mate-md5", nargs=2, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.expected_pairs <= 0:
        parser.error("expected-pairs must be positive")
    lock_path = Path.home() / ".cache/uni-rnaseq-resource.lock"
    with lock_path.open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        args.out.mkdir(parents=False, exist_ok=False)
        report = {"status": "RUNNING", "started_epoch": time.time(),
                  "expected_pairs": args.expected_pairs, "inputs": []}

        def save():
            target = args.out / "preflight.json"
            temp = args.out / "preflight.json.tmp"
            temp.write_text(json.dumps(report, indent=2) + "\n")
            os.replace(temp, target)

        save()
        try:
            argv = json.loads((args.gate / "stages/integrated.argv.json").read_text())
            integrated = Path(argv[0])
            observed = {"bytes": integrated.stat().st_size,
                        "sha256": digest(integrated, "sha256")}
            expected = json.loads((args.gate / "binary.json").read_text())
            if observed != expected:
                raise ValueError("host24 integrated binary identity mismatch")
            stock_reuse = json.loads((args.gate / "stock-reuse.json").read_text())
            stock = Path(stock_reuse["argv"][0])
            report["integrated"] = {"path": str(integrated), **observed}
            report["stock"] = {"path": str(stock), "bytes": stock.stat().st_size,
                               "sha256": digest(stock, "sha256"),
                               "provenance": "path from gate stock-reuse.json; hash recorded now"}
            report["base_argv"] = argv
            report["gate"] = str(args.gate)
            report["meminfo_before"] = Path("/proc/meminfo").read_text()
            save()
            for mate, md5 in enumerate(args.mate_md5, 1):
                path = args.sample_dir / f"{args.sample}_{mate}.fastq.gz"
                entry = {"path": str(path), "bytes": path.stat().st_size,
                         "mtime_ns": path.stat().st_mtime_ns}
                report["inputs"].append(entry)
                save()
                entry["md5"] = digest(path, "md5")
                if entry["md5"] != md5:
                    raise ValueError(f"mate {mate} MD5 differs from input manifest")
                save()
                entry["pairs"] = fastq_line_count(path)
                if entry["pairs"] != args.expected_pairs:
                    raise ValueError(f"mate {mate} pair count differs from manifest")
                after = path.stat()
                if (entry["bytes"], entry["mtime_ns"]) != (after.st_size, after.st_mtime_ns):
                    raise ValueError(f"input changed during verification: mate {mate}")
                entry["verified"] = True
                save()
                print(f"mate {mate}: MD5 verified, {entry['pairs']} four-line records", flush=True)
            report["status"] = "PASS_INPUTS_ONLY"
            report["finished_epoch"] = time.time()
            save()
        except Exception as exc:
            report["status"] = "FAILED"
            report["error"] = str(exc)
            save()
            raise


if __name__ == "__main__":
    main()
