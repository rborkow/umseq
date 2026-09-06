# Card P2A-UMQC-PERF-2 — the last serial RSeQC sweeps: junctions (18 + 22 s) and read_distribution (14 s)

## Context
`crates/umbam/src/qc.rs`. Read `docs/design-phase2b.md` § "P2A-UMQC-PERF" and
`bench/PHASE2A-tier1-validation.md` "Run 4" for the current 76M-record stage times. The
per-tid parallel pattern (`par_chunks` over `coordinate_order()`, fold/reduce) is already
in `read_distribution`, `bam_stat`, `qualimap_report`. All three targets below are gated
byte-identical on Tier 0 (`crates/umbam/tests/tier0_qc.rs`) and the gates **must stay
green** — the tool source in `docs/tool-src/rseqc-5.x-relevant.py` is authoritative
where the card disagrees (source beats card, silently; stop only if source and golden
disagree with each other).

## Targets (76M records, 20 threads)

### 1. `junction_annotation` — 17.9 s, fully serial → ≤ 3 s
Semantics to preserve exactly:
- Filter `flag & 0x304 == 0`, not a duplicate, `mapq >= 30`; every intron from
  `cigar_introns`; `< 50 bp` → filtered; known/partial/novel via `intron_starts`/
  `intron_ends` per (uppercase) chrom.
- The `.xls` lists junctions **in first-encounter order over `coordinate_order()`**
  (Python dict insertion order). That ordering is what makes this look serial. It isn't:
  first-encounter position of a junction = the minimum coordinate-order index at which
  it appears. So: parallel pass emits `(chrom_tid, start, end, order_index)` per
  qualifying intron ≥ 50 bp; `par_sort_unstable` by `(tid, start, end, order_index)`;
  run walk yields count + first index per junction; then sort the runs by first index for
  output. Event totals (known/partial/novel/filtered) are fold/reduce sums. Do the
  chrom uppercase + `CHR`→`chr` mapping once per tid, not per record.
- Membership tests: replace `HashMap<String, HashSet<i32>>` keyed by chrom string with a
  per-tid `Vec<sorted i32>` + `binary_search` (or keep the HashSet but index by tid).
  Resolve the BED's chrom names to tids once (`chromosome_names`), as the tid-resolved
  BED model already does for other outputs.

### 2. `junction_saturation` — 22.0 s → ≤ 4 s
Same event extraction as (1) — **share it**: one parallel intron pass feeding both
outputs (they filter identically; saturation just drops the `< 50` events). Then:
- The seeded xorshift shuffle over `events` is a semantic commitment (documented in the
  function): the 5–95% points are intentionally non-golden but must stay deterministic.
  Keep the shuffle exactly (same seed, same swap sequence over the same input order —
  so `events` must be built in coordinate order, which the sorted-by-`order_index`
  output of the shared pass gives you). The shuffle is a serial O(n) swap loop over
  ~60M events; that's fine (~0.5 s).
- The expensive part is `seen: HashMap<(String,i32,i32),u64>` with `String` clones per
  event and a full `seen.keys().filter(known)` re-scan **at every 5% step** (20 scans of
  a growing map). Replace: events as `(tid: u32, start: i32, end: i32)` (12 bytes, `Copy`);
  per step, insert the chunk into a `HashSet<(u32,i32,i32)>` and maintain running
  `known`/`all` counters incrementally (a junction's known-ness is fixed, so increment
  on first insert). One `known_junctions` lookup per *new* junction, none per step.
  Precompute known-ness once per distinct junction from the run walk in (1) if
  convenient (`HashMap<junction, bool>`).
- Outputs must remain byte-identical: the 100% column is order-independent; the earlier
  columns depend only on the shuffle order, which is unchanged if `events` is the same
  sequence.

### 3. `read_distribution` — 13.6 s, already parallel → ≤ 5 s
It does up to ten `contains()` calls per exon block, each a `HashMap<String,…>` lookup
by chrom name plus a `partition_point`. Convert the `BedModel` interval maps used here
(`cds, intron, utr5, utr3, up1, up5, up10, down1, down5, down10`) to **per-tid**
`Vec<Vec<Interval>>` resolved once (unknown chroms → empty), and pass `tid` instead of
`&str`. Keep the exact `start < p < end` semantics in `contains`. If the per-tid vectors
are built once in `BedModel` (or a `TidBedModel` built next to `FeatureIndex`), other
users of `contains` (`infer_experiment`, `inner_distance`) get it for free — fine, but
don't refactor beyond what's needed to hit the target.

## Rules
- Tier 0 gates: `source ~/miniconda3/etc/profile.d/conda.sh && conda activate rnaseq &&
  cargo test -p umbam --release -- --ignored` and `cargo test -p umbam --release` all
  green; `cargo fmt`; `cargo clippy -p umbam --all-targets -- -D warnings` clean.
- Print nothing new; keep the `timing.tsv` row names.
- `unsafe` only in `umem`; no commits; don't touch `docs/ bench/ .hermes/ KANBAN.md
  crates/umem/ crates/umgpu/`, `lib.rs` beyond what a shared helper strictly needs, or
  the Spark. I run the 76M validation there.
- Never fabricate a timing. Report Tier 0 `timing.tsv` before/after for the three rows
  and the gate table. If any output changes, say which and why before touching the gate.
