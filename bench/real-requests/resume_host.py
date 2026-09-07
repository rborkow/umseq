#!/usr/bin/env python3
"""Resume at validation/conversion; never regenerate the accepted STAR capture."""
import json
import os
from pathlib import Path
import re
import time

os.environ.setdefault('OUT', str(Path.home()/'uni-rnaseq-probe-lab/real-requests-host2'))
import run_host as h


def main():
    if os.environ.get('REAL_REQUESTS_LOCKED') != '1' or time.time() >= h.CUTOFF:
        raise RuntimeError('deadline-supervised resource lock required')
    h.OUT.mkdir()
    (h.OUT/'stages').mkdir()
    old = h.PROBE_LAB/'real-requests-host1'
    h.record(f'RESUME_FROM_CONVERT capture={old} cutoff={h.CUTOFF}')
    status = (old/'run.status').read_text()
    assert 'capture exit=0' in status and 'parity exit=0' in status
    parity = json.loads((old/'stock-parity/parity.json').read_text())
    assert parity['status'] == 'PARITY_MATCH_COUNTERS_ONLY'
    assert parity['input_reads'] == {'stock':20000000,'counters':20000000}
    os.environ['PATH'] = '/usr/local/cuda/bin:'+str(Path.home()/'.cargo/bin')+':'+os.environ['PATH']
    os.environ['CARGO_BUILD_JOBS'] = '4'
    tooling = h.ROOT/'tooling/replay'
    sources = [h.ROOT/'Cargo.toml',h.ROOT/'Cargo.lock']
    for root in (h.ROOT/'crates/umem',h.ROOT/'crates/umgpu',h.ROOT/'crates/umseed-probe',h.HERE):
        sources.extend(p for p in root.rglob('*') if p.is_file() and not any(x in p.parts for x in ('target','__pycache__','.git')) and not p.name.startswith('.env'))
    sources.extend(tooling/name for name in ('format.cpp','format.hpp','sha256.cpp','sha256.hpp'))
    (h.OUT/'source-sha256.json').write_text(json.dumps({str(p.relative_to(h.ROOT)):h.file_identity(p) for p in sorted(set(sources))},indent=2)+'\n')
    inventory = json.loads((old/'binding-inputs.json').read_text())
    for role,name,expected in inventory['entries']:
        if role == 2 and expected is not None:
            assert h.file_identity(h.INDEX/name) == expected, 'captured index changed: '+name
    active = h.verify_active(h.INDEX,(old/'star/probe.active').read_bytes())
    (h.OUT/'active-identity-verified.json').write_text(json.dumps(active)+'\n')
    traces = sorted((old/'star').glob('*.ssir'))
    capture = json.loads((old/'star/capture-manifest.json').read_text())
    assert len(traces) == len(capture['workers']) == 20
    (h.OUT/'capture-reuse.json').write_text(json.dumps({'root':str(old),'manifest':h.file_identity(old/'star/capture-manifest.json'),'parity':parity,'traces':{str(p):h.file_identity(p) for p in traces}},indent=2)+'\n')
    validator = h.OUT/'ssir-validate'
    h.run('validator-build',['c++','-std=c++17','-O2','-I',tooling,h.HERE/'ssir_validate.cpp',tooling/'format.cpp',tooling/'sha256.cpp','-o',validator])
    ids = json.loads((old/'star/binding.json').read_text())['ids']
    request = h.OUT/'real-requests.bin'
    h.run('convert',['python3','-B',h.HERE/'ssir_to_umprobe.py','--validator',validator,
                    '--source-sha256',ids['source'],'--index-sha256',ids['index'],'--runtime-sha256',ids['runtime'],
                    '--index-parameters-sha256',h.file_identity(h.INDEX/'genomeParameters.txt')['sha256'],
                    '--output',request,'--manifest',h.OUT/'real-requests.json',*traces])
    count = json.loads((h.OUT/'real-requests.json').read_text())['actual_count']
    assert count == capture['actual_count'] <= 1000000, 'capture was filtered or truncated'
    h.run('probe-build',['cargo','build','--offline','--locked','--release','-p','umseed-probe','--features','cuda'])
    h.run('probe-tests',['cargo','test','--offline','--locked','--release','-p','umseed-probe','--features','cuda'])
    binary = h.ROOT/'target/release/umseed-probe'
    h.run('sass',['cuobjdump','--dump-sass',binary])
    loads = {b.splitlines()[0].strip():len(re.findall(r'\bLDG(?:\.|\s)',b)) for b in (h.OUT/'stages/sass.stdout').read_text().split('Function : ')[1:] if 'probe_thread_kernel' in b.splitlines()[0]}
    assert loads and all(loads.values()), 'no observable thread loads'
    (h.OUT/'sass-loads.json').write_text(json.dumps(loads)+'\n')
    (h.OUT/'binaries.json').write_text(json.dumps({str(p):h.file_identity(p) for p in (binary,validator)},indent=2)+'\n')
    h.run('clocks-before',['nvidia-smi','--query-gpu=name,clocks.sm,utilization.gpu,power.draw','--format=csv'])
    for label,requests,n,pages in (('real',request,count,'huge'),
                                  ('synthetic',h.PROBE_LAB/'probe-measure-host1/probe-requests.bin',1000000,'huge'),
                                  ('real-4k',request,count,'small4k')):
        h.run(label,[binary,'run','--index',h.INDEX,'--requests',requests,'--output',h.OUT/(label+'.tsv'),
                     '--diagnostics',h.OUT/(label+'-distributions.json'),'--variant','thread','--pages',pages,'--overlap',
                     '--counts',str(n),'--repeats','3','--split-provenance','accepted SPLIT; original real capture reused without filtering',
                     '--cutoff-unix',str(h.CUTOFF)])
    h.run('clocks-after',['nvidia-smi','--query-gpu=name,clocks.sm,utilization.gpu,power.draw','--format=csv'])
    h.record('COMPLETE_REQUIRES_ORCHESTRATOR_VERIFICATION')


if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        if h.OUT.exists():
            h.record('FAILED '+str(exc))
        raise
