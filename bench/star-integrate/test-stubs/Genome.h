#ifndef STAR_INTEGRATE_TEST_GENOME_H
#define STAR_INTEGRATE_TEST_GENOME_H
#include <cstdint>
// STAR IncludeDefine.h:50 `#define uint unsigned long long` (a macro, so it also
// overrides the BSD `typedef unsigned int uint` that macOS <sys/types.h> provides).
#ifndef uint
#define uint unsigned long long
#endif
class Genome {
public:
  struct {
    char *charArray;
    uint64_t lengthByte;
    uint64_t wordLength;
  } SA, SAi;
  struct {
    unsigned gSAindexNbases;
    uint64_t gSAsparseD;
  } pGe;
  char *G;
  uint64_t nGenome, nSAbyte, nSA;
  uint *genomeSAindexStart;
  unsigned GstrandBit;
  uint64_t SAiMarkAbsentMaskC, SAiMarkNmask, SAiMarkNmaskC;
  Genome()
      : G(0), nGenome(0), nSAbyte(0), nSA(0), genomeSAindexStart(0),
        GstrandBit(32) {
    SA.charArray = SAi.charArray = 0;
    SA.lengthByte = SAi.lengthByte = 0;
    SA.wordLength = SAi.wordLength = 35;
    pGe.gSAindexNbases = 2;
    pGe.gSAsparseD = 1;
    SAiMarkAbsentMaskC = 0x800000000ULL;
    SAiMarkNmask = ~SAiMarkAbsentMaskC;
    SAiMarkNmaskC = SAiMarkAbsentMaskC;
  }
};
#endif
