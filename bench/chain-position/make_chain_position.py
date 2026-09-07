#!/usr/bin/env python3
"""Prepare a private, counters-only STAR chain-position build.

This deliberately reuses the accepted SPLIT inventory gate and base counters.  It
does not create SSIR records, alter the source tree, or invoke STAR.
"""
import argparse
import difflib
import importlib.util
import json
from pathlib import Path
import shutil
import sys

HERE = Path(__file__).resolve().parent


def load_oracle(tooling):
    p = tooling / "real_index_oracle.py"
    spec = importlib.util.spec_from_file_location("chain_position_rio", p)
    if spec is None or spec.loader is None:
        raise ValueError("cannot load verified real_index_oracle.py")
    module = importlib.util.module_from_spec(spec)
    sys.path.insert(0, str(tooling))  # its verified make_oracle.py dependency
    spec.loader.exec_module(module)
    return module


def replace_once(rio, text, old, new):
    return rio.replace_once(text, old, new)


def hooks(rio, name, original):
    r = lambda text, old, new: replace_once(rio, text, old, new)
    text = '#include "split_capture_hooks.hpp"\n#include "chain_position_hooks.hpp"\n' + original
    if name == "STAR.cpp":
        text = r(text, '#include "parametersDefault.xxd"\n',
                 '#include "parametersDefault.xxd"\n#include "split_capture_impl.hpp"\n#include "chain_position_impl.hpp"\n')
        text = r(text, '    P.inputParameters(argInN, argIn);\n',
                 '    P.inputParameters(argInN, argIn);\n    split_capture::guard(P);\n    chain_position::guard(P);\n')
        text = r(text, '    // prepare chunks and spawn mapping threads\n',
                 '    split_capture::startup(genomeMain);\n    chain_position::startup();\n\n    // prepare chunks and spawn mapping threads\n')
        # Baseline lacks this line; retain its exact finish insertion point.
        text = r(text, '    delete P.inOut; // to close files\n\n    return 0;',
                 '    delete P.inOut; // to close files\n\n    split_capture::finish();\n    chain_position::finish();\n    return 0;')
        return text
    if name == "ReadAlign_oneRead.cpp":
        text = r(text, '    statsRA.readN++;',
                 '    split_capture::read_begin(Read1, Lread);\n    chain_position::read_begin();\n\n    statsRA.readN++;')
        return r(text, '        mapOneRead();\n',
                 '        mapOneRead();\n        split_capture::read_end();\n        chain_position::read_end();\n')
    if name == "ReadAlign_mapOneRead.cpp":
        text = r(text, '    resetN(); //reset aligns counters to 0',
                 '    split_capture::split_count(Nsplit);\n\n    resetN(); //reset aligns counters to 0')
        text = r(text, '                if (flagDirMap || istart>0) {//check if the 1st piece in reveree direction does not need to be remapped\n',
                 '                chain_position::chain_opportunity(ip, istart, iDir, splitR[0][ip], splitR[1][ip], Lstart);\n'
                 '                if (flagDirMap || istart>0) {//check if the 1st piece in reveree direction does not need to be remapped\n')
        text = r(text, '                };//if (flagDirMap || istart>0)\n\n                if (P.seedSearchLmax>0)',
                 '                } else {\n                    chain_position::reverse_suppressed(ip, istart, iDir, splitR[0][ip], splitR[1][ip], Lstart);\n                };//if (flagDirMap || istart>0)\n\n                if (P.seedSearchLmax>0)')
        call = 'maxMappableLength2strands(Shift, seedLength, iDir, 0, mapGen.nSA-1, L, splitR[2][ip]);//L=max mappable length, unique or multiple'
        old = '                        ' + call + '\n'
        new = ('                        split_capture::outer_begin(ip, istart, 0, splitR[2][ip], Shift, seedLength, iDir);\n'
               '                        chain_position::outer_begin(ip, istart, iDir, splitR[0][ip], splitR[1][ip], Lstart, Lmapped, Shift, seedLength);\n'
               '                        ' + call + '\n'
               '                        split_capture::outer_end();\n'
               '                        chain_position::outer_end();\n')
        return r(text, old, new)
    if name == "ReadAlign_maxMappableLength2strands.cpp":
        text = r(text, '                --Lind;\n',
                 '                split_capture::reduced();\n                --Lind;\n')
        text = r(text, '        // define upper bound for suffix array range search.',
                 '        split_capture::prefix_ready(Lind);\n\n        // define upper bound for suffix array range search.')
        text = r(text, '            // very short seq, already found hits',
                 '            split_capture::branch(0, iSA1noN, iSA2good);\n            // very short seq, already found hits')
        text = r(text, '            bool comparRes;\n',
                 '            split_capture::branch(1, iSA1noN, iSA2good);\n            bool comparRes;\n')
        direct = '            maxL=compareSeqToGenome(mapGen, Read1, pieceStart, pieceLength, Lind, iSA1, dirR, comparRes);'
        text = r(text, direct, '            split_capture::inner_phase(0);\n' + direct + '\n            split_capture::direct_end();')
        old = '            };        \n            Nrep = maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL, indStartEnd);'
        new = ('            };        \n'
               '            split_capture::branch(2, iSA1noN, iSA2good);\n'
               '            split_capture::inner_begin(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL);\n'
               '            chain_position::inner_begin(pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL);\n'
               '            Nrep = maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL, indStartEnd);\n'
               '            split_capture::inner_end(maxL, indStartEnd[0], indStartEnd[1], Nrep);\n'
               '            chain_position::inner_end();')
        return r(text, old, new)
    if name == "SuffixArrayFuns.cpp":
        prefix, tail = text.split('\nuint findMultRange(', 1)
        prefix = r(prefix, '    register int64 ii;\n',
                   '    split_capture::compare_guard(mapGen, s2, S, N, L, iSA, dirR);\n    register int64 ii;\n')
        prefix = r(prefix, '    SAstr &= mapGen.GstrandMask;\n',
                   '    SAstr &= mapGen.GstrandMask;\n    split_capture::compare_begin(SAstr, dirR, dirG, S, N, L);\n    chain_position::compare_begin();\n')
        for condition in ('s[ii]!=g[ii]', 's[ii]!=g[-ii]', 's[-ii]!=g[ii]', 's[-ii]!=g[-ii]'):
            prefix = r(prefix, '            if (' + condition + ')\n            {',
                       '            if (' + condition + ')\n            {\n                chain_position::compared(ii+1);')
        prefix = r(prefix, '        return N; //exact match', '        chain_position::compared(N-L);\n        return N; //exact match')
        prefix = prefix.replace('        return N;\n    } else', '        chain_position::compared(N-L);\n        return N;\n    } else')
        prefix = r(prefix, '        return N;\n    };', '        chain_position::compared(N-L);\n        return N;\n    };')
        tail = tail.replace('        uint L1c=compareSeqToGenome(mapGen,s,S,L3,L1b,i1c,dirR,compRes);', '        split_capture::inner_phase(2);\n        chain_position::phase(2);\n        uint L1c=compareSeqToGenome(mapGen,s,S,L3,L1b,i1c,dirR,compRes);', 1)
        tail = tail.replace('    L1=compareSeqToGenome(mapGen,s,S,N,L,i1,dirR,compRes);', '    split_capture::inner_phase(0);\n    chain_position::phase(0);\n    L1=compareSeqToGenome(mapGen,s,S,N,L,i1,dirR,compRes);', 1)
        tail = tail.replace('    L2=compareSeqToGenome(mapGen,s,S,N,L,i2,dirR,compRes);', '    split_capture::inner_phase(0);\n    chain_position::phase(0);\n    L2=compareSeqToGenome(mapGen,s,S,N,L,i2,dirR,compRes);', 1)
        tail = tail.replace('        L3=compareSeqToGenome(mapGen,s,S,N,L,i3,dirR,compRes);', '        split_capture::inner_phase(1);\n        chain_position::phase(1);\n        L3=compareSeqToGenome(mapGen,s,S,N,L,i3,dirR,compRes);', 1)
        # Preserve the original SPLIT byte callbacks alongside the new attribution.
        for argument in ('ii+1', 'N-L'):
            call = f'chain_position::compared({argument});'
            prefix = prefix.replace(call, f'split_capture::compared({argument});\n                '+call)
        return prefix + '\nuint findMultRange(' + tail
    raise ValueError('unsupported file: ' + name)


def prepare(tooling, source, root):
    rio = load_oracle(tooling)
    source, root = source.absolute(), root.absolute()
    rio.require(not root.resolve().is_relative_to(source.resolve()), 'private root inside source')
    root.mkdir(mode=0o700)
    inv = rio.inventory(source)
    rio.require(len(inv) == 344 and rio.digest(rio.encoded(inv)) == rio.FULL_INVENTORY_SHA256,
                'complete pinned source inventory mismatch')
    (root / 'source-inventory.json').write_text(json.dumps(inv, indent=2, sort_keys=True) + '\n')
    shutil.copytree(source, root / 'baseline')
    shutil.copytree(source, root / 'counters')
    changed = ('STAR.cpp', 'ReadAlign_oneRead.cpp', 'ReadAlign_mapOneRead.cpp', 'ReadAlign_maxMappableLength2strands.cpp', 'SuffixArrayFuns.cpp')
    for name in changed:
        (root / 'counters' / name).write_text(hooks(rio, name, (source / name).read_text()))
    base_additions = ('capture_writer.hpp', 'capture_writer.cpp', 'sha256.hpp', 'sha256.cpp', 'split_capture_hooks.hpp', 'split_capture_impl.hpp')
    for name in base_additions:
        shutil.copyfile(tooling / name, root / 'counters' / name)
    for name in ('chain_position_hooks.hpp', 'chain_position_impl.hpp'):
        shutil.copyfile(HERE / name, root / 'counters' / name)
    diff = ''.join(''.join(difflib.unified_diff((root/'baseline'/n).read_text().splitlines(True), (root/'counters'/n).read_text().splitlines(True), fromfile='baseline/'+n, tofile='counters/'+n)) for n in changed)
    (root/'hook.diff').write_text(diff)
    data = {'status':'PREPARED_COUNTERS_ONLY_NO_SSIR', 'source_inventory_sha256':rio.digest(rio.encoded(inv)), 'changed':changed, 'base_tooling':str(tooling), 'patch_sha256':rio.digest(diff.encode())}
    (root/'preparation.json').write_text(json.dumps(data, indent=2, sort_keys=True)+'\n')
    return root


if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--tooling', type=Path, required=True)
    p.add_argument('--source', type=Path, required=True)
    p.add_argument('--private-root', type=Path, required=True)
    a = p.parse_args()
    try: print(prepare(a.tooling, a.source, a.private_root))
    except Exception as e: print('chain-position preparation failed: ' + str(e), file=sys.stderr); sys.exit(1)
