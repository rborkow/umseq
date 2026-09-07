#!/usr/bin/env python3
import importlib.util, tempfile, unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('generator',ROOT/'make_star_integrate.py'); m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
class PatchGuard(unittest.TestCase):
 def test_unknown_line_fails(self):
  class R:
   def replace_once(self,t,o,n):
    if o not in t: raise ValueError('missing exact hook')
    return t.replace(o,n,1)
  with self.assertRaises(ValueError): m.patch(R(),'ReadAlign_maxMappableLength2strands.cpp','not STAR')
 def test_enabled_hook_has_real_consumer_surface(self):
  h=(ROOT/'star_integrate.hpp').read_text()
  self.assertIn('uint64_t &maxL',h)
  self.assertNotIn('inline bool lookup',h)
  cpp=(ROOT/'star_integrate.cpp').read_text()
  self.assertIn('usi_search_batch_v1',cpp)
  self.assertIn('target = 65536',cpp)
  self.assertIn('std::unordered_map',cpp)
  self.assertIn('current_window->ranges[current_index]',cpp)
  self.assertNotIn('find_if(current_window->frames',cpp)
  self.assertIn('in.generation != c.generation',cpp)
  self.assertIn('backend returned invalid successful result',cpp)
  self.assertIn('std::min<uint>(nstart, 2)',(ROOT/'star_integrate_window.cpp').read_text())
  self.assertIn('chain.istart >= 2',cpp)
  self.assertIn('lookup_index.equal_range',cpp)
 def test_real_call_patch_has_oracle_and_cpu_fallback(self):
  class R:
   def replace_once(self,t,o,n):
    if o not in t: raise ValueError('missing exact hook')
    return t.replace(o,n,1)
  src='            Nrep = maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL, indStartEnd);'
  got=m.patch(R(),'ReadAlign_maxMappableLength2strands.cpp',src)
  self.assertIn('star_integrate::strict()',got)
  self.assertIn('star_integrate::note_cpu_fallback()',got)
  self.assertIn('abort()',got)
 def test_window_hooks_are_staged(self):
  class R:
   def replace_once(self,t,o,n):
    if o not in t: raise ValueError('missing exact hook')
    return t.replace(o,n,1)
  got=m.patch(R(),'ReadAlignChunk_mapChunk.cpp','        readStatus=RA->oneRead(); //map one read')
  self.assertIn('prepare_window(*this)',got)
  got=m.patch(R(),'ReadAlign_mapOneRead.cpp','int ReadAlign::mapOneRead() {')
  self.assertIn('begin_map(*this)',got)
 def test_real_source_has_chain_suppression_and_retirement_hooks(self):
  root=Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source')
  if not root.exists(): self.skipTest('pinned private source unavailable')
  class R:
   def replace_once(self,t,o,n):
    if o not in t: raise ValueError('missing exact hook')
    return t.replace(o,n,1)
  chunk=m.patch(R(),'ReadAlignChunk_mapChunk.cpp',(root/'ReadAlignChunk_mapChunk.cpp').read_text())
  read=m.patch(R(),'ReadAlign_mapOneRead.cpp',(root/'ReadAlign_mapOneRead.cpp').read_text())
  inner=m.patch(R(),'ReadAlign_maxMappableLength2strands.cpp',(root/'ReadAlign_maxMappableLength2strands.cpp').read_text())
  self.assertIn('star_integrate::end_chunk()',chunk)
  self.assertIn('star_integrate::set_chain(ip, splitR[2][ip], istart, Nstart, Lstart, Lmapped, splitR[0][ip], splitR[1][ip], Nsplit)',read)
  self.assertIn('star_integrate::reverse_suppressed(ip)',read)
  self.assertIn('star_integrate::build_current_inner_call(starIntegrateCall)',inner)
  self.assertNotIn('InnerCall starIntegrateCall={pieceStart',inner)
  self.assertIn('splitR[1][ip], Nsplit)',read)
if __name__=='__main__': unittest.main()
