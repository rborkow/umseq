#!/usr/bin/env python3
"""Prepare a fresh bounded real-STAR INNER capture build.

This is deliberately an adapter around the *verified counters-only* hook
layout.  It never alters the SSIR v1 writer or parser.  The generated build is
private and has a separate ``capture`` tree; the supplied base private tree is
read-only evidence, not a relocated cache.

The implementation file is copied from the approved capture implementation so
the actual STAR arguments, buffer ownership and SSIR record writer remain the
ones which were reviewed with the seed experiment.  Its only profile changes
are: a 20-thread guard, a bounded capture stop, and per-worker complete-read
files.  The host runner supplies the approved tooling directory explicitly.
"""
import argparse
import difflib
import json
from pathlib import Path
import shutil
import sys
import importlib.util


HOOK_FILES = (
    "STAR.cpp", "ReadAlign_oneRead.cpp", "ReadAlign_mapOneRead.cpp",
    "ReadAlign_maxMappableLength2strands.cpp", "SuffixArrayFuns.cpp",
)
SUPPORT = ("capture_writer.hpp", "capture_writer.cpp", "sha256.hpp", "sha256.cpp",
           "split_capture_hooks.hpp", "split_capture_impl.hpp")


def replace_once(text, old, new):
    if text.count(old) != 1:
        raise ValueError("pinned STAR hook anchor changed: " + old[:48])
    return text.replace(old, new)


def load_split(replay):
    """Load the approved generator, rather than duplicating its fragile hooks."""
    script = replay / "make_split_capture.py"
    if not script.is_file() or not (replay / "real_index_oracle.py").is_file():
        raise ValueError("approved tooling missing make_split_capture.py/real_index_oracle.py")
    sys.path.insert(0, str(replay))
    spec = importlib.util.spec_from_file_location("approved_split_capture", script)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def hooks(name, original, split):
    """Use every approved split phase hook, changing only the call namespace."""
    text = split.hooks(name, original).replace("split_capture::", "capture::")
    text = text.replace('#include "split_capture_hooks.hpp"', '#include "capture_hooks.hpp"')
    if name != "STAR.cpp":
        return text
    text = replace_once(text, '#include "split_capture_impl.hpp"\n',
                        '#include "real_inner_capture_impl.hpp"\n')
    text = replace_once(text, 'capture::startup(genomeMain);',
                        'capture::startup(genomeMain, argInN, argIn);')
    text = replace_once(text, '    genomeMain.freeMemory();\n',
                        '    capture::verify_end(genomeMain);\n    genomeMain.freeMemory();\n')
    return text


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source', type=Path, required=True)
    p.add_argument('--private-root', type=Path, required=True)
    p.add_argument('--base-private-root', type=Path, required=True)
    p.add_argument('--tooling', type=Path, required=True,
                   help='approved tooling-seed-split-v3 root')
    p.add_argument('--limit', type=int, default=1_000_000)
    p.add_argument('--per-file-record-cap', type=int, default=100_000)
    p.add_argument('--per-file-read-cap', type=int, default=10_000)
    a = p.parse_args()
    if (a.limit != 1_000_000 or a.per_file_record_cap != 100_000
            or a.per_file_read_cap != 10_000):
        p.error('real capture has fixed 1000000/100000/10000 caps')
    if a.private_root.exists() or not a.source.is_dir() or not a.base_private_root.is_dir():
        p.error('private-root must be fresh; source and base-private-root must exist')
    replay = a.tooling / 'replay'
    split = load_split(replay)
    for name in SUPPORT:
        if not (replay / name).is_file():
            p.error('approved tooling missing ' + name)
    inventory = split.rio.inventory(a.source)
    if len(inventory) != 344 or split.rio.digest(split.rio.encoded(inventory)) != split.rio.FULL_INVENTORY_SHA256:
        p.error('complete pinned source inventory mismatch')
    a.private_root.mkdir(mode=0o700)
    shutil.copytree(a.source, a.private_root / 'baseline')
    shutil.copytree(a.source, a.private_root / 'capture')
    capture = a.private_root / 'capture'
    (a.private_root / 'source-inventory.json').write_text(json.dumps(inventory, indent=2, sort_keys=True) + '\n')
    for name in HOOK_FILES:
        before = (a.source / name).read_text()
        (capture / name).write_text(hooks(name, before, split))
    for name in SUPPORT:
        shutil.copy2(replay / name, capture / name)
    # Declarations are a namespace-only view of the approved hook ABI.
    declarations = (replay / 'split_capture_hooks.hpp').read_text().replace('namespace split_capture', 'namespace capture')
    declarations = declarations.replace('void startup(Genome&);', 'void startup(Genome&, int, char**);\nvoid verify_end(Genome&);')
    (capture / 'capture_hooks.hpp').write_text(declarations)
    # The host adapter implementation is intentionally a named, reviewable
    # source in this repository; it is copied beside STAR, never injected into
    # the installed tree.
    implementation = Path(__file__).with_name('real_inner_capture_impl.hpp')
    if not implementation.is_file():
        p.error('missing real_inner_capture_impl.hpp beside adapter')
    shutil.copy2(implementation, capture / implementation.name)
    diff = ''.join(''.join(difflib.unified_diff((a.source / n).read_text().splitlines(True),
                                                 (capture / n).read_text().splitlines(True),
                                                 fromfile='baseline/' + n, tofile='capture/' + n)) for n in HOOK_FILES)
    (a.private_root / 'hook.diff').write_text(diff)
    (a.private_root / 'preparation.json').write_text(json.dumps({
        'status': 'PREPARED_REAL_INNER_CAPTURE_NOT_RUN', 'source': str(a.source),
        'source_inventory_sha256': split.rio.digest(split.rio.encoded(inventory)),
        'base_private_root': str(a.base_private_root), 'limit': a.limit,
        'per_file_record_cap': a.per_file_record_cap,
        'per_file_read_cap': a.per_file_read_cap, 'wire': 'SSIRv1 unchanged',
        'selection': 'first bounded complete arrivals per worker; complete reads only; stop capture only',
    }, indent=2, sort_keys=True) + '\n')
    print(a.private_root)


if __name__ == '__main__':
    try:
        main()
    except Exception as exc:
        print('make_real_inner_capture: ' + str(exc), file=sys.stderr)
        sys.exit(2)
