# P2C-TRIM-FASTQC — port Trim Galore + FastQC to Rust (`umtrim`), byte-gated

Two phases. Phase A (Luna): goldens + fixture + crate scaffold. Phase B (Terra): the port.
Do not dispatch B until A's goldens are verified non-empty by the orchestrator.

## Why

19.1 CPU-min of the projected 137 per sample: `FASTQ_FASTQC_UMITOOLS_TRIMGALORE:TRIMGALORE`
10.4 CPU-min (`trim_galore --cores 4 --paired --gzip --fastqc_args '-t 8'` → cutadapt +
FastQC on trimmed), `FASTQC` (raw) 6.7, `FQ_LINT` 2× 2.7. All deterministic, all
byte-gateable, all embarrassingly parallel. Low risk, medium yield, no GPU.

Containers cached on the Spark (`docker images`):
`community.wave.seqera.io/library/trim-galore:2.1.0--7df2aae1e1928c85`,
`community.wave.seqera.io/library/fastqc:0.12.1--df99cb252670875a`,
`community.wave.seqera.io/library/fq:0.12.0--256df3027a85ed7c`.
Real invocation: Spark `~/uni-rnaseq/runs/tier1-full/work/42/c3f1a9ee47bf2354ae8861c2e979fe/.command.sh`.

## Phase A — goldens and scaffold (gpt-5.6-luna, danger-full-access for ssh to Spark)

Files: `scripts/make_tier0_trim.sh`, `crates/umtrim/` (Cargo scaffold, `COMPAT.md` stub,
`tests/tier0_gate.rs` with `#[ignore]` gates that `cmp` outputs), `docs/tool-src/
trim-galore-0.6.x-relevant.pl`, `docs/tool-src/cutadapt-relevant.py`, `docs/tool-src/
fastqc-0.12.1-modules.txt` (list of modules + the `fastqc_data.txt` format). Workspace
`Cargo.toml` member add.

1. On the Spark, from the existing chr22 Tier-0 fixture (`~/uni-rnaseq-data/tier0`; read
   `scripts/make_tier0.sh` for how it was built), run each container with the **exact**
   nf-core argv on the fixture FASTQs: `trim_galore` (produces `*_val_{1,2}.fq.gz`,
   `*_trimming_report.txt`, and via `--fastqc_args` the FastQC zip/html), `fastqc` raw,
   `fq lint`. Goldens to `~/uni-rnaseq-data/tier0/trim/` on both boxes (rsync to Mac).
   Assert every golden non-empty. Record versions (`cutadapt --version`, `fastqc --version`,
   `trim_galore --version`) in `tier0/trim/versions.txt`.
2. Extract the tool sources from the containers into `docs/tool-src/` (Trim Galore is a Perl
   script; cutadapt is Python — the relevant modules: `adapters.py`, `align` (C extension —
   note it, the algorithm is in `_align.pyx`), `modifiers.py`, `qualtrim.py`; FastQC is Java —
   extract the module list and `fastqc_data.txt` writer behaviour by running it, not by
   decompiling). NOTICE.md entries for each license.
3. Gzip determinism check: `trim_galore --gzip` output is gzip'd by `gzip`/`pigz`; two runs
   of the container on the fixture must `cmp`-match. If they do not (timestamps in the gzip
   header), document the normalization (`gzip -dc | cmp`) in `COMPAT.md` now — this is the
   one non-obvious gate rule and it must be measured, not assumed.
4. Scaffold `umtrim` with the gate tests written first (RED): `cmp` of decompressed
   `_val_1.fq`, `_val_2.fq`, `trimming_report.txt` (which fields are timing/path and must be
   excluded — list them from the golden), FastQC `fastqc_data.txt` per module, `fq lint`
   exit + stdout.

Finish with: golden inventory with byte sizes, versions, the gzip determinism result, the
list of excluded report fields with justification, and what Terra needs to know.

## Phase B — the port (gpt-5.6-terra, workspace-write) — written after A lands

Scope for B: cutadapt's paired-end adapter trimming as Trim Galore invokes it (auto-detected
adapter — Trim Galore's detection rule is in the Perl, extract it; quality trim `-q 20`;
`--length 20`; `--stringency 1`; `-e 0.1`), the trimming report, then FastQC's modules on
both raw and trimmed. `fq lint` last (smallest). Source-as-spec; the cutadapt alignment is
semi-global with the documented error-rate rule — port it exactly, do not substitute a
"better" aligner. Gates: Tier-0 `cmp` then one full-depth sample vs the nf-core outputs.
Rayon over read chunks; output order must equal input order. Time each stage; comparator is
the 20-thread CPU arm of our own crate vs the trace CPU-min, reported separately.
