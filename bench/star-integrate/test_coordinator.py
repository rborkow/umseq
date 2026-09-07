#!/usr/bin/env python3
"""Compile/run actual star_integrate.cpp; fake USI is transport-only, not GPU evidence."""
import json, os, shutil, signal, subprocess, tempfile
from pathlib import Path
HERE=Path(__file__).resolve().parent
STAR=Path('/private/tmp/star-full-source.UVdsuH/STAR-2.7.11b/source')
REPLAY=Path('/private/tmp/p2c-capture-local-01/helpers')
def main():
  if not STAR.exists() or not REPLAY.exists(): raise SystemExit('pinned STAR/replay fixture unavailable')
  cxx=shutil.which('clang++') or shutil.which('c++')
  with tempfile.TemporaryDirectory(prefix='star-coordinator-') as tmp:
    exe=Path(tmp)/'coordinator-production-fixture'
    subprocess.run([cxx,'-std=c++11','-pthread','-Wall','-Wextra','-Werror','-I'+str(HERE/'test-stubs'),'-I'+str(HERE),'-I'+str(REPLAY),'-I'+str(STAR),str(HERE/'test_coordinator.cpp'),str(REPLAY/'sha256.cpp'),'-o',str(exe)],check=True,timeout=90)
    sidecar=Path(tmp)/'sidecar.jsonl'
    env=dict(os.environ, STAR_INTEGRATE_SIDECAR=str(sidecar))
    subprocess.run([str(exe)],env=env,check=True,timeout=30)
    rows=[json.loads(line) for line in sidecar.read_text().splitlines() if line.strip()]
    assert len(rows)==1
    # These are precisely gate (i)'s local schema assertions; this fixture is
    # transport-only and makes no claim of CUDA/STAR parity.
    assert rows[0]['gpu_consumed'] > 0
    assert rows[0]['batch_faults'] == rows[0]['rejected'] == 0
    bad=subprocess.run([str(exe),'strict-invalid-success'],text=True,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=10)
    if bad.returncode != -signal.SIGABRT: raise AssertionError('strict child did not SIGABRT: '+repr(bad.returncode)+'\n'+bad.stderr)
    if 'backend returned invalid successful result' not in bad.stderr: raise AssertionError('missing strict diagnostic:\n'+bad.stderr)
if __name__=='__main__': main()
