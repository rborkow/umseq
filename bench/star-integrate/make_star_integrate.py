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
        text=once(rio,text,'    genomeMain.genomeLoad();\n','    genomeMain.genomeLoad();\n    star_integrate::setup(P, genomeMain);\n')
        return once(rio,text,'    delete P.inOut; // to close files\n\n    return 0;','    delete P.inOut; // to close files\n\n    star_integrate::finish();\n    star_integrate_work::finish();\n    return 0;')
    if name=='ReadAlign_maxMappableLength2strands.cpp':
        # The hook is deliberately at the original inner call, after STAR's prefix
        # preparation.  The default macro is a no-op and preserves stock control flow.
        text='#include "star_integrate.hpp"\n#include "star_integrate_work.hpp"\n#include <cstdio>\n'+text
        old='            Nrep = maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL, indStartEnd);'
        new='''            if (star_integrate::enabled_fast()) {
                uint starIntegrateLIn=maxL;
                star_integrate::InnerCall starIntegrateCall={};
                starIntegrateCall.start=pieceStart; starIntegrateCall.length=pieceLength;
                starIntegrateCall.low=iSA1 & mapGen.SAiMarkNmask; starIntegrateCall.high=iSA2;
                starIntegrateCall.dir=dirR; starIntegrateCall.prefix=maxL; starIntegrateCall.distance=iDist;
                starIntegrateCall=star_integrate::build_current_inner_call(starIntegrateCall);
                uint64_t starIntegrateRange[2]={0,0}, starIntegrateNrep=0, starIntegrateMaxL=maxL;
                bool starIntegrateHit=star_integrate::lookup(P, mapGen, Read1, (uint64_t) Lread, starIntegrateCall, starIntegrateRange, starIntegrateNrep, starIntegrateMaxL);
                if (starIntegrateHit) { indStartEnd[0]=starIntegrateRange[0]; indStartEnd[1]=starIntegrateRange[1]; Nrep=starIntegrateNrep; maxL=starIntegrateMaxL; }
                star_integrate_work::inner_call(starIntegrateHit);
                if (starIntegrateHit) {
                    if (star_integrate::strict()) {
                        uint cpuRange[2], cpuL=starIntegrateLIn;
                        star_integrate_work::Scope starIntegrateOracle=star_integrate_work::oracle_scope();
                        star_integrate_work::compare_begin();
                        uint cpuN=maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, cpuL, cpuRange);
                        star_integrate_work::compared(cpuL>=starIntegrateLIn ? cpuL-starIntegrateLIn : 0);
                        if (cpuN!=Nrep || cpuL!=maxL || cpuRange[0]!=indStartEnd[0] || cpuRange[1]!=indStartEnd[1]) {
                            fprintf(stderr, "STAR_INTEGRATE strict mismatch piece=%llu fragment=%llu distance=%llu start=%llu length=%llu low=%llu high=%llu dir=%llu gpu=(%llu,%llu,%llu,%llu) cpu=(%llu,%llu,%llu,%llu)\\n", (unsigned long long) pieceStart, (unsigned long long) iFrag, (unsigned long long) iDist, (unsigned long long) pieceStart, (unsigned long long) pieceLength, (unsigned long long) (iSA1 & mapGen.SAiMarkNmask), (unsigned long long) iSA2, (unsigned long long) dirR, (unsigned long long) Nrep, (unsigned long long) maxL, (unsigned long long) indStartEnd[0], (unsigned long long) indStartEnd[1], (unsigned long long) cpuN, (unsigned long long) cpuL, (unsigned long long) cpuRange[0], (unsigned long long) cpuRange[1]);
                            abort();
                        }
                    }
                } else {
                    star_integrate_work::Scope starIntegrateFallback=star_integrate_work::fallback_scope();
                    star_integrate_work::compare_begin();
                    star_integrate::note_cpu_fallback();
                    Nrep = maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL, indStartEnd);
                    star_integrate_work::compared(maxL>=starIntegrateLIn ? maxL-starIntegrateLIn : 0);
                }
            } else {
                Nrep = maxMappableLength(mapGen, Read1, pieceStart, pieceLength, iSA1 & mapGen.SAiMarkNmask, iSA2, dirR, maxL, indStartEnd);
            }'''
        return once(rio,text,old,new)
    if name=='SuffixArrayFuns.cpp':
        prefix,tail=text.split('\nuint findMultRange(',1)
        return prefix+'\nuint findMultRange('+tail
    if name=='ReadAlignChunk_mapChunk.cpp':
        text='#include "star_integrate.hpp"\n'+text
        text=once(rio,text,'        readStatus=RA->oneRead(); //map one read',
                  '        star_integrate::prepare_window(*this);\n        readStatus=RA->oneRead(); //map one read')
        if '    }; //reads cycle' in text:
            text=once(rio,text,'    }; //reads cycle',
                      '    }; //reads cycle\n\n    star_integrate::end_chunk();')
        return text
    if name=='ReadAlign_oneRead.cpp':
        text='#include "star_integrate.hpp"\n'+text
        old='''    if (P.readNmates==2) {//combine two mates together
        Lread=readLength[0]+readLength[1]+1;
        readLengthPairOriginal=readLengthOriginal[0]+readLengthOriginal[1]+1;
        if (Lread>DEF_readSeqLengthMax) {
            ostringstream errOut;
            errOut << "EXITING because of FATAL ERROR in reads input: Lread of the pair = " << Lread << "   while DEF_readSeqLengthMax=" << DEF_readSeqLengthMax <<endl;
            errOut << "Read Name="<<readNameMates[0]<<endl;
            errOut << "SOLUTION: increase DEF_readSeqLengthMax in IncludeDefine.h and re-compile STAR"<<endl<<flush;
            exitWithError(errOut.str(),std::cerr, P.inOut->logMain, EXIT_CODE_INPUT_FILES, P);
        };

        //marker for spacer base
        Read1[0][readLength[0]]=MARK_FRAG_SPACER_BASE;

        //copy 2nd mate into Read1[0] & reverse-complement
        complementSeqNumbers(Read1[1],Read1[0]+readLength[0]+1,readLength[1]);//complement. Here Read1[1] is still the 2nd mate's numeric-sequence. Later Read1[1] will be reverse complement of the combined read.
        for (uint ii=0;ii<readLength[1]/2;ii++) {
            swap(Read1[0][Lread-ii-1],Read1[0][ii+readLength[0]+1]); //reverse
        };

    } else {//1 mate

        if (readStatus[0]==-1) {//finished with the stream
            return -1;
        };

        Lread=readLength[0];
        readLengthPairOriginal=readLengthOriginal[0];
        readLength[1]=0;

    };

    readFileType=readStatus[0];

    complementSeqNumbers(Read1[0],Read1[1],Lread); //returns complement of Reads[ii]
    for (uint ii=0;ii<Lread;ii++) {//reverse
        Read1[2][Lread-ii-1]=Read1[1][ii];
    };
'''
        old=old.replace('Read1[0][readLength[0]]=MARK_FRAG_SPACER_BASE;\n\n        //copy', 'Read1[0][readLength[0]]=MARK_FRAG_SPACER_BASE;\n        \n        //copy')
        old=old.replace('    };\n\n    readFileType', '    };\n      \n    readFileType')
        new='''    if (P.readNmates==2) {//combine two mates together
        Lread=readLength[0]+readLength[1]+1;
        readLengthPairOriginal=readLengthOriginal[0]+readLengthOriginal[1]+1;
        if (Lread>DEF_readSeqLengthMax) {
            ostringstream errOut;
            errOut << "EXITING because of FATAL ERROR in reads input: Lread of the pair = " << Lread << "   while DEF_readSeqLengthMax=" << DEF_readSeqLengthMax <<endl;
            errOut << "Read Name="<<readNameMates[0]<<endl;
            errOut << "SOLUTION: increase DEF_readSeqLengthMax in IncludeDefine.h and re-compile STAR"<<endl<<flush;
            exitWithError(errOut.str(),std::cerr, P.inOut->logMain, EXIT_CODE_INPUT_FILES, P);
        };
    } else {//1 mate
        if (readStatus[0]==-1) {//finished with the stream
            return -1;
        };
        Lread=readLength[0];
        readLengthPairOriginal=readLengthOriginal[0];
        readLength[1]=0;
    };

    // readLoad intentionally remains above: it owns stream position, names,
    // qualities, lengths and clipping.  Its pinned implementation converts
    // SeqNum before returning, so this hook replaces only the downstream
    // combine/complement/reverse work with the lookahead's post-clip bytes.
    bool starIntegrateRead1=star_integrate::handoff_read1(*this);
    if (!starIntegrateRead1) {
        if (P.readNmates==2) {
            Read1[0][readLength[0]]=MARK_FRAG_SPACER_BASE;
            complementSeqNumbers(Read1[1],Read1[0]+readLength[0]+1,readLength[1]);
            for (uint ii=0;ii<readLength[1]/2;ii++) {
                swap(Read1[0][Lread-ii-1],Read1[0][ii+readLength[0]+1]);
            };
        };
        complementSeqNumbers(Read1[0],Read1[1],Lread);
        for (uint ii=0;ii<Lread;ii++) {
            Read1[2][Lread-ii-1]=Read1[1][ii];
        };
    };

    readFileType=readStatus[0];
'''
        return once(rio,text,old,new)
    if name=='ReadAlign_mapOneRead.cpp':
        text='#include "star_integrate.hpp"\n'+text
        text=once(rio,text,'int ReadAlign::mapOneRead() {',
                  'int ReadAlign::mapOneRead() {\n    star_integrate::begin_map(*this);')
        call='                        maxMappableLength2strands(Shift, seedLength, iDir, 0, mapGen.nSA-1, L, splitR[2][ip]);//L=max mappable length, unique or multiple'
        if call in text:
            text=once(rio,text,call,
                      '                        if (star_integrate::enabled_fast()) star_integrate::set_chain(ip, splitR[2][ip], istart, Nstart, Lstart, Lmapped, splitR[0][ip], splitR[1][ip], Nsplit);\n'+call)
        if '                            flagDirMap=false;\n' in text:
            text=once(rio,text,'                            flagDirMap=false;\n',
                      '                            flagDirMap=false;\n                            if (star_integrate::enabled_fast()) star_integrate::reverse_suppressed(ip);\n')
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
    changed=('STAR.cpp','ReadAlignChunk_mapChunk.cpp','ReadAlign_mapOneRead.cpp','ReadAlign_oneRead.cpp','ReadAlign_maxMappableLength2strands.cpp','SuffixArrayFuns.cpp','Makefile')
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
