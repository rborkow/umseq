# Production STAR gate design

`run_production_gate.py` accepts an argv JSON array with one each of `{STAR}`,
`{R1}`, `{R2}`, and `{OUT}`. It optionally substitutes `{GENOME_DIR}` from
`--genome-dir` and `{GTF}` from `--sjdb-gtf`; static absolute paths are also
valid template tokens. It records rendered argv and environments through
the shared stage runner, refuses an existing evidence root, records executable
and optional input hashes, and holds the usual resource lock. The template is
pinned to `--runThreadN 16`. `--stock-twice` runs two stock arms before any
integrated work; run it first.

The default comparator is literal `cmp` for both BAMs, both junction files,
and `Log.final.out` after only four timing fields are removed (`Started job on`,
`Started mapping on`, `Finished on`, and mapping speed). If stock-twice proves
that BAM bytes are nondeterministic, `--compare namesorted-sam` is an explicit,
printed normalization: `samtools view` drops BAM headers, then `LC_ALL=C sort
-s -k1,1` stably sorts records by read name. The junction files and non-timing
log remain byte-compared. This is a documented comparator choice, never an
implicit fallback.

## Two-pass index lifetime

The production argv has two mapping-time index transitions. `STAR.cpp:136-149`
loads the index and inserts the mapping-time GTF junctions before any chunks are
constructed. `twoPassRunPass1.cpp:15-73` maps pass 1 and joins its mapping
threads; `:92` then inserts pass-1 junctions before `STAR.cpp:197` constructs
pass-2 chunks. In each insertion `sjdbBuildIndex.cpp:126,289-300` grows `G`,
replaces the packed `SA` allocation, changes `nSA`, and rebuilds `SAi`.

The generator explicitly calls `star_integrate::rearm(P, genomeMain)` after
each insertion. It compares the borrowed identity (`G`, `SA.charArray`,
`SA.lengthByte`, `nSA`, and `nGenome`); on change it runs the proven close path
(drain windows, join coordinator, drop lookahead/admission and destroy the
borrowed context) and calls re-entrant setup on the new buffers. The explicit
sites are deliberately used instead of a branch in every seed hook: their
source ordering proves no mapping thread is live, and one-pass mapping’s hot
path remains unchanged.

`PackedArray::allocateArray` is already advised by the generated
`PackedArray.cpp` hook, so the replacement `SA2.charArray` is advised. The
`G1` allocation including `genomeInsertL` headroom is advised in
`Genome_genomeLoad.cpp`, covering appended junction sequence. At final finish,
`index_anon_huge_bytes` is sampled from the current `G`, `SA`, and `SAi`
buffers. The sidecar reports cumulative totals plus `index_generations` and
per-generation submitted/consumed deltas; production acceptance must require
generation 2 consumed work.

## Transcriptome BAM

The integration hook is in seed mapping (`ReadAlign_oneRead` and
`ReadAlign_maxMappableLength2strands`), upstream of output construction. STAR
calls `quantTranscriptome(chunkTr, nTr, trMult, alignTrAll)` from
`source/ReadAlign_outputAlignments.cpp:51-56`.
The projection iterates that same selected alignment set and emits the second
BAM at `source/ReadAlign_quantTranscriptome.cpp:7-76`.

Verdict: no separate hook or reordered projection stream is introduced; the
projection consumes `trMult` after mapping selection. Confidence **medium-high**:
an earlier seed-level divergence can still alter `trMult`. If wrong, the
transcriptome BAM comparison fails independently of genomic BAM parity.

## Output order and seed zero

The production argv uses unsorted BAM and 16 mapping threads. STAR flushes the
unsorted genomic and transcriptome BGZF streams before its final aggregation
(`source/STAR.cpp:214-224`);
the output path is therefore subject to worker/chunk order, while seed zero
also controls STAR's randomized primary-alignment choices. Verdict: byte
determinism is **unknown** without the stock control; confidence **low** that
literal BAM `cmp` is valid. `--stock-twice --compare cmp` is the first host
launch. A stock mismatch is a result, not permission to silently normalize;
rerun the full gate with the explicitly selected and recorded `namesorted-sam`
mode if that comparison is approved.
