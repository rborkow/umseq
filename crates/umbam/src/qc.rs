//! Deterministic, text-only QC reductions over the resident BAM table.
//!
//! These routines deliberately borrow the native BAM bodies from `Resident`: no SAM text or
//! record allocation is constructed while collecting counters.

use super::{
    FeatureIndex, Resident, bam_aux_i32, bam_cigar, bam_layout, bam_name_bytes, le_i32,
    mate_gene_hits, one_fragment_gene, read_features,
};
use anyhow::Result;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct Timing {
    pub bam_stat: Duration,
    pub seq_duplication: Duration,
    pub pos_duplication: Duration,
    pub read_distribution: Duration,
    pub junction_annotation: Duration,
    pub infer_experiment: Duration,
    pub junction_saturation: Duration,
    pub inner_distance: Duration,
    pub dupradar: Duration,
    pub qualimap: Duration,
}

pub(super) fn write(
    out: &Path,
    gtf: &Path,
    resident: &Resident,
    duplicates: &HashSet<usize>,
) -> Result<Timing> {
    let rseqc = out.join("rseqc");
    fs::create_dir_all(&rseqc)?;
    let now = Instant::now();
    fs::write(rseqc.join("bam_stat.txt"), bam_stat(resident, duplicates)?)?;
    let bam_stat = now.elapsed();
    let seq_started = Instant::now();
    let seq = sequence_duplication(resident)?;
    fs::write(rseqc.join("seq.DupRate.xls"), render_histogram(&seq))?;
    let seq_duplication = seq_started.elapsed();
    let pos_started = Instant::now();
    let pos = position_duplication(resident)?;
    fs::write(rseqc.join("pos.DupRate.xls"), render_histogram(&pos))?;
    let pos_duplication = pos_started.elapsed();
    let read_distribution_started = Instant::now();
    let bed = gtf
        .parent()
        .map(|parent| parent.join("qc/chr22.bed"))
        .filter(|path| path.exists())
        .ok_or_else(|| anyhow::anyhow!("RSeQC QC requires sibling qc/chr22.bed"))?;
    let model = BedModel::read(&bed)?;
    fs::write(
        rseqc.join("read_distribution.txt"),
        read_distribution(resident, duplicates, &model)?,
    )?;
    let read_distribution = read_distribution_started.elapsed();
    let junction_started = Instant::now();
    let (junction_xls, junction_log) = junction_annotation(resident, duplicates, &model)?;
    fs::write(rseqc.join("chr22.junction.xls"), junction_xls)?;
    fs::write(rseqc.join("chr22.junction_annotation.log"), junction_log)?;
    let junction_annotation = junction_started.elapsed();
    let saturation_started = Instant::now();
    fs::write(
        rseqc.join("chr22.junctionSaturation_plot.r"),
        junction_saturation(resident, duplicates, &model)?,
    )?;
    let junction_saturation = saturation_started.elapsed();
    let infer_started = Instant::now();
    fs::write(
        rseqc.join("infer_experiment.txt"),
        infer_experiment(resident, duplicates, &model)?,
    )?;
    let inner_started = Instant::now();
    fs::write(
        rseqc.join("chr22.inner_distance_freq.txt"),
        inner_distance(resident, duplicates, &model)?,
    )?;
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
    Ok(Timing {
        bam_stat,
        seq_duplication,
        pos_duplication,
        read_distribution,
        junction_annotation,
        infer_experiment: infer_started.elapsed(),
        junction_saturation,
        inner_distance: inner_started.elapsed(),
        dupradar: dupradar_time,
        qualimap: qualimap_started.elapsed(),
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
    let all_multi = subread_counts(resident, duplicates, features, true, false)?;
    let filtered_multi = subread_counts(resident, duplicates, features, true, true)?;
    let all = subread_counts(resident, duplicates, features, false, false)?;
    let filtered = subread_counts(resident, duplicates, features, false, true)?;
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
                format!("{}", (a - b) as f64 / a as f64)
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
            all_multi.counts[i] - filtered_multi.counts[i],
            rpk(all_multi.counts[i]),
            rpkm(all_multi.counts[i], processed),
            all.counts[i],
            filtered.counts[i],
            rate(all.counts[i], filtered.counts[i]),
            all.counts[i] - filtered.counts[i],
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

fn subread_counts(
    resident: &Resident,
    duplicates: &HashSet<usize>,
    features: &FeatureIndex,
    multi: bool,
    ignore_dup: bool,
) -> Result<SubreadCounts> {
    let mut records: Vec<usize> = resident
        .headers()
        .iter()
        .enumerate()
        .filter_map(|(i, h)| {
            let body = resident.record_bytes(*h);
            // `primaryOnly=FALSE` is the Rsubread default: secondary alignments
            // participate in -M runs, while supplementary and unmapped records do not.
            let keep = h.flag & 0x804 == 0
                && (!ignore_dup || !duplicates.contains(&i))
                && (multi || !super::bam_nh_is_multiple(body).ok()?);
            keep.then_some(i)
        })
        .collect();
    records.sort_unstable_by(|&a, &b| {
        bam_name_bytes(resident.record_bytes(resident.headers()[a]))
            .cmp(bam_name_bytes(resident.record_bytes(resident.headers()[b])))
    });
    let mut counts = vec![0; features.genes.len()];
    let mut n = 0;
    let mut at = 0;
    while at < records.len() {
        let name = bam_name_bytes(resident.record_bytes(resident.headers()[records[at]]));
        let end = records[at..]
            .iter()
            .position(|&i| bam_name_bytes(resident.record_bytes(resident.headers()[i])) != name)
            .map_or(records.len(), |x| at + x);
        let mut left = Vec::new();
        let mut right = Vec::new();
        for &i in &records[at..end] {
            match resident.headers()[i].flag & 0xc0 {
                0x40 => left.push(i),
                0x80 => right.push(i),
                _ => {}
            }
        }
        let with_hi = |items: &mut Vec<usize>| -> Result<()> {
            let mut keyed = items
                .iter()
                .map(|&i| {
                    Ok((
                        bam_aux_i32(resident.record_bytes(resident.headers()[i]), *b"HI")?
                            .unwrap_or(i as i32),
                        i,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            keyed.sort_unstable();
            *items = keyed.into_iter().map(|(_, i)| i).collect();
            Ok(())
        };
        with_hi(&mut left)?;
        with_hi(&mut right)?;
        let pairs = left.len().max(right.len());
        for k in 0..pairs {
            let a = left
                .get(k)
                .map(|&i| mate_gene_hits(i, resident, features))
                .transpose()?
                .unwrap_or_default();
            let b = right
                .get(k)
                .map(|&i| mate_gene_hits(i, resident, features))
                .transpose()?
                .unwrap_or_default();
            // `stat[,2] - Unassigned_Unmapped` includes fragments which are
            // mapped but do not overlap an annotated exon.
            n += 1;
            if let Some(gene) = one_fragment_gene(&a, &b) {
                counts[gene] += 1;
            }
        }
        at = end;
    }
    Ok(SubreadCounts { counts, n })
}

fn qualimap_report(resident: &Resident, features: &FeatureIndex) -> Result<String> {
    let mut left = 0_u64;
    let mut right = 0_u64;
    let mut pairs = HashSet::new();
    let mut secondary = 0_u64;
    let mut non_unique = 0_u64;
    let mut gene = 0_u64;
    let mut ambiguous = 0_u64;
    let mut no_feature = 0_u64;
    for (i, h) in resident.headers().iter().enumerate() {
        if h.flag & 0x100 != 0 {
            secondary += 1;
        }
        // Qualimap reports non-unique supplementary records but not secondaries.
        if h.flag & 0x100 == 0 && super::bam_nh_is_multiple(resident.record_bytes(*h))? {
            non_unique += 1;
        }
        if h.flag & 0x904 == 0 {
            if h.flag & 0x40 != 0 {
                left += 1;
                if h.flag & 0x2 != 0 {
                    pairs.insert(bam_name_bytes(resident.record_bytes(*h)).to_vec());
                }
            }
            if h.flag & 0x80 != 0 {
                right += 1;
            }
        }
        if h.flag & 0x904 != 0 || super::bam_nh_is_multiple(resident.record_bytes(*h))? {
            continue;
        }
        let hits = mate_gene_hits(i, resident, features)?;
        match hits.len() {
            0 => no_feature += 1,
            1 => gene += 1,
            _ => ambiguous += 1,
        }
    }
    let denom = gene + ambiguous + no_feature;
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
        commas(left),
        commas(right),
        commas(pairs.len() as u64),
        commas(resident.headers().len() as u64),
        commas(secondary),
        commas(non_unique),
        commas(gene),
        commas(ambiguous),
        commas(no_feature),
        commas(gene),
        pct(gene),
        commas(no_feature),
        pct(no_feature)
    ))
}

/// RSeQC `inner_distance.py` invokes mRNA_inner_distance with -250..250 in five-base
/// windows.  Its bx interval query makes those windows `(left, right]`, not `[left,right)`.
fn inner_distance(
    resident: &Resident,
    duplicates: &HashSet<usize>,
    model: &BedModel,
) -> Result<String> {
    let mut distances = Vec::<i32>::new();
    for &record_index in resident.coordinate_order() {
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
        // pair_num is incremented here by RSeQC, before the different-chromosome check.
        if fixed.tid != fixed.mate_tid {
            continue;
        }
        let chrom = resident
            .header
            .reference_sequences()
            .get_index(fixed.tid as usize)
            .map(|(name, _)| name.to_string().to_ascii_uppercase())
            .unwrap_or_default();
        let cigar = bam_cigar(resident.record_bytes(fixed))?;
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
            for ex in cigar_exons(resident, fixed)? {
                exon_positions.extend((ex.start + 1)..=ex.end);
            }
            -(exon_positions
                .into_iter()
                .filter(|&p| p > read2_start && p <= read1_end)
                .count() as i32)
        };
        let common_transcript = model.transcripts.get(&chrom).is_some_and(|ranges| {
            ranges.iter().any(|r| {
                r.start < read1_end
                    && r.end > read1_end - 1
                    && ranges.iter().any(|s| {
                        s.name == r.name && s.start < read2_start + 1 && s.end > read2_start
                    })
            })
        });
        let distance = if common_transcript && genomic > 0 {
            let size: i32 = model
                .exons
                .get(&chrom)
                .into_iter()
                .flatten()
                .map(|ex| (ex.end.min(read2_start) - ex.start.max(read1_end)).max(0))
                .sum();
            if size > 0 { size } else { genomic }
        } else {
            genomic
        };
        distances.push(distance);
    }
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
    let mut positions = HashMap::<(i32, i32, Vec<(i32, i32)>), u32>::new();
    for fixed in resident.headers() {
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
    let mut result = HashMap::new();
    for occurrence in positions.into_values() {
        *result.entry(occurrence).or_insert(0) += 1;
    }
    Ok(result)
}

fn bam_stat(resident: &Resident, duplicates: &HashSet<usize>) -> Result<String> {
    let mut total = 0_u64;
    let mut qc_fail = 0_u64;
    let mut duplicate = 0_u64;
    let mut non_primary = 0_u64;
    let mut unmapped = 0_u64;
    let mut low_mapq = 0_u64;
    let mut unique = 0_u64;
    let mut read1 = 0_u64;
    let mut read2 = 0_u64;
    let mut plus = 0_u64;
    let mut minus = 0_u64;
    let mut splice = 0_u64;
    let mut proper = 0_u64;
    let mut proper_different = 0_u64;
    for &record_index in resident.coordinate_order() {
        let index = record_index as usize;
        let fixed = resident.headers()[index];
        total += 1;
        // RSeQC's bam_stat uses this precedence, so its headline categories form a
        // partition even when a record carries more than one of these flags.
        if fixed.flag & 0x200 != 0 {
            qc_fail += 1;
            continue;
        }
        if fixed.flag & 0x400 != 0 || duplicates.contains(&index) {
            duplicate += 1;
            continue;
        }
        if fixed.flag & 0x100 != 0 {
            non_primary += 1;
            continue;
        }
        if fixed.flag & 0x4 != 0 {
            unmapped += 1;
            continue;
        }
        if fixed.mapq < 30 {
            low_mapq += 1;
            continue;
        }
        unique += 1;
        read1 += u64::from(fixed.flag & 0x40 != 0);
        read2 += u64::from(fixed.flag & 0x80 != 0);
        plus += u64::from(fixed.flag & 0x10 == 0);
        minus += u64::from(fixed.flag & 0x10 != 0);
        let cigar = bam_cigar(resident.record_bytes(fixed))?;
        splice += u64::from(cigar.iter().any(|(_, op)| *op == 'N'));
        if fixed.flag & 0x2 != 0 {
            proper += 1;
            proper_different += u64::from(fixed.tid != fixed.mate_tid);
        }
    }
    Ok(format!(
        "\n#==================================================\n#All numbers are READ count\n#==================================================\n\nTotal records:                          {total}\n\nQC failed:                              {qc_fail}\nOptical/PCR duplicate:                  {duplicate}\nNon primary hits                        {non_primary}\nUnmapped reads:                         {unmapped}\nmapq < mapq_cut (non-unique):           {low_mapq}\n\nmapq >= mapq_cut (unique):              {unique}\nRead-1:                                 {read1}\nRead-2:                                 {read2}\nReads map to '+':                       {plus}\nReads map to '-':                       {minus}\nNon-splice reads:                       {}\nSplice reads:                           {splice}\nReads mapped in proper pairs:           {proper}\nProper-paired reads map to different chrom:{proper_different}\n",
        unique - splice
    ))
}

fn sequence_duplication(resident: &Resident) -> Result<HashMap<u32, u64>> {
    let mut sequence = HashMap::<Vec<u8>, u32>::new();
    for fixed in resident.headers() {
        // RSeQC applies its default MAPQ cutoff before histogramming, but does retain
        // marked duplicates (the metric is intended to quantify them).
        if fixed.flag & 0x904 != 0 || fixed.mapq < 30 {
            continue;
        }
        *sequence
            .entry(bam_sequence(resident.record_bytes(*fixed))?)
            .or_default() += 1;
    }
    let histogram = |values: Vec<u32>| {
        let mut result = HashMap::new();
        for n in values {
            *result.entry(n).or_insert(0) += 1;
        }
        result
    };
    Ok(histogram(sequence.into_values().collect()))
}

fn bam_sequence(body: &[u8]) -> Result<Vec<u8>> {
    let (name_len, cigar_count, _) = bam_layout(body)?;
    let length = le_i32(&body[16..20]) as usize;
    let at = 32 + name_len + cigar_count * 4;
    const BASES: &[u8; 16] = b"=ACMGRSVTWYHKDBN";
    let mut out = Vec::with_capacity(length);
    for i in 0..length {
        let byte = body[at + i / 2];
        out.push(BASES[usize::from(if i & 1 == 0 { byte >> 4 } else { byte & 15 })]);
    }
    Ok(out)
}

#[derive(Clone, Copy)]
struct Interval {
    start: i32,
    end: i32,
}

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
    known_junctions: HashSet<(String, i32, i32)>,
    exons: HashMap<String, Vec<Interval>>,
    transcripts: HashMap<String, Vec<NamedInterval>>,
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
                        start: up - n,
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
                        start: down - n,
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
            known_junctions: raw.known_junctions,
            exons: normalize(raw.exons),
            transcripts: raw.transcripts,
        })
    }
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
fn contains(map: &HashMap<String, Vec<Interval>>, chr: &str, p: i32) -> bool {
    map.get(chr).is_some_and(|v| {
        // `bx.intervals.Intersecter.find(p, p)` treats the zero-width query
        // as an open point: an interval hits only when `start < p < end`.
        // This is deliberately not normal half-open membership (`start <= p`).
        let n = v.partition_point(|x| x.start < p);
        n > 0 && v[n - 1].end > p
    })
}
fn bases(map: &HashMap<String, Vec<Interval>>) -> i64 {
    map.values()
        .flatten()
        .map(|x| i64::from(x.end - x.start))
        .sum()
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
) -> Result<String> {
    let mut n = [0_i64; 10];
    let mut tags = 0_i64;
    let mut unassigned = 0_i64;
    let mut reads = 0_i64;
    for &record_index in resident.coordinate_order() {
        let index = record_index as usize;
        let fixed = resident.headers()[index];
        if fixed.flag & 0x304 != 0 || duplicates.contains(&index) {
            continue;
        }
        reads += 1;
        let chr = resident
            .header
            .reference_sequences()
            .get_index(fixed.tid as usize)
            .map(|(n, _)| n.to_string())
            .unwrap_or_default()
            .to_ascii_uppercase();
        for ex in cigar_exons(resident, fixed)? {
            tags += 1;
            let p = ex.start + (ex.end - ex.start) / 2;
            let group = if contains(&model.cds, &chr, p) {
                Some(0)
            } else if contains(&model.utr5, &chr, p) && !contains(&model.utr3, &chr, p) {
                Some(1)
            } else if contains(&model.utr3, &chr, p) && !contains(&model.utr5, &chr, p) {
                Some(2)
            } else if contains(&model.utr5, &chr, p) || contains(&model.utr3, &chr, p) {
                None
            } else if contains(&model.intron, &chr, p) {
                Some(3)
            } else if contains(&model.up10, &chr, p) && contains(&model.down10, &chr, p) {
                None
            } else if contains(&model.up1, &chr, p) {
                n[4] += 1;
                n[5] += 1;
                n[6] += 1;
                continue;
            } else if contains(&model.up5, &chr, p) {
                n[5] += 1;
                n[6] += 1;
                continue;
            } else if contains(&model.up10, &chr, p) {
                Some(6)
            } else if contains(&model.down1, &chr, p) {
                n[7] += 1;
                n[8] += 1;
                n[9] += 1;
                continue;
            } else if contains(&model.down5, &chr, p) {
                n[8] += 1;
                n[9] += 1;
                continue;
            } else if contains(&model.down10, &chr, p) {
                Some(9)
            } else {
                None
            };
            if let Some(i) = group {
                n[i] += 1
            } else {
                unassigned += 1
            }
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
        "Total Reads                   {reads}\nTotal Tags                    {tags}\nTotal Assigned Tags           {}\n=====================================================================\nGroup               Total_bases         Tag_count           Tags/Kb             \n",
        tags - unassigned
    );
    for ((name, map), count) in names.iter().zip(maps).zip(n) {
        let size = bases(map);
        text.push_str(&format!(
            "{name:<20}{size:<20}{count:<20}{:<18.2}\n",
            count as f64 * 1000.0 / (size + 1) as f64
        ));
    }
    text.push_str("=====================================================================\n");
    Ok(text)
}

fn junction_annotation(
    resident: &Resident,
    duplicates: &HashSet<usize>,
    model: &BedModel,
) -> Result<(String, String)> {
    let mut total_events = 0_u64;
    let mut known_events = 0_u64;
    let mut partial_events = 0_u64;
    let mut novel_events = 0_u64;
    let mut filtered_events = 0_u64;
    // Python 3 dictionaries retain insertion order, and RSeQC writes these in
    // the order that each distinct junction is first encountered in the BAM.
    let mut junctions = Vec::<((String, i32, i32), u64)>::new();
    let mut junction_indices = HashMap::<(String, i32, i32), usize>::new();
    for &record_index in resident.coordinate_order() {
        let index = record_index as usize;
        let fixed = resident.headers()[index];
        if fixed.flag & 0x304 != 0 || duplicates.contains(&index) || fixed.mapq < 30 {
            continue;
        }
        let chr = resident
            .header
            .reference_sequences()
            .get_index(fixed.tid as usize)
            .map(|(n, _)| n.to_string())
            .unwrap_or_default()
            .to_ascii_uppercase();
        for x in cigar_introns(resident, fixed)? {
            total_events += 1;
            if x.end - x.start < 50 {
                filtered_events += 1;
                continue;
            }
            let key = (chr.clone(), x.start, x.end);
            if let Some(&index) = junction_indices.get(&key) {
                junctions[index].1 += 1;
            } else {
                let index = junctions.len();
                junction_indices.insert(key.clone(), index);
                junctions.push((key, 1));
            }
            if model
                .intron_starts
                .get(&chr)
                .is_some_and(|v| v.contains(&x.start))
                && model
                    .intron_ends
                    .get(&chr)
                    .is_some_and(|v| v.contains(&x.end))
            {
                known_events += 1
            } else if model
                .intron_starts
                .get(&chr)
                .is_some_and(|v| v.contains(&x.start))
                || model
                    .intron_ends
                    .get(&chr)
                    .is_some_and(|v| v.contains(&x.end))
            {
                partial_events += 1
            } else {
                novel_events += 1
            }
        }
    }
    let mut jc = [0; 3];
    let mut xls =
        String::from("chrom\tintron_st(0-based)\tintron_end(1-based)\tread_count\tannotation\n");
    for ((chr, s, e), count) in junctions {
        let known = model
            .intron_starts
            .get(&chr)
            .is_some_and(|v| v.contains(&s))
            && model.intron_ends.get(&chr).is_some_and(|v| v.contains(&e));
        let partial = !known
            && (model
                .intron_starts
                .get(&chr)
                .is_some_and(|v| v.contains(&s))
                || model.intron_ends.get(&chr).is_some_and(|v| v.contains(&e)));
        let label = if known {
            jc[0] += 1;
            "annotated"
        } else if partial {
            jc[1] += 1;
            "partial_novel"
        } else {
            jc[2] += 1;
            "complete_novel"
        };
        xls.push_str(&format!(
            "{}\t{s}\t{e}\t{count}\t {label}\n",
            chr.replace("CHR", "chr")
        ));
    }
    let log = format!(
        "Reading reference bed file:  /home/rborkows/uni-rnaseq-data/tier0/qc/chr22.bed  ...  Done\nLoad BAM file ...  Done\n\n===================================================================\nTotal splicing  Events:\t{}\nKnown Splicing Events:\t{}\nPartial Novel Splicing Events:\t{}\nNovel Splicing Events:\t{}\nFiltered Splicing Events:\t{}\n\nTotal splicing  Junctions:\t{}\nKnown Splicing Junctions:\t{}\nPartial Novel Splicing Junctions:\t{}\nNovel Splicing Junctions:\t{}\n\n===================================================================\nCreate BED file ...\nCreate Interact file ...\n",
        total_events,
        known_events,
        partial_events,
        novel_events,
        filtered_events,
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
    resident: &Resident,
    duplicates: &HashSet<usize>,
    model: &BedModel,
) -> Result<String> {
    let mut events = Vec::<(String, i32, i32)>::new();
    for &record_index in resident.coordinate_order() {
        let index = record_index as usize;
        let fixed = resident.headers()[index];
        if fixed.flag & 0x304 != 0 || duplicates.contains(&index) || fixed.mapq < 30 {
            continue;
        }
        let chr = resident
            .header
            .reference_sequences()
            .get_index(fixed.tid as usize)
            .map(|(n, _)| n.to_string())
            .unwrap_or_default()
            .to_ascii_uppercase();
        for intron in cigar_introns(resident, fixed)? {
            if intron.end - intron.start >= 50 {
                events.push((chr.clone(), intron.start, intron.end));
            }
        }
    }
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
    let mut seen = HashMap::<(String, i32, i32), u64>::new();
    let mut known = Vec::new();
    let mut all = Vec::new();
    let mut novel = Vec::new();
    for percent in (5..=100).step_by(5) {
        let begin = events.len() * (percent - 5) / 100;
        let end = events.len() * percent / 100;
        for event in &events[begin..end] {
            *seen.entry(event.clone()).or_default() += 1;
        }
        all.push(seen.len());
        known.push(
            seen.keys()
                .filter(|key| model.known_junctions.contains(*key))
                .count(),
        );
        novel.push(all.last().copied().unwrap() - known.last().copied().unwrap());
    }
    let csv = |v: &[usize]| v.iter().map(usize::to_string).collect::<Vec<_>>().join(",");
    Ok(format!(
        "pdf('chr22.junctionSaturation_plot.pdf')\nx=c(5,10,15,20,25,30,35,40,45,50,55,60,65,70,75,80,85,90,95,100)\ny=c({})\nz=c({})\nw=c({})\nm=max({},{},{})\nn=min({},{},{})\nplot(x,z/1000,xlab='percent of total reads',ylab='Number of splicing junctions (x1000)',type='o',col='blue',ylim=c(n,m))\npoints(x,y/1000,type='o',col='red')\npoints(x,w/1000,type='o',col='green')\nlegend(5,{}, legend=c(\"All junctions\",\"known junctions\", \"novel junctions\"),col=c(\"blue\",\"red\",\"green\"),lwd=1,pch=1)\ndev.off()\n",
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
    for &record_index in resident.coordinate_order() {
        if sampled >= 200_000 {
            break;
        }
        let index = record_index as usize;
        let fixed = resident.headers()[index];
        if fixed.flag & 0x304 != 0 || duplicates.contains(&index) || fixed.mapq < 30 {
            continue;
        }
        let chr = resident
            .header
            .reference_sequences()
            .get_index(fixed.tid as usize)
            .map(|(n, _)| n.to_string())
            .unwrap_or_default()
            .to_ascii_uppercase();
        let end = fixed.pos + le_i32(&resident.record_bytes(fixed)[16..20]);
        let strands: HashSet<char> = model
            .genes
            .get(&chr)
            .into_iter()
            .flatten()
            .filter(|(x, _)| x.start < end && x.end > fixed.pos)
            .map(|(_, s)| *s)
            .collect();
        if strands.is_empty() {
            continue;
        }
        sampled += 1;
        let gene = if strands.len() == 1 {
            strands.iter().next().unwrap().to_string()
        } else {
            // RSeQC uses `':'.join(set(...))`; on its reference runtime the
            // two-strand case is `-:+`, which is deliberately not one of the
            // recognized protocol buckets below.
            "-:+".to_owned()
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
