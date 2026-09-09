# Six end-to-end threads (authorized 2026-09-08, "cue up all 6")

Starting state: `aa9c2f7`, tree clean. Per-sample cost model (CPU-min): nf-core stock 222.9 →
umbam 148.5 → +STAR advised 140.6 / +STAR GPU 136.9 (projected). Remaining buckets in the
136.9: Salmon 51 (37%), STAR ~55 (40%), trim/FastQC 19 (14%), umbam 12–14.

The GPU seed search is parked: −5.6% robust / −8.1% mean-only after eleven parity rounds,
every remaining lever host-side and marginal.

## Threads, in value order

| # | Card | What decides it | Owner | Host |
|---|---|---|---|---|
| 4 | `P2C-SALMON-ENVELOPE` | Does an identical second run reproduce `quant.sf` byte-for-byte? If not, the run-to-run envelope (p99 rel ΔTPM) is Salmon's own noise floor. | orchestrator | Spark, running |
| 2 | `P2C-STAR-PRODUCTION-GATE` | Integrated STAR under nf-core's real argv (`--twopassMode Basic --quantMode TranscriptomeSAM --outSAMtype BAM Unsorted`, RG line, `--runRNGseed 0`), `cmp` both BAMs + SJ + log vs stock same argv, `gpu_consumed > 0`, strict 0/0/0. Two-pass = second index/junction lifecycle; TranscriptomeSAM = second output stream. | Terra prepares runner+source patch; orchestrator runs | Spark, after #4 |
| 1 | `P2C-THP-X86-PORT` | Stock STAR on one x86 Batch instance: THP `never` vs `madvise`+patch vs `always`, 3 rotated runs each, `cmp` outputs. ≥8% CPU-s → fleet-wide; <3% → GB10 curiosity. | Luna prepares the AMI-side script + patch bundle; user/orchestrator runs on Batch | AWS |
| 3 | `P2C-PIPELINE-RUN` | Re-gated on the advised-bypass path (−12% ablated), not the GPU. Six Tier 2A samples through nf-core with integrated STAR (advised, GPU off) + umbam; CPU-min/sample from trace → cost model measured, not projected. Requires #2 green. | orchestrator | Spark, after #2 |
| 5 | `P2C-SALMON-NEXT` | Only after #4. If nondeterministic: write the statistical-equivalence policy (`crates/umbam/COMPAT.md`-style) before any code. Targets from profile: CAS contention 22.7%, soft-float long double 15.9% (arm64-only — measure on x86 before crediting), inflate 4.6%. | Astra design review first | — |
| 6 | `P2C-TRIM-FASTQC` | Trim Galore/cutadapt (10.4 + 6.7 CPU-min) + FastQC + fq lint (2×2.7). Deterministic, byte-gateable on trimmed FASTQ and FastQC data tables. Rust port in a new `umtrim` crate, Tier-0 fixture golden from the nf-core containers. | Luna scaffolds + goldens; Terra port | Mac then Spark |

## Sequencing

Spark lane (serial, under the lock): #4 now → #2 gate → #3 six samples.
Worker slot A: Terra on #2 (files: `bench/star-integrate/run_production_gate.py`, `test_production_gate.py`, generator/source-patch edits for TranscriptomeSAM + two-pass only if the gate finds them needed).
Worker slot B: Luna on #1 (files: new `bench/thp-x86/` only) then #6 scaffold (new `crates/umtrim/`, `scripts/make_tier0_trim.sh`).
#5 waits on #4's number; Astra review card written then.

## Decision rules

- #4: `cmp` identical → Salmon is byte-gateable, today's diff was a protocol error, find it. Differs → record envelope table, Salmon replacement needs a policy (project decision, user).
- #2: any `cmp` difference in Aligned.out.bam / Aligned.toTranscriptome.out.bam / SJ.out.tab = not done; document, do not normalize. Expect 2–3 host rounds on consumer bookkeeping.
- #1: report CPU-s per arm with the comparator named; ≥8% funds an AMI/launch-template change, <3% closes the thread.
- #3: measured CPU-min/sample replaces the "PROJECTED" rows in `cost-model.json`; saturated-box, trace-derived.
- #6: byte-identical trimmed FASTQ + FastQC `fastqc_data.txt` on the chr22 fixture before any full-depth run.

## What this is not

Not a memo. Not a GPU card. No Salmon code change before #4 and #5's policy. No Batch
infrastructure change before #1's number.
