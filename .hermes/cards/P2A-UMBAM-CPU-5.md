# Card P2A-UMBAM-CPU-5 — featureCounts paired-end assignment rule (last gate)

`crates/umbam` passes 5/6 Tier 0 gates. `featurecounts_gate` fails: 272 genes differ. Your
COMPAT.md example (`ERR188140.30725820`: mate 1 overlaps genes A and B, mate 2 overlaps only
A, Subread assigns A) shows the union rule in the previous card was wrong. Replace it with a
rule you **measure**, not one you assume. Hypothesis to test first (this is how Subread's
`vote_and_add_count` behaves for `-p --countReadPairs` without `-O`):

1. Compute the gene set hit by each mate independently (≥1 bp overlap of any M/=/X block
   with any exon of the gene; N and D are gaps).
2. If both mates hit ≥1 gene: candidate set = **intersection** of the two sets if non-empty,
   else the **union**.
3. If only one mate hits anything: candidate set = that mate's set.
4. Assign the fragment iff the candidate set has exactly one gene; otherwise unassigned
   (ambiguous, or no features).

Run the gate. If it still fails, use `featureCounts -R CORE` on the sorted BAM (the tool is
in the `rnaseq` conda env: `source ~/miniconda3/etc/profile.d/conda.sh && conda activate
rnaseq`) to get per-fragment assignments, join to yours by read name, and characterize the
disagreeing class (report counts by: both-mates-hit / one-mate-hit / spliced / chimeric /
mate-unmapped). Adjust the rule to what the data says; put the final rule and the evidence
in `COMPAT.md` replacing the current featureCounts section.

Also: `peak_rss_bytes` is 5.86 GB on an 82 MB input — genomecov is allocating
chromosome-length depth arrays. Replace with per-tid arrays sized to `[min_start, max_end)`
of that tid's records, or an event-sorted sweep. Target peak RSS < 1.5 GB on Tier 0. Do this
**after** the gate passes; the gate must still be byte-identical afterward.

Rules unchanged: don't edit `tests/tier0.rs` except to add tests; `cargo fmt`;
`cargo clippy -p umbam --all-targets -- -D warnings` clean; no `git commit`; don't touch
`docs/ bench/ scripts/ .hermes/ KANBAN.md crates/umem/` or the Spark.

Finish with the six-gate table, `timing.tsv` at `--threads 12` (release), and the COMPAT.md
featureCounts section.
