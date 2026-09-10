# Production-argv STAR gate (two-pass, TranscriptomeSAM, BAM Unsorted)

Card `P2C-STAR-PRODUCTION-GATE`; runner `bench/star-integrate/run_production_gate.py`;
argv template `bench/star-integrate/nfcore_star_argv.json` (nf-core's real invocation from
`runs/tier1-full`, 16 threads, `--runRNGseed 0`). Design and the three lifecycle hypotheses:
`docs/design-production-gate.md`.

## Step 1 — stock determinism under this argv (`--stock-twice`, 2026-09-08)

Two consecutive stock STAR 2.7.11b runs, same binary/argv/inputs (ERR188140 20M-pair slice),
`~/uni-rnaseq-probe-lab/production-stock-twice-20260908`. Evidence:
`bench/evidence/production-gate/stock-twice-{argv.json,time.tsv}`.

| run | wall s | user s | sys s | max RSS kB |
|---|---:|---:|---:|---:|
| stock-1 | 456.88 | 1826.69 | 42.56 | 40,067,556 |
| stock-2 | 440.40 | 1832.01 | 34.31 | 40,067,340 |

Result: **`cmp Aligned.out.bam` FAILS between two stock runs.** `SJ.out.tab` and
`_STARpass1/SJ.out.tab` are byte-identical. BAM headers are identical once the `@PG`/`@CO`
lines (which embed the output path) are dropped. Record bodies (`samtools view`, no header)
differ as emitted but are **identical after a full-line `LC_ALL=C sort`** for
`Aligned.out.bam` (md5 `70c631201961` both runs). So the difference is thread-chunk output
order under `--outSAMtype BAM Unsorted` with the default `--outSAMorder`, not alignment
content.

`Aligned.toTranscriptome.out.bam` differs even after a whole-line sort (`811ca3d4727e` vs
`b332501565e2`; the earlier "0 diff lines" reading in this session was wrong — `diff` had
been killed at 500M lines). `cmp` of the sorted SAMs finds line 189: same read, same 22
records, same transcripts/positions/CIGAR/NH — **only the primary flag (0x100) moves
between two of the eleven equivalent pairs, and `HI:i` renumbers.** On a 2M-line slice every
differing record is a primary↔secondary swap (77,648 each way, 19,412 reads). Source:
`ReadAlign_quantTranscriptome.cpp:69`,
`alignT[int(rngUniformReal0to1(rngMultOrder)*nAlignT)].primaryFlag=true;` — the primary
transcriptome alignment of a multimapper is chosen with a per-thread RNG, so it depends on
which thread processed the read. With `HI:i` stripped and 0x100 masked, both runs are
identical after sort (md5 `65136fb0ef1e` both, full file). Every other field is compared
exactly.

Consequence: the production gate's comparator for the two BAMs is the documented
normalization — headers removed, records sorted — with `SJ.out.tab`, pass-1 `SJ.out.tab`,
and non-timing `Log.final.out` fields kept byte-exact. This is the same class of
normalization AGENTS.md permits for umbam (documented, exact, applied identically to both
arms); it is not "identical" in the byte sense and is labelled as such in every result.

Comparator as implemented (`run_production_gate.py::normalize_bam`, `--compare
namesorted-sam`): both BAMs header-dropped and whole-line sorted (a stable name-only sort
was measured insufficient); transcriptome BAM additionally `HI` stripped and 0x100 cleared;
SJ, pass-1 SJ and non-timing `Log.final.out` byte-exact.

## Step 2 — integrated binary `ef22723c…` under this argv (2026-09-08): NOT ENGAGED

`~/uni-rnaseq-probe-lab/production-gate-integrated-20260908`. Both stages exit 0. Sidecar:
`mode: cpu-bypass`, `submitted: 0`, `gpu_consumed: 0`, `setup_wall_s: 8e-07`. The runner
rejected the run on `gpu_consumed == 0`, as designed. Cause, from source:
`star_integrate.cpp:511-517`, `admitted()` returns false when `p.twoPass.yes ||
p.sjdbInsert.yes` — nf-core's argv sets both (`--twopassMode Basic`, `--sjdbGTFfile`). The
integration never installs, and the binary runs as stock. There is therefore no stale-index
failure to observe (design-doc hypothesis 1 was right about the hazard and the guard that
exists for it is what fired). Every STAR measurement in this project so far — parity rounds,
the −11.9% madvise ablation, the GPU arms, the cost-model buckets — was taken under the
one-pass, no-`sjdbInsert` invocation that `admitted()` permits, which is not the production
invocation. Whether the madvise hook (outside `admitted()`, in `Genome_genomeLoad` /
`PackedArray`) still fires under two-pass is not established by this run: the sidecar's
`index_anon_huge_bytes` is 0 in bypass mode, but that counter is written by the coordinator
setup that did not run; smaps of the real process is the check.

Next: a lifecycle card — lift the `twoPass`/`sjdbInsert` guard and re-arm the borrowed index
after `sjdbInsertJunctions` (`twoPassRunPass1.cpp:15-92`, `sjdbBuildIndex.cpp:109-126`),
with the pass-1→pass-2 transition as the new strict-gate hazard. Until it passes, the
production number is unmeasured and P2C-PIPELINE-RUN stays gated.

### Step 2 parity and an unplanned observation

The non-engaged integrated run is output-identical to stock under the documented
normalization (both BAMs, both SJ files) — the bypass path is stock. Times (`/usr/bin/time`,
single pair, uncontrolled, perf not attached): stock 491.76 wall / **1868.82 user** / 40.53
sys; integrated-bypass 466.07 / **1704.91** / 44.51 — **−8.8% user-s** with zero GPU work.
The only code difference in bypass is the madvise hook, which sits outside `admitted()`.
This is one pair and is not a result; it is the reason the next launch is a same-binary
`STAR_INTEGRATE_THP=0/1` ablation under the two-pass argv (3 rotated pairs, warm cache,
live-process `AnonHugePages`), `~/uni-rnaseq-probe-lab/thp-twopass-20260908`.
