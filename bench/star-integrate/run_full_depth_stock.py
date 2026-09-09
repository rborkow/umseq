#!/usr/bin/env python3
"""Produce a full-depth stock golden while integration repairs are reviewed.

This stage is independently useful; it does not claim integrated parity. The final
comparator must consume the executed argv recorded here, not reconstruct it.
"""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess


def main():
    home = Path.home()
    lab = home / "uni-rnaseq-probe-lab"
    preflight = lab / "full-depth-preflight-20260908"
    inputs = json.loads((preflight / "preflight.json").read_text())
    index = json.loads((preflight / "index-checker.json").read_text())
    if inputs["status"] != "PASS_INPUTS_ONLY" or index["status"] != "PASS_INDEX_AND_CHECKER_ONLY":
        raise RuntimeError("preflight did not pass")
    with (home / ".cache/uni-rnaseq-resource.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        output = lab / "full-depth-stock-20260908"
        output.mkdir(exist_ok=False)
        stock = Path(inputs["stock"]["path"])
        if hashlib.sha256(stock.read_bytes()).hexdigest() != inputs["stock"]["sha256"]:
            raise ValueError("stock binary changed since preflight")
        for entry in inputs["inputs"]:
            stat = Path(entry["path"]).stat()
            if (stat.st_size, stat.st_mtime_ns) != (entry["bytes"], entry["mtime_ns"]):
                raise ValueError("full FASTQ changed since verification")
        argv = list(inputs["base_argv"])
        if "--readMapNumber" in argv:
            raise ValueError("full-depth input cannot be capped")
        argv[0] = str(stock)
        read_i = argv.index("--readFilesIn")
        if argv[read_i + 3] != "--readFilesCommand":
            raise ValueError("base argv is not the verified two-mate invocation")
        argv[read_i + 1:read_i + 3] = [x["path"] for x in inputs["inputs"]]
        argv[argv.index("--outFileNamePrefix") + 1] = str(output) + "/"
        (output / "argv.json").write_text(json.dumps(argv, indent=2) + "\n")
        (output / "preflight.json").write_text(json.dumps(inputs, indent=2) + "\n")
        env = {k: v for k, v in os.environ.items() if not k.startswith("STAR_INTEGRATE")}
        command = ["/usr/bin/time", "-f", "%e\t%U\t%S\t%M\t%x", "-o",
                   str(output / "diagnostic-time.tsv"),
                   "timeout", "--signal=TERM", "--kill-after=30s", "1800s", *argv]
        (output / "status").write_text("RUNNING_STOCK_ONLY\n")
        with (output / "stdout").open("w") as stdout, (output / "stderr").open("w") as stderr:
            run = subprocess.run(command, env=env, stdout=stdout, stderr=stderr)
        (output / "exit.code").write_text(str(run.returncode) + "\n")
        if run.returncode:
            (output / "status").write_text("FAILED_STOCK\n")
            raise RuntimeError(f"stock failed with exit {run.returncode}")
        for name in ("Aligned.out.sam", "SJ.out.tab", "Log.final.out"):
            if not (output / name).is_file() or (output / name).stat().st_size == 0:
                raise RuntimeError(f"missing/empty stock output: {name}")
        counts = [int(line.split("|")[1].strip())
                  for line in (output / "Log.final.out").read_text().splitlines()
                  if line.split("|")[0].strip() == "Number of input reads"]
        if counts != [inputs["expected_pairs"]]:
            raise RuntimeError(f"stock input-count mismatch: {counts}")
        (output / "status").write_text("PASS_STOCK_ONLY_NOT_PARITY\n")
        print("PASS_STOCK_ONLY_NOT_PARITY", counts[0], flush=True)


if __name__ == "__main__":
    main()
