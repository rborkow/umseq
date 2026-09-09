#!/usr/bin/env python3
"""Full-depth, frozen-binary STAR parity gate; it never compiles or deletes."""
import argparse
import hashlib
import json
from pathlib import Path

from measurement_common import (clean_env, executable_identity, fresh_dir, identity,
                                load_base_argv, run_stage, star_argv, verify_lock)


def sidecar_stats(path):
    rows = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    if len(rows) != 1:
        raise RuntimeError("expected exactly one final sidecar row")
    stats = rows[0]
    required_zero = ("batch_faults", "rejected", "shift_mismatch", "flag_mismatch", "step_count_mismatch")
    if not isinstance(stats, dict) or "gpu_consumed" not in stats or any(name not in stats for name in required_zero):
        raise RuntimeError("strict sidecar accounting is incomplete")
    if (not isinstance(stats["gpu_consumed"], (int, float)) or
            stats["gpu_consumed"] <= 0 or
            any(not isinstance(stats[name], (int, float)) or stats[name] != 0 for name in required_zero)):
        raise RuntimeError("strict sidecar accounting rejected: " + json.dumps(stats, sort_keys=True))
    return stats


def comparator_command(tool, stock_dir, strict_dir, stock, strict, expected, out):
    # seed_split_parity.py uses literal space splitting for these diagnostic fields.
    # Rejecting whitespace preserves the checker’s supported serialization exactly.
    if any(any(ch.isspace() for ch in token) for token in stock + strict):
        raise ValueError("STAR argv tokens containing whitespace are unsupported by comparator")
    return ["python3", "-B", str(tool), "--stock", str(stock_dir), "--counters", str(strict_dir),
            "--stock-command", " ".join(stock), "--counter-command", " ".join(strict),
            "--expected-pairs", str(expected), "--out", str(out)]


def stock_reuse(root, current_stock, base, mate1, mate2, expected_pairs):
    """Validate an immutable stock result before allowing it to replace a fresh run."""
    root = Path(root)
    argv_file = root / "argv.json"
    status_file = root / "status"
    exit_file = root / "exit.code"
    if not argv_file.is_file() or not status_file.is_file() or not exit_file.is_file():
        raise ValueError("stock reuse requires argv.json, status, and exit.code")
    argv = json.loads(argv_file.read_text())
    if not isinstance(argv, list) or not all(isinstance(x, str) for x in argv):
        raise ValueError("stock reuse argv must be a JSON array of strings")
    if status_file.read_text().strip() != "PASS_STOCK_ONLY_NOT_PARITY":
        raise ValueError("stock reuse success marker is absent")
    if exit_file.read_text().strip() != "0":
        raise ValueError("stock reuse exit is not zero")
    preflight = root / "preflight.json"
    if not preflight.is_file():
        raise ValueError("stock reuse verified preflight is missing")
    manifest = json.loads(preflight.read_text())
    if not isinstance(manifest, dict) or manifest.get("status") != "PASS_INPUTS_ONLY":
        raise ValueError("stock reuse preflight did not pass")
    if type(manifest.get("expected_pairs")) is not int or manifest["expected_pairs"] != expected_pairs:
        raise ValueError("stock reuse pair count differs from independent manifest")
    expected = star_argv(base, current_stock, mate1, mate2, root)
    # Reconstruct with the original golden's output prefix. Every other token,
    # including the recorded stock executable, must agree exactly.
    if argv != expected:
        raise ValueError("stock reuse argv differs from candidate parameters")
    actual_stock = executable_identity(current_stock)
    recorded_stock = manifest.get("stock", {})
    if any(recorded_stock.get(key) != value for key, value in actual_stock.items()):
        raise ValueError("stock reuse executable identity differs")
    inputs = manifest.get("inputs")
    if not isinstance(inputs, list) or len(inputs) != 2:
        raise ValueError("stock reuse input identity is incomplete")
    for path, recorded in zip((mate1, mate2), inputs):
        stat = path.stat()
        if (not isinstance(recorded, dict) or recorded.get("verified") is not True or
                recorded.get("path") != str(path) or recorded.get("pairs") != expected_pairs or
                recorded.get("bytes") != stat.st_size or recorded.get("mtime_ns") != stat.st_mtime_ns):
            raise ValueError("stock reuse input identity differs: " + str(path))
        digest = hashlib.md5()
        with path.open("rb") as stream:
            for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
                digest.update(block)
        if digest.hexdigest() != recorded.get("md5"):
            raise ValueError("stock reuse input identity checksum differs: " + str(path))
    for name in ("Aligned.out.sam", "SJ.out.tab", "Log.final.out"):
        path = root / name
        if not path.is_file() or path.stat().st_size == 0:
            raise ValueError("stock reuse output missing or empty: " + name)
    counts = [int(line.split("|")[1].strip())
              for line in (root / "Log.final.out").read_text().splitlines()
              if line.split("|")[0].strip() == "Number of input reads"]
    if counts != [expected_pairs]:
        raise ValueError("stock reuse output pair count differs from verified preflight")
    return root, argv


def strict_environment(sidecar, drop_index_cache, collapse_index="0"):
    if drop_index_cache not in ("0", "1"):
        raise ValueError("drop-index-cache must be 0 or 1")
    if collapse_index not in ("0", "1"):
        raise ValueError("collapse-index must be 0 or 1")
    return clean_env({"STAR_INTEGRATE": "1", "STAR_INTEGRATE_STRICT": "1",
                      "STAR_INTEGRATE_SIDECAR": sidecar, "STAR_INTEGRATE_THP": "1",
                      "STAR_INTEGRATE_DROP_INDEX_CACHE": drop_index_cache,
                      "STAR_INTEGRATE_COLLAPSE_INDEX": collapse_index})


def main(args):
    if args.stock is None:
        raise ValueError("--stock is required to establish the candidate executable identity")
    for path in (args.stock, args.integrated, args.tooling / "seed_split_parity.py", args.mate1, args.mate2, args.base_argv):
        if path is None: continue
        if not path.is_file():
            raise ValueError("required file missing: " + str(path))
    if args.expected_pairs <= 0:
        raise ValueError("--expected-pairs must be independently supplied and positive")
    output = fresh_dir(args.output)
    lock = verify_lock(args.lock)
    try:
        base = load_base_argv(args.base_argv)
        if "--readMapNumber" in base:
            raise ValueError("full-depth accepted argv must not contain --readMapNumber")
        (output / "identity.json").write_text(json.dumps({
            "stock": executable_identity(args.stock),
            "integrated": executable_identity(args.integrated),
            "mate1": identity(args.mate1, True), "mate2": identity(args.mate2, True),
            "checker": executable_identity(args.tooling / "seed_split_parity.py"),
            "base_argv": str(args.base_argv), "expected_pairs": args.expected_pairs,
        }, indent=2) + "\n")
        stock_dir, strict_dir = output / "stock", output / "strict"
        strict_dir.mkdir()
        if args.reuse_stock:
            reused, stock = stock_reuse(args.reuse_stock, args.stock, base, args.mate1, args.mate2, args.expected_pairs)
            stock_dir.symlink_to(reused, target_is_directory=True)
        else:
            stock_dir.mkdir()
            stock = star_argv(base, args.stock, args.mate1, args.mate2, stock_dir)
        strict = star_argv(base, args.integrated, args.mate1, args.mate2, strict_dir)
        sidecar = strict_dir / "integrate-stats.jsonl"
        if not args.reuse_stock:
            run_stage(output, "stock", stock, clean_env(), args.timeout_s)
        run_stage(output, "strict", strict, strict_environment(sidecar, args.drop_index_cache, args.collapse_index), args.timeout_s)
        if not (stock_dir / "Aligned.out.sam").is_file() or not (strict_dir / "Aligned.out.sam").is_file():
            raise RuntimeError("successful STAR run missing Aligned.out.sam")
        comparator = comparator_command(args.tooling / "seed_split_parity.py", stock_dir, strict_dir, stock, strict, args.expected_pairs, output / "parity")
        run_stage(output, "parity", comparator, clean_env(), args.timeout_s)
        parity = json.loads((output / "parity" / "parity.json").read_text())
        if parity.get("status") != "PARITY_MATCH_COUNTERS_ONLY":
            raise RuntimeError("checker did not report PARITY_MATCH_COUNTERS_ONLY")
        stats = sidecar_stats(sidecar)
        (output / "gate-summary.json").write_text(json.dumps({"status": "PARITY_MATCH_COUNTERS_ONLY_REQUIRED", "stats": stats}, indent=2) + "\n")
    finally:
        if lock is not None:
            lock.close()


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--stock", type=Path, required=True); p.add_argument("--reuse-stock", type=Path)
    p.add_argument("--integrated", type=Path, required=True)
    p.add_argument("--tooling", type=Path, required=True); p.add_argument("--base-argv", type=Path, required=True)
    p.add_argument("--mate1", type=Path, required=True); p.add_argument("--mate2", type=Path, required=True)
    p.add_argument("--expected-pairs", type=int, required=True); p.add_argument("--output", type=Path, required=True)
    p.add_argument("--timeout-s", type=int, required=True); p.add_argument("--lock", type=Path, default=Path.home() / ".cache/uni-rnaseq-resource.lock")
    p.add_argument("--hash-inputs", action="store_true", help="hash binaries and FASTQs before mapping; otherwise record size/path only")
    p.add_argument("--drop-index-cache", choices=("0", "1"), default="0",
                   help="explicit post-load eviction policy; 1 preserves historical gates")
    p.add_argument("--collapse-index", choices=("0", "1"), default="0",
                   help="explicit opt-in post-load collapse policy")
    main(p.parse_args())
