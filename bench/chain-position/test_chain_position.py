#!/usr/bin/env python3
"""Portable aggregation and source-patching RED/GREEN tests."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

HERE=Path(__file__).resolve().parent
TOOLING=Path('/Users/rborkows/projects/uni-rnaseq-seed/experiments/star-seed/replay')
SOURCE=Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source')

def load(name):
    spec=importlib.util.spec_from_file_location(name,HERE/(name+'.py'));m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m

class Aggregation(unittest.TestCase):
 def fixture(self):
  # offsets cover initial, 20/40 grid, adaptive 1/21, and the overflow bucket.
  return {'schema':'chain-position-v1','actual':{'inner_requests':10,'gathers':100,'compared_bytes':1000},'offsets':[
   {'offset':0,'inner_requests':2,'gathers':20,'compared_bytes':200},{'offset':20,'inner_requests':2,'gathers':30,'compared_bytes':300},
   {'offset':40,'inner_requests':1,'gathers':10,'compared_bytes':100},{'offset':1,'inner_requests':3,'gathers':30,'compared_bytes':300},
   {'offset':21,'inner_requests':1,'gathers':10,'compared_bytes':100},{'offset':-1,'inner_requests':1,'gathers':0,'compared_bytes':0}],
   'work_partition':[{'inner_requests':2,'gathers':20,'compared_bytes':200},{'inner_requests':3,'gathers':40,'compared_bytes':400},{'inner_requests':5,'gathers':40,'compared_bytes':400}],
   'joint_gathers_bytes_by_initial_istart_iDir':[{'initial':1,'istart':0,'iDir':0,'inner_requests':2,'gathers':20,'compared_bytes':200},{'initial':0,'istart':1,'iDir':1,'inner_requests':3,'gathers':30,'compared_bytes':300},{'initial':0,'istart':0,'iDir':0,'inner_requests':5,'gathers':50,'compared_bytes':500}],
   'reverse_suppression':{'chain_opportunities':9,'suppressed_reverse_chains':2,'actual_by_ip_iDir':[2,1,3,4], 'opportunity_by_ip_iDir':[3,2,3,4], 'suppressed_by_ip_iDir':[0,0,1,1]}}
 def test_overlap_and_adaptive(self):
  m=load('summarize_chain_position')
  with tempfile.TemporaryDirectory() as d:
   p=Path(d)/'chain.json';p.write_text(json.dumps(self.fixture()));r=m.main(p)
  self.assertEqual(r['initial']['gathers'],20);self.assertEqual(r['grid_only_added']['gathers'],40);self.assertEqual(r['union_initial_grid20']['gathers'],60);self.assertEqual(r['adaptive_tail']['gathers'],40);self.assertEqual(r['initial_gather_share'],.2);self.assertTrue(r['recommend_grid20'])
  self.assertEqual(r['reverse_suppression']['suppressed_reverse_chains'],2) # two pieces, reverse only; no invented work
 def test_zero_denominator(self):
  m=load('summarize_chain_position');x=self.fixture();x['actual']={'inner_requests':0,'gathers':0,'compared_bytes':0};x['offsets']=[]
  x['work_partition']=[x['actual'].copy() for _ in range(3)];x['joint_gathers_bytes_by_initial_istart_iDir']=[]
  with tempfile.TemporaryDirectory() as d:
   p=Path(d)/'zero.json';p.write_text(json.dumps(x));r=m.main(p)
  self.assertIsNone(r['initial_gather_share']);self.assertFalse(r['recommend_grid20'])
 def test_split_reconciliation(self):
  m=load('summarize_chain_position')
  with tempfile.TemporaryDirectory() as d:
   d=Path(d);p=d/'chain';p.write_text(json.dumps(self.fixture()));s=d/'split';s.write_text(json.dumps({'inner_requests':10,'inner':{'calls_by_direction':[10,20,30,40],'compared_bytes_by_direction':[100,200,300,400]}}));self.assertEqual(m.main(p,s)['status'],'OK')
class Patching(unittest.TestCase):
 def test_green_pinned_source(self):
  m=load('make_chain_position');rio=m.load_oracle(TOOLING)
  for name in ('STAR.cpp','ReadAlign_oneRead.cpp','ReadAlign_mapOneRead.cpp','ReadAlign_maxMappableLength2strands.cpp','SuffixArrayFuns.cpp'):
   out=m.hooks(rio,name,(SOURCE/name).read_text());self.assertIn('chain_position::',out)
 def test_red_source_drift(self):
  m=load('make_chain_position');rio=m.load_oracle(TOOLING);bad=(SOURCE/'ReadAlign_mapOneRead.cpp').read_text().replace('maxMappableLength2strands(Shift, seedLength, iDir, 0, mapGen.nSA-1, L, splitR[2][ip])','changedMappableLength(Shift, seedLength, iDir, 0, mapGen.nSA-1, L, splitR[2][ip])',1)
  with self.assertRaises(ValueError):m.hooks(rio,'ReadAlign_mapOneRead.cpp',bad)
if __name__=='__main__': unittest.main()
