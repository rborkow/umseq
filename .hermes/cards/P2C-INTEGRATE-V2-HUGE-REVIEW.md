# P2C-INTEGRATE-V2-HUGE-REVIEW — adversarial review of the round-7 huge-page result

You are reviewing a measurement, not writing code. Output a review document only.

## The claim under review

`bench/PHASE2C-integrate-1.md`, section "Round 7". Read it first, then the raw rows in
`bench/evidence/integrate-1-host/timing-round7-raw.tsv` and the stats in
`bench/evidence/integrate-1-host/timing-round7-r1-gpu-stats.jsonl`.

The claim: **a `madvise(MADV_HUGEPAGE)` call after each `new char[]` in STAR 2.7.11b's
`Genome_genomeLoad.cpp` / `PackedArray.cpp` (before the file read populates the array)
reduces STAR's total CPU-seconds on a 20M-read paired-end sample by 12.3%** (three rotated
repeats: 725.20/27.09, 722.15/35.20, 721.91/32.16 user/sys stock vs 643.75/18.43,
643.69/17.36, 641.55/20.92 with the patch; 20 threads; GB10/aarch64, 121 GB, THP
`[madvise]`). The proposed mechanism: STAR's seed search is TLB-bound on a 30 GB
genome/SA/SAindex working set at 4 KB pages, and 2 MB pages fix it.

The GPU seed-search arm on top of that baseline measured a further −1.1% CPU-s (user −7.8%,
sys +43 s). That number is *not* under review here except where your findings about the
baseline change how it must be read.

## What the patch actually is

The generator `bench/star-integrate/make_star_integrate.py` (lines ~19–25) injects, under
`#if defined(STAR_INTEGRATE) && defined(__linux__)`:

```
static void starIntegrateAdviseHuge(char *p, uint64_t n) {
    const uintptr_t page=4096, lo=((uintptr_t)p+page-1)&~(page-1), hi=((uintptr_t)p+n)&~(page-1);
    if (hi>lo) madvise((void *)lo,hi-lo,MADV_HUGEPAGE);
}
```
called immediately after `charArray=new char[lengthByte];` in `PackedArray::allocateArray`
and after each `G1=new char[…]` in `Genome_genomeLoad.cpp`, and
`posix_fadvise(fd,0,0,POSIX_FADV_DONTNEED)` on each index file after its read.
STAR source for reference: `docs/tool-src/` has excerpts; the full pinned source is not in
the repo, cite by file/function name from your knowledge of 2.7.11b and say when you are
citing from memory.

The "bypass" arm is this generated binary run with `STAR_INTEGRATE` **unset** — the T5A
verbatim-stock seed loop (`docs`/bench round 5a–5b: measured +1.8% vs stock before the
madvise was added), so the −12.3% is (madvise effect) + (−1.8% hook floor) ≈ −14% attributable
to the pages if the floor is unchanged.

## Questions to answer, with evidence or a stated way to get it

1. **Is the mechanism plausible at this magnitude?** STAR's `maxMappableLength2strands` /
   `compareSeqToGenome` do random 8-byte reads into a 24 GB packed SA and byte reads into a
   3 GB genome per seed step. Estimate the TLB-miss cost per seed step at 4 KB vs 2 MB pages
   on an ARM Neoverse-class core with a ~2k-entry L2 TLB (GB10 is Grace-derived; if you know
   the Cortex-X925/Neoverse V3 TLB geometry, use it and cite). Does a 12–14% total-CPU
   reduction fall out of a page-walk-per-gather model against round 7's measured
   ~1.28 G gathers per 20M reads (`compared_bytes_hit`/`gathers_hit` in the stats file)?
   Show the arithmetic.
2. **Is there prior art?** Has anyone reported THP/hugepage effects on STAR (or BWA/bowtie
   FM-index tools, which have the same random-access pattern)? Cite what you know; say
   "none known" if none.
3. **Confounds in the measurement.** The three arms ran rotated (stock/bypass/gpu,
   gpu/stock/bypass, bypass/gpu/stock) under one `flock`, same input, same box. Check:
   (a) stock's own index arrays — does stock STAR 2.7.11b get THP by default under
   `[madvise]` policy? (It should not: `new char[]` → malloc → mmap without advice.) Is
   there any path by which the *stock* arm could have been silently huge-paged (e.g. glibc
   `MALLOC_HUGETLB`/`glibc.malloc.hugetlb` tunable set in the environment)? Say what to
   check on the box (`/sys/kernel/mm/transparent_hugepage/enabled`, `GLIBC_TUNABLES`,
   `AnonHugePages` in the stock process's smaps) — the orchestrator will run it.
   (b) `posix_fadvise(DONTNEED)` evicts the index from page cache after load in the patched
   arms; the *following* arm re-reads 30 GB from disk. Could this move CPU-s (not just wall)
   between arms, and in which direction? The stock rows show sys 27→35→32 across repeats.
   (c) Anything about `--genomeLoad NoSharedMemory` vs shared-memory modes that changes the
   allocation path and makes the patch inapplicable to how people run STAR.
4. **Portability of the claim.** On x86-64 (typical cluster nodes, the team's AWS Batch
   `c6i`/`r6i`-class instances): THP policy is commonly `[always]` on Amazon Linux 2 / AL2023
   (verify from your knowledge and say how sure you are), in which case stock STAR may
   *already* be getting huge pages there and the patch is a no-op on Batch — which would
   make this a **workstation-specific** result, not a general STAR speedup. That
   distinction goes in the memo either way. What is the cheapest experiment on an x86 box
   that settles it (e.g. `perf stat -e dTLB-load-misses` on stock STAR, or reading
   `AnonHugePages` from `/proc/<pid>/smaps_rollup` during a stock run)?
5. **What would falsify the 12.3%?** Name the single cheapest measurement. (Candidates:
   stock binary with `THP=always` via `prctl(PR_SET_THP_DISABLE)` inversion or a
   `LD_PRELOAD` that advises every large mmap; or the patched binary with the madvise line
   compiled out but `fadvise` left in, to separate the two changes. The second is what the
   bench doc's "Open" section asks for — say whether you agree it is the right split.)
6. **Reading the GPU arm against the corrected baseline.** If your answer to (3)/(4) changes
   the baseline, say what the GPU's −1.1% becomes and whether the +43 s sys in the GPU arm
   is consistent with page-table work from ATS access to 2 MB host pages (GB10's GPU walks
   the CPU's page tables; does it benefit from 2 MB PTEs the same way, or does it thrash a
   separate translation cache?). Cite the NVIDIA HMM/ATS documentation for Grace-Hopper /
   GB10 where you can.

## Output

Write `docs/review-integrate-v2-huge.md` with sections: **Verdict** (one paragraph:
credible / credible with caveats / not yet credible, and why); **Blocking** (anything that
must be measured before the number goes in a memo); **Should fix**; **Answers** (numbered 1–6
above, with arithmetic and citations, memory-cited claims flagged); **Suggested
measurements** (ordered by cost, each with what result would mean). No code changes, no
edits to any other file. Be adversarial: the orchestrator wants this number to be true,
which is exactly why you were asked.
