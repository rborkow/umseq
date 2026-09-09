# P2C-SALMON-ENVELOPE — is Salmon 1.10.3 alignment-quant run-to-run deterministic?

Owner: orchestrator (Spark, under the lock). Status: RUNNING since 2026-09-08 ~17:31 PDT.

## Why

Screen run 1 (`bench/PHASE2D-salmon-thp.md` §Result) produced `quant.sf`/`quant.genes.sf`
that differ from the nf-core golden at line 2 with identical names/order/counts. Hypothesis:
multithreaded online-EM nondeterminism. Disqualifier: a second identical run. Nothing about
Salmon can be planned until this is settled — byte-gateable or not is a project-shaping fact.

## Protocol

Copy of the accepted run-1 runner, `~/uni-rnaseq-probe-lab/salmon-envelope-runner-20260908/`
(sha256 `6a015236…` of `salmon_alignment_screen.py`), changed only in: output root
`salmon-envelope-r2-20260908`, container name `uni-rnaseq-salmon-envelope-r2`, and the
perf-stop wait (30 s → 600 s; run 1's runner timed out waiting for `perf` to flush 114 MB
and reported exit 1 despite a good target exit 0 — the runner bug, not the run). Same image
digest, same argv, same read-only inputs, same 16 threads.

## Acceptance / decision

- `cmp` run-2 `quant.sf` vs run-1 `quant.sf` AND vs golden. Three-way.
- **Identical to run 1 but not golden** → deterministic; today's diff is a protocol/input
  difference vs nf-core — find it (container env? `--numBootstraps`? thread count? BAM
  identity?) before any Salmon work.
- **Differs from both** → nondeterministic. Record the run-1 vs run-2 envelope table (rows
  differing, median/p99/max rel ΔTPM, ΔNumReads, per-gene worst cases) in
  `bench/PHASE2D-salmon-thp.md` and `bench/evidence/salmon-alignment-screen/envelope.tsv`.
  Consequence: any Salmon replacement needs a statistical-equivalence policy (user decision)
  — P2C-SALMON-NEXT is gated on it.
- **Identical to golden** → run 1 was wrong somehow; investigate run 1's artifacts.

Also report: in-container time (wall/user/sys) run 1 vs run 2, as a first repeatability
number for the Salmon bucket (not a controlled timing).
