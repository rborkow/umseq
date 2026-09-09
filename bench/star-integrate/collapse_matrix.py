#!/usr/bin/env python3
"""Cost-inclusive same-binary collapse ablation with live residency checks.

Fresh processes, file-index prewarm only; no separate full-workload warmups.
Not directly comparable to the preceding full-workload-warmup timing series.
"""
import argparse
import hashlib
import json
from pathlib import Path

from cache_policy_bench import sidecar_stats
from measurement_common import load_base_argv, run_stage, star_argv, verify_lock
from measurement_runner import _env, warm_index
from observe_index_residency import ResidencyObservation


def order(repeat):
    arms = [("bypass-off", False, False), ("gpu-off", True, False),
            ("bypass-on", False, True), ("gpu-on", True, True)]
    shift = (repeat - 1) % len(arms)
    return arms[shift:] + arms[:shift]


def collapse_rows(text, enabled):
    rows = [dict(word.split("=", 1) for word in line.split()[1:])
            for line in text.splitlines()
            if line.startswith("STAR_INTEGRATE_COLLAPSE ")]
    if len(rows) != 3 or {r.get("label") for r in rows} != {"Genome", "SA", "SAindex"}:
        raise ValueError("missing/duplicate collapse diagnostics")
    for row in rows:
        if enabled:
            if not (row["attempted"] == "1" and row["result"] == "0"
                    and row["errno"] == "0" and int(row["bytes"]) > 0):
                raise ValueError("collapse intervention failed: " + str(row))
        elif row["state"] != "skipped-policy" or row["attempted"] != "0":
            raise ValueError("off arm unexpectedly attempted collapse")
    return rows


def residency_result(samples, calls, enabled):
    full = []
    for row in samples:
        maps = [m for m in row.get("large_anonymous_mappings", [])
                if "hg" in m.get("VmFlags", "").split()]
        if len(maps) == 3 and all(m.get("Rss", -1) == m.get("Size", 0) for m in maps):
            full.append((row, maps))
    if not full:
        raise ValueError("no full-resident sample of the three large advised VMAs")
    row, maps = full[-1]
    maps = sorted(maps, key=lambda m: m["extent_bytes"])
    if enabled:
        requests = sorted(calls, key=lambda c: int(c["bytes"]))
        for mapping, call in zip(maps, requests):
            size = int(call["bytes"])
            if not (0 <= mapping["extent_bytes"] - size <= 4 * 1024**2):
                raise ValueError("collapse request does not match candidate VMA size")
            if mapping["AnonHugePages"] * 1024 < size:
                raise ValueError("successful call did not deliver requested huge-page coverage")
    return {"timestamp_unix_s": row["timestamp_unix_s"], "mappings": maps,
            "huge_bytes": sum(m["AnonHugePages"] * 1024 for m in maps),
            "extent_bytes": sum(m["extent_bytes"] for m in maps),
            "fully_resident_samples": len(full)}


def main(args):
    lock = verify_lock(args.lock)
    _ = lock
    if args.repeats < 1:
        raise ValueError("positive repeats required")
    if hashlib.sha256(args.integrated.read_bytes()).hexdigest() != args.sha256:
        raise ValueError("candidate identity mismatch")
    args.output.mkdir(parents=True, exist_ok=False)
    base = load_base_argv(args.base_argv)
    provenance = {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()}
    provenance["protocol"] = "fresh processes, untimed full index-file read before each, no workload warmup, all target startup/collapse included; observer overhead outside target CPU but can perturb wall"
    (args.output / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    accepted = []
    for repeat in range(1, args.repeats + 1):
        for arm, gpu, collapse in order(repeat):
            name = f"r{repeat}-{arm}"
            directory = args.output / name
            directory.mkdir()
            env = _env(directory, gpu, 1)
            env["STAR_INTEGRATE_COLLAPSE_INDEX"] = str(int(collapse))
            (directory / "controls.json").write_text(json.dumps({k: v for k, v in env.items() if k.startswith("STAR_INTEGRATE")}, indent=2) + "\n")
            argv = star_argv(base, args.integrated, args.mate1, args.mate2, directory / "star_")
            warm_index(base, directory / "index-prewarm.json")
            observer = ResidencyObservation(directory, args.integrated, interval_s=2)
            print("START", name, flush=True)
            observer.start()
            try:
                run_stage(args.output, name, argv, env, args.timeout_s)
            finally:
                observed = observer.finish()
                (directory / "observer-summary.json").write_text(json.dumps(observed, indent=2) + "\n")
            calls = collapse_rows((args.output / "stages" / (name + ".stderr")).read_text(), collapse)
            stats = sidecar_stats(directory / "integrate-stats.jsonl") if gpu else None
            samples = [json.loads(x) for x in (directory / "host-observation.jsonl").read_text().splitlines()]
            residency = residency_result(samples, calls, collapse)
            timing = (args.output / "stages" / (name + ".time.tsv")).read_text().strip().split("\t")
            if len(timing) != 5 or timing[-1] != "0":
                raise ValueError("expected successful GNU time row")
            wall, user, system, rss = map(float, timing[:4])
            row = {"repeat": repeat, "arm": arm, "wall_s": wall,
                   "user_s": user, "system_s": system, "cpu_s": user + system,
                   "max_rss_kb": rss, "collapse": calls, "residency": residency,
                   "stats": stats}
            accepted.append(row)
            with (args.output / "accepted.jsonl").open("a") as stream:
                stream.write(json.dumps(row) + "\n")
            print("ACCEPTED", name, "cpu_s", row["cpu_s"], "huge_bytes", residency["huge_bytes"], flush=True)
    assert len(accepted) == 4 * args.repeats
    print("COMPLETE_COST_INCLUSIVE_MATRIX", len(accepted), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    for name in ("integrated", "base-argv", "mate1", "mate2", "output", "lock"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--timeout-s", type=int, default=1200)
    main(parser.parse_args())
