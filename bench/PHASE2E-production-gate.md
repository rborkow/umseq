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
content. Transcriptome BAM: pending (see below).

Consequence: the production gate's comparator for the two BAMs is the documented
normalization — headers removed, records sorted — with `SJ.out.tab`, pass-1 `SJ.out.tab`,
and non-timing `Log.final.out` fields kept byte-exact. This is the same class of
normalization AGENTS.md permits for umbam (documented, exact, applied identically to both
arms); it is not "identical" in the byte sense and is labelled as such in every result.

Note for the comparator: the runner's `namesorted-sam` mode sorts stably by read name only
(`sort -s -k1,1`). Whether that is sufficient (multimapper records of one read kept in a
stable relative order across runs) or whether a full-record sort is required is being
checked on the stock pair before the integrated run.
