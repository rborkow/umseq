#!/usr/bin/env python3
"""Recompute REAL-REQUESTS medians from retained raw rows; never invent absent gates."""
import argparse
import csv
import json
from pathlib import Path
import statistics


def summarize(root):
    status = (root / "run.status").read_text()
    if "COMPLETE_REQUIRES_ORCHESTRATOR_VERIFICATION" not in status:
        raise ValueError("measurement did not complete")
    manifest = json.loads((root / "real-requests.json").read_text())
    count = manifest["actual_count"]
    if not 0 < count <= 1000000:
        raise ValueError("real request count outside authorized bound")
    result = {"real_request_count": count, "arms": {}}
    for name in ("real", "synthetic", "real-4k"):
        lines = (root / (name + ".tsv")).read_text().splitlines()
        if not any(line.startswith("# PROBE COMPLETE") for line in lines):
            raise ValueError(name + " has no completion marker")
        all_rows = list(csv.DictReader((line for line in lines if not line.startswith("#")), delimiter="\t"))
        rows = [row for row in all_rows if row["mode"] == "PROBE-isolated"]
        overlap = [row for row in all_rows if row["mode"] != "PROBE-isolated"]
        if len(overlap) != 3:
            raise ValueError(name + " lacks three retained disjoint-half overlap rows")
        if len(rows) != 3 or {int(r["repeat"]) for r in rows} != {1, 2, 3}:
            raise ValueError(name + " does not contain exactly three repeats")
        expected = 1000000 if name == "synthetic" else count
        for row in rows:
            if row["mode"] != "PROBE-isolated" or int(row["requests_per_arm"]) != expected or int(row["cpu_workers"]) != 20:
                raise ValueError(name + " changed the batch/comparator")
        for field in ("cpu_gpu_output_checksum", "sa_gathers", "compared_bytes", "loop_trips", "comparisons"):
            if len({r[field] for r in rows}) != 1:
                raise ValueError(name + " changed output/work between repeats: " + field)
        cpu = [float(r["cpu_s"]) for r in rows]
        gpu = [float(r["gpu_wall_s"]) for r in rows]
        c, g = statistics.median(cpu), statistics.median(gpu)
        result["arms"][name] = {
            "requests": expected, "overlap_rows": overlap, "cpu20_median_s": c, "gpu_wall_median_s": g,
            "cpu20_s": cpu, "gpu_wall_s": gpu, "cpu20_over_gpu": c / g,
            "gpu_repeat_range_over_median_pct": (max(gpu) - min(gpu)) / g * 100,
            "checksum": rows[0]["cpu_gpu_output_checksum"],
            "dependent_gathers_per_second": int(rows[0]["sa_gathers"]) / g,
            "distributions": json.loads((root / (name + "-distributions.json")).read_text())}
    real, small = result["arms"]["real"], result["arms"]["real-4k"]
    if real["checksum"] != small["checksum"]:
        raise ValueError("4K control changed outputs")
    result["real_4k_slowdown_over_huge"] = small["gpu_wall_median_s"] / real["gpu_wall_median_s"]
    result["decision_region"] = ">=5x rigor justified, user decision required" if real["cpu20_over_gpu"] >= 5 else "below 5x, user decision required"
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    args = parser.parse_args()
    print(json.dumps(summarize(args.root), indent=2))
