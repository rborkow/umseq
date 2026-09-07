#!/usr/bin/env python3
"""Producer-to-consumer regressions for the measured chain classifications."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

HERE=Path(__file__).resolve().parent

def module(name):
    spec=importlib.util.spec_from_file_location(name,HERE/(name+'.py'))
    result=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result

class CounterIntegration(unittest.TestCase):
    def test_cpp_split_edge_grid_and_overflow(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            subprocess.run(['c++','-std=c++17','-I',str(HERE),str(HERE/'counter_fixture.cpp'),'-o',str(root/'fixture')],check=True)
            subprocess.run([str(root/'fixture'),str(root)],check=True)
            raw=json.loads((root/'chain-position-counters.json').read_text())
            result=module('summarize_chain_position').main(root/'chain-position-counters.json')
        self.assertEqual(raw['actual']['inner_requests'],5)
        self.assertEqual(result['initial']['gathers'],50)
        self.assertEqual(result['grid_only_added']['gathers'],40)
        self.assertEqual(result['union_initial_grid20']['gathers'],90)
        self.assertEqual(result['adaptive_tail']['gathers'],30)
        self.assertEqual(result['actual']['compared_bytes'],1200)
        self.assertEqual(result['reverse_suppression']['suppressed_reverse_chains'],1)

    def test_original_split_byte_calls_preserved(self):
        tooling=Path('/Users/rborkows/projects/uni-rnaseq-seed/experiments/star-seed/replay')
        source=Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source/SuffixArrayFuns.cpp')
        patcher=module('make_chain_position')
        text=patcher.hooks(patcher.load_oracle(tooling),'SuffixArrayFuns.cpp',source.read_text())
        self.assertEqual(text.count('split_capture::compared('),8)

if __name__=='__main__': unittest.main()
