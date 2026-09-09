#!/usr/bin/env python3
"""Verify the frozen STAR index/checker against accepted gate evidence."""
import fcntl
import hashlib
import json
from pathlib import Path


def identity(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(8 * 1024 * 1024), b""):
            h.update(block)
    return {"bytes": path.stat().st_size, "sha256": h.hexdigest()}


def main():
    home = Path.home()
    lab = home / "uni-rnaseq-probe-lab"
    gate = lab / "integrate-gate-host24"
    output = lab / "full-depth-preflight-20260908/index-checker.json"
    if output.exists():
        raise FileExistsError(output)
    with (home / ".cache/uni-rnaseq-resource.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        argv = json.loads((gate / "stages/integrated.argv.json").read_text())
        index = Path(argv[argv.index("--genomeDir") + 1])
        inventory = json.loads((lab / "real-requests-host1/binding-inputs.json").read_text())
        report = {"status": "RUNNING", "files": []}
        targets = [(index / name, expected) for role, name, expected in inventory["entries"]
                   if role == 2 and expected is not None]
        checker = lab / "integrate-source-v1/tooling/replay/seed_split_parity.py"
        pin = json.loads((gate / "gate-summary.json").read_text())["unchanged_parity_checker"]
        targets.append((checker, pin))
        try:
            for path, expected in targets:
                observed = identity(path)
                entry = {"path": str(path), "observed": observed, "expected": expected,
                         "match": observed == expected}
                report["files"].append(entry)
                output.write_text(json.dumps(report, indent=2) + "\n")
                if not entry["match"]:
                    raise ValueError(f"identity changed: {path}")
                print("MATCH", path.name, flush=True)
            report["status"] = "PASS_INDEX_AND_CHECKER_ONLY"
            output.write_text(json.dumps(report, indent=2) + "\n")
        except Exception as exc:
            report["status"] = "FAILED"
            report["error"] = str(exc)
            output.write_text(json.dumps(report, indent=2) + "\n")
            raise


if __name__ == "__main__":
    main()
