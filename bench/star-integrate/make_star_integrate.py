#!/usr/bin/env python3
"""Make a private STAR baseline/integration pair without touching its source.

The generator intentionally fails if any pinned source line used as a hook moves.
It is a staging tool, not a patch applicator for a developer's working tree.
"""
import argparse, difflib, importlib.util, json, shutil, sys
from pathlib import Path

HERE=Path(__file__).resolve().parent

def oracle(tooling):
    p=tooling/'real_index_oracle.py'; spec=importlib.util.spec_from_file_location('star_integrate_oracle',p)
    if not spec or not spec.loader: raise ValueError('verified real_index_oracle.py is required')
    m=importlib.util.module_from_spec(spec); sys.path.insert(0,str(tooling)); spec.loader.exec_module(m); return m
def once(rio,text,old,new): return rio.replace_once(text,old,new)
def patch(rio,name,text):
    if name=='STAR.cpp':
        text=once(rio,text,'#include "parametersDefault.xxd"\n','#include "parametersDefault.xxd"\n#include "star_integrate.hpp"\n#include "star_integrate_work.hpp"\n')
        return once(rio,text,'    delete P.inOut; // to close files\n\n    return 0;','    delete P.inOut; // to close files\n\n    star_integrate::finish();\n    star_integrate_work::finish();\n    return 0;')
    if name=='ReadAlign_maxMappableLength2strands.cpp':
        # The hook is deliberately at the original inner call, after STAR's prefix
        # preparation.  The default macro is a no-op and preserves stock control flow.
        text='#include "star_integrate.hpp"\n#include "star_integrate_work.hpp"\n#include <cstdio>\n'+text
        old='            Nrep = maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL, indStartEnd);'
        new='            uint starIntegrateLIn=maxL;\n            star_integrate::InnerCall starIntegrateCall={pieceStart,pieceLength,iSA1 & mapGen.SAiMarkNmask,iSA2,dirR,maxL,0,0,iDist,0,0,0,star_integrate::current_generation(),pieceStart,pieceLength,star_integrate::INITIAL_KIND,iReadAll,star_integrate::current_epoch()};\n            // STAR uint is unsigned long long; the coordinator ABI is uint64_t (unsigned long on Linux). Same width, distinct types: marshal through exact-type locals.\n            uint64_t starIntegrateRange[2]={0,0}, starIntegrateNrep=0, starIntegrateMaxL=maxL;\n            bool starIntegrateHit=star_integrate::lookup(P, mapGen, Read1, (uint64_t) Lread, starIntegrateCall, starIntegrateRange, starIntegrateNrep, starIntegrateMaxL);\n            if (starIntegrateHit) { indStartEnd[0]=starIntegrateRange[0]; indStartEnd[1]=starIntegrateRange[1]; Nrep=starIntegrateNrep; maxL=starIntegrateMaxL; }\n            star_integrate_work::inner_call(starIntegrateHit);\n            if (starIntegrateHit) {\n                if (star_integrate::strict()) {\n                    uint cpuRange[2], cpuL=starIntegrateLIn;\n                    star_integrate_work::Scope starIntegrateOracle=star_integrate_work::oracle_scope();\n                    uint cpuN=maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, cpuL, cpuRange);\n                    if (cpuN!=Nrep || cpuL!=maxL || cpuRange[0]!=indStartEnd[0] || cpuRange[1]!=indStartEnd[1]) {\n                        fprintf(stderr, "STAR_INTEGRATE strict mismatch piece=%llu fragment=%llu distance=%llu start=%llu length=%llu low=%llu high=%llu dir=%llu gpu=(%llu,%llu,%llu,%llu) cpu=(%llu,%llu,%llu,%llu)\\n", (unsigned long long) pieceStart, (unsigned long long) iFrag, (unsigned long long) iDist, (unsigned long long) pieceStart, (unsigned long long) pieceLength, (unsigned long long) (iSA1 & mapGen.SAiMarkNmask), (unsigned long long) iSA2, (unsigned long long) dirR, (unsigned long long) Nrep, (unsigned long long) maxL, (unsigned long long) indStartEnd[0], (unsigned long long) indStartEnd[1], (unsigned long long) cpuN, (unsigned long long) cpuL, (unsigned long long) cpuRange[0], (unsigned long long) cpuRange[1]);\n                        abort();\n                    }\n                }\n            } else {\n                star_integrate_work::Scope starIntegrateFallback=star_integrate_work::fallback_scope();\n                Nrep = maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL, indStartEnd);\n            }'
        return once(rio,text,old,new)
    if name=='SuffixArrayFuns.cpp':
        prefix,tail=text.split('\nuint findMultRange(',1)
        prefix='#include "star_integrate_work.hpp"\n'+prefix
        prefix=once(rio,prefix,'    SAstr &= mapGen.GstrandMask;\n','    SAstr &= mapGen.GstrandMask;\n    star_integrate_work::compare_begin();\n')
        for condition in ('s[ii]!=g[ii]', 's[ii]!=g[-ii]', 's[-ii]!=g[ii]', 's[-ii]!=g[-ii]'):
            prefix=once(rio,prefix,'            if ('+condition+')\n            {','            if ('+condition+')\n            {\n                star_integrate_work::compared(ii+1);')
        prefix=once(rio,prefix,'        return N; //exact match','        star_integrate_work::compared(N-L);\n        return N; //exact match')
        prefix=once(rio,prefix,'        return N;\n    } else if (!dirR && dirG)', '        star_integrate_work::compared(N-L);\n        return N;\n    } else if (!dirR && dirG)')
        prefix=once(rio,prefix,'        return N;\n    } else {//if (!dirR && !dirG)', '        star_integrate_work::compared(N-L);\n        return N;\n    } else {//if (!dirR && !dirG)')
        prefix=once(rio,prefix,'        return N;\n    };\n};', '        star_integrate_work::compared(N-L);\n        return N;\n    };\n};')
        return prefix+'\nuint findMultRange('+tail
    if name=='ReadAlignChunk_mapChunk.cpp':
        text='#include "star_integrate.hpp"\n'+text
        text=once(rio,text,'        readStatus=RA->oneRead(); //map one read',
                  '        star_integrate::prepare_window(*this);\n        readStatus=RA->oneRead(); //map one read')
        if '    }; //reads cycle' in text:
            text=once(rio,text,'    }; //reads cycle',
                      '    }; //reads cycle\n\n    star_integrate::end_chunk();')
        return text
    if name=='ReadAlign_mapOneRead.cpp':
        text='#include "star_integrate.hpp"\n'+text
        text=once(rio,text,'int ReadAlign::mapOneRead() {',
                  'int ReadAlign::mapOneRead() {\n    star_integrate::begin_map(*this);')
        call='                        maxMappableLength2strands(Shift, seedLength, iDir, 0, mapGen.nSA-1, L, splitR[2][ip]);//L=max mappable length, unique or multiple'
        if call in text:
            text=once(rio,text,call,
                      '                        star_integrate::set_chain(ip, splitR[2][ip], istart, Nstart, Lstart, Lmapped, splitR[0][ip], splitR[1][ip]);\n'+call)
        if '                            flagDirMap=false;\n' in text:
            text=once(rio,text,'                            flagDirMap=false;\n',
                      '                            flagDirMap=false;\n                            star_integrate::reverse_suppressed(ip);\n')
        return text
    if name=='Makefile':
        return once(rio,text,'ReadAlign_maxMappableLength2strands.o binarySearch2.o\\\n','ReadAlign_maxMappableLength2strands.o binarySearch2.o star_integrate.o star_integrate_window.o star_integrate_work.o sha256.o\\\n')
    raise ValueError(name)
def main(a):
    rio=oracle(a.tooling); source=a.source.resolve(); root=a.private_root.resolve()
    rio.require(not root.is_relative_to(source),'private root must not be inside source')
    inv=rio.inventory(source); rio.require(len(inv)==344 and rio.digest(rio.encoded(inv))==rio.FULL_INVENTORY_SHA256,'pinned 344-file source inventory mismatch')
    if root.exists(): raise ValueError('private root already exists; refuse overwrite')
    root.mkdir(mode=0o700); shutil.copytree(source,root/'baseline'); shutil.copytree(source,root/'integrated')
    changed=('STAR.cpp','ReadAlignChunk_mapChunk.cpp','ReadAlign_mapOneRead.cpp','ReadAlign_maxMappableLength2strands.cpp','SuffixArrayFuns.cpp','Makefile')
    for n in changed: (root/'integrated'/n).write_text(patch(rio,n,(source/n).read_text()))
    shutil.copy2(HERE/'star_integrate.hpp',root/'integrated'/'star_integrate.hpp')
    shutil.copy2(HERE/'star_integrate.cpp',root/'integrated'/'star_integrate.cpp')
    shutil.copy2(HERE/'star_integrate_window.cpp',root/'integrated'/'star_integrate_window.cpp')
    shutil.copy2(HERE/'star_integrate_work.hpp',root/'integrated'/'star_integrate_work.hpp')
    shutil.copy2(HERE/'star_integrate_work.cpp',root/'integrated'/'star_integrate_work.cpp')
    # The ABI headers must live in the private tree too: STAR's Makefile runs its `-MM`
    # dependency scan without CXXFLAGSextra, so an include path is not enough.
    shutil.copy2(HERE/'usi.h',root/'integrated'/'usi.h')
    shutil.copy2(HERE.parent.parent/'crates'/'umgpu'/'shim'/'seed_probe_abi.h',root/'integrated'/'seed_probe_abi.h')
    usi=(root/'integrated'/'usi.h').read_text()
    (root/'integrated'/'usi.h').write_text(usi.replace('#include "../../crates/umgpu/shim/seed_probe_abi.h"','#include "seed_probe_abi.h"'))
    # Reuse the accepted replay SHA-256 implementation for resident STAR hashes.
    for n in ('sha256.hpp','sha256.cpp'):
        shutil.copy2(a.tooling/n,root/'integrated'/n)
    diff=''.join(''.join(difflib.unified_diff((root/'baseline'/n).read_text().splitlines(True),(root/'integrated'/n).read_text().splitlines(True),fromfile='baseline/'+n,tofile='integrated/'+n)) for n in changed)
    (root/'hook.diff').write_text(diff); (root/'source-inventory.json').write_text(json.dumps(inv,indent=2,sort_keys=True)+'\n')
    (root/'preparation.json').write_text(json.dumps({'status':'PRIVATE_DEFAULT_OFF_STAR_INTEGRATE','source_inventory_sha256':rio.digest(rio.encoded(inv)),'changed':changed,'patch_sha256':rio.digest(diff.encode()),'tooling':str(a.tooling)},indent=2,sort_keys=True)+'\n')
    print(root)
if __name__=='__main__':
    p=argparse.ArgumentParser(); p.add_argument('--source',type=Path,required=True);p.add_argument('--tooling',type=Path,required=True);p.add_argument('--private-root',type=Path,required=True)
    try: main(p.parse_args())
    except Exception as e: print('star-integrate preparation failed: '+str(e),file=sys.stderr);sys.exit(1)
