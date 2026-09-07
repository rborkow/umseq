#!/usr/bin/env python3
"""Fast contract checks for the private real-capture adapter.

These do not require STAR or an index.  They deliberately test the generated
hook surface and the binding-header offsets that the worker replay mutates.
"""
import importlib.util
import struct
import tempfile
import unittest
from pathlib import Path


HERE = Path(__file__).resolve().parent


class RealCaptureContracts(unittest.TestCase):
    def test_no_generated_body_stub(self):
        body = (HERE / "real_inner_capture_impl.hpp").read_text()
        self.assertIn('#include "split_capture_impl.hpp"', body)
        self.assertNotIn("real_inner_capture_impl.generated", body)
        self.assertIn("split_capture::inner_phase", body)
        self.assertIn("first bounded", (HERE / "make_real_inner_capture.py").read_text())

    def test_worker_header_count_is_file_local(self):
        # Writer's v1 header places expected complete reads at offset 48.
        header = bytearray(168)
        header[48:56] = struct.pack("<Q", 999)
        count = 37
        header[48:56] = struct.pack("<Q", count)
        self.assertEqual(struct.unpack_from("<Q", header, 48)[0], count)
        self.assertEqual(header[56:152], bytes(96))  # source/index/runtime remain unchanged

    def test_generator_uses_approved_phase_hook_generator(self):
        text = (HERE / "make_real_inner_capture.py").read_text()
        self.assertIn("split.hooks(name, original)", text)
        self.assertIn('replace("split_capture::", "capture::")', text)
        self.assertIn("capture::verify_end(genomeMain)", text)

    def test_real_impl_counts_comparison_calls_once(self):
        body = (HERE / "real_inner_capture_impl.hpp").read_text()
        begin = body.index("void compare_begin")
        end = body.index("\nvoid compared", begin)
        self.assertIn("++x.work.calls", body[begin:end])
        compared = body[end:body.index("\n\nstatic void replay", end)]
        self.assertNotIn("work.calls", compared)
        self.assertIn("+=bytes", compared.replace(" ", ""))


if __name__ == "__main__":
    unittest.main()
