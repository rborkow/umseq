#!/usr/bin/env python3
"""Bind an observed real request capture with the existing SSIRv1 encoder.

No index reconstruction or preflight STAR run. `prepare` streams file hashes;
STAR startup supplies active bytes/effective parameters/argv. Only the three
pure encoder functions from the approved run_capture.py are executed. The wire
and parser are unchanged; per-file caps use the already supported v1 maxima.
"""
import argparse
import ast
import hashlib
import json
import os
from pathlib import Path
import struct
import sys
from types import SimpleNamespace


def require(condition, message):
    if not condition:
        raise ValueError(message)


def file_identity(path):
    path = Path(path)
    before = path.stat()
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            hasher.update(chunk)
    digest = hasher.hexdigest()
    after = path.stat()
    require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns) ==
            (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns),
            "file changed while hashing: " + str(path))
    return {"bytes": before.st_size, "sha256": digest}


def verify_active(index, meta):
    """Check observed active arrays against disk with the probe's exact padding."""
    index = Path(index)
    nums = struct.unpack_from("<25Q", meta)
    ng, disk_sa, active_sa, active_sai, prefix = nums[0], nums[2], nums[3], nums[8], nums[10]
    require(0 <= active_sa - disk_sa <= 8, "SA tail extent")
    descriptions = [("Genome", 0, ng, bytes([5]) * 200, bytes([5]) * 200),
                    ("SA", 0, disk_sa, b"", bytes(active_sa - disk_sa)),
                    ("SAindex", (prefix + 2) * 8, active_sai, b"", b"")]
    results = []
    for i, (name, offset, length, before, after) in enumerate(descriptions):
        path = index / name
        require(path.stat().st_size == offset + length, name + " disk extent")
        digest = hashlib.sha256(before)
        with path.open("rb") as stream:
            stream.seek(offset)
            remaining = length
            while remaining:
                chunk = stream.read(min(1 << 20, remaining))
                require(bool(chunk), name + " truncated")
                digest.update(chunk)
                remaining -= len(chunk)
        digest.update(after)
        expected = meta[len(meta) - 96 + i * 32:len(meta) - 64 + i * 32]
        require(digest.digest() == expected, name + " active identity differs from probe disk/padding")
        results.append(digest.hexdigest())
    return results


def build_header(meta, entries, blobs, count, encoder):
    tree = ast.parse(Path(encoder).read_text())
    names = {"pack", "encode_entry", "header"}
    funcs = [node for node in tree.body if isinstance(node, ast.FunctionDef) and node.name in names]
    require({node.name for node in funcs} == names, "approved encoder functions missing")
    namespace = {
        "struct": struct,
        "rio": SimpleNamespace(require=require, digest=lambda value: hashlib.sha256(value).hexdigest()),
        "FILE_CAP": 256 << 20, "RECORD_CAP": 100000, "READ_CAP": 10000,
    }
    exec(compile(ast.Module(body=funcs, type_ignores=[]), str(encoder), "exec"), namespace)
    return namespace["header"](meta, entries, blobs, count)


def exclusive(path, data):
    with Path(path).open("xb") as stream:
        stream.write(data)


def prepare(args):
    entries = []
    source_files = sorted(p for p in args.source.rglob("*") if p.is_file())
    require(len(source_files) == 344, "expected complete pinned 344-file source")
    for path in source_files:
        entries.append((1, path.relative_to(args.source).as_posix(), file_identity(path)))
    index_files = sorted(p for p in args.index.iterdir() if p.is_file())
    for path in index_files:
        entries.append((2, path.name, file_identity(path)))
    if not (args.index / "sjdbInfo.txt").exists():
        entries.append((2, "sjdbInfo.txt", None))
    for i, path in enumerate(args.reads, 1):
        entries.append((3, f"reads_{i}.fastq.gz", file_identity(path)))
    builds = [("baseline", args.baseline), ("instrumented", args.capture),
              ("patch", args.private / "hook.diff"),
              ("header-helper", Path(__file__)), ("header-encoder", args.encoder),
              ("build-command", args.build_command)]
    # Bind the actual additional observer and all source files changed by hooks.
    for name in ("real_inner_capture_impl.hpp", "split_capture_impl.hpp", "split_capture_hooks.hpp",
                 "capture_writer.cpp", "capture_writer.hpp", "capture_hooks.hpp", "sha256.cpp", "sha256.hpp",
                 "STAR.cpp", "ReadAlign_oneRead.cpp", "ReadAlign_mapOneRead.cpp",
                 "ReadAlign_maxMappableLength2strands.cpp", "SuffixArrayFuns.cpp"):
        path = args.private / "capture" / name
        if path.is_file():
            builds.append(("capture/" + name, path))
    for name, path in builds:
        entries.append((4, name, file_identity(path)))
    result = {"entries": entries, "encoder": str(args.encoder),
              "build_options": args.build_command.read_text(),
              "expected_argv": str(args.expected_argv),
              "paths": {"source": str(args.source), "index": str(args.index),
                        "reads": [str(p) for p in args.reads], "private": str(args.private)}}
    exclusive(args.output, (json.dumps(result, indent=2, sort_keys=True) + "\n").encode())
    print(args.output)


def bind(directory):
    directory = Path(directory)
    inventory = Path(os.environ["SSIR_REAL_INVENTORY"])
    config = json.loads(inventory.read_text())
    entries = [tuple(item) for item in config["entries"]]
    actual_argv = (directory / "argv.bin").read_bytes()
    require(actual_argv == Path(config["expected_argv"]).read_bytes(), "actual STAR argv differs from planned argv")
    for name in ("argv.bin", "probe.effective"):
        entries.append((5, name, file_identity(directory / name)))
    entries.append((5, "binding-inputs.json", file_identity(inventory)))
    meta = (directory / "probe.active").read_bytes()
    effective = (directory / "probe.effective").read_bytes()
    header, ids = build_header(meta, entries, [config["build_options"].encode(), actual_argv, effective],
                               1, config["encoder"])
    exclusive(directory / "binding.header", header)
    exclusive(directory / "binding.header.sha256", (hashlib.sha256(header).hexdigest() + "\n").encode())
    manifest = {"scope": "bounded real request research capture; no independent loader or production admission",
                "ids": ids, "entries": entries, "paths": config["paths"],
                "active_metadata_sha256": hashlib.sha256(meta).hexdigest(),
                "active_array_hashes": {name: meta[-96 + i * 32: -96 + (i + 1) * 32 or None].hex()
                                        for i, name in enumerate(("Genome_padded", "SA_active", "SAi_active"))},
                "header_sha256": hashlib.sha256(header).hexdigest()}
    exclusive(directory / "binding.json", (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode())
    print("SSIR_REAL_BOUND", json.dumps(ids, sort_keys=True))


def main():
    if len(sys.argv) == 2 and sys.argv[1] != "prepare":
        bind(sys.argv[1])
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["prepare"])
    for name in ("source", "index", "baseline", "capture", "private", "encoder", "build-command", "expected-argv", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--reads", type=Path, nargs="+", required=True)
    prepare(parser.parse_args())


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print("real_binding: " + str(exc), file=sys.stderr)
        sys.exit(2)
