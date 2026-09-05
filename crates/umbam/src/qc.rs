//! Deterministic, text-only QC reductions over the resident BAM table.
//!
//! These routines deliberately borrow the native BAM bodies from `Resident`: no SAM text or
//! record allocation is constructed while collecting counters.

use super::{Resident, bam_cigar, bam_layout, le_i32};
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
}

pub(super) fn write(
    out: &Path,
    resident: &Resident,
    duplicates: &HashSet<usize>,
) -> Result<Timing> {
    let rseqc = out.join("rseqc");
    fs::create_dir_all(&rseqc)?;
    let now = Instant::now();
    fs::write(rseqc.join("bam_stat.txt"), bam_stat(resident, duplicates)?)?;
    let bam_stat = now.elapsed();
    let now = Instant::now();
    let seq = sequence_duplication(resident)?;
    fs::write(rseqc.join("seq.DupRate.xls"), render_histogram(&seq))?;
    let seq_duplication = now.elapsed();
    let now = Instant::now();
    let pos = position_duplication(resident)?;
    fs::write(rseqc.join("pos.DupRate.xls"), render_histogram(&pos))?;
    Ok(Timing {
        bam_stat,
        seq_duplication,
        pos_duplication: now.elapsed(),
    })
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
    for (index, fixed) in resident.headers().iter().enumerate() {
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
        let cigar = bam_cigar(resident.record_bytes(*fixed))?;
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

fn render_histogram(histogram: &HashMap<u32, u64>) -> String {
    let mut entries = histogram.iter().collect::<Vec<_>>();
    entries.sort_unstable_by_key(|(occurrence, _)| **occurrence);
    let mut text = String::from("Occurrence\tUniqReadNumber\n");
    for (occurrence, count) in entries {
        text.push_str(&format!("{occurrence}\t{count}\n"));
    }
    text
}
