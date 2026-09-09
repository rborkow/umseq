#!/usr/bin/env python3
import importlib.util, shutil, subprocess, tempfile, unittest
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
  self.assertIn('usi_search_batch_v3',cpp)
  self.assertIn('ProbeRequestV3',cpp)
  self.assertIn('ProbeOutputV3',cpp)
  self.assertIn('target = 65536',cpp)
  self.assertIn('current_window->ranges[current_index]',cpp)
  self.assertNotIn('find_if(current_window->frames',cpp)
  self.assertIn('in.generation != c.generation',cpp)
  self.assertIn('chain_rejected_other',cpp)
  self.assertIn('chain_cursor',cpp)
  self.assertIn('std::min<uint>(nstart, 2)',(ROOT/'star_integrate_window.cpp').read_text())
  self.assertIn('chain.istart >= 2',cpp)
  self.assertNotIn('unordered_multimap',cpp)
  self.assertIn('current_window->cursors[current_index]',cpp)
  self.assertIn('positional_misses',cpp)
  # Every field the window puts in the chain key must also be set by the
  # generated hook, or same_call() misses every candidate (round-6 host16:
  # gpu_consumed == 0 because the window set prefix = seedMapMin and the hook
  # left it 0). Assert the pair stays in sync.
  gen=(ROOT/'make_star_integrate.py').read_text()
  self.assertIn('starIntegrateCall.prefix=P.seedMapMin',gen)
  self.assertIn('call.prefix = p.seedMapMin',(ROOT/'star_integrate_window.cpp').read_text())
  # Borrowed index lifetime: star_integrate::finish() must run before
  # genomeMain.freeMemory() in the generated STAR.cpp (the GPU gathers from
  # STAR's own arrays; a drain after free is a CUDA 700).
  self.assertIn('star_integrate::finish(); // joins the coordinator',gen)
  new=gen[gen.index('star_integrate::finish(); // joins the coordinator'):]
  self.assertLess(new.index('star_integrate::finish()'),new.index('genomeMain.freeMemory();'))
 def test_real_call_patch_has_oracle_and_cpu_fallback(self):
  class R:
   def replace_once(self,t,o,n):
    if o not in t: raise ValueError('missing exact hook')
    return t.replace(o,n,1)
  root=Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source')
  if not root.exists(): self.skipTest('pinned private source unavailable')
  got=m.patch(R(),'ReadAlign_maxMappableLength2strands.cpp',(root/'ReadAlign_maxMappableLength2strands.cpp').read_text())
  self.assertIn('star_integrate::strict()',got)
  self.assertIn('star_integrate::note_cpu_fallback()',got)
  self.assertIn('star_integrate::fail_strict',got)
  # Bypass dispatch is once per function entry.  Its direct stock loop returns
  # before the enabled-only frame lookup; the original bookkeeping remains in
  # both loops.
  self.assertEqual(got.count('static const bool starIntegrateEnabled'),1)
  self.assertLess(got.index('if (!starIntegrateEnabled)'),got.index('const bool starIntegrateHit'))
  self.assertLess(got.index('return Nrep;'),got.index('const bool starIntegrateHit'))
  self.assertIn('starIntegrateStockOuter(); // full stock',got)
 def test_window_hooks_are_staged(self):
  class R:
   def replace_once(self,t,o,n):
    if o not in t: raise ValueError('missing exact hook')
    return t.replace(o,n,1)
  got=m.patch(R(),'ReadAlignChunk_mapChunk.cpp','        readStatus=RA->oneRead(); //map one read')
  self.assertIn('prepare_window(*this)',got)
  got=m.patch(R(),'ReadAlign_mapOneRead.cpp','int ReadAlign::mapOneRead() {')
  self.assertIn('begin_map(*this)',got)
 def test_one_read_handoff_is_after_readload(self):
  root=Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source')
  if not root.exists(): self.skipTest('pinned private source unavailable')
  class R:
   def replace_once(self,t,o,n):
    if t.count(o) != 1: raise ValueError('missing or duplicate exact hook')
    return t.replace(o,n,1)
  got=m.patch(R(),'ReadAlign_oneRead.cpp',(root/'ReadAlign_oneRead.cpp').read_text())
  self.assertIn('star_integrate::handoff_read1(*this)',got)
  self.assertGreater(got.index('star_integrate::handoff_read1(*this)'),got.index('readLoad('))
  self.assertIn("'ReadAlign_oneRead.cpp'",(ROOT/'make_star_integrate.py').read_text())
  preferred=Path('/opt/homebrew/opt/llvm/bin/clang++')
  cxx=str(preferred if preferred.exists() else (shutil.which('clang++') or shutil.which('c++')))
  with tempfile.TemporaryDirectory(prefix='star-one-read-hook-') as tmp:
   patched=Path(tmp)/'ReadAlign_oneRead.cpp'; patched.write_text(got)
   subprocess.run([cxx,'-std=c++11','-DSTAR_INTEGRATE=1','-fopenmp','-I/opt/homebrew/opt/libomp/include','-I'+str(ROOT),'-I'+str(root),'-c',str(patched),'-o',str(Path(tmp)/'hook.o')],check=True,timeout=90)
 def test_identity_setup_precedes_frame_publication(self):
  class R:
   def replace_once(self,t,o,n):
    if o not in t: raise ValueError('missing exact hook')
    return t.replace(o,n,1)
  upstream=Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source/STAR.cpp')
  if not upstream.exists(): self.skipTest('pinned private source unavailable')
  got=m.patch(R(),'STAR.cpp',upstream.read_text())
  self.assertLess(got.index('genomeMain.genomeLoad();'),got.index('star_integrate::setup(P, genomeMain);'))
  self.assertNotIn('setup(chunk.P, chunk.mapGen)',(ROOT/'star_integrate_window.cpp').read_text())
  cpp=(ROOT/'star_integrate.cpp').read_text()
  self.assertNotIn('ifstream',cpp)
  self.assertNotIn('sampled_file_matches',cpp)
 def test_loader_hugepage_and_drop_cache_hooks_precede_reads(self):
  root=Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source')
  if not root.exists(): self.skipTest('pinned private source unavailable')
  class R:
   def replace_once(self,t,o,n):
    if t.count(o) != 1: raise ValueError('missing or duplicate exact hook')
    return t.replace(o,n,1)
  genome=m.patch(R(),'Genome_genomeLoad.cpp',(root/'Genome_genomeLoad.cpp').read_text())
  packed=m.patch(R(),'PackedArray.cpp',(root/'PackedArray.cpp').read_text())
  self.assertIn('defined(STAR_INTEGRATE)',genome)
  self.assertLess(genome.index('starIntegrateAdviseHuge(G1,nGenome+L+L);'),genome.index('fstreamReadBig(GenomeIn,G,nGenome)'))
  self.assertLess(packed.index('starIntegrateAdviseHuge(charArray,lengthByte);'),packed.index('memset(charArray+lengthByte'))
  self.assertGreater(genome.index('starIntegrateDropFile(pGe.gDir+"/Genome");'),genome.index('SAiIn.close();'))
  self.assertIn('POSIX_FADV_DONTNEED',genome)
  self.assertIn('STAR_INTEGRATE_DROP_INDEX_CACHE',genome)
  self.assertIn("drop[0]=='1'",genome)
  self.assertIn('int e=posix_fadvise',genome)
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
  self.assertIn('star_integrate::enabled_fast()',inner)
  self.assertNotIn('InnerCall starIntegrateCall={pieceStart',inner)
  self.assertIn('splitR[1][ip], Nsplit)',read)
if __name__=='__main__': unittest.main()
