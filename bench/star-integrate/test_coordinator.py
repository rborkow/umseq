#!/usr/bin/env python3
"""Compile/run actual star_integrate.cpp; fake USI is transport-only, not GPU evidence."""
import importlib.util, json, os, shutil, subprocess, tempfile
from pathlib import Path
HERE=Path(__file__).resolve().parent
STAR=Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source')
REPLAY=Path('/private/tmp/p2c-capture-local-01/helpers')
def main():
  if not STAR.exists() or not REPLAY.exists(): raise SystemExit('pinned STAR/replay fixture unavailable')
  cxx=shutil.which('clang++') or shutil.which('c++')
  hook_cxx='/opt/homebrew/opt/llvm/bin/clang++' if Path('/opt/homebrew/opt/llvm/bin/clang++').exists() else cxx
  spec=importlib.util.spec_from_file_location('star_integrate_generator',HERE/'make_star_integrate.py')
  generator=importlib.util.module_from_spec(spec); spec.loader.exec_module(generator)
  class ReplaceOnce:
    def replace_once(self,text,old,new):
      if text.count(old) != 1: raise ValueError('missing or duplicate exact hook')
      return text.replace(old,new,1)
  with tempfile.TemporaryDirectory(prefix='star-coordinator-') as tmp:
    tmp=Path(tmp)
    # Compile the actual generated upstream hook in the same regression that
    # drives its builder-shaped call through the real coordinator lookup.
    generated=tmp/'ReadAlign_maxMappableLength2strands.cpp'
    generated.write_text(generator.patch(ReplaceOnce(),'ReadAlign_maxMappableLength2strands.cpp',(STAR/'ReadAlign_maxMappableLength2strands.cpp').read_text()))
    subprocess.run([hook_cxx,'-std=c++11','-DSTAR_INTEGRATE=1','-I'+str(HERE),'-I'+str(STAR),'-I/opt/homebrew/opt/libomp/include','-fopenmp','-c',str(generated),'-o',str(tmp/'generated-hook.o')],check=True,timeout=90)
    exe=tmp/'coordinator-production-fixture'
    subprocess.run([cxx,'-std=c++11','-pthread','-Wall','-Wextra','-Werror','-I'+str(HERE/'test-stubs'),'-I'+str(HERE),'-I'+str(REPLAY),'-I'+str(STAR),str(HERE/'test_coordinator.cpp'),str(REPLAY/'sha256.cpp'),'-o',str(exe)],check=True,timeout=90)
    sidecar=Path(tmp)/'sidecar.jsonl'
    env=dict(os.environ, STAR_INTEGRATE_SIDECAR=str(sidecar))
    key=subprocess.run([str(exe),'generated-key-hook'],text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=True,timeout=30)
    wwb=subprocess.run([str(exe),'whole-window-batching'],text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,check=True,timeout=30)
    assert 'batches=2 consumed=7' in wwb.stdout, wwb.stdout+wwb.stderr
    if key.stdout.strip() != 'generated key hook: consumed=1 key_misses=0':
      raise AssertionError('generated hook key fixture did not consume exactly once:\n'+key.stdout+key.stderr)
    subprocess.run([str(exe),'positional-shuffled'],check=True,timeout=30)
    subprocess.run([str(exe)],env=env,check=True,timeout=30)
    rows=[json.loads(line) for line in sidecar.read_text().splitlines() if line.strip()]
    assert len(rows)==1
    # These are precisely gate (i)'s local schema assertions; this fixture is
    # transport-only and makes no claim of CUDA/STAR parity.
    assert rows[0]['gpu_consumed'] > 0
    assert rows[0]['batch_faults'] == rows[0]['rejected'] == 0
    assert rows[0]['key_misses'] == sum(rows[0]['miss_reasons'][name] for name in (
      'read_bytes','positional_exhausted','chain_rejected_residue','no_job',
      'key_mismatch','not_ready','device_stopped','shift','cas_lost'))
    assert rows[0]['no_window'] == rows[0]['miss_reasons']['no_window']
    assert set(rows[0]['not_ready_where']) == {'queued','filling','draining'}
    assert isinstance(rows[0]['device_stop_status'], dict)
if __name__=='__main__': main()
