#ifndef STAR_INTEGRATE_TEST_GENOME_H
#define STAR_INTEGRATE_TEST_GENOME_H
#include <cstdint>
class Genome { public: struct { char *charArray; uint64_t lengthByte; } SA,SAi; char *G; uint64_t nGenome,nSAbyte,nSA; unsigned GstrandBit; Genome():G(0),nGenome(0),nSAbyte(0),nSA(0),GstrandBit(32) { SA.charArray=SAi.charArray=0; SA.lengthByte=SAi.lengthByte=0; } };
#endif
