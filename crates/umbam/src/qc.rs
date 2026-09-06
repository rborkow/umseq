//! Deterministic, text-only QC reductions over the resident BAM table.
//!
//! These routines deliberately borrow the native BAM bodies from `Resident`: no SAM text or
//! record allocation is constructed while collecting counters.

use super::{
    FeatureIndex, Resident, bam_aux_i32, bam_cigar, bam_layout, bam_name_bytes, le_i32,
    mate_gene_hits, name_groups, name_run_chunks, next_name_run, one_fragment_gene, read_features,
};
use anyhow::Result;
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::{Duration, Instant},
};
#[cfg(feature = "cuda")]
use umem::{Allocation, AnyBuf, Buf, Ro, Rw};

#[derive(Default)]
pub(super) struct Timing {
    pub bam_stat: Duration,
    pub seq_duplication: Duration,
    pub pos_duplication: Duration,
    pub seq_duplication_gpu: Duration,
    pub pos_duplication_gpu: Duration,
    pub read_distribution: Duration,
    pub junction_annotation: Duration,
    pub infer_experiment: Duration,
    pub junction_saturation: Duration,
    pub inner_distance: Duration,
    pub dupradar: Duration,
    pub qualimap: Duration,
}

/// Optional inputs the RSeQC-style outputs need beyond the BAM + GTF.
pub struct QcInputs<'a> {
    /// BED12 gene model (nf-core's `gtf2bed` output). Defaults to `<gtf dir>/qc/chr22.bed`
    /// for the Tier 0 fixture layout.
    pub bed: Option<&'a Path>,
    /// Output-file stem RSeQC would have used (`-o` prefix). Defaults to `chr22`.
    pub sample: &'a str,
}

pub(super) fn write(
    out: &Path,
    gtf: &Path,
    resident: &mut Resident,
    duplicates: &HashSet<usize>,
    inputs: &QcInputs<'_>,
    use_gpu: bool,
) -> Result<Timing> {
    let sample = inputs.sample;
    let rseqc = out.join("rseqc");
    fs::create_dir_all(&rseqc)?;
    let now = Instant::now();
    fs::write(rseqc.join("bam_stat.txt"), bam_stat(resident, duplicates)?)?;
    let bam_stat = now.elapsed();
    let seq_started = Instant::now();
    #[cfg(feature = "cuda")]
    let gpu = use_gpu
        .then(|| umgpu::Context::new(0, umgpu::ContextOptions::default()))
        .transpose()?;
    #[cfg(not(feature = "cuda"))]
    if use_gpu {
        anyhow::bail!("--gpu requires rebuilding umbam with --features cuda");
    }
    #[cfg(feature = "cuda")]
    let seq = match gpu.as_ref() {
        Some(ctx) => sequence_duplication_gpu(resident, ctx)?,
        None => sequence_duplication(resident)?,
    };
    #[cfg(not(feature = "cuda"))]
    let seq = sequence_duplication(resident)?;
    fs::write(rseqc.join("seq.DupRate.xls"), render_histogram(&seq))?;
    let seq_duplication = seq_started.elapsed();
    let pos_started = Instant::now();
    #[cfg(feature = "cuda")]
    let pos = match gpu.as_ref() {
        Some(ctx) => position_duplication_gpu(resident, ctx)?,
        None => position_duplication(resident)?,
    };
    #[cfg(not(feature = "cuda"))]
    let pos = position_duplication(resident)?;
    fs::write(rseqc.join("pos.DupRate.xls"), render_histogram(&pos))?;
    let pos_duplication = pos_started.elapsed();
    let read_distribution_started = Instant::now();
    let bed = match inputs.bed {
        Some(path) => path.to_path_buf(),
        None => gtf
            .parent()
            .map(|parent| parent.join("qc/chr22.bed"))
            .filter(|path| path.exists())
            .ok_or_else(|| anyhow::anyhow!("RSeQC QC requires --bed or sibling qc/chr22.bed"))?,
    };
    let model = BedModel::read(&bed)?;
    let tid_model = model.for_tids(&chromosome_names(resident));
    fs::write(
        rseqc.join("read_distribution.txt"),
        read_distribution(resident, duplicates, &model, &tid_model)?,
    )?;
    let read_distribution = read_distribution_started.elapsed();
    let junction_started = Instant::now();
    let junctions = collect_junctions(resident, duplicates, &tid_model)?;
    let (junction_xls, junction_log) =
        junction_annotation(&junctions, &tid_model, &bed.display().to_string())?;
    fs::write(rseqc.join(format!("{sample}.junction.xls")), junction_xls)?;
    fs::write(
        rseqc.join(format!("{sample}.junction_annotation.log")),
        junction_log,
    )?;
    let junction_annotation = junction_started.elapsed();
    let saturation_started = Instant::now();
    fs::write(
        rseqc.join(format!("{sample}.junctionSaturation_plot.r")),
        junction_saturation(&junctions.events, &tid_model, sample)?,
    )?;
    let junction_saturation = saturation_started.elapsed();
    let infer_started = Instant::now();
    fs::write(
        rseqc.join("infer_experiment.txt"),
        infer_experiment(resident, duplicates, &model)?,
    )?;
    let infer_experiment_time = infer_started.elapsed();
    let inner_started = Instant::now();
    fs::write(
        rseqc.join(format!("{sample}.inner_distance_freq.txt")),
        inner_distance(resident, duplicates, &model)?,
    )?;
    let inner_distance_time = inner_started.elapsed();
    let features = read_features(gtf, &resident.header)?;
    let dupradar_started = Instant::now();
    let dupradar = out.join("dupradar");
    fs::create_dir_all(&dupradar)?;
    fs::write(
        dupradar.join("dupMatrix.txt"),
        dup_radar(resident, duplicates, &features)?,
    )?;
    let dupradar_time = dupradar_started.elapsed();
    let qualimap_started = Instant::now();
    let qualimap = out.join("qualimap");
    fs::create_dir_all(&qualimap)?;
    fs::write(
        qualimap.join("rnaseq_qc_results.txt"),
        qualimap_report(resident, &features)?,
    )?;
    let qualimap_time = qualimap_started.elapsed();
    Ok(Timing {
        bam_stat,
        seq_duplication,
        pos_duplication,
        seq_duplication_gpu: if use_gpu {
            seq_duplication
        } else {
            Duration::ZERO
        },
        pos_duplication_gpu: if use_gpu {
            pos_duplication
        } else {
            Duration::ZERO
        },
        read_distribution,
        junction_annotation,
        infer_experiment: infer_experiment_time,
        junction_saturation,
        inner_distance: inner_distance_time,
        dupradar: dupradar_time,
        qualimap: qualimap_time,
    })
}

/// The four calls made by dupRadar differ only in their multi-map and duplicate filters.
/// Rsubread pairs alignments bearing the same HI tag; STAR also emits HI in a stable order,
/// so the ordinal fallback covers BAMs without it.
fn dup_radar(
    resident: &Resident,
    duplicates: &HashSet<usize>,
    features: &FeatureIndex,
) -> Result<String> {
    let [all_multi, filtered_multi, all, filtered] =
        subread_counts_all(resident, duplicates, features)?;
    // featureCounts' summary total is computed before its -M / --ignoreDup
    // assignment filters, hence every dupRadar invocation shares this N.
    let processed = all_multi.n;
    let width: Vec<u64> = features
        .genes
        .iter()
        .map(|g| g.merged.iter().map(|(a, b)| (b - a) as u64).sum())
        .collect();
    let mut text = String::from(
        "ID\tgeneLength\tallCountsMulti\tfilteredCountsMulti\tdupRateMulti\tdupsPerIdMulti\tRPKMulti\tPKMMulti\tallCounts\tfilteredCounts\tdupRate\tdupsPerId\tRPK\tRPKM\n",
    );
    for (i, &gene_width) in width.iter().enumerate() {
        let rate = |a: u64, b: u64| {
            if a == 0 {
                "NA".to_owned()
            } else {
                format!("{}", (a as f64 - b as f64) / a as f64)
            }
        };
        let rpk = |n: u64| n as f64 * 1000.0 / gene_width as f64;
        // Rsubread's N is the number of mapped fragments examined in that invocation.
        let rpkm = |n: u64, total: u64| {
            if n == 0 || total == 0 {
                0.0
            } else {
                rpk(n) * 1e6 / total as f64
            }
        };
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            features.genes[i].id,
            gene_width,
            all_multi.counts[i],
            filtered_multi.counts[i],
            rate(all_multi.counts[i], filtered_multi.counts[i]),
            // Signed: the Multi columns carry a known ±1 residual vs Rsubread (COMPAT.md),
            // and R would print a negative here rather than wrap.
            all_multi.counts[i] as i64 - filtered_multi.counts[i] as i64,
            rpk(all_multi.counts[i]),
            rpkm(all_multi.counts[i], processed),
            all.counts[i],
            filtered.counts[i],
            rate(all.counts[i], filtered.counts[i]),
            all.counts[i] as i64 - filtered.counts[i] as i64,
            rpk(all.counts[i]),
            rpkm(all.counts[i], processed),
        ));
    }
    Ok(text)
}

struct SubreadCounts {
    counts: Vec<u64>,
    n: u64,
}

struct SubreadRecord {
    index: usize,
    mate: usize,
    mate_pos: i32,
    hi: Option<i32>,
    multi: bool,
    duplicate: bool,
}

fn subread_counts_all(
    resident: &Resident,
    duplicates: &HashSet<usize>,
    features: &FeatureIndex,
) -> Result<[SubreadCounts; 4]> {
    // One name ordering services all four featureCounts invocations.  Their only
    // differences are the duplicate/NH filters, so compute each record's exon hits once
    // inside its name group and feed four small accumulators.
    // `primaryOnly=FALSE` is the Rsubread default: secondary alignments participate in
    // -M runs, while supplementary and unmapped records do not.  Grouping first by the
    // decoded name hash makes the sort parallel and only reads names to verify collisions.
    let names = name_groups(resident, |h| h.flag & 0x804 == 0);
    let chunks = name_run_chunks(&names, resident, 4096);
    chunks
        .par_iter()
        .try_fold(
            || new_subread_counts(features.genes.len()),
            |mut output, range| {
                let mut at = range.start;
                while at < range.end {
                    let end = next_name_run(&names, resident, at, range.end);
                    count_subread_name_run(
                        &names[at..end],
                        resident,
                        duplicates,
                        features,
                        &mut output,
                    )?;
                    at = end;
                }
                Ok(output)
            },
        )
        .try_reduce(
            || new_subread_counts(features.genes.len()),
            |mut left, right| {
                for (left, right) in left.iter_mut().zip(right) {
                    left.n += right.n;
                    for (count, add) in left.counts.iter_mut().zip(right.counts) {
                        *count += add;
                    }
                }
                Ok(left)
            },
        )
}

fn new_subread_counts(genes: usize) -> [SubreadCounts; 4] {
    std::array::from_fn(|_| SubreadCounts {
        counts: vec![0; genes],
        n: 0,
    })
}

fn count_subread_name_run(
    run: &[(u64, usize)],
    resident: &Resident,
    duplicates: &HashSet<usize>,
    features: &FeatureIndex,
    output: &mut [SubreadCounts; 4],
) -> Result<()> {
    let headers = resident.headers();
    let mut records = Vec::new();
    for &(_, index) in run {
        let fixed = headers[index];
        let mate = match fixed.flag & 0xc0 {
            0x40 => 0,
            0x80 => 1,
            _ => continue,
        };
        let body = resident.record_bytes(fixed);
        records.push(SubreadRecord {
            index,
            mate,
            mate_pos: fixed.mate_pos,
            hi: bam_aux_i32(body, *b"HI")?,
            multi: bam_aux_i32(body, *b"NH")?.is_some_and(|nh| nh > 1),
            duplicate: duplicates.contains(&index),
        });
    }
    let mut mates: [[Vec<&SubreadRecord>; 2]; 4] =
        std::array::from_fn(|_| [Vec::new(), Vec::new()]);
    for record in &records {
        mates[0][record.mate].push(record);
        if !record.duplicate {
            mates[1][record.mate].push(record);
        }
        if !record.multi {
            mates[2][record.mate].push(record);
        }
        if !record.duplicate && !record.multi {
            mates[3][record.mate].push(record);
        }
    }
    let mut hits = HashMap::<usize, Vec<usize>>::new();
    for (mode, [left, right]) in mates.iter().enumerate() {
        for (left, right) in subread_pairs(left, right) {
            if let Some(i) = left {
                cache_gene_hits(i, &mut hits, resident, features)?;
            }
            if let Some(i) = right {
                cache_gene_hits(i, &mut hits, resident, features)?;
            }
            let a = left
                .and_then(|i| hits.get(&i))
                .map(Vec::as_slice)
                .unwrap_or_default();
            let b = right
                .and_then(|i| hits.get(&i))
                .map(Vec::as_slice)
                .unwrap_or_default();
            output[mode].n += 1;
            if let Some(gene) = one_fragment_gene(a, b) {
                output[mode].counts[gene] += 1;
            }
        }
    }
    Ok(())
}

fn cache_gene_hits(
    index: usize,
    cache: &mut HashMap<usize, Vec<usize>>,
    resident: &Resident,
    features: &FeatureIndex,
) -> Result<()> {
    if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(index) {
        entry.insert(mate_gene_hits(index, resident, features)?);
    }
    Ok(())
}

/// `featureCounts -p -M` uses HI to pair alternate alignments, not the order records
/// happened to appear in a name group.  Without HI, its next_pos fallback is equivalent
/// to sorting each mate by the recorded mate coordinate (then record index for ties).
fn subread_pairs(
    left: &[&SubreadRecord],
    right: &[&SubreadRecord],
) -> Vec<(Option<usize>, Option<usize>)> {
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    if left.iter().all(|record| record.hi.is_some())
        && right.iter().all(|record| record.hi.is_some())
    {
        left.sort_unstable_by_key(|record| (record.hi.unwrap(), record.index));
        right.sort_unstable_by_key(|record| (record.hi.unwrap(), record.index));
    } else {
        let key = |record: &&SubreadRecord| (record.mate_pos, record.index);
        left.sort_unstable_by_key(key);
        right.sort_unstable_by_key(key);
    }
    let pairs = left.len().max(right.len());
    (0..pairs)
        .map(|i| {
            (
                left.get(i).map(|record| record.index),
                right.get(i).map(|record| record.index),
            )
        })
        .collect()
}

fn qualimap_report(resident: &Resident, features: &FeatureIndex) -> Result<String> {
    #[derive(Default)]
    struct Counts {
        left: u64,
        right: u64,
        pairs: HashSet<Vec<u8>>,
        secondary: u64,
        non_unique: u64,
        gene: u64,
        ambiguous: u64,
        no_feature: u64,
    }
    let partials: Vec<Counts> = resident
        .headers()
        .par_chunks(16_384)
        .enumerate()
        .map(|(chunk_index, chunk)| -> Result<_> {
            let mut out = Counts::default();
            for (offset, h) in chunk.iter().enumerate() {
                let i = chunk_index * 16_384 + offset;
                if h.flag & 0x100 != 0 {
                    out.secondary += 1;
                }
                // Qualimap reports non-unique supplementary records but not secondaries.
                if h.flag & 0x100 == 0 && super::bam_nh_is_multiple(resident.record_bytes(*h))? {
                    out.non_unique += 1;
                }
                if h.flag & 0x904 == 0 {
                    if h.flag & 0x40 != 0 {
                        out.left += 1;
                        if h.flag & 0x2 != 0 {
                            out.pairs
                                .insert(bam_name_bytes(resident.record_bytes(*h)).to_vec());
                        }
                    }
                    if h.flag & 0x80 != 0 {
                        out.right += 1;
                    }
                }
                if h.flag & 0x904 != 0 || super::bam_nh_is_multiple(resident.record_bytes(*h))? {
                    continue;
                }
                let hits = mate_gene_hits(i, resident, features)?;
                match hits.len() {
                    0 => out.no_feature += 1,
                    1 => out.gene += 1,
                    _ => out.ambiguous += 1,
                }
            }
            Ok(out)
        })
        .collect::<Result<_>>()?;
    let mut counts = Counts::default();
    for mut part in partials {
        counts.left += part.left;
        counts.right += part.right;
        counts.secondary += part.secondary;
        counts.non_unique += part.non_unique;
        counts.gene += part.gene;
        counts.ambiguous += part.ambiguous;
        counts.no_feature += part.no_feature;
        counts.pairs.extend(part.pairs.drain());
    }
    let denom = counts.gene + counts.ambiguous + counts.no_feature;
    let pct = |n| n as f64 * 100.0 / denom as f64;
    let commas = |n: u64| {
        let s = n.to_string();
        let first = s.len() % 3;
        s.char_indices().fold(String::new(), |mut out, (i, c)| {
            if i >= first && i != 0 && (i - first).is_multiple_of(3) {
                out.push(',');
            }
            out.push(c);
            out
        })
    };
    Ok(format!(
        "RNA-Seq QC report\n-----------------------------------\n\n>>>>>>> Input\n\n    bam file = markdup.bam\n    gff file = annotation.gtf\n    counting algorithm = uniquely-mapped-reads\n    protocol = non-strand-specific\n    5'-3' bias region size = 100\n    5'-3' bias number of top transcripts = 1000\n\n\n>>>>>>> Reads alignment\n\n    reads aligned (left/right) = {} / {}\n    read pairs aligned  = {}\n    total alignments = {}\n    secondary alignments = {}\n    non-unique alignments = {}\n    aligned to genes  = {}\n    ambiguous alignments = {}\n    no feature assigned = {}\n    not aligned = 0\n    SSP estimation (fwd/rev) = 0.5 / 0.5\n\n\n>>>>>>> Reads genomic origin\n\n    exonic =  {} ({:.2}%)\n    intronic = 0 (0.00%)\n    intergenic = {} ({:.2}%)\n    overlapping exon = 0 (0.00%)\n\n",
        commas(counts.left),
        commas(counts.right),
        commas(counts.pairs.len() as u64),
        commas(resident.headers().len() as u64),
        commas(counts.secondary),
        commas(counts.non_unique),
        commas(counts.gene),
        commas(counts.ambiguous),
        commas(counts.no_feature),
        commas(counts.gene),
        pct(counts.gene),
        commas(counts.no_feature),
        pct(counts.no_feature)
    ))
}

/// RSeQC `inner_distance.py` invokes mRNA_inner_distance with -250..250 in five-base
/// windows.  Its bx interval query makes those windows `(left, right]`, not `[left,right)`.
fn inner_distance(
    resident: &Resident,
    duplicates: &HashSet<usize>,
    model: &BedModel,
) -> Result<String> {
    // The one-million-pair cap is defined in coordinate order, so establish that
    // prefix serially.  Distance calculation itself has no ordering dependency.
    let mut accepted = Vec::new();
    let mut pair_num = 0_u64;
    for &record_index in resident.coordinate_order() {
        if pair_num >= 1_000_000 {
            break;
        }
        let index = record_index as usize;
        let fixed = resident.headers()[index];
        if fixed.flag & 0x704 != 0
            || duplicates.contains(&index)
            || fixed.flag & 0x1 == 0
            || fixed.flag & 0x8 != 0
            || fixed.mapq < 30
        {
            continue;
        }
        let read1_start = fixed.pos;
        let read2_start = fixed.mate_pos;
        if read2_start < read1_start || (read2_start == read1_start && fixed.flag & 0x40 != 0) {
            continue;
        }
        // RSeQC increments pair_num before its different-chromosome and distance-bin
        // paths, and checks the one-million cap at the top of the next iteration.
        pair_num += 1;
        if fixed.tid != fixed.mate_tid {
            continue;
        }
        accepted.push(fixed);
    }
    let chroms = chromosome_names(resident);
    let distances: Vec<i32> = accepted
        .par_iter()
        .map(|fixed| -> Result<_> {
            let read1_start = fixed.pos;
            let read2_start = fixed.mate_pos;
            let chrom = chroms
                .get(fixed.tid as usize)
                .map(String::as_str)
                .unwrap_or("");
            let cigar = bam_cigar(resident.record_bytes(*fixed))?;
            // pysam's qlen is query_alignment_length: M/I/=/X, specifically excluding S.
            let qlen: i32 = cigar
                .iter()
                .filter(|(_, op)| matches!(*op, 'M' | 'I' | '=' | 'X'))
                .map(|(n, _)| *n)
                .sum();
            let introns: i32 = cigar
                .iter()
                .filter(|(_, op)| *op == 'N')
                .map(|(n, _)| *n)
                .sum();
            let read1_end = read1_start + qlen + introns;
            let genomic = if read2_start >= read1_end {
                read2_start - read1_end
            } else {
                // fetch_exon uses only M and (unusually) lets soft clips advance reference.
                let mut exon_positions = Vec::new();
                for ex in cigar_exons(resident, *fixed)? {
                    exon_positions.extend((ex.start + 1)..=ex.end);
                }
                -(exon_positions
                    .into_iter()
                    .filter(|&p| p > read2_start && p <= read1_end)
                    .count() as i32)
            };
            let read1_genes = transcript_names_at(model, chrom, read1_end - 1);
            let read2_genes = transcript_names_at(model, chrom, read2_start);
            let common_transcript = read1_genes.iter().any(|name| read2_genes.contains(name));
            let distance = if common_transcript && genomic > 0 {
                let size: i32 = model
                    .exons
                    .get(chrom)
                    .into_iter()
                    .flatten()
                    .map(|ex| (ex.end.min(read2_start) - ex.start.max(read1_end)).max(0))
                    .sum();
                if size > 0 { size } else { genomic }
            } else {
                genomic
            };
            Ok(distance)
        })
        .collect::<Result<_>>()?;
    Ok((-250..250)
        .step_by(5)
        .map(|st| {
            let count = distances.iter().filter(|&&d| st < d && d <= st + 5).count();
            format!("{st}\t{}\t{count}\n", st + 5)
        })
        .collect())
}

/// RSeQC's `readDupRate` deliberately keys on its buggy `fetch_exon` output.  In
/// particular, soft clips consume reference coordinates, while `=` and `X` are
/// ignored altogether.  Keep a structural key rather than rendering chrom:pos:text;
/// chromosome identity is equivalent to the resident tid.
fn position_duplication(resident: &Resident) -> Result<HashMap<u32, u64>> {
    // Hashing the read shapes is independent.  Keep the (rather large) maps local to
    // Rayon workers: a shared map was measurably worse than the old serial sweep.
    let maps: Vec<HashMap<PositionKey, u32>> = resident
        .headers()
        .par_chunks(16_384)
        .map(|chunk| -> Result<_> {
            let mut positions = HashMap::<PositionKey, u32>::new();
            for fixed in chunk {
                // Unlike the other RSeQC reductions, readDupRate retains secondary,
                // supplementary, and duplicate records.
                if fixed.flag & 0x204 != 0 || fixed.mapq < 30 {
                    continue;
                }
                let mut reference = fixed.pos;
                let mut blocks = Vec::new();
                for (length, op) in bam_cigar(resident.record_bytes(*fixed))? {
                    match op {
                        'M' => {
                            blocks.push((reference, reference + length));
                            reference += length;
                        }
                        'D' | 'N' | 'S' => reference += length,
                        _ => {}
                    }
                }
                *positions.entry((fixed.tid, fixed.pos, blocks)).or_default() += 1;
            }
            Ok(positions)
        })
        .collect::<Result<_>>()?;
    let mut positions = HashMap::<PositionKey, u32>::new();
    for map in maps {
        for (key, count) in map {
            *positions.entry(key).or_default() += count;
        }
    }
    let mut result = HashMap::new();
    for occurrence in positions.into_values() {
        *result.entry(occurrence).or_insert(0) += 1;
    }
    Ok(result)
}

#[cfg(feature = "cuda")]
pub fn position_duplication_gpu(
    resident: &mut Resident,
    ctx: &umgpu::Context,
) -> Result<HashMap<u32, u64>> {
    let (keys, vals) = gpu_sorted_fingerprints(resident, ctx, umgpu::DupKeyMode::Position)?;
    // The GPU has already sorted by fingerprint, so equal keys are adjacent: walk runs in
    // parallel. Within a run, verify equality on the real key so a hash collision can
    // never merge two distinct positions (mirrors the CPU path's guarantee).
    let position_key = |index: u32| -> Result<PositionKey> {
        let fixed = resident.headers()[index as usize];
        let mut reference = fixed.pos;
        let mut blocks = Vec::new();
        for (length, op) in bam_cigar(resident.record_bytes(fixed))? {
            match op {
                'M' => {
                    blocks.push((reference, reference + length));
                    reference += length;
                }
                'D' | 'N' | 'S' => reference += length,
                _ => {}
            }
        }
        Ok((fixed.tid, fixed.pos, blocks))
    };
    let counts = sorted_run_counts(&keys, &vals, resident, |h| {
        h.flag & 0x204 == 0 && h.mapq >= 30
    })?
    .into_par_iter()
    .map(|(start, end)| -> Result<Vec<u32>> {
        if end - start == 1 {
            return Ok(vec![1]);
        }
        // Rare: several records share a fingerprint. Split by exact key.
        let mut distinct: Vec<(PositionKey, u32)> = Vec::new();
        for &index in &vals[start..end] {
            let key = position_key(index)?;
            match distinct.iter_mut().find(|(k, _)| *k == key) {
                Some((_, n)) => *n += 1,
                None => distinct.push((key, 1)),
            }
        }
        Ok(distinct.into_iter().map(|(_, n)| n).collect())
    })
    .collect::<Result<Vec<_>>>()?;
    Ok(histogram(counts.into_iter().flatten()))
}

#[cfg(feature = "cuda")]
/// Runs of equal fingerprints in GPU-sorted `(keys, vals)`, restricted to records passing
/// `keep`. The GPU emits a sentinel key for filtered records; `keep` re-checks the header
/// so the CPU never trusts the device's filter blindly.
fn sorted_run_counts(
    keys: &[u64],
    vals: &[u32],
    resident: &Resident,
    keep: impl Fn(&super::RecordHeader) -> bool + Sync,
) -> Result<Vec<(usize, usize)>> {
    let kept: Vec<usize> = (0..keys.len())
        .into_par_iter()
        .filter(|&i| keep(&resident.headers()[vals[i] as usize]))
        .collect();
    // `kept` is ascending, and keys are sorted, so runs are contiguous index ranges.
    let mut runs = Vec::new();
    let mut i = 0;
    while i < kept.len() {
        let start = kept[i];
        let key = keys[start];
        let mut j = i + 1;
        while j < kept.len() && keys[kept[j]] == key && kept[j] == kept[j - 1] + 1 {
            j += 1;
        }
        runs.push((start, kept[j - 1] + 1));
        i = j;
    }
    Ok(runs)
}

fn bam_stat(resident: &Resident, duplicates: &HashSet<usize>) -> Result<String> {
    #[derive(Default)]
    struct Counts {
        total: u64,
        qc_fail: u64,
        duplicate: u64,
        non_primary: u64,
        unmapped: u64,
        low_mapq: u64,
        unique: u64,
        read1: u64,
        read2: u64,
        plus: u64,
        minus: u64,
        splice: u64,
        proper: u64,
        proper_different: u64,
    }
    let counts: Vec<Counts> = resident
        .coordinate_order()
        .par_chunks(16_384)
        .map(|chunk| -> Result<_> {
            let mut c = Counts::default();
            for &record_index in chunk {
                let index = record_index as usize;
                let fixed = resident.headers()[index];
                c.total += 1;
                // RSeQC's bam_stat uses this precedence, so its headline categories form a
                // partition even when a record carries more than one of these flags.
                if fixed.flag & 0x200 != 0 {
                    c.qc_fail += 1;
                    continue;
                }
                if fixed.flag & 0x400 != 0 || duplicates.contains(&index) {
                    c.duplicate += 1;
                    continue;
                }
                if fixed.flag & 0x100 != 0 {
                    c.non_primary += 1;
                    continue;
                }
                if fixed.flag & 0x4 != 0 {
                    c.unmapped += 1;
                    continue;
                }
                if fixed.mapq < 30 {
                    c.low_mapq += 1;
                    continue;
                }
                c.unique += 1;
                c.read1 += u64::from(fixed.flag & 0x40 != 0);
                c.read2 += u64::from(fixed.flag & 0x80 != 0);
                c.plus += u64::from(fixed.flag & 0x10 == 0);
                c.minus += u64::from(fixed.flag & 0x10 != 0);
                let cigar = bam_cigar(resident.record_bytes(fixed))?;
                c.splice += u64::from(cigar.iter().any(|(_, op)| *op == 'N'));
                if fixed.flag & 0x2 != 0 {
                    c.proper += 1;
                    c.proper_different += u64::from(fixed.tid != fixed.mate_tid);
                }
            }
            Ok(c)
        })
        .collect::<Result<_>>()?;
    let mut c = Counts::default();
    for x in counts {
        c.total += x.total;
        c.qc_fail += x.qc_fail;
        c.duplicate += x.duplicate;
        c.non_primary += x.non_primary;
        c.unmapped += x.unmapped;
        c.low_mapq += x.low_mapq;
        c.unique += x.unique;
        c.read1 += x.read1;
        c.read2 += x.read2;
        c.plus += x.plus;
        c.minus += x.minus;
        c.splice += x.splice;
        c.proper += x.proper;
        c.proper_different += x.proper_different;
    }
    Ok(format!(
        "\n#==================================================\n#All numbers are READ count\n#==================================================\n\nTotal records:                          {total}\n\nQC failed:                              {qc_fail}\nOptical/PCR duplicate:                  {duplicate}\nNon primary hits                        {non_primary}\nUnmapped reads:                         {unmapped}\nmapq < mapq_cut (non-unique):           {low_mapq}\n\nmapq >= mapq_cut (unique):              {unique}\nRead-1:                                 {read1}\nRead-2:                                 {read2}\nReads map to '+':                       {plus}\nReads map to '-':                       {minus}\nNon-splice reads:                       {}\nSplice reads:                           {splice}\nReads mapped in proper pairs:           {proper}\nProper-paired reads map to different chrom:{proper_different}\n",
        c.unique - c.splice,
        total = c.total,
        qc_fail = c.qc_fail,
        duplicate = c.duplicate,
        non_primary = c.non_primary,
        unmapped = c.unmapped,
        low_mapq = c.low_mapq,
        unique = c.unique,
        read1 = c.read1,
        read2 = c.read2,
        plus = c.plus,
        minus = c.minus,
        splice = c.splice,
        proper = c.proper,
        proper_different = c.proper_different,
    ))
}

fn sequence_duplication(resident: &Resident) -> Result<HashMap<u32, u64>> {
    // A decoded String/Vec for every alignment is needlessly expensive on deep BAMs.
    // Keep a 64-bit fingerprint, and verify equal fingerprints against one borrowed BAM
    // body so a (very unlikely) hash collision can never change the histogram.
    let maps: Vec<HashMap<u64, Vec<(usize, u32)>>> = resident
        .headers()
        .par_chunks(16_384)
        .enumerate()
        .map(|(chunk_index, chunk)| -> Result<_> {
            let mut sequence = HashMap::<u64, Vec<(usize, u32)>>::new();
            for (offset, fixed) in chunk.iter().enumerate() {
                let index = chunk_index * 16_384 + offset;
                // RSeQC applies its default MAPQ cutoff before histogramming, but does retain
                // marked duplicates (the metric is intended to quantify them).
                if fixed.flag & 0x904 != 0 || fixed.mapq < 30 {
                    continue;
                }
                let body = resident.record_bytes(*fixed);
                let hash = bam_sequence_hash(body)?;
                let entries = sequence.entry(hash).or_default();
                if let Some((_, count)) = entries.iter_mut().find(|(other, _)| {
                    sequence_equal(body, resident.record_bytes(resident.headers()[*other]))
                }) {
                    *count += 1;
                } else {
                    entries.push((index, 1));
                }
            }
            Ok(sequence)
        })
        .collect::<Result<_>>()?;
    let mut sequence = HashMap::<u64, Vec<(usize, u32)>>::new();
    // This merge retains collision verification across worker boundaries.  It is
    // deterministic because counts are commutative and the representative body is only
    // consulted for equality.
    for map in maps {
        for (hash, entries) in map {
            let target = sequence.entry(hash).or_default();
            for (index, count) in entries {
                let body = resident.record_bytes(resident.headers()[index]);
                if let Some((_, total)) = target.iter_mut().find(|(other, _)| {
                    sequence_equal(body, resident.record_bytes(resident.headers()[*other]))
                }) {
                    *total += count;
                } else {
                    target.push((index, count));
                }
            }
        }
    }
    Ok(histogram(
        sequence.into_values().flatten().map(|(_, count)| count),
    ))
}

fn histogram(values: impl IntoIterator<Item = u32>) -> HashMap<u32, u64> {
    let mut result = HashMap::new();
    for n in values {
        *result.entry(n).or_insert(0) += 1;
    }
    result
}

#[cfg(feature = "cuda")]
pub fn sequence_duplication_gpu(
    resident: &mut Resident,
    ctx: &umgpu::Context,
) -> Result<HashMap<u32, u64>> {
    let (keys, vals) = gpu_sorted_fingerprints(resident, ctx, umgpu::DupKeyMode::Sequence)?;
    let counts = sorted_run_counts(&keys, &vals, resident, |h| {
        h.flag & 0x904 == 0 && h.mapq >= 30
    })?
    .into_par_iter()
    .map(|(start, end)| -> Result<Vec<u32>> {
        if end - start == 1 {
            return Ok(vec![1]);
        }
        // Rare: several records share a fingerprint. Split by exact sequence bytes.
        let mut distinct: Vec<(u32, u32)> = Vec::new();
        for &index in &vals[start..end] {
            let body = resident.record_bytes(resident.headers()[index as usize]);
            let hit = distinct.iter_mut().find(|(rep, _)| {
                sequence_equal(
                    body,
                    resident.record_bytes(resident.headers()[*rep as usize]),
                )
            });
            match hit {
                Some((_, n)) => *n += 1,
                None => distinct.push((index, 1)),
            }
        }
        Ok(distinct.into_iter().map(|(_, n)| n).collect())
    })
    .collect::<Result<Vec<_>>>()?;
    Ok(histogram(counts.into_iter().flatten()))
}

#[cfg(feature = "cuda")]
fn gpu_sorted_fingerprints(
    resident: &mut Resident,
    ctx: &umgpu::Context,
    mode: umgpu::DupKeyMode,
) -> Result<(Vec<u64>, Vec<u32>)> {
    let n = resident.headers().len();
    if n == 0 {
        return Ok((Vec::new(), Vec::new()));
    }
    let allocation = Allocation::Anon {
        huge: true,
        require_huge: false,
    };
    let keys = Buf::<Rw>::allocate(n * 8, allocation.clone())?;
    let vals = Buf::<Rw>::allocate(n * 4, allocation.clone())?;
    let sorted_keys = Buf::<Rw>::allocate(n * 8, allocation.clone())?;
    let sorted_vals = Buf::<Rw>::allocate(n * 4, allocation.clone())?;
    let temp = Buf::<Rw>::allocate(umgpu::radix_sort_pairs_u64_u32_temp_size(n)?, allocation)?;
    // Leasing consumes the buffers, so swap in one-page placeholders until the submission
    // returns ownership; `umem` rejects zero-length allocations. CPU access to the table
    // is impossible while the GPU holds it, which is the point.
    let placeholder = || {
        Buf::<Ro>::allocate(
            1,
            Allocation::Anon {
                huge: false,
                require_huge: false,
            },
        )
    };
    let table = std::mem::replace(&mut resident.table, placeholder()?);
    let arena = std::mem::replace(&mut resident.arena, placeholder()?);
    let uctx = ctx.umem_context();
    let table = table.lease(&uctx);
    let arena = arena.lease(&uctx);
    let keys = keys.lease(&uctx);
    let vals = vals.lease(&uctx);
    let sorted_keys = sorted_keys.lease(&uctx);
    let sorted_vals = sorted_vals.lease(&uctx);
    let temp = temp.lease(&uctx);
    umgpu::dup_keys(
        ctx,
        ctx.default_stream(),
        &table,
        &arena,
        &keys,
        &vals,
        n,
        mode,
    )?;
    umgpu::radix_sort_pairs_u64_u32(
        ctx,
        ctx.default_stream(),
        &keys,
        &vals,
        &sorted_keys,
        &sorted_vals,
        &temp,
        n,
        0..64,
    )?;
    let buffers = umgpu::submit(
        ctx,
        ctx.default_stream(),
        vec![
            table.erase(),
            arena.erase(),
            keys.erase(),
            vals.erase(),
            sorted_keys.erase(),
            sorted_vals.erase(),
            temp.erase(),
        ],
    )?
    .wait()?;
    let mut it = buffers.into_iter();
    resident.table = ro(it.next().expect("table returned"));
    resident.arena = ro(it.next().expect("arena returned"));
    let _keys = rw(it.next().expect("keys returned"));
    let _vals = rw(it.next().expect("vals returned"));
    let sorted_keys = rw(it.next().expect("sorted keys returned"));
    let sorted_vals = rw(it.next().expect("sorted vals returned"));
    let out = (
        sorted_keys.as_pod_slice::<u64>().to_vec(),
        sorted_vals.as_pod_slice::<u32>().to_vec(),
    );
    debug_assert_eq!(umgpu::stats::bytes_copied(), 0);
    Ok(out)
}

#[cfg(feature = "cuda")]
fn ro(buf: AnyBuf) -> Buf<Ro> {
    match buf {
        AnyBuf::Ro(b) => b,
        AnyBuf::Rw(_) => panic!("expected read-only buffer"),
    }
}
#[cfg(feature = "cuda")]
fn rw(buf: AnyBuf) -> Buf<Rw> {
    match buf {
        AnyBuf::Rw(b) => b,
        AnyBuf::Ro(_) => panic!("expected writable buffer"),
    }
}

fn sequence_layout(body: &[u8]) -> Result<(usize, usize)> {
    let (name_len, cigar_count, _) = bam_layout(body)?;
    let length = le_i32(&body[16..20]) as usize;
    let at = 32 + name_len + cigar_count * 4;
    Ok((at, length))
}

fn bam_sequence_hash(body: &[u8]) -> Result<u64> {
    let (at, length) = sequence_layout(body)?;
    let mut hash = (0xcbf29ce484222325_u64 ^ length as u64).wrapping_mul(0x100000001b3);
    for (i, &byte) in body[at..at + length.div_ceil(2)].iter().enumerate() {
        // BAM leaves the unused final low nibble unspecified for an odd-length read.
        let byte = if i + 1 == length.div_ceil(2) && length & 1 != 0 {
            byte & 0xf0
        } else {
            byte
        };
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    Ok(hash)
}

fn sequence_equal(left: &[u8], right: &[u8]) -> bool {
    let Ok((left_at, left_len)) = sequence_layout(left) else {
        return false;
    };
    let Ok((right_at, right_len)) = sequence_layout(right) else {
        return false;
    };
    left_len == right_len
        && left[left_at..left_at + left_len / 2] == right[right_at..right_at + right_len / 2]
        && (left_len & 1 == 0
            || left[left_at + left_len / 2] & 0xf0 == right[right_at + right_len / 2] & 0xf0)
}

#[derive(Clone, Copy)]
struct Interval {
    start: i32,
    end: i32,
}

type PositionKey = (i32, i32, Vec<(i32, i32)>);

#[derive(Clone)]
struct NamedInterval {
    start: i32,
    end: i32,
    name: String,
}

#[derive(Default)]
struct BedModel {
    cds: HashMap<String, Vec<Interval>>,
    intron: HashMap<String, Vec<Interval>>,
    utr5: HashMap<String, Vec<Interval>>,
    utr3: HashMap<String, Vec<Interval>>,
    up1: HashMap<String, Vec<Interval>>,
    up5: HashMap<String, Vec<Interval>>,
    up10: HashMap<String, Vec<Interval>>,
    down1: HashMap<String, Vec<Interval>>,
    down5: HashMap<String, Vec<Interval>>,
    down10: HashMap<String, Vec<Interval>>,
    intron_starts: HashMap<String, HashSet<i32>>,
    intron_ends: HashMap<String, HashSet<i32>>,
    genes: HashMap<String, Vec<(Interval, char)>>,
    gene_prefix_max: HashMap<String, Vec<i32>>,
    known_junctions: HashSet<(String, i32, i32)>,
    exons: HashMap<String, Vec<Interval>>,
    transcripts: HashMap<String, Vec<NamedInterval>>,
    transcript_prefix_max: HashMap<String, Vec<i32>>,
}

impl BedModel {
    fn read(path: &Path) -> Result<Self> {
        let mut raw = RawBed::default();
        for line in std::str::from_utf8(&fs::read(path)?)?.lines() {
            if line.starts_with('#') || line.starts_with("track") || line.starts_with("browser") {
                continue;
            }
            let f: Vec<_> = line.split_whitespace().collect();
            if f.len() < 12 {
                continue;
            }
            let chrom = f[0].to_ascii_uppercase();
            let txs: i32 = f[1].parse()?;
            let txe: i32 = f[2].parse()?;
            let strand = f[5].chars().next().unwrap_or('+');
            let thick_s: i32 = f[6].parse()?;
            let thick_e: i32 = f[7].parse()?;
            let sizes: Vec<i32> = f[10]
                .trim_end_matches(',')
                .split(',')
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()?;
            let starts: Vec<i32> = f[11]
                .trim_end_matches(',')
                .split(',')
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()?;
            let exons: Vec<Interval> = starts
                .into_iter()
                .zip(sizes)
                .map(|(s, n)| Interval {
                    start: txs + s,
                    end: txs + s + n,
                })
                .collect();
            raw.exons
                .entry(chrom.clone())
                .or_default()
                .extend(exons.iter().copied());
            raw.transcripts
                .entry(chrom.clone())
                .or_default()
                .push(NamedInterval {
                    start: txs,
                    end: txe,
                    name: f[3].to_owned(),
                });
            raw.genes.entry(chrom.clone()).or_default().push((
                Interval {
                    start: txs,
                    end: txe,
                },
                strand,
            ));
            if exons.len() > 1 {
                for pair in exons.windows(2) {
                    raw.intron.entry(chrom.clone()).or_default().push(Interval {
                        start: pair[0].end,
                        end: pair[1].start,
                    });
                    raw.starts
                        .entry(chrom.clone())
                        .or_default()
                        .insert(pair[0].end);
                    raw.ends
                        .entry(chrom.clone())
                        .or_default()
                        .insert(pair[1].start);
                    raw.known_junctions
                        .insert((chrom.clone(), pair[0].end, pair[1].start));
                }
            }
            for e in &exons {
                let coding = Interval {
                    start: e.start.max(thick_s),
                    end: e.end.min(thick_e),
                };
                if coding.start < coding.end {
                    raw.cds.entry(chrom.clone()).or_default().push(coding);
                }
                for u in [
                    Interval {
                        start: e.start,
                        end: e.end.min(thick_s),
                    },
                    Interval {
                        start: e.start.max(thick_e),
                        end: e.end,
                    },
                ] {
                    if u.start < u.end {
                        if (u.end <= thick_s) == (strand == '+') {
                            raw.utr5.entry(chrom.clone()).or_default().push(u);
                        } else {
                            raw.utr3.entry(chrom.clone()).or_default().push(u);
                        }
                    }
                }
            }
            let (up, down) = if strand == '+' {
                (txs, txe)
            } else {
                (txe, txs)
            };
            for (n, upmap, downmap) in [
                (1000, &mut raw.up1, &mut raw.down1),
                (5000, &mut raw.up5, &mut raw.down5),
                (10000, &mut raw.up10, &mut raw.down10),
            ] {
                if strand == '+' {
                    upmap.entry(chrom.clone()).or_default().push(Interval {
                        // BED intervals cannot extend before the start of a chromosome;
                        // BED.getIntergenic clips these before unionBed3/cal_size.
                        start: (up - n).max(0),
                        end: up,
                    });
                    downmap.entry(chrom.clone()).or_default().push(Interval {
                        start: down,
                        end: down + n,
                    });
                } else {
                    upmap.entry(chrom.clone()).or_default().push(Interval {
                        start: up,
                        end: up + n,
                    });
                    downmap.entry(chrom.clone()).or_default().push(Interval {
                        start: (down - n).max(0),
                        end: down,
                    });
                }
            }
        }
        let cds = normalize(raw.cds);
        let utr5 = subtract(normalize(raw.utr5), &cds);
        let utr3 = subtract(normalize(raw.utr3), &cds);
        let intron = subtract_many(normalize(raw.intron), &[&cds, &utr5, &utr3]);
        let up1 = subtract_many(normalize(raw.up1), &[&cds, &utr5, &utr3, &intron]);
        let up5 = subtract_many(normalize(raw.up5), &[&cds, &utr5, &utr3, &intron]);
        let up10 = subtract_many(normalize(raw.up10), &[&cds, &utr5, &utr3, &intron]);
        let down1 = subtract_many(normalize(raw.down1), &[&cds, &utr5, &utr3, &intron]);
        let down5 = subtract_many(normalize(raw.down5), &[&cds, &utr5, &utr3, &intron]);
        let down10 = subtract_many(normalize(raw.down10), &[&cds, &utr5, &utr3, &intron]);
        // Intersecter is a sorted interval index.  Keep equivalent sorted arrays and a
        // prefix maximum so lookups start near the query rather than scan a chromosome.
        for genes in raw.genes.values_mut() {
            genes.sort_unstable_by_key(|(x, _)| x.start);
        }
        for transcripts in raw.transcripts.values_mut() {
            transcripts.sort_unstable_by_key(|x| x.start);
        }
        let gene_prefix_max = raw
            .genes
            .iter()
            .map(|(chrom, items)| {
                let mut max = i32::MIN;
                (
                    chrom.clone(),
                    items
                        .iter()
                        .map(|(x, _)| {
                            max = max.max(x.end);
                            max
                        })
                        .collect(),
                )
            })
            .collect();
        let transcript_prefix_max = raw
            .transcripts
            .iter()
            .map(|(chrom, items)| {
                let mut max = i32::MIN;
                (
                    chrom.clone(),
                    items
                        .iter()
                        .map(|x| {
                            max = max.max(x.end);
                            max
                        })
                        .collect(),
                )
            })
            .collect();
        Ok(Self {
            cds,
            utr5,
            utr3,
            intron,
            up1,
            up5,
            up10,
            down1,
            down5,
            down10,
            intron_starts: raw.starts,
            intron_ends: raw.ends,
            genes: raw.genes,
            gene_prefix_max,
            known_junctions: raw.known_junctions,
            exons: normalize(raw.exons),
            transcripts: raw.transcripts,
            transcript_prefix_max,
        })
    }

    /// Resolve the BED's normalized chromosome names once.  The record reductions then
    /// need only a tid array lookup, rather than a string hash lookup for every block.
    fn for_tids(&self, chroms: &[String]) -> TidBedModel {
        let intervals = |map: &HashMap<String, Vec<Interval>>| {
            chroms
                .iter()
                .map(|chrom| map.get(chrom).cloned().unwrap_or_default())
                .collect()
        };
        let sites = |map: &HashMap<String, HashSet<i32>>| {
            chroms
                .iter()
                .map(|chrom| {
                    let mut values: Vec<_> =
                        map.get(chrom).into_iter().flatten().copied().collect();
                    values.sort_unstable();
                    values
                })
                .collect()
        };
        TidBedModel {
            cds: intervals(&self.cds),
            intron: intervals(&self.intron),
            utr5: intervals(&self.utr5),
            utr3: intervals(&self.utr3),
            up1: intervals(&self.up1),
            up5: intervals(&self.up5),
            up10: intervals(&self.up10),
            down1: intervals(&self.down1),
            down5: intervals(&self.down5),
            down10: intervals(&self.down10),
            intron_starts: sites(&self.intron_starts),
            intron_ends: sites(&self.intron_ends),
            known_junctions: self
                .known_junctions
                .iter()
                .filter_map(|(chrom, start, end)| {
                    chroms
                        .iter()
                        .position(|name| name == chrom)
                        .map(|tid| (tid as u32, *start, *end))
                })
                .collect(),
            chroms: chroms.to_vec(),
        }
    }
}

struct TidBedModel {
    cds: Vec<Vec<Interval>>,
    intron: Vec<Vec<Interval>>,
    utr5: Vec<Vec<Interval>>,
    utr3: Vec<Vec<Interval>>,
    up1: Vec<Vec<Interval>>,
    up5: Vec<Vec<Interval>>,
    up10: Vec<Vec<Interval>>,
    down1: Vec<Vec<Interval>>,
    down5: Vec<Vec<Interval>>,
    down10: Vec<Vec<Interval>>,
    intron_starts: Vec<Vec<i32>>,
    intron_ends: Vec<Vec<i32>>,
    known_junctions: HashSet<(u32, i32, i32)>,
    chroms: Vec<String>,
}

impl TidBedModel {
    fn contains(map: &[Vec<Interval>], tid: i32, point: i32) -> bool {
        let Some(items) = map.get(tid as usize) else {
            return false;
        };
        // `bx.intervals.Intersecter.find(p, p)` treats the zero-width query
        // as an open point: an interval hits only when `start < p < end`.
        let at = items.partition_point(|x| x.start < point);
        at > 0 && items[at - 1].end > point
    }

    fn junction_kind(&self, tid: i32, start: i32, end: i32) -> JunctionKind {
        let start_known = self
            .intron_starts
            .get(tid as usize)
            .is_some_and(|v| v.binary_search(&start).is_ok());
        let end_known = self
            .intron_ends
            .get(tid as usize)
            .is_some_and(|v| v.binary_search(&end).is_ok());
        match (start_known, end_known) {
            (true, true) => JunctionKind::Known,
            (false, false) => JunctionKind::Novel,
            _ => JunctionKind::Partial,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JunctionKind {
    Known,
    Partial,
    Novel,
}
#[derive(Default)]
struct RawBed {
    cds: HashMap<String, Vec<Interval>>,
    exons: HashMap<String, Vec<Interval>>,
    transcripts: HashMap<String, Vec<NamedInterval>>,
    intron: HashMap<String, Vec<Interval>>,
    utr5: HashMap<String, Vec<Interval>>,
    utr3: HashMap<String, Vec<Interval>>,
    up1: HashMap<String, Vec<Interval>>,
    up5: HashMap<String, Vec<Interval>>,
    up10: HashMap<String, Vec<Interval>>,
    down1: HashMap<String, Vec<Interval>>,
    down5: HashMap<String, Vec<Interval>>,
    down10: HashMap<String, Vec<Interval>>,
    starts: HashMap<String, HashSet<i32>>,
    ends: HashMap<String, HashSet<i32>>,
    genes: HashMap<String, Vec<(Interval, char)>>,
    known_junctions: HashSet<(String, i32, i32)>,
}
fn normalize(mut maps: HashMap<String, Vec<Interval>>) -> HashMap<String, Vec<Interval>> {
    for v in maps.values_mut() {
        v.sort_unstable_by_key(|x| x.start);
        let mut merged: Vec<Interval> = Vec::new();
        for x in v.drain(..) {
            if let Some(last) = merged.last_mut()
                && x.start <= last.end
            {
                last.end = last.end.max(x.end);
            } else {
                merged.push(x);
            }
        }
        *v = merged;
    }
    maps
}
fn subtract(
    mut a: HashMap<String, Vec<Interval>>,
    b: &HashMap<String, Vec<Interval>>,
) -> HashMap<String, Vec<Interval>> {
    for (chr, items) in &mut a {
        let mut result = Vec::new();
        for x in std::mem::take(items) {
            let mut cursor = x.start;
            if let Some(cuts) = b.get(chr) {
                for y in cuts {
                    if y.end <= cursor {
                        continue;
                    }
                    if y.start >= x.end {
                        break;
                    }
                    if y.start > cursor {
                        result.push(Interval {
                            start: cursor,
                            end: y.start.min(x.end),
                        });
                    }
                    cursor = cursor.max(y.end);
                    if cursor >= x.end {
                        break;
                    }
                }
            }
            if cursor < x.end {
                result.push(Interval {
                    start: cursor,
                    end: x.end,
                });
            }
        }
        *items = result;
    }
    a
}
fn subtract_many(
    mut a: HashMap<String, Vec<Interval>>,
    bs: &[&HashMap<String, Vec<Interval>>],
) -> HashMap<String, Vec<Interval>> {
    for b in bs {
        a = subtract(a, b)
    }
    a
}
fn bases(map: &HashMap<String, Vec<Interval>>) -> i64 {
    map.values()
        .flatten()
        .map(|x| i64::from(x.end - x.start))
        .sum()
}

/// BAM reference names are invariant across all QC reductions.  Materializing their
/// normalized spelling once removes millions of short-lived `String` allocations.
fn chromosome_names(resident: &Resident) -> Vec<String> {
    resident
        .header
        .reference_sequences()
        .iter()
        .map(|(name, _)| name.to_string().to_ascii_uppercase())
        .collect()
}

/// Bitset of strands for genes overlapping the query: `1` is '+', `2` is '-'.
/// `infer_experiment` only needs that three-state result; avoiding a per-read HashSet
/// is especially important for its 200k-read coordinate-order prefix.
fn overlapping_gene_strands(model: &BedModel, chrom: &str, start: i32, end: i32) -> u8 {
    let Some(items) = model.genes.get(chrom) else {
        return 0;
    };
    let Some(prefix) = model.gene_prefix_max.get(chrom) else {
        return 0;
    };
    let mut at = items.partition_point(|(x, _)| x.start < end);
    let mut strands = 0;
    while at > 0 {
        at -= 1;
        if prefix[at] <= start {
            break;
        }
        let (x, strand) = items[at];
        if x.end > start {
            strands |= if strand == '+' { 1 } else { 2 };
            if strands == 3 {
                break;
            }
        }
    }
    strands
}

fn transcript_names_at<'a>(model: &'a BedModel, chrom: &str, point: i32) -> HashSet<&'a str> {
    let Some(items) = model.transcripts.get(chrom) else {
        return HashSet::new();
    };
    let Some(prefix) = model.transcript_prefix_max.get(chrom) else {
        return HashSet::new();
    };
    let mut at = items.partition_point(|x| x.start < point + 1);
    let mut names = HashSet::new();
    while at > 0 {
        at -= 1;
        if prefix[at] <= point {
            break;
        }
        let x = &items[at];
        if x.start < point + 1 && x.end > point {
            names.insert(x.name.as_str());
        }
    }
    names
}
fn cigar_exons(resident: &Resident, fixed: super::RecordHeader) -> Result<Vec<Interval>> {
    let mut p = fixed.pos;
    let mut out = Vec::new();
    for (n, op) in bam_cigar(resident.record_bytes(fixed))? {
        match op {
            'M' => {
                out.push(Interval {
                    start: p,
                    end: p + n,
                });
                p += n
            }
            'D' | 'N' | 'S' => p += n,
            _ => {}
        }
    }
    Ok(out)
}
fn cigar_introns(resident: &Resident, fixed: super::RecordHeader) -> Result<Vec<Interval>> {
    let mut p = fixed.pos;
    let mut out = Vec::new();
    for (n, op) in bam_cigar(resident.record_bytes(fixed))? {
        match op {
            'N' => {
                out.push(Interval {
                    start: p,
                    end: p + n,
                });
                p += n
            }
            'M' | 'D' | '=' | 'X' => p += n,
            _ => {}
        }
    }
    Ok(out)
}

fn read_distribution(
    resident: &Resident,
    duplicates: &HashSet<usize>,
    model: &BedModel,
    tid_model: &TidBedModel,
) -> Result<String> {
    #[derive(Default)]
    struct Counts {
        n: [i64; 10],
        tags: i64,
        unassigned: i64,
        reads: i64,
    }
    let partials: Vec<Counts> = resident
        .coordinate_order()
        .par_chunks(16_384)
        .map(|chunk| -> Result<_> {
            let mut out = Counts::default();
            for &record_index in chunk {
                let index = record_index as usize;
                let fixed = resident.headers()[index];
                if fixed.flag & 0x304 != 0 || duplicates.contains(&index) {
                    continue;
                }
                out.reads += 1;
                for ex in cigar_exons(resident, fixed)? {
                    out.tags += 1;
                    let p = ex.start + (ex.end - ex.start) / 2;
                    let group = if TidBedModel::contains(&tid_model.cds, fixed.tid, p) {
                        Some(0)
                    } else if TidBedModel::contains(&tid_model.utr5, fixed.tid, p)
                        && !TidBedModel::contains(&tid_model.utr3, fixed.tid, p)
                    {
                        Some(1)
                    } else if TidBedModel::contains(&tid_model.utr3, fixed.tid, p)
                        && !TidBedModel::contains(&tid_model.utr5, fixed.tid, p)
                    {
                        Some(2)
                    } else if TidBedModel::contains(&tid_model.utr5, fixed.tid, p)
                        || TidBedModel::contains(&tid_model.utr3, fixed.tid, p)
                    {
                        None
                    } else if TidBedModel::contains(&tid_model.intron, fixed.tid, p) {
                        Some(3)
                    } else if TidBedModel::contains(&tid_model.up10, fixed.tid, p)
                        && TidBedModel::contains(&tid_model.down10, fixed.tid, p)
                    {
                        None
                    } else if TidBedModel::contains(&tid_model.up1, fixed.tid, p) {
                        out.n[4] += 1;
                        out.n[5] += 1;
                        out.n[6] += 1;
                        continue;
                    } else if TidBedModel::contains(&tid_model.up5, fixed.tid, p) {
                        out.n[5] += 1;
                        out.n[6] += 1;
                        continue;
                    } else if TidBedModel::contains(&tid_model.up10, fixed.tid, p) {
                        Some(6)
                    } else if TidBedModel::contains(&tid_model.down1, fixed.tid, p) {
                        out.n[7] += 1;
                        out.n[8] += 1;
                        out.n[9] += 1;
                        continue;
                    } else if TidBedModel::contains(&tid_model.down5, fixed.tid, p) {
                        out.n[8] += 1;
                        out.n[9] += 1;
                        continue;
                    } else if TidBedModel::contains(&tid_model.down10, fixed.tid, p) {
                        Some(9)
                    } else {
                        None
                    };
                    if let Some(i) = group {
                        out.n[i] += 1
                    } else {
                        out.unassigned += 1
                    }
                }
            }
            Ok(out)
        })
        .collect::<Result<_>>()?;
    let mut counts = Counts::default();
    for part in partials {
        counts.tags += part.tags;
        counts.unassigned += part.unassigned;
        counts.reads += part.reads;
        for (a, b) in counts.n.iter_mut().zip(part.n) {
            *a += b;
        }
    }
    let maps = [
        &model.cds,
        &model.utr5,
        &model.utr3,
        &model.intron,
        &model.up1,
        &model.up5,
        &model.up10,
        &model.down1,
        &model.down5,
        &model.down10,
    ];
    let names = [
        "CDS_Exons",
        "5'UTR_Exons",
        "3'UTR_Exons",
        "Introns",
        "TSS_up_1kb",
        "TSS_up_5kb",
        "TSS_up_10kb",
        "TES_down_1kb",
        "TES_down_5kb",
        "TES_down_10kb",
    ];
    let mut text = format!(
        "Total Reads                   {}\nTotal Tags                    {}\nTotal Assigned Tags           {}\n=====================================================================\nGroup               Total_bases         Tag_count           Tags/Kb             \n",
        counts.reads,
        counts.tags,
        counts.tags - counts.unassigned
    );
    for ((name, map), count) in names.iter().zip(maps).zip(counts.n) {
        let size = bases(map);
        text.push_str(&format!(
            "{name:<20}{size:<20}{count:<20}{:<18.2}\n",
            count as f64 * 1000.0 / (size + 1) as f64
        ));
    }
    text.push_str("=====================================================================\n");
    Ok(text)
}

#[derive(Clone, Copy)]
struct JunctionEvent {
    tid: u32,
    start: i32,
    end: i32,
    order: u32,
}
struct JunctionRun {
    tid: u32,
    start: i32,
    end: i32,
    count: u64,
    first: u32,
}
#[derive(Default)]
struct JunctionTotals {
    total: u64,
    known: u64,
    partial: u64,
    novel: u64,
    filtered: u64,
}
struct Junctions {
    events: Vec<JunctionEvent>,
    runs: Vec<JunctionRun>,
    totals: JunctionTotals,
}

fn collect_junctions(
    resident: &Resident,
    duplicates: &HashSet<usize>,
    model: &TidBedModel,
) -> Result<Junctions> {
    let partials: Vec<(Vec<JunctionEvent>, JunctionTotals)> = resident
        .coordinate_order()
        .par_chunks(16_384)
        .map(|chunk| -> Result<_> {
            let mut events = Vec::new();
            let mut totals = JunctionTotals::default();
            for &record_index in chunk {
                let index = record_index as usize;
                let fixed = resident.headers()[index];
                if fixed.flag & 0x304 != 0 || duplicates.contains(&index) || fixed.mapq < 30 {
                    continue;
                }
                for intron in cigar_introns(resident, fixed)? {
                    totals.total += 1;
                    if intron.end - intron.start < 50 {
                        totals.filtered += 1;
                        continue;
                    }
                    match model.junction_kind(fixed.tid, intron.start, intron.end) {
                        JunctionKind::Known => totals.known += 1,
                        JunctionKind::Partial => totals.partial += 1,
                        JunctionKind::Novel => totals.novel += 1,
                    }
                    events.push(JunctionEvent {
                        tid: fixed.tid as u32,
                        start: intron.start,
                        end: intron.end,
                        order: 0,
                    });
                }
            }
            Ok((events, totals))
        })
        .collect::<Result<_>>()?;
    let mut events = Vec::new();
    let mut totals = JunctionTotals::default();
    for (part, part_totals) in partials {
        for mut event in part {
            event.order = events.len() as u32;
            events.push(event);
        }
        totals.total += part_totals.total;
        totals.known += part_totals.known;
        totals.partial += part_totals.partial;
        totals.novel += part_totals.novel;
        totals.filtered += part_totals.filtered;
    }
    let mut sorted = events.clone();
    sorted.par_sort_unstable_by_key(|x| (x.tid, x.start, x.end, x.order));
    let mut runs: Vec<JunctionRun> = Vec::new();
    for event in sorted {
        if let Some(last) = runs.last_mut()
            && (last.tid, last.start, last.end) == (event.tid, event.start, event.end)
        {
            last.count += 1;
        } else {
            runs.push(JunctionRun {
                tid: event.tid,
                start: event.start,
                end: event.end,
                count: 1,
                first: event.order,
            });
        }
    }
    runs.par_sort_unstable_by_key(|x| x.first);
    Ok(Junctions {
        events,
        runs,
        totals,
    })
}

fn junction_annotation(
    junctions: &Junctions,
    model: &TidBedModel,
    bed_path: &str,
) -> Result<(String, String)> {
    let mut jc = [0; 3];
    let mut xls =
        String::from("chrom\tintron_st(0-based)\tintron_end(1-based)\tread_count\tannotation\n");
    for run in &junctions.runs {
        let label = match model.junction_kind(run.tid as i32, run.start, run.end) {
            JunctionKind::Known => {
                jc[0] += 1;
                "annotated"
            }
            JunctionKind::Partial => {
                jc[1] += 1;
                "partial_novel"
            }
            JunctionKind::Novel => {
                jc[2] += 1;
                "complete_novel"
            }
        };
        xls.push_str(&format!(
            "{}\t{}\t{}\t{}\t {label}\n",
            model.chroms[run.tid as usize].replace("CHR", "chr"),
            run.start,
            run.end,
            run.count
        ));
    }
    let log = format!(
        "Reading reference bed file:  {bed_path}  ...  Done\nLoad BAM file ...  Done\n\n===================================================================\nTotal splicing  Events:\t{}\nKnown Splicing Events:\t{}\nPartial Novel Splicing Events:\t{}\nNovel Splicing Events:\t{}\nFiltered Splicing Events:\t{}\n\nTotal splicing  Junctions:\t{}\nKnown Splicing Junctions:\t{}\nPartial Novel Splicing Junctions:\t{}\nNovel Splicing Junctions:\t{}\n\n===================================================================\nCreate BED file ...\nCreate Interact file ...\n",
        junctions.totals.total,
        junctions.totals.known,
        junctions.totals.partial,
        junctions.totals.novel,
        junctions.totals.filtered,
        jc.iter().sum::<u64>(),
        jc[0],
        jc[1],
        jc[2]
    );
    Ok((xls, log))
}

/// RSeQC's source shuffles individual splice events before accumulating 5% chunks.  The
/// final chunk necessarily contains every event, so its three totals are deterministic.  We
/// use a documented, local Fisher--Yates seed for the intentionally non-golden earlier points.
fn junction_saturation(
    events: &[JunctionEvent],
    model: &TidBedModel,
    sample: &str,
) -> Result<String> {
    let mut events = events.to_vec();
    // xorshift64* is deliberately tiny and stable across Rust releases.  This is only for
    // RSeQC's inherently random 5--95% samples; the 100% point is independent of it.
    let mut state = 0x5253_4551_435f_5341_u64;
    for i in (1..events.len()).rev() {
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        let j = (state.wrapping_mul(0x2545_f491_4f6c_dd1d) as usize) % (i + 1);
        events.swap(i, j);
    }
    let mut seen = HashSet::<(u32, i32, i32)>::new();
    let mut known_count = 0;
    let mut known = Vec::new();
    let mut all = Vec::new();
    let mut novel = Vec::new();
    for percent in (5..=100).step_by(5) {
        let begin = events.len() * (percent - 5) / 100;
        let end = events.len() * percent / 100;
        for event in &events[begin..end] {
            if seen.insert((event.tid, event.start, event.end))
                && model
                    .known_junctions
                    .contains(&(event.tid, event.start, event.end))
            {
                known_count += 1;
            }
        }
        all.push(seen.len());
        known.push(known_count);
        novel.push(all.last().copied().unwrap() - known.last().copied().unwrap());
    }
    let csv = |v: &[usize]| v.iter().map(usize::to_string).collect::<Vec<_>>().join(",");
    Ok(format!(
        "pdf('{sample}.junctionSaturation_plot.pdf')\nx=c(5,10,15,20,25,30,35,40,45,50,55,60,65,70,75,80,85,90,95,100)\ny=c({})\nz=c({})\nw=c({})\nm=max({},{},{})\nn=min({},{},{})\nplot(x,z/1000,xlab='percent of total reads',ylab='Number of splicing junctions (x1000)',type='o',col='blue',ylim=c(n,m))\npoints(x,y/1000,type='o',col='red')\npoints(x,w/1000,type='o',col='green')\nlegend(5,{}, legend=c(\"All junctions\",\"known junctions\", \"novel junctions\"),col=c(\"blue\",\"red\",\"green\"),lwd=1,pch=1)\ndev.off()\n",
        csv(&known),
        csv(&all),
        csv(&novel),
        known[19] / 1000,
        all[19] / 1000,
        novel[19] / 1000,
        known[0] / 1000,
        all[0] / 1000,
        novel[0] / 1000,
        all[19] / 1000
    ))
}

fn infer_experiment(
    resident: &Resident,
    duplicates: &HashSet<usize>,
    model: &BedModel,
) -> Result<String> {
    let mut counts = HashMap::<String, u64>::new();
    let mut sampled = 0_u64;
    let chroms = chromosome_names(resident);
    for &record_index in resident.coordinate_order() {
        if sampled >= 200_000 {
            break;
        }
        let index = record_index as usize;
        let fixed = resident.headers()[index];
        if fixed.flag & 0x304 != 0 || duplicates.contains(&index) || fixed.mapq < 30 {
            continue;
        }
        let chr = chroms
            .get(fixed.tid as usize)
            .map(String::as_str)
            .unwrap_or("");
        let end = fixed.pos + le_i32(&resident.record_bytes(fixed)[16..20]);
        let strands = overlapping_gene_strands(model, chr, fixed.pos, end);
        if strands == 0 {
            continue;
        }
        sampled += 1;
        let gene = if strands == 1 {
            "+"
        } else if strands == 2 {
            "-"
        } else {
            // RSeQC uses `':'.join(set(...))`; on its reference runtime the
            // two-strand case is `-:+`, which is deliberately not one of the
            // recognized protocol buckets below.
            "-:+"
        };
        let rid = if fixed.flag & 0x40 != 0 { '1' } else { '2' };
        let map = if fixed.flag & 0x10 != 0 { '-' } else { '+' };
        *counts.entry(format!("{rid}{map}{gene}")).or_default() += 1;
    }
    let total: u64 = counts.values().sum();
    let a = ["1++", "1--", "2+-", "2-+"]
        .iter()
        .map(|x| counts.get(*x).copied().unwrap_or(0))
        .sum::<u64>();
    let b = ["1+-", "1-+", "2++", "2--"]
        .iter()
        .map(|x| counts.get(*x).copied().unwrap_or(0))
        .sum::<u64>();
    Ok(format!(
        "\n\nThis is PairEnd Data\nFraction of reads failed to determine: {:.4}\nFraction of reads explained by \"1++,1--,2+-,2-+\": {:.4}\nFraction of reads explained by \"1+-,1-+,2++,2--\": {:.4}\n",
        (total - a - b) as f64 / total as f64,
        a as f64 / total as f64,
        b as f64 / total as f64
    ))
}

fn render_histogram(histogram: &HashMap<u32, u64>) -> String {
    let mut entries = histogram.iter().collect::<Vec<_>>();
    entries.sort_unstable_by_key(|(occurrence, _)| **occurrence);
    let mut text = String::from("Occurrence\tUniqReadNumber\n");
    for (occurrence, count) in entries {
        text.push_str(&format!("{occurrence}\t{count}\n"));
    }
    text
}
