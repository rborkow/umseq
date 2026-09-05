//! One-pass, resident CPU BAM-chain primitives.
//!
//! The arena stores complete SAM alignment lines; the fixed table is permuted by an index.

use anyhow::{Context, Result, bail};
use noodles_bam as bam;
use noodles_bgzf as bgzf;
use noodles_sam::{
    self as sam,
    header::record::value::{
        Map,
        map::header::{Header as SamHeader, sort_order::COORDINATE, tag::SORT_ORDER},
    },
};
use rayon::prelude::*;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, File},
    io::{self, Read, Write},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use umem::{Allocation, Buf, Pod, Ro, Rw};

/// Fixed metadata shared by CPU and future GPU implementations.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct RecordHeader {
    pub tid: i32,
    pub pos: i32,
    pub flag: u16,
    pub mapq: u8,
    pub _padding: u8,
    pub mate_tid: i32,
    pub mate_pos: i32,
    pub tlen: i32,
    pub name_hash: u64,
    pub offset: u64,
    pub len: u32,
    pub _padding2: u32,
}

// SAFETY: repr(C), integer-only fields, initialized padding, and all bit patterns are valid.
unsafe impl Pod for RecordHeader {}

/// The table and arena are GPU-ready; order is the CPU-only sorting permutation.
pub struct Resident {
    header: sam::Header,
    pub table: Buf<Ro>,
    pub arena: Buf<Ro>,
    arena_used: usize,
    order: Vec<u32>,
}

#[derive(Default)]
struct Timing {
    decode: Duration,
    sort: Duration,
    write_sorted: Duration,
    markdup: Duration,
    write_markdup: Duration,
    index: Duration,
    featurecounts: Duration,
    genomecov: Duration,
}

/// Runs the resident CPU control path.
pub fn chain(input: &Path, gtf: &Path, out_dir: &Path, threads: usize) -> Result<()> {
    fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build()?;
    let now = Instant::now();
    let mut resident = decode(input, threads)?;
    let decode = now.elapsed();
    let now = Instant::now();
    let (table, order) = (&resident.table, &mut resident.order);
    let headers = table.as_pod_slice::<RecordHeader>();
    pool.install(|| {
        order.par_sort_unstable_by(|a, b| {
            compare_headers(&headers[*a as usize], &headers[*b as usize], *a, *b)
        })
    });
    let sort = now.elapsed();
    let sorted = out_dir.join("sorted.bam");
    let now = Instant::now();
    write_bam(&sorted, &resident, threads)?;
    let write_sorted = now.elapsed();
    let now = Instant::now();
    let markdup_result = mark_duplicates(&resident)?;
    let markdup_compute = now.elapsed();
    let markdup = out_dir.join("markdup.bam");
    let now = Instant::now();
    write_bam_with_duplicates(&markdup, &resident, threads, &markdup_result.duplicates)?;
    let write_markdup = now.elapsed();
    let now = Instant::now();
    write_index(&sorted)?;
    write_index(&markdup)?;
    let index = now.elapsed();
    write_flagstat_and_idxstats(out_dir, &resident)?;
    write_markdup_metrics(out_dir, &markdup_result.metrics)?;
    let now = Instant::now();
    write_featurecounts(out_dir, input, gtf, &resident, &pool)?;
    let featurecounts = now.elapsed();
    let now = Instant::now();
    write_genomecov(out_dir, &resident, &pool)?;
    let genomecov = now.elapsed();
    write_timing(
        out_dir,
        &Timing {
            decode,
            sort,
            write_sorted,
            markdup: markdup_compute,
            write_markdup,
            index,
            featurecounts,
            genomecov,
        },
    )?;
    Ok(())
}

impl Resident {
    fn headers(&self) -> &[RecordHeader] {
        self.table.as_pod_slice()
    }

    fn record_bytes(&self, fixed: RecordHeader) -> &[u8] {
        let start = fixed.offset as usize;
        let end = start + fixed.len as usize;
        assert!(end <= self.arena_used, "record span exceeds arena");
        &self.arena.as_slice()[start..end]
    }
}

fn decode(input: &Path, threads: usize) -> Result<Resident> {
    // The multithreaded BGZF reader inflates blocks concurrently.  Unlike the old path, the
    // record stream stays in BAM's native representation: bodies are copied verbatim into the
    // resident arena and no `RecordBuf` or SAM text is built while decoding.
    let file = File::open(input).with_context(|| format!("open {}", input.display()))?;
    let workers = NonZeroUsize::new(threads.max(1)).expect("clamped");
    let mut reader = bam::io::Reader::from(bgzf::io::MultithreadedReader::with_worker_count(
        workers, file,
    ));
    let header = reader.read_header()?;
    let mut stream = Vec::new();
    reader.into_inner().read_to_end(&mut stream)?;
    let mut starts = Vec::new();
    let mut offset = 0usize;
    while offset < stream.len() {
        let size = le_u32(
            stream
                .get(offset..offset + 4)
                .context("truncated BAM block size")?,
        ) as usize;
        let end = offset
            .checked_add(4 + size)
            .context("BAM block size overflow")?;
        if end > stream.len() {
            bail!("truncated BAM record body")
        }
        starts.push((offset, size));
        offset = end;
    }
    let mut arena = Buf::<Rw>::allocate(
        stream.len().max(1),
        Allocation::Anon {
            huge: true,
            require_huge: false,
        },
    )?;
    arena.as_mut_slice()[..stream.len()].copy_from_slice(&stream);
    let entries: Vec<RecordHeader> = starts
        .par_iter()
        .map(|&(offset, len)| {
            bam_record_header(
                &stream[offset + 4..offset + 4 + len],
                (offset + 4) as u64,
                len as u32,
            )
        })
        .collect::<Result<_>>()?;
    let bytes = entries
        .len()
        .checked_mul(std::mem::size_of::<RecordHeader>())
        .context("table size overflow")?
        .max(1);
    let mut table = Buf::<Rw>::allocate(
        bytes,
        Allocation::Anon {
            huge: true,
            require_huge: false,
        },
    )?;
    table.as_pod_mut_slice::<RecordHeader>()[..entries.len()].copy_from_slice(&entries);
    let order = (0..entries.len())
        .map(|i| u32::try_from(i).context("too many records"))
        .collect::<Result<Vec<_>>>()?;
    Ok(Resident {
        header,
        table: table.freeze(),
        arena: arena.freeze(),
        arena_used: stream.len(),
        order,
    })
}

fn bam_record_header(body: &[u8], offset: u64, len: u32) -> Result<RecordHeader> {
    if body.len() < 32 {
        bail!("BAM record body is shorter than its fixed fields")
    }
    Ok(RecordHeader {
        tid: le_i32(&body[0..4]),
        pos: le_i32(&body[4..8]),
        flag: le_u16(&body[14..16]),
        mapq: body[9],
        mate_tid: le_i32(&body[20..24]),
        mate_pos: le_i32(&body[24..28]),
        tlen: le_i32(&body[28..32]),
        name_hash: 0,
        offset,
        len,
        _padding: 0,
        _padding2: 0,
    })
}

fn le_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes(bytes.try_into().expect("u16 slice"))
}
fn le_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("u32 slice"))
}
fn le_i32(bytes: &[u8]) -> i32 {
    i32::from_le_bytes(bytes.try_into().expect("i32 slice"))
}

/// Coordinate order with `samtools sort` tie-breaking: equal (tid, pos) keep input order.
/// Matching samtools here matters because Picard MarkDuplicates breaks score ties by the
/// order it streams the sorted input, so our sort must produce the same stream.
fn compare_headers(a: &RecordHeader, b: &RecordHeader, a_index: u32, b_index: u32) -> Ordering {
    (a.tid, a.pos, a_index).cmp(&(b.tid, b.pos, b_index))
}

fn write_bam(path: &Path, resident: &Resident, threads: usize) -> Result<()> {
    write_bam_with_duplicates(path, resident, threads, &HashSet::new())
}

fn write_bam_with_duplicates(
    path: &Path,
    resident: &Resident,
    threads: usize,
    duplicates: &HashSet<usize>,
) -> Result<()> {
    let mut header = resident.header.clone();
    header
        .header_mut()
        .get_or_insert_with(Map::<SamHeader>::default)
        .other_fields_mut()
        .insert(SORT_ORDER, COORDINATE.into());
    let workers = NonZeroUsize::new(threads.max(1)).expect("clamped");
    let mut header_bytes = Vec::new();
    bam::io::Writer::from(&mut header_bytes).write_header(&header)?;
    let mut writer = bgzf::io::MultithreadedWriter::with_worker_count(workers, File::create(path)?);
    writer.write_all(&header_bytes)?;
    for index in &resident.order {
        let fixed = resident.headers()[*index as usize];
        let bytes = resident.record_bytes(fixed);
        writer.write_all(&fixed.len.to_le_bytes())?;
        if duplicates.contains(&(*index as usize)) {
            let mut patched = bytes.to_vec();
            let flag = le_u16(&patched[14..16]) | 0x400;
            patched[14..16].copy_from_slice(&flag.to_le_bytes());
            writer.write_all(&patched)?;
        } else {
            writer.write_all(bytes)?;
        }
    }
    writer.finish()?;
    Ok(())
}

#[derive(Clone)]
struct DupRecord {
    index: usize,
    name: String,
    flag: u16,
    tid: i32,
    pos: i32,
    cigar: String,
    score: u64,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct FragmentEnd {
    tid: i32,
    pos: i32,
    reverse: bool,
}

struct MarkdupResult {
    duplicates: HashSet<usize>,
    metrics: DupMetrics,
}

#[derive(Default)]
struct DupMetrics {
    unpaired_examined: u64,
    pairs_examined: u64,
    secondary_or_supplementary: u64,
    unmapped: u64,
    unpaired_duplicates: u64,
    pair_duplicates: u64,
}

fn mark_duplicates(resident: &Resident) -> Result<MarkdupResult> {
    let records = (0..resident.headers().len())
        .map(|index| duplicate_record(resident, index))
        .collect::<Result<Vec<_>>>()?;
    // Rank of each record in coordinate order; Picard's tie-break is streaming order.
    let mut sorted_rank = vec![0usize; records.len()];
    for (rank, &index) in resident.order.iter().enumerate() {
        sorted_rank[index as usize] = rank;
    }
    let mut metrics = DupMetrics::default();
    let mut primary_by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (position, record) in records.iter().enumerate() {
        if record.flag & 0x900 != 0 {
            metrics.secondary_or_supplementary += 1;
        } else if record.flag & 0x4 != 0 {
            metrics.unmapped += 1;
        } else {
            primary_by_name
                .entry(record.name.clone())
                .or_default()
                .push(position);
        }
    }

    let mut paired = Vec::new();
    let mut used = HashSet::new();
    for positions in primary_by_name.values() {
        let first = positions
            .iter()
            .copied()
            .find(|&p| records[p].flag & 0x40 != 0);
        let second = positions
            .iter()
            .copied()
            .find(|&p| records[p].flag & 0x80 != 0);
        if let (Some(a), Some(b)) = (first, second)
            && records[a].flag & 0x1 != 0
            && records[b].flag & 0x1 != 0
            && records[a].flag & 0x8 == 0
            && records[b].flag & 0x8 == 0
        {
            paired.push((a, b));
            used.insert(a);
            used.insert(b);
        }
    }
    metrics.pairs_examined = paired.len() as u64;
    let mut pair_ends = HashSet::new();
    let mut pair_groups: BTreeMap<(FragmentEnd, FragmentEnd), Vec<(usize, usize)>> =
        BTreeMap::new();
    for &(a, b) in &paired {
        let left = fragment_end(&records[a]);
        let right = fragment_end(&records[b]);
        pair_ends.insert(left);
        pair_ends.insert(right);
        let key = if left <= right {
            (left, right)
        } else {
            (right, left)
        };
        pair_groups.entry(key).or_default().push((a, b));
    }
    let mut duplicates = HashSet::new();
    for group in pair_groups.values() {
        let best = best_record_pair(group, &records, &sorted_rank);
        for &(a, b) in group {
            if (a, b) != best {
                duplicates.insert(records[a].index);
                duplicates.insert(records[b].index);
                metrics.pair_duplicates += 1;
            }
        }
    }

    let unpaired: Vec<usize> = records
        .iter()
        .enumerate()
        .filter_map(|(position, record)| {
            (record.flag & 0x900 == 0 && record.flag & 0x4 == 0 && !used.contains(&position))
                .then_some(position)
        })
        .collect();
    metrics.unpaired_examined = unpaired.len() as u64;
    let mut unpaired_groups: BTreeMap<FragmentEnd, Vec<usize>> = BTreeMap::new();
    for position in unpaired {
        unpaired_groups
            .entry(fragment_end(&records[position]))
            .or_default()
            .push(position);
    }
    for (end, group) in unpaired_groups {
        let best = (!pair_ends.contains(&end)).then(|| best_single(&group, &records, &sorted_rank));
        for position in group {
            if Some(position) != best {
                duplicates.insert(records[position].index);
                metrics.unpaired_duplicates += 1;
            }
        }
    }
    Ok(MarkdupResult {
        duplicates,
        metrics,
    })
}

fn duplicate_record(resident: &Resident, index: usize) -> Result<DupRecord> {
    let body = resident.record_bytes(resident.headers()[index]);
    let fixed = resident.headers()[index];
    Ok(DupRecord {
        index,
        name: bam_name(body)?.to_owned(),
        flag: fixed.flag,
        tid: fixed.tid,
        pos: fixed.pos,
        cigar: bam_cigar(body)?
            .iter()
            .map(|&(len, op)| format!("{len}{op}"))
            .collect(),
        score: bam_qualities(body)?
            .iter()
            .copied()
            .filter(|quality| *quality >= 15)
            .map(u64::from)
            .sum(),
    })
}

fn fragment_end(record: &DupRecord) -> FragmentEnd {
    let reverse = record.flag & 0x10 != 0;
    let (leading, trailing, reference) = cigar_shape(&record.cigar);
    let pos = if reverse {
        record.pos + reference - 1 + trailing
    } else {
        record.pos - leading
    };
    FragmentEnd {
        tid: record.tid,
        pos,
        reverse,
    }
}

fn cigar_shape(cigar: &str) -> (i32, i32, i32) {
    let operations = cigar_operations(cigar);
    let clip = |operation: char| operation == 'S' || operation == 'H';
    let leading = operations
        .iter()
        .take_while(|(_, operation)| clip(*operation))
        .map(|(n, _)| *n)
        .sum();
    let trailing = operations
        .iter()
        .rev()
        .take_while(|(_, operation)| clip(*operation))
        .map(|(n, _)| *n)
        .sum();
    let reference = operations
        .iter()
        .filter(|(_, op)| matches!(op, 'M' | '=' | 'X' | 'D' | 'N'))
        .map(|(n, _)| *n)
        .sum();
    (leading, trailing, reference)
}

fn cigar_operations(cigar: &str) -> Vec<(i32, char)> {
    let mut number = 0_i32;
    let mut operations = Vec::new();
    for byte in cigar.bytes() {
        if byte.is_ascii_digit() {
            number = number * 10 + i32::from(byte - b'0');
        } else {
            operations.push((number, char::from(byte)));
            number = 0;
        }
    }
    operations
}

fn bam_layout(body: &[u8]) -> Result<(usize, usize, usize)> {
    if body.len() < 32 {
        bail!("short BAM record")
    }
    let name_len = usize::from(body[8]);
    let cigar_count = usize::from(le_u16(&body[12..14]));
    let seq_len = le_i32(&body[16..20]);
    if seq_len < 0 {
        bail!("negative BAM sequence length")
    }
    let cigar_end = 32usize
        .checked_add(name_len)
        .and_then(|n| n.checked_add(cigar_count * 4))
        .context("BAM CIGAR overflow")?;
    let qual_start = cigar_end
        .checked_add((seq_len as usize).div_ceil(2))
        .context("BAM sequence overflow")?;
    let aux_start = qual_start
        .checked_add(seq_len as usize)
        .context("BAM quality overflow")?;
    if aux_start > body.len() {
        bail!("truncated BAM variable fields")
    }
    Ok((name_len, cigar_count, aux_start))
}

fn bam_name(body: &[u8]) -> Result<&str> {
    let (name_len, _, _) = bam_layout(body)?;
    if name_len == 0 || 32 + name_len > body.len() || body[31 + name_len] != 0 {
        bail!("invalid BAM read name")
    }
    std::str::from_utf8(&body[32..31 + name_len]).context("BAM read name is UTF-8")
}

fn bam_cigar(body: &[u8]) -> Result<Vec<(i32, char)>> {
    let (name_len, count, _) = bam_layout(body)?;
    const OPS: &[u8] = b"MIDNSHP=XB";
    (0..count)
        .map(|i| {
            let value = le_u32(&body[32 + name_len + i * 4..36 + name_len + i * 4]);
            let op = OPS
                .get((value & 0x0f) as usize)
                .context("invalid BAM CIGAR op")?;
            Ok(((value >> 4) as i32, char::from(*op)))
        })
        .collect()
}

fn bam_qualities(body: &[u8]) -> Result<&[u8]> {
    let (_, _, aux_start) = bam_layout(body)?;
    let length = le_i32(&body[16..20]) as usize;
    Ok(&body[aux_start - length..aux_start])
}

fn bam_nh_is_multiple(body: &[u8]) -> Result<bool> {
    let (_, _, mut at) = bam_layout(body)?;
    while at < body.len() {
        if at + 3 > body.len() {
            bail!("truncated BAM auxiliary field")
        }
        let tag = &body[at..at + 2];
        let ty = body[at + 2];
        at += 3;
        let value = match ty {
            b'c' | b'C' => {
                let v = *body.get(at).context("truncated BAM aux")? as i64;
                at += 1;
                v
            }
            b's' | b'S' => {
                let v = le_u16(body.get(at..at + 2).context("truncated BAM aux")?) as i64;
                at += 2;
                v
            }
            b'i' | b'I' => {
                let v = le_u32(body.get(at..at + 4).context("truncated BAM aux")?) as i64;
                at += 4;
                v
            }
            b'f' => {
                at += 4;
                0
            }
            b'A' => {
                at += 1;
                0
            }
            b'Z' | b'H' => {
                let end = body[at..]
                    .iter()
                    .position(|&b| b == 0)
                    .context("unterminated BAM aux")?;
                at += end + 1;
                0
            }
            b'B' => {
                if at + 5 > body.len() {
                    bail!("truncated BAM array")
                };
                let n = le_u32(&body[at + 1..at + 5]) as usize;
                let width = match body[at] {
                    b'c' | b'C' | b'A' => 1,
                    b's' | b'S' => 2,
                    b'i' | b'I' | b'f' => 4,
                    _ => bail!("invalid BAM array type"),
                };
                at += 5 + n * width;
                0
            }
            _ => bail!("invalid BAM auxiliary type"),
        };
        if at > body.len() {
            bail!("truncated BAM auxiliary value")
        }
        if tag == b"NH" && value > 1 {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Picard `SUM_OF_BASE_QUALITIES` scoring: highest score wins. On a tie Picard keeps the
/// pair it encountered first while streaming the coordinate-sorted input, i.e. the one whose
/// earlier-positioned read has the lowest sorted rank (verified on Tier 0: 167/167 tied sets).
fn best_record_pair(
    group: &[(usize, usize)],
    records: &[DupRecord],
    sorted_rank: &[usize],
) -> (usize, usize) {
    *group
        .iter()
        .min_by(|(a1, b1), (a2, b2)| {
            let score1 = records[*a1].score + records[*b1].score;
            let score2 = records[*a2].score + records[*b2].score;
            let first1 = sorted_rank[*a1].min(sorted_rank[*b1]);
            let first2 = sorted_rank[*a2].min(sorted_rank[*b2]);
            score2.cmp(&score1).then_with(|| first1.cmp(&first2))
        })
        .expect("nonempty duplicate group")
}

/// Same rule as [`best_record_pair`] for unpaired reads.
fn best_single(group: &[usize], records: &[DupRecord], sorted_rank: &[usize]) -> usize {
    *group
        .iter()
        .min_by(|a, b| {
            records[**b]
                .score
                .cmp(&records[**a].score)
                .then_with(|| sorted_rank[**a].cmp(&sorted_rank[**b]))
        })
        .expect("nonempty duplicate group")
}

fn write_markdup_metrics(out: &Path, metrics: &DupMetrics) -> Result<()> {
    let denominator = metrics.unpaired_examined + 2 * metrics.pairs_examined;
    let numerator = metrics.unpaired_duplicates + 2 * metrics.pair_duplicates;
    let percent = if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    };
    fs::write(
        out.join("markdup.metrics.txt"),
        format!(
            "## htsjdk.samtools.metrics.StringHeader\n# MarkDuplicates --OPTICAL_DUPLICATE_PIXEL_DISTANCE 0 --READ_NAME_REGEX null\n\n## METRICS CLASS\tpicard.sam.DuplicationMetrics\nLIBRARY\tUNPAIRED_READS_EXAMINED\tREAD_PAIRS_EXAMINED\tSECONDARY_OR_SUPPLEMENTARY_RDS\tUNMAPPED_READS\tUNPAIRED_READ_DUPLICATES\tREAD_PAIR_DUPLICATES\tREAD_PAIR_OPTICAL_DUPLICATES\tPERCENT_DUPLICATION\tESTIMATED_LIBRARY_SIZE\nUnknown Library\t{}\t{}\t{}\t{}\t{}\t{}\t0\t{percent:.6}\t\n",
            metrics.unpaired_examined,
            metrics.pairs_examined,
            metrics.secondary_or_supplementary,
            metrics.unmapped,
            metrics.unpaired_duplicates,
            metrics.pair_duplicates
        ),
    )?;
    Ok(())
}

fn write_index(path: &Path) -> Result<()> {
    let index = bam::fs::index(path)?;
    bam::bai::fs::write(PathBuf::from(format!("{}.bai", path.display())), &index)?;
    Ok(())
}

fn write_flagstat_and_idxstats(out: &Path, resident: &Resident) -> Result<()> {
    let mut counters = Flagstat::default();
    let mut idxstats = vec![(0_u64, 0_u64); resident.header.reference_sequences().len()];
    let mut no_coordinate = 0_u64;

    for fixed in resident.headers() {
        counters.add(*fixed);
        if fixed.tid < 0 {
            no_coordinate += 1;
        } else if let Some(counts) = idxstats.get_mut(fixed.tid as usize) {
            if fixed.flag & 0x4 == 0 {
                counts.0 += 1;
            } else {
                counts.1 += 1;
            }
        }
    }

    fs::write(out.join("flagstat.txt"), counters.render())?;
    let sequences = header_sequences(&resident.header)?;
    let mut text = String::new();
    for ((name, length), (mapped, unmapped)) in sequences.into_iter().zip(idxstats) {
        text.push_str(&format!("{name}\t{length}\t{mapped}\t{unmapped}\n"));
    }
    text.push_str(&format!("*\t0\t0\t{no_coordinate}\n"));
    fs::write(out.join("idxstats.txt"), text)?;
    Ok(())
}

fn header_sequences(header: &sam::Header) -> Result<Vec<(String, u64)>> {
    let mut writer = sam::io::Writer::new(Vec::new());
    writer.write_header(header)?;
    let text = String::from_utf8(writer.into_inner()).context("SAM header is UTF-8")?;
    text.lines()
        .filter(|line| line.starts_with("@SQ\t"))
        .map(|line| {
            let mut name = None;
            let mut length = None;
            for field in line.split('\t').skip(1) {
                if let Some(value) = field.strip_prefix("SN:") {
                    name = Some(value.to_owned());
                }
                if let Some(value) = field.strip_prefix("LN:") {
                    length = Some(value.parse()?);
                }
            }
            Ok((
                name.context("@SQ without SN")?,
                length.context("@SQ without LN")?,
            ))
        })
        .collect()
}

#[derive(Default)]
struct Flagstat {
    total: u64,
    primary: u64,
    secondary: u64,
    supplementary: u64,
    duplicates: u64,
    primary_duplicates: u64,
    mapped: u64,
    primary_mapped: u64,
    paired: u64,
    read1: u64,
    read2: u64,
    proper: u64,
    both_mapped: u64,
    singletons: u64,
    different_chr: u64,
    different_chr_mapq5: u64,
}

impl Flagstat {
    fn add(&mut self, record: RecordHeader) {
        let flag = record.flag;
        self.total += 1;
        let primary = flag & 0x900 == 0;
        if primary {
            self.primary += 1;
        }
        if flag & 0x100 != 0 {
            self.secondary += 1;
        }
        if flag & 0x800 != 0 {
            self.supplementary += 1;
        }
        if flag & 0x400 != 0 {
            self.duplicates += 1;
            if primary {
                self.primary_duplicates += 1;
            }
        }
        let mapped = flag & 0x4 == 0;
        if mapped {
            self.mapped += 1;
            if primary {
                self.primary_mapped += 1;
            }
        }
        if primary && flag & 0x1 != 0 {
            self.paired += 1;
            if flag & 0x40 != 0 {
                self.read1 += 1;
            }
            if flag & 0x80 != 0 {
                self.read2 += 1;
            }
            if mapped && flag & 0x2 != 0 {
                self.proper += 1;
            }
            if mapped && flag & 0x8 == 0 {
                self.both_mapped += 1;
                if record.tid != record.mate_tid {
                    self.different_chr += 1;
                    if record.mapq >= 5 {
                        self.different_chr_mapq5 += 1;
                    }
                }
            }
            if mapped && flag & 0x8 != 0 {
                self.singletons += 1;
            }
        }
    }

    fn percent(numerator: u64, denominator: u64) -> String {
        if denominator == 0 {
            "N/A".to_owned()
        } else {
            format!("{:.2}%", numerator as f64 * 100.0 / denominator as f64)
        }
    }

    fn render(&self) -> String {
        let total_percent = |n| Self::percent(n, self.total);
        let primary_percent = |n| Self::percent(n, self.primary);
        let paired_percent = |n| Self::percent(n, self.paired);
        format!(
            "{} + 0 in total (QC-passed reads + QC-failed reads)\n{} + 0 primary\n{} + 0 secondary\n{} + 0 supplementary\n{} + 0 duplicates\n{} + 0 primary duplicates\n{} + 0 mapped ({} : N/A)\n{} + 0 primary mapped ({} : N/A)\n{} + 0 paired in sequencing\n{} + 0 read1\n{} + 0 read2\n{} + 0 properly paired ({} : N/A)\n{} + 0 with itself and mate mapped\n{} + 0 singletons ({} : N/A)\n{} + 0 with mate mapped to a different chr\n{} + 0 with mate mapped to a different chr (mapQ>=5)\n",
            self.total,
            self.primary,
            self.secondary,
            self.supplementary,
            self.duplicates,
            self.primary_duplicates,
            self.mapped,
            total_percent(self.mapped),
            self.primary_mapped,
            primary_percent(self.primary_mapped),
            self.paired,
            self.read1,
            self.read2,
            self.proper,
            paired_percent(self.proper),
            self.both_mapped,
            self.singletons,
            paired_percent(self.singletons),
            self.different_chr,
            self.different_chr_mapq5
        )
    }
}

#[derive(Default)]
struct Gene {
    id: String,
    chrom: String,
    strand: String,
    exons: Vec<(i32, i32)>,
    merged: Vec<(i32, i32)>,
}

/// A coarse genomic index keeps interval queries proportional to genes near a read, rather
/// than to every gene on a chromosome. Coordinates are zero-based half-open throughout.
struct FeatureIndex {
    genes: Vec<Gene>,
    by_tid: HashMap<i32, HashMap<i32, Vec<usize>>>,
}

const FEATURE_BIN: i32 = 16 * 1024;

fn write_featurecounts(
    out: &Path,
    input: &Path,
    gtf: &Path,
    resident: &Resident,
    _pool: &rayon::ThreadPool,
) -> Result<()> {
    let features = read_features(gtf, &resident.header)?;
    let totals = count_fragments(resident, &features)?;
    let input = input.canonicalize().unwrap_or_else(|_| input.to_owned());
    let mut text = format!(
        "# Program:featureCounts v2.1.1; Command:\"umbam\"\nGeneid\tChr\tStart\tEnd\tStrand\tLength\t{}\n",
        input.display()
    );
    for (index, gene) in features.genes.iter().enumerate() {
        let join = |which: usize| -> String {
            gene.exons
                .iter()
                .map(|exon| match which {
                    0 => gene.chrom.clone(),
                    1 => (exon.0 + 1).to_string(),
                    2 => exon.1.to_string(),
                    _ => gene.strand.clone(),
                })
                .collect::<Vec<_>>()
                .join(";")
        };
        let length: i32 = gene.merged.iter().map(|(start, end)| end - start).sum();
        text.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            gene.id,
            join(0),
            join(1),
            join(2),
            join(3),
            length,
            totals[index]
        ));
    }
    fs::write(out.join("featureCounts.txt"), text)?;
    Ok(())
}

fn read_features(gtf: &Path, header: &sam::Header) -> Result<FeatureIndex> {
    let mut genes = Vec::<Gene>::new();
    let mut gene_ids = HashMap::<String, usize>::new();
    for line in fs::read_to_string(gtf)?.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 9 || fields[2] != "exon" {
            continue;
        }
        let id = fields[8]
            .split(';')
            .find_map(|field| field.trim().strip_prefix("gene_id "))
            .and_then(|value| value.trim_matches('"').split('"').next())
            .context("exon has no gene_id")?
            .to_owned();
        let start: i32 = fields[3].parse::<i32>()? - 1;
        let end: i32 = fields[4].parse()?;
        let index = *gene_ids.entry(id.clone()).or_insert_with(|| {
            let index = genes.len();
            genes.push(Gene {
                id,
                chrom: fields[0].to_owned(),
                strand: fields[6].to_owned(),
                ..Gene::default()
            });
            index
        });
        genes[index].exons.push((start, end));
    }
    let names: HashMap<_, _> = header_sequences(header)?
        .into_iter()
        .enumerate()
        .map(|(tid, (name, _))| (name, tid as i32))
        .collect();
    let mut by_tid: HashMap<i32, HashMap<i32, Vec<usize>>> = HashMap::new();
    for (index, gene) in genes.iter_mut().enumerate() {
        let mut sorted = gene.exons.clone();
        sorted.sort_unstable();
        for (start, end) in sorted {
            if let Some(last) = gene.merged.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
            } else {
                gene.merged.push((start, end));
            }
        }
        if let Some(&tid) = names.get(&gene.chrom) {
            let bins = by_tid.entry(tid).or_default();
            for &(start, end) in &gene.merged {
                for bin in start.div_euclid(FEATURE_BIN)..=(end - 1).div_euclid(FEATURE_BIN) {
                    let entries = bins.entry(bin).or_default();
                    if entries.last() != Some(&index) && !entries.contains(&index) {
                        entries.push(index);
                    }
                }
            }
        }
    }
    Ok(FeatureIndex { genes, by_tid })
}

fn records_by_tid(resident: &Resident) -> Vec<(i32, Vec<usize>)> {
    let mut records = BTreeMap::<i32, Vec<usize>>::new();
    for (index, fixed) in resident.headers().iter().enumerate() {
        if fixed.tid >= 0 {
            records.entry(fixed.tid).or_default().push(index);
        }
    }
    records.into_iter().collect()
}

fn count_fragments(resident: &Resident, features: &FeatureIndex) -> Result<Vec<u64>> {
    // featureCounts votes separately for each mate before combining the votes.  Keeping
    // fragments global (rather than grouping by reference) also preserves chimeric pairs.
    let mut fragments = HashMap::<String, ([Option<usize>; 2], bool)>::new();
    for (index, fixed) in resident.headers().iter().copied().enumerate() {
        if fixed.flag & 0x904 != 0 {
            continue;
        }
        let mate = match fixed.flag & 0xc0 {
            0x40 => 0,
            0x80 => 1,
            _ => continue,
        };
        let entry = fragments
            .entry(bam_name(resident.record_bytes(fixed))?.to_owned())
            .or_insert(([None, None], true));
        // Multiple primary records for the same mate are not a valid paired fragment;
        // omit it just as featureCounts omits multi-mapping reads without -M.
        if entry.0[mate].replace(index).is_some() {
            entry.1 = false;
        }
        entry.1 &= !bam_nh_is_multiple(resident.record_bytes(fixed))?;
    }
    let mut totals = vec![0_u64; features.genes.len()];
    for (_name, ([first, second], is_unique)) in fragments {
        if !is_unique || (first.is_none() && second.is_none()) {
            continue;
        }
        let first = first
            .map(|index| mate_gene_hits(index, resident, features))
            .transpose()?
            .unwrap_or_default();
        let second = second
            .map(|index| mate_gene_hits(index, resident, features))
            .transpose()?
            .unwrap_or_default();
        let candidates = if !first.is_empty() && !second.is_empty() {
            let intersection = first.intersection(&second).copied().collect::<HashSet<_>>();
            if intersection.is_empty() {
                first.union(&second).copied().collect()
            } else {
                intersection
            }
        } else if first.is_empty() {
            second
        } else {
            first
        };
        if candidates.len() == 1 {
            let gene = *candidates.iter().next().expect("one candidate");
            totals[gene] += 1;
        }
    }
    Ok(totals)
}

fn mate_gene_hits(
    index: usize,
    resident: &Resident,
    features: &FeatureIndex,
) -> Result<HashSet<usize>> {
    let fixed = resident.headers()[index];
    let Some(bins) = features.by_tid.get(&fixed.tid) else {
        return Ok(HashSet::new());
    };
    let cigar = bam_cigar(resident.record_bytes(fixed))?;
    let blocks = aligned_blocks(fixed.pos, &cigar);
    let mut candidates = HashSet::<usize>::new();
    for &(start, end) in &blocks {
        for bin in start.div_euclid(FEATURE_BIN)..=(end - 1).div_euclid(FEATURE_BIN) {
            if let Some(genes) = bins.get(&bin) {
                candidates.extend(genes);
            }
        }
    }
    candidates.retain(|&gene| {
        blocks.iter().any(|&(start, end)| {
            features.genes[gene]
                .merged
                .iter()
                .any(|&(a, b)| start < b && a < end)
        })
    });
    Ok(candidates)
}

fn aligned_blocks(pos: i32, cigar: &[(i32, char)]) -> Vec<(i32, i32)> {
    let mut reference = pos;
    let mut blocks = Vec::new();
    for &(length, op) in cigar {
        match op {
            'M' | '=' | 'X' => {
                blocks.push((reference, reference + length));
                reference += length;
            }
            'N' | 'D' => reference += length,
            _ => {}
        }
    }
    blocks
}

fn genomecov_blocks(pos: i32, cigar: &[(i32, char)]) -> Vec<(i32, i32)> {
    let mut reference = pos;
    let mut blocks = Vec::new();
    for &(length, op) in cigar {
        match op {
            'M' | '=' | 'X' => {
                blocks.push((reference, reference + length));
                reference += length;
            }
            'D' | 'N' => reference += length,
            _ => {}
        }
    }
    blocks
}

fn write_genomecov(out: &Path, resident: &Resident, pool: &rayon::ThreadPool) -> Result<()> {
    let sequences = header_sequences(&resident.header)?;
    let records = records_by_tid(resident);
    let coverage: Vec<Result<(i32, String)>> = pool.install(|| {
        records
            .par_iter()
            .map(|(tid, indices)| genomecov_tid(*tid, indices, resident, &sequences))
            .collect()
    });
    let mut pieces = BTreeMap::new();
    for piece in coverage {
        let (tid, text) = piece?;
        pieces.insert(tid, text);
    }
    let text = sequences
        .iter()
        .enumerate()
        .filter_map(|(tid, _)| pieces.remove(&(tid as i32)))
        .collect::<String>();
    fs::write(out.join("genomecov.bg"), text)?;
    Ok(())
}

fn genomecov_tid(
    tid: i32,
    indices: &[usize],
    resident: &Resident,
    sequences: &[(String, u64)],
) -> Result<(i32, String)> {
    let (name, length) = &sequences[tid as usize];
    let length = i32::try_from(*length).context("reference length does not fit i32")?;
    // A sorted event sweep is proportional to aligned blocks, not reference length.
    // Some Tier 0 records cover several whole chromosomes, so even a per-tid span can
    // still be hundreds of megabases.
    let mut events = Vec::new();
    for &index in indices {
        let fixed = resident.headers()[index];
        if fixed.flag & 0x4 != 0 {
            continue;
        }
        let cigar = bam_cigar(resident.record_bytes(fixed))?;
        for (start, end) in genomecov_blocks(fixed.pos, &cigar) {
            let start = start.clamp(0, length);
            let end = end.clamp(0, length);
            if start < end {
                events.push((start, 1_i32));
                events.push((end, -1_i32));
            }
        }
    }
    if events.is_empty() {
        return Ok((tid, String::new()));
    }
    events.sort_unstable_by_key(|&(position, _)| position);
    let mut text = String::new();
    let mut depth = 0_i32;
    let mut run_start = 0_i32;
    let mut event = 0;
    while event < events.len() {
        let position = events[event].0;
        let mut delta = 0;
        while event < events.len() && events[event].0 == position {
            delta += events[event].1;
            event += 1;
        }
        let previous = depth;
        depth += delta;
        if depth != previous {
            if previous > 0 {
                text.push_str(&format!("{name}\t{run_start}\t{position}\t{previous}\n"));
            }
            if depth > 0 {
                run_start = position;
            }
        }
    }
    debug_assert_eq!(depth, 0, "coverage events must balance");
    Ok((tid, text))
}

fn peak_rss_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage initializes its output on success.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } == 0 {
        // SAFETY: getrusage reported success.
        let rss = unsafe { usage.assume_init() }.ru_maxrss as u64;
        #[cfg(target_os = "macos")]
        {
            rss
        }
        #[cfg(not(target_os = "macos"))]
        {
            rss.saturating_mul(1024)
        }
    } else {
        0
    }
}

fn write_timing(out: &Path, timing: &Timing) -> io::Result<()> {
    fs::write(
        out.join("timing.tsv"),
        format!(
            "stage\tseconds\ndecode\t{:.6}\nsort\t{:.6}\nwrite_sorted\t{:.6}\nmarkdup\t{:.6}\nwrite_markdup\t{:.6}\nindex\t{:.6}\nfeaturecounts\t{:.6}\ngenomecov\t{:.6}\npeak_rss_bytes\t{}\n",
            timing.decode.as_secs_f64(),
            timing.sort.as_secs_f64(),
            timing.write_sorted.as_secs_f64(),
            timing.markdup.as_secs_f64(),
            timing.write_markdup.as_secs_f64(),
            timing.index.as_secs_f64(),
            timing.featurecounts.as_secs_f64(),
            timing.genomecov.as_secs_f64(),
            peak_rss_bytes()
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::RecordHeader;
    #[test]
    fn record_header_is_48_bytes() {
        assert_eq!(std::mem::size_of::<RecordHeader>(), 48);
    }
}
