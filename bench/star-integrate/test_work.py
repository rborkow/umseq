#!/usr/bin/env python3
"""Compile the real work observer and exercise its mock-data control arms.

This test is deliberately not evidence of GPU work.  It verifies the observer's
JSON accounting and verifies that generator hooks instrument actual STAR source
comparator/control branches without replacing their algorithms.
"""
import importlib.util
import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SOURCE = Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source')
CLANG = Path('/opt/homebrew/opt/llvm/bin/clang++')


def generator():
    spec = importlib.util.spec_from_file_location('star_integrate_generator', ROOT / 'make_star_integrate.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class ReplaceOnce:
    def replace_once(self, text, old, new):
        if text.count(old) != 1:
            raise ValueError('missing or duplicate exact hook')
        return text.replace(old, new, 1)


class WorkObserver(unittest.TestCase):
    def test_mock_data_real_observer_json_and_tls_retirement(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            binary = d / 'work-test'
            subprocess.run([str(CLANG), '-std=c++11', '-DSTAR_INTEGRATE=1', '-pthread',
                            '-I' + str(ROOT), str(ROOT / 'test_work.cpp'),
                            str(ROOT / 'star_integrate_work.cpp'), '-o', str(binary)], check=True)
            sidecar = d / 'integrate.jsonl'
            env = dict(os.environ, STAR_INTEGRATE_SIDECAR=str(sidecar))
            subprocess.run([str(binary)], env=env, check=True)
            row = json.loads((Path(str(sidecar) + '.work.json')).read_text())
            self.assertEqual(row, {
                'inner_calls': 3, 'hit_calls': 1, 'fallback_calls': 2, 'oracle_calls': 1,
                'fallback_compared_bytes': 6, 'fallback_gathers': 2,
                'oracle_compared_bytes': 5, 'oracle_gathers': 1,
            })

    def test_actual_source_comparator_and_inner_control_hooks(self):
        if not SOURCE.exists():
            self.skipTest('pinned source unavailable')
        patch = generator().patch
        comparator = patch(ReplaceOnce(), 'SuffixArrayFuns.cpp', (SOURCE / 'SuffixArrayFuns.cpp').read_text())
        self.assertNotIn('star_integrate_work::', comparator)
        with tempfile.TemporaryDirectory() as d:
            generated = Path(d) / 'SuffixArrayFuns.cpp'
            generated.write_text(comparator)
            # Compile the generated upstream comparator itself; no synthetic
            # replacement of STAR's comparator/control algorithm is involved.
            subprocess.run([str(CLANG), '-std=c++11', '-DSTAR_INTEGRATE=1',
                            '-I' + str(ROOT), '-I' + str(SOURCE),
                            '-I/opt/homebrew/opt/libomp/include', '-fopenmp', '-c', str(generated),
                            '-o', str(Path(d) / 'SuffixArrayFuns.o')], check=True)
        control = patch(ReplaceOnce(), 'ReadAlign_maxMappableLength2strands.cpp',
                        (SOURCE / 'ReadAlign_maxMappableLength2strands.cpp').read_text())
        self.assertNotIn('star_integrate_work::inner_call', control)
        self.assertNotIn('star_integrate_work::fallback_scope()', control)
        self.assertIn('static const bool starIntegrateEnabled=star_integrate::enabled_fast()', control)
        self.assertIn('const bool starIntegrateHit=star_integrate::lookup(', control)
        self.assertIn('starIntegrateStockOuter(); // full stock', control)
        self.assertIn('strict_read1(Read1', control)
        self.assertIn('Nrep = maxMappableLength(mapGen, Read1, pieceStart', control)
        with tempfile.TemporaryDirectory() as d:
            generated = Path(d) / 'ReadAlign_maxMappableLength2strands.cpp'
            generated.write_text(control)
            subprocess.run([str(CLANG), '-std=c++11', '-DSTAR_INTEGRATE=1',
                            '-I' + str(ROOT), '-I' + str(SOURCE),
                            '-I/opt/homebrew/opt/libomp/include', '-fopenmp', '-c', str(generated),
                            '-o', str(Path(d) / 'control.o')], check=True)

    def test_default_off_header_compiles(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            source = d / 'off.cpp'
            source.write_text('#include "star_integrate_work.hpp"\nint main() { star_integrate_work::inner_call(false); return 0; }\n')
            subprocess.run([str(CLANG), '-std=c++11', '-I' + str(ROOT), str(source), '-o', str(d / 'off')], check=True)


if __name__ == '__main__':
    unittest.main()
