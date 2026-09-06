# Card P2A-UMQC-8 — full-depth fixes: four correctness items + three pathological stages

`umbam chain --qc` ran on a real 78M-pair sample (76.1M records, 6.7 GB) and was diffed
against that sample's nf-core outputs. Read `bench/PHASE2A-tier1-validation.md` — it has
the full table. Most outputs are identical at scale. Fix these, in order. There's no Tier 0
golden for some of them (chr22 can't exercise them), so the evidence is reasoning from the
tool source + the numbers in that doc; I re-run the full-depth comparison on the Spark.

## Correctness
1. **flagstat must reflect markdup.** Ours reports `0 + 0 duplicates`; nf-core runs
   `samtools flagstat` on the *markdup* BAM (17,192,968). Compute flagstat/idxstats after
   `mark_duplicates`, with the 0x400 set applied. Tier 0's golden was also post-markdup?
   — check `scripts/make_tier0.sh`; if the fixture golden is pre-markdup, regenerate it
   isn't your job — instead emit both `flagstat.txt` (post-markdup, the nf-core one) and
   keep the existing gate passing by pointing it at whichever the fixture used, and say
   which in the summary.
2. **inner_distance `sample_size` cap.** RSeQC (`mRNA_inner_distance`) breaks out of the
   read loop when `pair_num >= sample_size` (1,000,000). Our full-depth histogram sums to
   22,545,855; nf-core's to 941,753. `pair_num` increments only after all the skip
   conditions pass — read the loop top in `docs/tool-src/rseqc-5.x-relevant.py`; 941,753
   < 1M means some accepted pairs don't land in a bin (different-chromosome `NA` rows and
   out-of-range distances count toward `pair_num` but not the histogram). Mirror it
   exactly: process records in coordinate order, count `pair_num` where the source does,
   stop where the source does.
3. **read_distribution `Total_bases` for the six TSS/TES windows.** Tag counts are all
   identical; only the six upstream/downstream base totals differ (ours larger by
   ~500–120,000 bp). `process_gene_model` in the source builds those windows and then
   `cal_size` after `build_bitsets` — the merge. Likely: windows clipped at 0 (chromosome
   start) and/or a `max(0, ...)` we don't apply, or the `-x`/`bx` bitset size cap. Read
   `process_gene_model` + `cal_size` and reproduce; the 26,849,677 golden for TSS_up_1kb
   is the target.
4. **dupRadar `countMultiMappingReads=TRUE` pairing.** 21 of 78,900 genes differ by ±1–5 in
   `allCountsMulti`/`filteredCountsMulti`. Rsubread pairs mates *per alignment record*
   using HI (hit index) when NH>1 — mate i of read1 pairs with mate i of read2 — rather
   than "first 0x40 with first 0x80". Check `bam_layout`/aux parsing for `HI:i` and pair on
   `(name, HI)`; secondaries carry their own HI. If HI is absent Rsubread falls back to
   position matching (`next_pos`); implement HI first and report whether the residual
   goes to zero on Tier 0 (it has multimappers) — Tier 0's dupRadar golden is the gate.

## Performance (full-depth timings in the doc)
5. **`qc_infer_experiment` 772 s and `qc_inner_distance` 769 s** (2.5 s each on Tier 0):
   both are doing per-read lookups against the BED model in a way that scales with BED
   size × records — 69 MB BED, 76M records. Both need the per-tid sorted interval arrays +
   binary search that `read_distribution` and `featureCounts` already use (those run in
   ~30 s and ~8 s at full depth). Target < 30 s each.
6. **`qc_dupradar` 125 s**: four counting passes. Make it one pass over the shared name
   grouping with four accumulators (the filters are just flag/NH predicates). Target ~15 s.
7. `qc_seq_duplication` 36 s: hashing 76M sequences as owned `String`s? Hash the bytes in
   place (`&[u8]` key into a `HashMap<u64, u32>` of a 64-bit hash with collision check, like
   the name grouping). Target < 10 s.

All Tier 0 gates stay green (`source ~/miniconda3/etc/profile.d/conda.sh && conda activate
rnaseq && cargo test -p umbam --release -- --ignored && cargo test -p umbam --release`).
`cargo fmt`; strict clippy; `unsafe` only in `umem`; no commits; don't touch `docs/ bench/
.hermes/ KANBAN.md crates/umem/` or the Spark.

Finish with: what changed per item, Tier 0 `timing.tsv` before/after, gate table, and for
items 2–4 the specific source lines you mirrored.
