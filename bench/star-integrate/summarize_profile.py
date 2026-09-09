#!/usr/bin/env python3
"""Streaming self-sample attribution of perf script -F tid,ip,sym,dso.

CPU-second allocations are sampling estimates scaled by measured target CPU
seconds, not separately timed functions. Raw profiles remain authoritative.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re

FRAME = re.compile(r"^\s+([0-9a-fA-F]+) (.+) \((.+)\)$")
TID = re.compile(r"^\s*(\d+)\s*$")


def parse_stacks(path):
    result = {
        "total": 0, "kernel_samples": 0,
        "leaf": Counter(), "kernel_caller": Counter(),
        "kernel_application": Counter(), "per_tid": Counter(),
    }
    tid, frames = None, []

    def flush():
        if tid is None:
            return
        if not frames:
            raise ValueError(f"sample without frames: {path}, tid={tid}")
        leaf = frames[0]
        result["total"] += 1
        result["leaf"][leaf] += 1
        kernel = leaf[0] == "[kernel.kallsyms]"
        result["per_tid"][(tid, "kernel" if kernel else "user")] += 1
        if kernel:
            result["kernel_samples"] += 1
            caller = next((f for f in frames if f[0] != "[kernel.kallsyms]"), ("[unresolved]", "[unresolved]"))
            application = next((f for f in frames if f[0] == "STAR"), ("[unresolved]", "[unresolved]"))
            result["kernel_caller"][caller] += 1
            result["kernel_application"][application] += 1

    with Path(path).open() as stream:
        for number, line in enumerate(stream, 1):
            line = line.rstrip("\n")
            if not line.strip():
                flush()
                tid, frames = None, []
                continue
            match = TID.fullmatch(line)
            if match:
                flush()
                tid, frames = int(match[1]), []
                continue
            match = FRAME.fullmatch(line)
            if match and tid is not None:
                frames.append((Path(match[3]).name, match[2]))
                continue
            raise ValueError(f"unparsed line {path}:{number}: {line[:200]}")
    flush()
    if result["total"] == 0:
        raise ValueError(f"empty profile: {path}")
    return result


def summarize(root):
    arms = {}
    for arm in ("stock", "bypass", "gpu"):
        directory = root / ("r1-" + arm)
        fields = (directory / "star.time.tsv").read_text().strip().split("\t")
        if len(fields) != 5 or fields[-1] != "0":
            raise ValueError("invalid target GNU time: " + arm)
        cpu = float(fields[1]) + float(fields[2])
        parsed = parse_stacks(directory / "leaf-stacks.txt")
        row = {"arm": arm, "cpu_s": cpu, "raw_gnu_time": "\t".join(fields),
               "samples": parsed["total"], "kernel_samples": parsed["kernel_samples"],
               "attribution": "self-sample CPU-s estimates; one profiled invocation, not repeated timings"}
        for key in ("leaf", "kernel_caller", "kernel_application", "per_tid"):
            row[key] = [
                {"key": list(k), "samples": n, "estimated_cpu_s": n * cpu / parsed["total"]}
                for k, n in parsed[key].most_common()
            ]
        (root / (arm + "-attribution.json")).write_text(json.dumps(row, indent=2) + "\n")
        arms[arm] = row
        print(arm, "samples", row["samples"], "cpu_s", cpu, flush=True)
    maps = {arm: {tuple(r["key"]): r["estimated_cpu_s"] for r in row["leaf"]} for arm, row in arms.items()}
    keys = set().union(*(set(m) for m in maps.values()))
    differential = [{"key": list(k), **{arm: m.get(k, 0) for arm, m in maps.items()},
                     "gpu_minus_bypass_cpu_s": maps["gpu"].get(k, 0) - maps["bypass"].get(k, 0)} for k in keys]
    differential.sort(key=lambda row: abs(row["gpu_minus_bypass_cpu_s"]), reverse=True)
    (root / "self-differential.json").write_text(json.dumps(differential, indent=2) + "\n")
    print(json.dumps(differential[:20], indent=2), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    summarize(parser.parse_args().root)
