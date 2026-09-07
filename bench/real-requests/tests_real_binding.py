#!/usr/bin/env python3
"""The research binding must remain byte-compatible with existing SSIRv1."""
import hashlib
import os
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

import real_binding

SEED = Path(os.environ.get("SEED_WORKTREE", "/Users/rborkows/projects/uni-rnaseq-seed"))
REPLAY = SEED / "experiments/star-seed/replay"
TRACE = SEED / "experiments/star-seed/evidence/pe-capture-run2/capture/no-sj/capture.ssir"


def unpack_header(data):
    hlen = struct.unpack_from("<I", data, 12)[0]
    nums = struct.unpack_from("<25Q", data, 168)
    size = 8 * (25 + nums[10] + 1 + nums[20] + 1 + nums[20]) + 96
    meta = data[168:168 + size]
    pos = 168 + size
    count = struct.unpack_from("<I", data, pos)[0]
    pos += 4
    entries = []
    for _ in range(count):
        kind, n, present, length = struct.unpack_from("<HHIQ", data, pos)
        digest = data[pos + 16:pos + 48].hex()
        name = data[pos + 48:pos + 48 + n].decode()
        entries.append((kind, name, {"bytes": length, "sha256": digest} if present else None))
        pos += 48 + n
    blobs = []
    for _ in range(3):
        n = struct.unpack_from("<I", data, pos)[0]
        pos += 4
        blobs.append(data[pos:pos + n])
        pos += n
    assert pos == hlen
    return meta, entries, blobs


class BindingTests(unittest.TestCase):
    def test_existing_trace_header_changes_only_per_file_caps(self):
        data = TRACE.read_bytes()
        meta, entries, blobs = unpack_header(data)
        count = struct.unpack_from("<Q", data, 48)[0]
        header, ids = real_binding.build_header(meta, entries, blobs, count, REPLAY / "run_capture.py")
        expected = bytearray(data[:struct.unpack_from("<I", data, 12)[0]])
        struct.pack_into("<3Q", expected, 24, 256 << 20, 100000, 10000)
        self.assertEqual(header, bytes(expected))
        self.assertEqual(list(ids.values()), [data[n:n + 32].hex() for n in (56, 88, 120)])

    def test_streaming_inventory_matches_actual_bytes(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "input"
            value = bytes(range(256)) * 10000
            p.write_bytes(value)
            self.assertEqual(real_binding.file_identity(p), {"bytes": len(value), "sha256": hashlib.sha256(value).hexdigest()})

    def test_active_binding_matches_disk_padding_and_sai_payload(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / "Genome").write_bytes(b"\x00\x01")
            (root / "SA").write_bytes(b"abcdefghi")
            (root / "SAindex").write_bytes(bytes(24) + b"abcdefghijk")
            nums = [0] * 25
            nums[0], nums[2], nums[3], nums[8], nums[10] = 2, 9, 12, 11, 1
            digests = [hashlib.sha256(x).digest() for x in
                       (bytes([5]) * 200 + b"\x00\x01" + bytes([5]) * 200,
                        b"abcdefghi" + bytes(3), b"abcdefghijk")]
            meta = struct.pack("<25Q", *nums) + b"".join(digests)
            self.assertEqual(real_binding.verify_active(root, meta), [x.hex() for x in digests])
            (root / "SA").write_bytes(b"xbcdefghi")
            with self.assertRaisesRegex(ValueError, "SA active identity"):
                real_binding.verify_active(root, meta)

    def test_reverse_known_prefix_can_exhaust_read(self):
        # The real request at S=13,N=14,L_in=14 triggered the parser failure.
        # Preserve its real work counters/READ_END summaries, not a malformed mutation.
        fixture = Path(__file__).resolve().parents[1] / "evidence/seed-real-requests-host1/star/worker-0.ssir"
        data = fixture.read_bytes()
        at = struct.unpack_from("<I", data, 12)[0]
        found = False
        while at < len(data) - 120:
            size, kind = struct.unpack_from("<IH", data, at)
            if kind == 1 and data[at + 144] == 0:
                v = struct.unpack_from("<16Q", data, at + 16)
                if v[7] == 13 and v[8] == v[11] == 14:
                    found = True
                    break
            at += size
        self.assertTrue(found)
        end = at
        while struct.unpack_from("<H", data, end + 4)[0] != 2:
            end += struct.unpack_from("<I", data, end)[0]
        self.assertGreaterEqual(struct.unpack_from("<Q", data, end + 32)[0], 2,
                                "the real regression must have multiple pieces")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            trace = root / "fully-known.ssir"
            trace.write_bytes(data)
            exe = root / "validator"
            here = Path(__file__).parent
            subprocess.run(["c++", "-std=c++17", "-O2", "-I", str(REPLAY),
                            str(here / "ssir_validate.cpp"), os.environ.get("SSIR_FORMAT_SOURCE", str(REPLAY / "format.cpp")),
                            str(REPLAY / "sha256.cpp"), "-o", str(exe)], check=True)
            result = subprocess.run([str(exe), str(trace), data[56:88].hex(),
                                     data[88:120].hex(), data[120:152].hex()], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_validated_real_trace_projection_is_tuple_and_buffer_exact(self):
        import ssir_to_umprobe as convert
        data = TRACE.read_bytes()
        ids = [data[n:n + 32].hex() for n in (56, 88, 120)]
        hlen = struct.unpack_from("<I", data, 12)[0]
        expected = [record for read in convert.records(data, hlen) for record in read]
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            validator = root / "validate"
            subprocess.run(["c++", "-std=c++17", "-O2", "-I", str(REPLAY),
                            str(Path(__file__).with_name("ssir_validate.cpp")), str(REPLAY / "format.cpp"),
                            str(REPLAY / "sha256.cpp"), "-o", str(validator)], check=True)
            output = root / "requests.bin"
            subprocess.run([sys.executable, "-B", str(Path(__file__).with_name("ssir_to_umprobe.py")),
                            "--validator", str(validator), "--source-sha256", ids[0],
                            "--index-sha256", ids[1], "--runtime-sha256", ids[2],
                            "--index-parameters-sha256", "0" * 64, "--output", str(output),
                            "--manifest", str(root / "manifest.json"), str(TRACE)], check=True)
            raw = output.read_bytes()
            count, size, _ = struct.unpack_from("<3Q", raw, 8)
            self.assertEqual(count, len(expected))
            arena = raw[32 + count * 80:]
            self.assertEqual(len(arena), size)
            star = output.with_suffix(".star-tuples.bin").read_bytes()
            for i, (v, direction, b0, b1) in enumerate(expected):
                r = struct.unpack_from("<10Q", raw, 32 + i * 80)
                self.assertEqual(r[3:], (len(b0), v[7], v[8], v[11], v[9], v[10], direction))
                self.assertEqual(arena[r[1]:r[1] + r[3]], b0)
                self.assertEqual(arena[r[2]:r[2] + r[3]], b1)
                self.assertEqual(struct.unpack_from("<4Q", star, 16 + i * 32), v[12:16])


if __name__ == "__main__":
    unittest.main()
