#!/usr/bin/env python3
"""Compile/run actual star_integrate.cpp; fake USI is transport-only, not GPU evidence."""

import argparse
import importlib.util, json, os, shutil, subprocess, tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
STAR = Path("/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source")
REPLAY = Path("/private/tmp/p2c-capture-local-01/helpers")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source-dir", type=Path, default=HERE)
    parser.add_argument("--case", action="append")
    parser.add_argument("--asan", action="store_true")
    args = parser.parse_args()
    if not STAR.exists() or not REPLAY.exists():
        raise SystemExit("pinned STAR/replay fixture unavailable")
    cxx = shutil.which("clang++") or shutil.which("c++")
    hook_cxx = (
        "/opt/homebrew/opt/llvm/bin/clang++"
        if Path("/opt/homebrew/opt/llvm/bin/clang++").exists()
        else cxx
    )
    spec = importlib.util.spec_from_file_location(
        "star_integrate_generator", HERE / "make_star_integrate.py"
    )
    generator = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(generator)

    class ReplaceOnce:
        def replace_once(self, text, old, new):
            if text.count(old) != 1:
                raise ValueError("missing or duplicate exact hook")
            return text.replace(old, new, 1)

    with tempfile.TemporaryDirectory(prefix="star-coordinator-") as tmp:
        tmp = Path(tmp)
        for name in ("star_integrate.cpp", "star_integrate.hpp"):
            shutil.copy2(args.source_dir / name, tmp / name)
        shutil.copy2(HERE / "test_coordinator.cpp", tmp)
        # Compile the actual generated upstream hook in the same regression that
        # drives its builder-shaped call through the real coordinator lookup.
        generated = tmp / "ReadAlign_maxMappableLength2strands.cpp"
        generated.write_text(
            generator.patch(
                ReplaceOnce(),
                "ReadAlign_maxMappableLength2strands.cpp",
                (STAR / "ReadAlign_maxMappableLength2strands.cpp").read_text(),
            )
        )
        subprocess.run(
            [
                hook_cxx,
                "-std=c++11",
                "-DSTAR_INTEGRATE=1",
                "-I" + str(HERE),
                "-I" + str(STAR),
                "-I/opt/homebrew/opt/libomp/include",
                "-fopenmp",
                "-c",
                str(generated),
                "-o",
                str(tmp / "generated-hook.o"),
            ],
            check=True,
            timeout=90,
        )
        exe = tmp / "coordinator-production-fixture"
        subprocess.run(
            [
                cxx,
                "-std=c++11",
                *(
                    ["-fsanitize=address", "-fno-omit-frame-pointer", "-g"]
                    if args.asan
                    else []
                ),
                "-pthread",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-I" + str(HERE / "test-stubs"),
                "-I" + str(HERE),
                "-I" + str(REPLAY),
                "-I" + str(STAR),
                str(tmp / "test_coordinator.cpp"),
                str(REPLAY / "sha256.cpp"),
                "-o",
                str(exe),
            ],
            check=True,
            timeout=90,
        )
        if args.case:
            for case in args.case:
                subprocess.run([str(exe), case], check=True, timeout=30)
            return
        sidecar = Path(tmp) / "sidecar.jsonl"
        env = dict(os.environ, STAR_INTEGRATE_SIDECAR=str(sidecar))
        key = subprocess.run(
            [str(exe), "generated-key-hook"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
            timeout=30,
        )
        wwb = subprocess.run(
            [str(exe), "whole-window-batching"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
        )
        if wwb.returncode:
            raise AssertionError(wwb.stdout + wwb.stderr)
        assert "batches=2 consumed=7" in wwb.stdout, wwb.stdout + wwb.stderr
        pool = subprocess.run(
            [str(exe), "window-pool"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
            timeout=30,
        )
        assert pool.stdout.strip() == "window pool: rotation=1 not_ready=0", (
            pool.stdout + pool.stderr
        )
        boundary = subprocess.run(
            [str(exe), "chunk-boundary"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
            timeout=30,
        )
        assert (
            boundary.stdout.strip() == "chunk boundary: next_dropped=1 live_bytes=0"
        ), (boundary.stdout + boundary.stderr)
        refusal = subprocess.run(
            [str(exe), "prefetch-refusal"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
            timeout=30,
        )
        assert refusal.stdout.strip() == "prefetch refusal: refused=1 fallback=1", (
            refusal.stdout + refusal.stderr
        )
        current = subprocess.run(
            [str(exe), "current-refusal"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
        )
        if current.returncode:
            raise AssertionError(current.stdout + current.stderr)
        assert current.stdout.strip() == "current refusal: charge=0", (
            current.stdout + current.stderr
        )
        shutdown = subprocess.run(
            [str(exe), "shutdown-drain"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=True,
            timeout=30,
        )
        assert (
            shutdown.stdout.strip()
            == "shutdown drain: queued=0 live_requests=0 tails=280000"
        ), (shutdown.stdout + shutdown.stderr)
        for case in ("pool-reuse", "chain-accounting", "shutdown-delayed"):
            subprocess.run([str(exe), case], check=True, timeout=30)
        if key.stdout.strip() != "generated key hook: consumed=1 key_misses=0":
            raise AssertionError(
                "generated hook key fixture did not consume exactly once:\n"
                + key.stdout
                + key.stderr
            )
        subprocess.run([str(exe), "positional-shuffled"], check=True, timeout=30)
        lifecycle_sidecar = Path(tmp) / "lifecycle-sidecar.jsonl"
        lifecycle = subprocess.run(
            [str(exe), "index-lifecycle"],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=dict(os.environ, STAR_INTEGRATE_SIDECAR=str(lifecycle_sidecar)),
            check=True,
            timeout=30,
        )
        assert lifecycle.stdout.strip() == "index lifecycle: generations=2 stale_key_mismatch=1"
        lifecycle_row = json.loads(lifecycle_sidecar.read_text())
        assert lifecycle_row["index_generations"] == 2
        assert len(lifecycle_row["generations"]) == 2
        assert all(row["submitted"] > 0 and row["consumed"] > 0
                   for row in lifecycle_row["generations"])
        subprocess.run([str(exe)], env=env, check=True, timeout=30)
        rows = [
            json.loads(line)
            for line in sidecar.read_text().splitlines()
            if line.strip()
        ]
        assert len(rows) == 1
        # These are precisely gate (i)'s local schema assertions; this fixture is
        # transport-only and makes no claim of CUDA/STAR parity.
        assert rows[0]["gpu_consumed"] > 0
        assert rows[0]["batch_faults"] == rows[0]["rejected"] == 0
        assert rows[0]["key_misses"] == sum(
            rows[0]["miss_reasons"][name]
            for name in (
                "read_bytes",
                "positional_exhausted",
                "chain_rejected_residue",
                "no_job",
                "key_mismatch",
                "not_ready",
                "device_stopped",
                "cpu_admission",
                "cpu_resolved",
                "shift",
                "cas_lost",
            )
        )
        assert rows[0]["no_window"] == rows[0]["miss_reasons"]["no_window"]
        assert set(rows[0]["not_ready_where"]) == {"queued", "filling", "draining"}
        assert isinstance(rows[0]["device_stop_status"], dict)


if __name__ == "__main__":
    main()
