#!/usr/bin/env python3
"""Orchestrator-only host measurement, inside the shared lock/deadline wrapper."""
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

from real_binding import file_identity, verify_active

CUTOFF = 1788756895  # Renewed three-hour authorization, 2026-09-06 21:54:55 PDT.
ROOT = Path(__file__).resolve().parents[2]
LAB = Path.home() / "uni-rnaseq-seed-lab"
PROBE_LAB = Path.home() / "uni-rnaseq-probe-lab"
OUT = Path(os.environ.get("OUT", str(PROBE_LAB / "real-requests-host1")))
TOOLING = Path(os.environ.get("TOOLING", str(LAB / "tooling-seed-split-v3")))
SOURCE = Path(os.environ.get("SOURCE", str(LAB / "real-index-input-2.7.11b/source")))
BASE = LAB / "seed-split-private-v2"
INDEX = Path.home() / "uni-rnaseq/data/index/star_full"
READS = [Path.home() / f"uni-rnaseq/data/samples/ERR188140_20M/ERR188140_20M_{i}.fastq.gz" for i in (1, 2)]
HERE = Path(__file__).resolve().parent


def record(message):
    with (OUT / "run.status").open("a") as stream:
        stream.write(message + "\n")
    print(message, flush=True)


def run(name, command, env=None):
    if time.time() >= CUTOFF:
        raise RuntimeError("authorization cutoff reached")
    command = [str(item) for item in command]
    (OUT / "stages" / (name + ".argv.json")).write_text(json.dumps(command) + "\n")
    record("RUNNING " + name)
    timed = ["/usr/bin/time", "-f", "%e %U %S %M %x", "-o", str(OUT / "stages" / (name + ".time.tsv"))] + command
    with (OUT / "stages" / (name + ".stdout")).open("wb") as stdout, (OUT / "stages" / (name + ".stderr")).open("wb") as stderr:
        result = subprocess.run(timed, cwd=ROOT, env=env, stdout=stdout, stderr=stderr)
    record(f"{name} exit={result.returncode}")
    if result.returncode:
        raise RuntimeError(f"{name} failed: {result.returncode}")


def main():
    if os.environ.get("REAL_REQUESTS_LOCKED") != "1" or time.time() >= CUTOFF:
        raise RuntimeError("shared lock/deadline wrapper required")
    OUT.mkdir()
    (OUT / "stages").mkdir()
    for name in ("stock", "capture", "star"):
        (OUT / name).mkdir()
    private = OUT / "private"
    record("REAL_REQUESTS_RUNNING cutoff=" + str(CUTOFF))
    os.environ["PATH"] = "/usr/local/cuda/bin:" + str(Path.home() / ".cargo/bin") + ":" + os.environ["PATH"]
    os.environ["CARGO_BUILD_JOBS"] = "4"
    sources = [ROOT / "Cargo.toml", ROOT / "Cargo.lock"]
    for folder in (ROOT / "crates/umem", ROOT / "crates/umgpu", ROOT / "crates/umseed-probe", HERE):
        sources.extend(p for p in folder.rglob("*") if p.is_file() and not any(x in p.parts for x in ("target", "__pycache__", ".git")) and not p.name.startswith(".env"))
    (OUT / "source-sha256.json").write_text(json.dumps({str(p.relative_to(ROOT)): file_identity(p) for p in sorted(set(sources))}, indent=2) + "\n")
    run("clocks-before", ["nvidia-smi", "--query-gpu=name,clocks.sm,utilization.gpu,power.draw", "--format=csv"])
    run("probe-build", ["cargo", "build", "--offline", "--locked", "--release", "-p", "umseed-probe", "--features", "cuda"])
    run("probe-tests", ["cargo", "test", "--offline", "--locked", "--release", "-p", "umseed-probe", "--features", "cuda"])
    binary = ROOT / "target/release/umseed-probe"
    run("sass", ["cuobjdump", "--dump-sass", binary])
    loads = {}
    for block in (OUT / "stages/sass.stdout").read_text().split("Function : ")[1:]:
        if "probe_thread_kernel" in block.splitlines()[0]:
            loads[block.splitlines()[0].strip()] = len(re.findall(r"\bLDG(?:\.|\s)", block))
    if not loads or not all(loads.values()):
        raise RuntimeError("thread kernel has no observable SASS loads")
    (OUT / "sass-loads.json").write_text(json.dumps(loads) + "\n")
    run("prepare-capture", ["python3", "-B", HERE / "make_real_inner_capture.py", "--tooling", TOOLING,
                            "--source", SOURCE, "--private-root", private, "--base-private-root", BASE,
                            "--limit", "1000000", "--per-file-record-cap", "100000", "--per-file-read-cap", "10000"])
    build_env = dict(os.environ)
    for name in ("MAKEFLAGS", "MFLAGS", "CFLAGS", "CPPFLAGS", "CXXFLAGS", "LDFLAGS"):
        build_env.pop(name, None)
    run("build-capture", ["make", "-j2", "STAR", "CXX=" + shutil.which("g++"), "CC=" + shutil.which("gcc"),
                          "CXXFLAGSextra=", "LDFLAGSextra=", "CXXFLAGS_SIMD=", "BUILD_DATE=real-index-private",
                          "BUILD_PLACE=real-index-private", "-C", private / "capture"], build_env)
    common = ["--runMode", "alignReads", "--runThreadN", "20", "--runRNGseed", "777", "--genomeDir", str(INDEX),
              "--genomeLoad", "NoSharedMemory", "--readFilesIn", *map(str, READS), "--readFilesCommand", "zcat",
              "--twopassMode", "None", "--genomeType", "Full", "--genomeTransformType", "None", "--outSAMtype", "SAM",
              "--outSAMorder", "PairedKeepInputOrder", "--outSAMunmapped", "Within"]
    stock = [str(BASE / "baseline/STAR"), *common, "--outFileNamePrefix", str(OUT / "stock") + "/"]
    capture = [str(private / "capture/STAR"), *common, "--outFileNamePrefix", str(OUT / "capture") + "/"]
    expected_argv = OUT / "capture.expected-argv.bin"
    expected_argv.write_bytes(b"\0".join(item.encode() for item in capture) + b"\0")
    inventory = OUT / "binding-inputs.json"
    run("binding-inputs", ["python3", "-B", HERE / "real_binding.py", "prepare", "--source", SOURCE,
                           "--index", INDEX, "--baseline", stock[0], "--capture", capture[0], "--private", private,
                           "--encoder", TOOLING / "replay/run_capture.py", "--build-command", OUT / "stages/build-capture.argv.json",
                           "--expected-argv", expected_argv, "--output", inventory, "--reads", *READS])
    run("stock", stock)
    capture_env = dict(os.environ, SSIR_REAL_CAPTURE="1", SSIR_REAL_DIRECTORY=str(OUT / "star"),
                       SSIR_REAL_STOP_AFTER="1000000", SSIR_REAL_HEADER_HELPER=str(HERE / "real_binding.py"),
                       SSIR_REAL_INVENTORY=str(inventory))
    run("capture", capture, capture_env)
    run("parity", ["python3", "-B", TOOLING / "replay/seed_split_parity.py", "--stock", OUT / "stock",
                   "--counters", OUT / "capture", "--stock-command", " ".join(stock), "--counter-command", " ".join(capture),
                   "--expected-pairs", "20000000", "--out", OUT / "stock-parity"])
    active = verify_active(INDEX, (OUT / "star/probe.active").read_bytes())
    (OUT / "active-identity-verified.json").write_text(json.dumps(active) + "\n")
    validator = OUT / "ssir-validate"
    run("validator-build", ["c++", "-std=c++17", "-O2", "-I", TOOLING / "replay", HERE / "ssir_validate.cpp",
                            TOOLING / "replay/format.cpp", TOOLING / "replay/sha256.cpp", "-o", validator])
    binding = json.loads((OUT / "star/binding.json").read_text())
    ids = binding["ids"]
    traces = sorted((OUT / "star").glob("*.ssir"))
    if not traces:
        raise RuntimeError("no completed SSIR files")
    request = OUT / "real-requests.bin"
    run("convert", ["python3", "-B", HERE / "ssir_to_umprobe.py", "--validator", validator,
                    "--source-sha256", ids["source"], "--index-sha256", ids["index"], "--runtime-sha256", ids["runtime"],
                    "--index-parameters-sha256", file_identity(INDEX / "genomeParameters.txt")["sha256"],
                    "--output", request, "--manifest", OUT / "real-requests.json", *traces])
    count = json.loads((OUT / "real-requests.json").read_text())["actual_count"]
    for label, requests, n, pages in (("real", request, count, "huge"),
                                     ("synthetic", PROBE_LAB / "probe-measure-host1/probe-requests.bin", 1000000, "huge"),
                                     ("real-4k", request, count, "small4k")):
        run(label, [binary, "run", "--index", INDEX, "--requests", requests, "--output", OUT / (label + ".tsv"),
                    "--diagnostics", OUT / (label + "-distributions.json"), "--variant", "thread", "--pages", pages, "--overlap",
                    "--counts", str(n), "--repeats", "3", "--split-provenance", "accepted SPLIT20M+5M inner-only; REAL-REQUESTS",
                    "--cutoff-unix", str(CUTOFF)])
    run("clocks-after", ["nvidia-smi", "--query-gpu=name,clocks.sm,utilization.gpu,power.draw", "--format=csv"])
    (OUT / "binaries.json").write_text(json.dumps({str(p): file_identity(p) for p in (binary, Path(stock[0]), Path(capture[0]))}, indent=2) + "\n")
    record("COMPLETE_REQUIRES_ORCHESTRATOR_VERIFICATION")


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        if OUT.exists():
            record("FAILED " + str(exc))
        raise
