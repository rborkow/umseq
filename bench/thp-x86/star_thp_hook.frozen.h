#if defined(STAR_THP_PATCH) && defined(__linux__)
#include <sys/mman.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
static void starIntegrateAdviseHuge(char *p, uint64_t n) {
    // STAR_THP=0 disables the advice at run time (ablation arm: same binary,
    // same fadvise, base pages). Page size from the kernel, not a 4096 constant.
    static const int on = !(getenv("STAR_THP") && getenv("STAR_THP")[0]==(char)48);
    if (!on) return;
    const uintptr_t page=(uintptr_t)sysconf(_SC_PAGESIZE), lo=((uintptr_t)p+page-1)&~(page-1), hi=((uintptr_t)p+n)&~(page-1);
    if (hi>lo && madvise((void *)lo,hi-lo,MADV_HUGEPAGE)) perror("STAR_THP madvise(MADV_HUGEPAGE)");
}
#endif
