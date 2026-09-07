#!/usr/bin/env python3
"""Orchestrator gate(i) ONLY: private build, strict20M parity, no paired timings."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

CUTOFF = 1788836400
LAB = Path.home() / 'uni-rnaseq-probe-lab'
PINNED = Path.home() / 'uni-rnaseq-seed-lab/real-index-input-2.7.11b/source'
OLD = LAB / 'real-requests-host1'


def identity(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        for block in iter(lambda: f.read(1024 * 1024), b''):
            h.update(block)
    return {'bytes': path.stat().st_size, 'sha256': h.hexdigest()}


def main(a):
    root, out = a.source_root.resolve(), a.output.resolve()
    assert root.is_relative_to(LAB) and out.is_relative_to(LAB)
    assert not out.exists() and root != out
    assert os.environ.get('INTEGRATE_LOCKED') == '1' and time.time() < CUTOFF
    lock = Path.home() / '.cache/uni-rnaseq-resource.lock'
    assert os.fstat(9).st_ino == lock.stat().st_ino
    fcntl.flock(9, fcntl.LOCK_EX | fcntl.LOCK_NB)
    out.mkdir()
    (out / 'stages').mkdir()
    here, tooling = root / 'bench/star-integrate', root / 'tooling/replay'
    private = out / 'private'

    def record(text):
        with (out / 'run.status').open('a') as f:
            f.write(text + '\n')

    def run(label, args, env=None):
        left = min(int(CUTOFF - time.time()), 3600)
        if left <= 0:
            raise RuntimeError('authorization cutoff reached')
        args = list(map(str, args))
        (out / 'stages' / (label + '.argv.json')).write_text(json.dumps(args) + '\n')
        record('RUNNING ' + label)
        with (out / 'stages' / (label + '.stdout')).open('w') as stdout, (out / 'stages' / (label + '.stderr')).open('w') as stderr:
            rc = subprocess.run([
                '/usr/bin/time', '-f', '%e %U %S %M %x', '-o',
                str(out / 'stages' / (label + '.diagnostic-time.tsv')),
                'timeout', '--signal=TERM', '--kill-after=30s', str(left) + 's',
                *args,
            ], cwd=root, env=env, stdout=stdout, stderr=stderr).returncode
        record(f'{label} exit={rc}')
        if rc:
            raise RuntimeError(f'{label} failed with exit{rc}')

    try:
        record(f'GATE_I_ONLY strict_oracle=1 cutoff={CUTOFF}; times diagnostic NOT performance')
        busy = [x.strip() for x in subprocess.check_output(['ps', '-eo', 'comm='], text=True).splitlines()
                if x.strip() in {'STAR', 'umbam', 'nextflow', 'nvcc'} or x.strip().startswith('VLLM')]
        assert not busy, busy
        (out / 'meminfo-before.txt').write_text(Path('/proc/meminfo').read_text())
        (out / 'nvidia-before.txt').write_text(subprocess.check_output(['nvidia-smi'], text=True))
        old_parity = json.loads((OLD / 'stock-parity/parity.json').read_text())
        assert old_parity['status'] == 'PARITY_MATCH_COUNTERS_ONLY'
        stock_argv = json.loads((OLD / 'stages/stock.argv.json').read_text())
        assert stock_argv[stock_argv.index('--runThreadN') + 1] == '20'
        assert stock_argv[stock_argv.index('--outFileNamePrefix') + 1] == str(OLD / 'stock') + '/'
        (out / 'stock-reuse.json').write_text(json.dumps({
            'root': str(OLD / 'stock'), 'argv': stock_argv, 'parity': old_parity,
            'files': {p.name: identity(p) for p in (OLD / 'stock').iterdir() if p.is_file()},
        }, indent=2) + '\n')
        inventory = json.loads((OLD / 'binding-inputs.json').read_text())
        index = Path(stock_argv[stock_argv.index('--genomeDir') + 1])
        for role, name, expected in inventory['entries']:
            if role == 2 and expected is not None:
                assert identity(index / name) == expected, 'index changed: ' + name
        inputs = stock_argv[stock_argv.index('--readFilesIn') + 1:stock_argv.index('--readFilesCommand')]
        input_ids = {p: identity(Path(p)) for p in inputs}
        expected_inputs = {name: expected for role, name, expected in inventory['entries'] if role == 3}
        assert len(inputs) == len(expected_inputs) == 2
        for mate, path in enumerate(inputs, 1):
            assert input_ids[path] == expected_inputs[f'reads_{mate}.fastq.gz'], 'input changed: ' + path
        (out / 'inputs.json').write_text(json.dumps(input_ids, indent=2) + '\n')
        sources = {str(p.relative_to(root)): identity(p) for p in root.rglob('*')
                   if p.is_file() and '__pycache__' not in p.parts}
        (out / 'source-sha256.json').write_text(json.dumps(sources, indent=2) + '\n')
        run('abi', ['bash', here / 'test_abi.sh'])
        run('prepare', ['python3', '-B', here / 'make_star_integrate.py', '--tooling', tooling,
                        '--source', PINNED, '--private-root', private])
        for name in ('source-inventory.json', 'preparation.json', 'hook.diff'):
            shutil.copyfile(private / name, out / name)
        env = dict(os.environ)
        for name in ('MAKEFLAGS', 'MFLAGS', 'CFLAGS', 'CPPFLAGS', 'CXXFLAGS', 'LDFLAGS'):
            env.pop(name, None)
        env['PATH'] = str(Path.home() / '.cargo/bin') + ':/usr/local/cuda/bin:' + env['PATH']
        env['CARGO_TARGET_DIR'] = str(out / 'target')
        env['CARGO_BUILD_JOBS'] = '4'
        env['UMGPU_NVCC'] = '/usr/local/cuda/bin/nvcc'
        run('backend-build', ['cargo', 'build', '-p', 'umstar', '--release', '--features', 'cuda', '--offline'], env)
        run('backend-tests', ['cargo', 'test', '-p', 'umstar', '--features', 'cuda', '--offline'], env)
        lib = out / 'target/release/libumstar.a'
        (out / 'backend.json').write_text(json.dumps(identity(lib)) + '\n')
        run('build', ['make', '-j4', 'STAR', 'CXX=' + shutil.which('g++'), 'CC=' + shutil.which('gcc'),
                     'CXXFLAGSextra=-DSTAR_INTEGRATE=1 -I' + str(here),
                     'LDFLAGSextra=' + str(lib) + ' -L/usr/local/cuda/lib64 -Wl,-rpath,/usr/local/cuda/lib64 -lcudart -ldl -lm -lrt',
                     'CXXFLAGS_SIMD=', 'BUILD_DATE=real-index-private', 'BUILD_PLACE=real-index-private',
                     '-C', private / 'integrated'], env)
        binary = private / 'integrated/STAR'
        (out / 'binary.json').write_text(json.dumps(identity(binary)) + '\n')
        integrated = out / 'integrated'
        integrated.mkdir()
        args = list(stock_argv)
        args[0] = str(binary)
        args[args.index('--outFileNamePrefix') + 1] = str(integrated) + '/'
        sidecar = integrated / 'integrate-stats.jsonl'
        run('integrated', args, dict(env, STAR_INTEGRATE='1', STAR_INTEGRATE_STRICT='1',
                                    STAR_INTEGRATE_SIDECAR=str(sidecar)))
        assert not list(integrated.glob('*.ssir')), 'unexpected SSIR capture'
        # The accepted SPLIT comparator is copied unchanged, not reimplemented.
        run('parity', ['python3', '-B', tooling / 'seed_split_parity.py', '--stock', OLD / 'stock',
                       '--counters', integrated, '--stock-command', ' '.join(stock_argv),
                       '--counter-command', ' '.join(args), '--expected-pairs', '20000000', '--out', out / 'parity'])
        assert sidecar.is_file(), 'missing integration accounting'
        rows = [json.loads(line) for line in sidecar.read_text().splitlines() if line.strip()]
        assert len(rows) == 1, 'expected one final integration accounting row'
        stats = rows[0]
        assert stats['gpu_consumed'] > 0, 'parity without any GPU result consumed is not the integration gate'
        assert stats['batch_faults'] == 0 and stats['rejected'] == 0, stats
        (out / 'gate-summary.json').write_text(json.dumps({
            'status': 'PARITY_CHECKPOINT_REQUIRES_ORCHESTRATOR_VERIFICATION',
            'strict_oracle': True, 'performance_measured': False, 'stats': stats,
            'unchanged_parity_checker': identity(tooling / 'seed_split_parity.py'),
        }, indent=2) + '\n')
        (out / 'meminfo-after.txt').write_text(Path('/proc/meminfo').read_text())
        record('COMPLETE_GATE_I_STOP_BEFORE_PAIRED_TIMINGS')
        (out / 'exit.code').write_text('0\n')
    except Exception as exc:
        record('FAILED ' + str(exc))
        (out / 'exit.code').write_text('1\n')
        raise


if __name__ == '__main__':
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source-root', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    main(p.parse_args())
