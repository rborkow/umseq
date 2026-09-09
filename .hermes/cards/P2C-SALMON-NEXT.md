# P2C-SALMON-NEXT — Salmon as the next target (Astra design review; gated on P2C-SALMON-ENVELOPE)

Model: gpt-6-astra. Sandbox: read-only. Output: `docs/review-salmon-next.md` only. No code.
Envelope verdict is in; this card is dispatchable.

## Context (orchestrator fills in envelope result here)

**ENVELOPE VERDICT (measured 2026-09-08, P2C-SALMON-ENVELOPE): NOT deterministic.** Full section from `bench/PHASE2D-salmon-thp.md`:

## Envelope (run 2, 2026-09-08): Salmon alignment-quant is NOT run-to-run deterministic

Card `P2C-SALMON-ENVELOPE`. Identical second run of the same image digest, argv, inputs
and 16 threads (runner copy differs only in output root, container name, and the perf-stop
wait). Target exit 0; **507.39 wall / 4537.08 user / 66.75 sys s** (run 1: 535.35 / 4857.79 /
79.23 — −6.6% CPU-s run to run, uncontrolled, perf attached both times). `num_processed =
num_mapped = 69,176,421` again.

Three-way comparison, `bench/evidence/salmon-alignment-screen/envelope.tsv`:

| file | pair | rows differing (TPM / NumReads) | rel ΔTPM median / p99 / max | abs ΔNumReads p99 / max |
|---|---|---:|---|---|
| quant.sf | run2 vs run1 | 134,886 / 105,512 | 2.7e-4 / **5.5e-2** / 1.3e5 | 2.41 / 2,409 |
| quant.sf | run2 vs golden | 136,372 / 105,775 | 2.9e-4 / 5.0e-2 / 8.2 | 2.24 / 2,568 |
| quant.sf | run1 vs golden | 137,860 / 104,893 | 2.8e-4 / 5.8e-2 / 7.9 | 2.57 / 3,471 |
| quant.genes.sf | run2 vs run1 | 26,819 / 10,421 | 4.6e-5 / **3.8e-2** / 12.5 | 1.83 / 2,409 |
| quant.genes.sf | run2 vs golden | 27,871 / 10,393 | 8.6e-5 / 3.7e-2 / 58.5 | 1.90 / 1,514 |
| quant.genes.sf | run1 vs golden | 28,797 / 10,425 | 6.3e-5 / 3.6e-2 / 55.5 | 1.85 / 3,471 |

**Verdict:** the run-to-run pair has the same shape as either run vs the golden — ~27% of
transcript rows and ~35% of gene rows move, p99 relative TPM change ≈ 5% (transcript) /
4% (gene), with individual low-abundance genes moving 10–60×. The golden is one draw from
this distribution, not a fixed point. The hypothesis "multithreaded online-EM
nondeterminism" survives its disqualifier; the exact mechanism (thread-ordered atomic updates
in `AlignmentModel`, minibatch scheduling, VBEM) is for the design review, not asserted here.

**Consequences:** (1) Salmon has no byte-identical gate and cannot have one without a
single-threaded reference run (not funded; 16× the CPU). (2) Any Salmon replacement or
in-place patch is gated on a statistical-equivalence policy derived from this envelope — a
project decision. (3) The team's per-sample counts today carry this jitter; a DESeq2-level
question of whether it matters is outside this project but worth telling them. n=2 is the
minimum; the envelope numbers above are a first estimate, not a distribution.


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
