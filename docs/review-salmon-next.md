I couldn’t save `docs/review-salmon-next.md` because the sandbox is read-only. No files were changed. Review contents follow.

## Verdict

**Do not commission `umquant` yet.** A narrowly scoped Salmon patch is the better first Salmon experiment, conditional on resolving the sampled callers and adopting an explicit equivalence policy. The source establishes fresh random sampling during model training, shared online updates, and order-dependent VBEM accumulation; it does **not** establish that error-model contention accounts for the 22.7% CAS term. Boost’s internal promotion in `digamma` is the leading explanation for quad arithmetic, rather than explicit `long double` in the alignment model. The current envelope supports an initial rejection screen, not a validated statistical-equivalence gate. For an x86 fleet decision, obtain one matching x86 profile before implementation. Meanwhile, trim/FastQC remains the stronger candidate for a byte-gated replacement.

## Answer 1 — What is the CAS?

**The helper is identified; its dominant caller and contention fraction remain unresolved.** The tracked profile contains aggregate self-time, not the caller addresses needed to distinguish several source paths. Its 22.71% cannot honestly be assigned entirely to `AlignmentModel`. [Profile:1](/Users/rborkows/projects/uni-rnaseq/bench/evidence/salmon-alignment-screen/perf-top20-symbolized.txt:1)

There are at least three relevant floating-point CAS paths:

| Path | Source evidence | Implication |
|---|---|---|
| Error-model training | Each CIGAR transition calls `AtomicMatrix<double>::increment`; that updates both a cell and its row sum through `incLoopLog`. | Two log-space CAS loops per transition during training. |
| Online transcript abundance | Every eligible alignment calls `transcript.addMass`, which calls `incLoopLog`. | A competing explanation for an unresolved caller inside alignment-processing threads. |
| Collapsed VBEM | Equivalence classes contribute to shared `alphaOut[tid]` through `incLoop`. | Floating-point addition implemented with CAS, independently of error-model training. |

Sources: [AlignmentModel.cpp:411](/private/tmp/salmon-v1.10.3-screen-source/src/AlignmentModel.cpp:411), [AtomicMatrix.hpp:65](/private/tmp/salmon-v1.10.3-screen-source/include/AtomicMatrix.hpp:65), [SalmonQuantifyAlignments.cpp:606](/private/tmp/salmon-v1.10.3-screen-source/src/SalmonQuantifyAlignments.cpp:606), [Transcript.hpp:210](/private/tmp/salmon-v1.10.3-screen-source/include/Transcript.hpp:210), [CollapsedEMOptimizer.cpp:310](/private/tmp/salmon-v1.10.3-screen-source/src/CollapsedEMOptimizer.cpp:310).

Both increment helpers use `compare_exchange_strong` without explicit memory-order arguments: the source requests sequential consistency. `incLoopLog` recomputes `logAdd` after a failed comparison; `incLoop` recomputes addition. The outlined helper’s `_acq_rel` name does not identify the C++ object or prove the source requested only acquire-release ordering. [SalmonUtils.hpp:165](/private/tmp/salmon-v1.10.3-screen-source/include/SalmonUtils.hpp:165)

Two corrections materially weaken the “16 threads contending on error matrices” explanation:

- For an uncached first pass, `--threads 16` computes **six parser threads and ten quantification workers**, which share `alnLib`. [SalmonQuantifyAlignments.cpp:1767](/private/tmp/salmon-v1.10.3-screen-source/src/SalmonQuantifyAlignments.cpp:1767), [worker construction:1038](/private/tmp/salmon-v1.10.3-screen-source/src/SalmonQuantifyAlignments.cpp:1038)
- Error-model updates occur only before burn-in and only for sampled alignments. Default burn-in is five million fragments; transcript abundance updates continue outside that training condition. [SalmonDefaults.hpp:73](/private/tmp/salmon-v1.10.3-screen-source/include/SalmonDefaults.hpp:73), [training condition:854](/private/tmp/salmon-v1.10.3-screen-source/src/SalmonQuantifyAlignments.cpp:854)

The bench narrative reports approximately two percentage points under `AlignmentModel::update` and one under TBB queue pop, but leaves the principal caller unresolved. The tracked top-20 table cannot independently verify that caller breakdown. `ReaderWriterQueue::try_dequeue` also has its own 4.87% self-time entry; that entry is not an attribution of the CAS samples. [Bench:133](/Users/rborkows/projects/uni-rnaseq/bench/PHASE2D-salmon-thp.md:133), [profile:5](/Users/rborkows/projects/uni-rnaseq/bench/evidence/salmon-alignment-screen/perf-top20-symbolized.txt:5)

**True contention versus uncontended cost: not measured.** Successful atomics, failed comparisons, cache-line ownership transfers, false sharing, and outlined-call overhead can all contribute. GCC’s helper dispatches between LSE and an exclusive-load/store implementation; verify the executed branch rather than infer it from the symbol. [GCC helper implementation](https://gcc.gnu.org/pipermail/gcc-cvs/2020-April/281333.html)

Resolving attribution requires the existing raw profile, matching executable/build identity, caller PCs, and disassembly. Quantifying contention additionally requires attempts/successes by call site and phase, plus controlled worker-count measurements. A CAS failure ratio alone is not a fraction of CPU time lost to contention.

## Answer 2 — Where does `long double` come from?

**There is no explicit `long double` in the named alignment-model, library-format, or Salmon log-sum implementation.**

- Model probabilities, transition matrices, and likelihood accumulators are `double`. [AlignmentModel.hpp:59](/private/tmp/salmon-v1.10.3-screen-source/include/AlignmentModel.hpp:59), [AlignmentModel.cpp:135](/private/tmp/salmon-v1.10.3-screen-source/src/AlignmentModel.cpp:135)
- `LibraryFormat` describes categorical orientation/strandedness. [LibraryFormat.hpp:22](/private/tmp/salmon-v1.10.3-screen-source/include/LibraryFormat.hpp:22)
- The relevant log-sum operation is named `logAdd`; its arguments, intermediate, return type, and elementary functions are double precision. [SalmonMath.hpp:56](/private/tmp/salmon-v1.10.3-screen-source/include/SalmonMath.hpp:56)

The leading hot-path source is **implicit promotion inside Boost.Math**. Every VBEM iteration evaluates `digamma(alphaSum)` and, for eligible transcripts, `digamma(ap)`. Both receive doubles and return into double-valued calculations. [CollapsedEMOptimizer.cpp:250](/private/tmp/salmon-v1.10.3-screen-source/src/CollapsedEMOptimizer.cpp:250)

Boost documents that its default policy promotes double arguments to `long double` internally. The locally installed Boost 1.92 source demonstrates the mechanism: `digamma` selects an evaluation type, evaluates internally, and narrows the result; `evaluation<double, Policy>` selects `long double` when promotion is enabled. This is **supporting mechanism evidence, not proof of the container’s Boost version or configuration**. [Boost promotion policy](https://www.boost.org/doc/libs/1_87_0/libs/math/doc/html/math_toolkit/pol_ref/internal_promotion.html), [digamma.hpp:593](/opt/homebrew/include/boost/math/special_functions/digamma.hpp:593), [policy.hpp:764](/opt/homebrew/include/boost/math/policies/policy.hpp:764)

Other reachable promotion candidates are the normal CDF and binomial PDF used to initialize the fragment-length distribution. These are initialization calls, not evidence that they explain the sustained hot term. The alternative VBEM overload also calls `digamma`; the sampled TBB specialization points to the parallel overload above. [FragmentLengthDistribution.cpp:37](/private/tmp/salmon-v1.10.3-screen-source/src/FragmentLengthDistribution.cpp:37), [CollapsedEMOptimizer.cpp:118](/private/tmp/salmon-v1.10.3-screen-source/src/CollapsedEMOptimizer.cpp:118)

For this Linux AArch64 ABI, `long double` is binary128; ordinary GNU/Linux x86-64 uses x87 extended precision, despite commonly allocating 16 bytes of storage. Thus the observed quad-helper tax is architecture/build-specific and should disappear as that particular implementation cost on conventional x86-64. The mathematical work remains. [Arm ABI](https://github.com/ARM-software/abi-aa/blob/main/aapcs64/aapcs64.rst), [GCC x86 options](https://gcc.gnu.org/onlinedocs/gcc-15.1.0/gcc/x86-Options.html)

**Precision requirement:** the persistent algorithmic state is double, but that does not prove double intermediates reproduce its results. Promotion supplies extra precision before rounding. Disabling it can change VBEM updates, convergence, and outputs. No source-derived error bound establishes equivalence here.

The first precision experiment should target **only the confirmed hot `digamma` calls**, using a local non-promoting policy. A global precision-policy change would also affect initialization and obscure attribution. Confirm the container’s instantiated arithmetic first; the present evidence cannot assign every one of the 15.9 percentage points to `digamma`.

## Answer 3 — Equivalence policy given the envelope

**Keep the byte gate for verified invariant fields; propose a separate, explicitly approved numerical contract.** Nondeterminism does not itself authorize weaker compatibility requirements.

The source also disproves the proposed single-thread escape hatch. Each quantification worker seeds an engine from `random_device`, then randomly selects alignments for model training. Lower thread counts do not remove that randomness; the CLI also clamps requests below two threads. Neither deterministic output nor “16× CPU” follows from requesting one thread. [SalmonQuantifyAlignments.cpp:134](/private/tmp/salmon-v1.10.3-screen-source/src/SalmonQuantifyAlignments.cpp:134), [sampling:854](/private/tmp/salmon-v1.10.3-screen-source/src/SalmonQuantifyAlignments.cpp:854), [thread clamp:1625](/private/tmp/salmon-v1.10.3-screen-source/src/SalmonQuantifyAlignments.cpp:1625)

A useful **provisional rejection screen** is the componentwise maximum of the three recorded reference comparisons:

| Statistic | Transcript threshold | Gene threshold |
|---|---:|---:|
| Median relative ΔTPM | 2.91e−4 | 8.59e−5 |
| p99 relative ΔTPM | 5.82e−2 | 3.75e−2 |
| Maximum relative ΔTPM | 1.31e5 | 58.5 |
| p99 absolute ΔNumReads | 2.57 | 1.90 |
| Maximum absolute ΔNumReads | 3,471 | 3,471 |

These are directly taken from [envelope.tsv:2](/Users/rborkows/projects/uni-rnaseq/bench/evidence/salmon-alignment-screen/envelope.tsv:2). They are observed maxima, **not confidence limits, biological tolerances, or permission to perturb every row by these amounts**.

Before using them, recover the comparison program and freeze its denominator, zero handling, row population, quantile calculation, and rounding. The TSV does not specify these. In particular, the enormous maximum relative transcript change makes the zero/near-zero convention consequential.

The proposed final policy is:

1. **Exact structure:** preserve complete identifier sets, order, schema, and confirmed invariant values; require the recorded processed/mapped counts. Do not require every length column to be invariant: the existing comparison reports length-related differences. [Bench:115](/Users/rborkows/projects/uni-rnaseq/bench/PHASE2D-salmon-thp.md:115)
2. **Repeated comparisons:** compare multiple candidate runs against independent baseline runs and assess candidate–candidate variability. Passing one favorable pairing is insufficient.
3. **Global and local effects:** retain the statistics above, total-count differences, zero transitions, and per-feature signed shifts across replicates. A global p99 can miss consistent damage to a small set of genes. Do not exclude low-abundance rows.
4. **Calibrated acceptance:** collect additional unchanged-reference runs, freeze baseline-derived acceptance limits before inspecting candidate results, and demonstrate that held-out baseline runs satisfy the policy. Predeclare the confidence/error criterion as a project decision.

The two new runs provide only one controlled run-to-run pair; comparisons involving the golden are dependent and are not additional independent replicates. Consequently, this review supplies a numerical **screen**, but cannot honestly certify a production statistical-equivalence gate from the present evidence. Passing the screen means “continue validation.”

## Answer 4 — Replacement, patch, or another target?

The model contains **51.3 CPU-min for Salmon** and **19.1 for trim/FastQC/fq**. Those are projected buckets, distinct from the observed profiled Salmon execution. [cost-model.json:95](/Users/rborkows/projects/uni-rnaseq/bench/fig/cost-model.json:95)

“Odds” below are qualitative engineering judgments about reaching a useful, gated result. There are no measured success probabilities.

| Alternative | Odds and prize | First cheap disqualifier |
|---|---|---|
| **Patch Salmon in place** | **Moderate; strongest Salmon option.** On the requested rounded 51-minute basis, the 15.9% soft-float term represents 8.1 CPU-min of modeled exposure; CAS represents 11.6. These are arithmetic allocations, not predicted savings. | Confirm quad callers and dominant CAS callers before choosing a patch. Reject a precision candidate if it breaches the frozen numerical screen or fails to reduce measured stage CPU beyond baseline variability. |
| **Rust alignment-only `umquant`** | **Low at this stage.** Addresses a 51.3-minute bucket, but that entire bucket is not recoverable. No defensible net saving is established. | Establish an acceptable compatibility contract and a measured advantage from a bounded algorithmic experiment before funding the port. |
| **Leave Salmon; port trim/FastQC** | **Highest relative odds of a byte-gated result.** Zero Salmon saving; a separate 19.1-minute bucket is available, with unknown recoverable fraction. | Generate nonempty goldens and repeat them before implementation; verify report and compressed-output determinism. |

The profile-derived exposures cannot be added to claim an achievable saving: replacement arithmetic, merges, changed iteration counts, and synchronization costs remain. All options presently have **zero demonstrated new saving**.

Thread-local error-model accumulation deserves particular caution. Readers use the live model while workers train it. Periodic merging changes which model later fragments observe, not merely the summation order. Furthermore, error-model training ends at burn-in, whereas online transcript updates and VBEM provide separate CAS sources. A merge design cannot be justified by the aggregate 22.7% alone. [AlignmentModel.cpp:202](/private/tmp/salmon-v1.10.3-screen-source/src/AlignmentModel.cpp:202), [training condition:855](/private/tmp/salmon-v1.10.3-screen-source/src/SalmonQuantifyAlignments.cpp:855)

A Rust port inherits all of those semantics, plus fragment-length learning, equivalence-class construction, optimizer stopping behavior, and output aggregation. Language replacement alone does not remove the profiled algorithmic work.

**Recommendation:** pursue a small Salmon attribution card, then a local `digamma` precision experiment if confirmed and statistically gateable. Keep trim/FastQC ahead of a full Salmon rewrite. Its existing card appropriately makes golden generation and determinism checks prerequisites to the port. [P2C-TRIM-FASTQC:27](/Users/rborkows/projects/uni-rnaseq/.hermes/cards/P2C-TRIM-FASTQC.md:27)

## Answer 5 — What would one x86 profile settle?

A representative x86-64 Linux run should use the same inputs, Salmon version, invocation, and thread budget, while recording the architecture-specific image digest, compiler/dependency provenance, whole-stage CPU/wall measurements, and symbolized stacks.

It would settle whether:

- The quad-helper term disappears as expected and how much native `digamma` work remains.
- CAS, likelihood evaluation, VBEM, parsing, or decompression dominates on the intended fleet.
- A proposed patch targets meaningful x86 work or primarily a GB10-specific expense.

It would **not** establish a contention fraction, numerical equivalence, fleet-wide throughput, or repeatable savings. Architecture-specific reference envelopes remain necessary; do not assume cross-architecture output distributions match.

**Yes: the x86 profile should precede Salmon implementation for an x86 fleet decision.** It need not precede an explicitly GB10-only experiment. Do not subtract 15.9% from an x86 cost estimate: the existing profile and timing belong to the arm64 container. [Protocol:2](/Users/rborkows/projects/uni-rnaseq/bench/evidence/salmon-alignment-screen/protocol.json:2)

## Blocking unknowns

- Dominant unresolved CAS caller, executed atomic implementation, and phase-specific attribution.
- Container-matching Boost headers, policies, patches, build flags, and quad call chains. The verified upstream checkout is commit `a2f6912b3f9f9af91e3a4b0d74adcb3bdc4c9a32`; upstream source alone does not reproduce the container. [Bench:93](/Users/rborkows/projects/uni-rnaseq/bench/PHASE2D-salmon-thp.md:93)
- Exact envelope-computation semantics, additional independent reference runs, and project acceptance of statistical compatibility.
- Representative x86 profile and the intended deployment architecture.
- Controlled CPU/throughput evidence: the two reported timings are uncontrolled, perf-attached diagnostics. No saving should enter the cost model. [Bench:152](/Users/rborkows/projects/uni-rnaseq/bench/PHASE2D-salmon-thp.md:152)

## Suggested first card with its decision rule

**P2C-SALMON-ATTRIBUTION — resolve callers and select the experiment; no quantification changes.**

Recover the existing Spark profile and matching binary provenance. Attribute CAS and quad samples by caller and execution phase, retaining unresolved samples explicitly. Verify the container’s Boost promotion policy and the envelope comparator. If x86 is the deployment target, collect one matching x86 baseline profile before selecting implementation work.

Preserve evidence summaries under a named `bench/evidence/` directory and record conclusions in a bench document. Use the Spark lock and bounded private-lab execution for any new run.

**Decision rule:**

- If the quad term resolves to promoted VBEM `digamma`, and the numerical contract is approved, dispatch one local non-promoting-policy experiment.
- If CAS resolves mainly to transcript abundance, VBEM, or queues, reject the proposed **error-matrix merge** hypothesis.
- If error-matrix CAS is substantial, require contention evidence before funding thread-local accumulation.
- If attribution or equivalence remains unresolved, keep Salmon unchanged and proceed with trim/FastQC golden generation.
- Advance a later candidate only after it passes the frozen numerical screen and repeated controlled measurements show lower stage CPU. Statistical qualification and saturated-box cost validation remain required before adoption.
