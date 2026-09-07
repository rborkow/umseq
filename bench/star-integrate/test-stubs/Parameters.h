#ifndef STAR_INTEGRATE_TEST_PARAMETERS_H
#define STAR_INTEGRATE_TEST_PARAMETERS_H
#include <string>
class Parameters { public: struct { std::string gLoad,gDir; int gSAsparseD; unsigned gSAindexNbases; struct { int type; } transform; } pGe; struct { bool yes; } twoPass,sjdbInsert,wasp,peOverlap; int outFilterBySJoutStage,runThreadN; unsigned seedSearchLmax; Parameters():outFilterBySJoutStage(0),runThreadN(1),seedSearchLmax(0) { pGe.gSAsparseD=1; pGe.transform.type=0; } };
#endif
