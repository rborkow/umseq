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

The hook is installed immediately after `genomeMain.genomeLoad()` in
`source/STAR.cpp:136`, before `twoPassRunPass1` at `source/STAR.cpp:170`.
Pass 1 constructs `Genome genomeMain1=genomeMain`, maps it, then calls
`sjdbInsertJunctions` at `source/twoPassRunPass1.cpp:15-92`.
That loads pass-1 junctions and calls `sjdbBuildIndex`
(`source/sjdbInsertJunctions.cpp:24-64`).
`sjdbBuildIndex` changes `nGenome` and `nSA`, defines a replacement packed SA,
and adjusts SAi entries (`source/sjdbBuildIndex.cpp:109-126`).

Verdict: the borrowed coordinator setup is stale across this transition unless
the backing objects happen to retain compatible storage; confidence **high**.
No generator re-arm was added: calling setup/finish around this transition has
not been proven safe for existing worker/coordinator state. If wrong, the gate
will fail by a strict mismatch, crash, sidecar rejection, or any output compare;
it cannot pass merely on GPU activity.

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
