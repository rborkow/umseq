# P2C-SALMON-NEXT — Salmon as the next target (Astra design review; gated on P2C-SALMON-ENVELOPE)

Model: gpt-6-astra. Sandbox: read-only. Output: `docs/review-salmon-next.md` only. No code.
**Do not dispatch until P2C-SALMON-ENVELOPE has its verdict; paste that verdict and the
envelope table into this card's Context before dispatch.**

## Context (orchestrator fills in envelope result here)

<ENVELOPE VERDICT + TABLE>

Salmon `quant` (alignment mode, 1.10.3, 16 threads) is 51 CPU-min of the projected 137 per
sample — the largest untouched bucket now that STAR is measured. Profile of the real nf-core
invocation on the GB10 (`bench/PHASE2D-salmon-thp.md` §Result, `bench/evidence/
salmon-alignment-screen/perf-top20-symbolized.txt`): self-time `salmon` 74.6%, `libgcc_s`
15.9% (soft-float `__multf3/__divtf3/__addtf3` — 128-bit `long double` on aarch64), libm 5.2%,
kernel 1.5%. Top symbols: `__aarch64_cas8_acq_rel` 22.7% (mostly under one unresolved caller
in the alignment-processing threads; 2% `AlignmentModel::update`), `AlignmentModel::
logLikelihood` 18.8%, VBEM `start_for` 7.4%, `ReaderWriterQueue::try_dequeue` 4.9%,
`inflate_fast` 4.6%, `bam_get_seq` 2.5%. Zero huge-page term; the THP hypothesis is closed.

Upstream source at tag v1.10.3 is at `/private/tmp/salmon-v1.10.3-screen-source` (Mac);
`src/SalmonQuantifyAlignments.cpp`, `include/AlignmentModel.hpp`, `src/AlignmentModel.cpp`,
`include/AtomicMatrix.hpp`, `src/CollapsedEMOptimizer.cpp`, `src/SalmonUtils.cpp`.

## Questions to answer, with file:line citations

1. **What is the CAS?** Resolve the 22.7%: is it `AtomicMatrix<double>` compare-exchange
   loops in `AlignmentModel::update`'s inlined callers, `std::atomic<double>` fetch-add
   emulation in the EM, or the TBB queue? Read the source and say which, and what fraction of
   it is true contention (16 threads on shared error-model matrices) vs uncontended atomic
   RMW cost on aarch64 (LSE `cas` is not free even uncontended).
2. **Where does `long double` come from?** Find every `long double` on the hot path (grep
   `AlignmentModel`, `LibraryFormat`, `SalmonMath`, `logSumExp`). On x86-64 it is 80-bit x87
   hardware; on aarch64 it is soft-float quad. State whether the 15.9% is an arm64-only tax
   and what precision the algorithm actually needs — and whether changing it would change
   outputs (it will: this is exactly why the envelope matters).
3. **Given the envelope**: if Salmon is nondeterministic, propose the equivalence policy a
   replacement would be gated on — what statistic, what threshold, derived from the measured
   envelope, not invented. If deterministic, say what the byte gate is.
4. **Is a replacement even the right shape?** Alternatives: (a) patch Salmon in place
   (thread-local model accumulation with periodic merge; `double` for `long double`) and gate
   against the envelope; (b) Rust port of alignment-mode quant only (`umquant`), source-as-spec,
   like umbam; (c) leave Salmon, port trim/FastQC instead (P2C-TRIM-FASTQC, 19 CPU-min,
   deterministic). Give odds and CPU-min prize for each, against the 51 CPU-min bucket, and
   what the first cheap disqualifier is for the one you recommend.
5. **x86 caveat**: the aarch64 profile may misdirect an x86 fleet decision (soft-float term
   vanishes). Say what one x86 profile would settle and whether it should precede any code.

Output sections: Verdict (one paragraph) / Answers 1–5 with citations / Blocking unknowns /
Suggested first card with its decision rule. No implementation.
