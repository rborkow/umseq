//! PROBE deterministic synthetic SA_SEARCH_FULL requests from real mate1 FASTQ bytes.
use crate::index::{probe_allocate, probe_sha256};
use anyhow::{Context, Result, ensure};
use flate2::read::MultiGzDecoder;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::Path,
};
use umem::{Buf, Ro};
use umgpu::ProbeRequest;

pub const PROBE_COUNTS: [usize; 4] = [64_000, 256_000, 1_000_000, 4_000_000];
const MAGIC: &[u8; 8] = b"UMPROBE1";
const STAR_TUPLES_MAGIC: &[u8; 8] = b"UMSTAR01";
fn words(r: ProbeRequest) -> [u64; 10] {
    [
        r.tag, r.s0, r.s1, r.read_len, r.start, r.length, r.prefix, r.low, r.high, r.dir,
    ]
}
fn request(w: [u64; 10]) -> ProbeRequest {
    ProbeRequest {
        tag: w[0],
        s0: w[1],
        s1: w[2],
        read_len: w[3],
        start: w[4],
        length: w[5],
        prefix: w[6],
        low: w[7],
        high: w[8],
        dir: w[9],
    }
}

pub fn probe_generate(
    fastq: &Path,
    output: &Path,
    index: &Path,
    count: usize,
    seed: u64,
) -> Result<()> {
    ensure!(
        count > 0 && count <= 4_000_000 && count.is_multiple_of(2),
        "PROBE count must be even and <=4000000"
    );
    ensure!(!output.exists(), "PROBE refusing to overwrite request file");
    let parameters = fs::read_to_string(index.join("genomeParameters.txt"))?;
    let strand: u64 = parameters
        .lines()
        .find_map(|l| l.trim().strip_prefix("### GstrandBit "))
        .context("PROBE generator requires explicit ### GstrandBit")?
        .trim()
        .parse()?;
    ensure!(
        (32..=53).contains(&strand),
        "PROBE generator unsupported strand bit"
    );
    let n_sa = fs::metadata(index.join("SA"))?
        .len()
        .checked_mul(8)
        .context("PROBE SA size overflow")?
        / (strand + 1);
    ensure!(n_sa > 0, "PROBE empty SA");
    let mut input = BufReader::new(MultiGzDecoder::new(File::open(fastq)?));
    let mut lines = [String::new(), String::new(), String::new(), String::new()];
    let mut requests = Vec::with_capacity(count);
    let mut reads = Vec::new();
    let mut state = seed.max(1);
    let mut visited = 0u64;
    let mut sampled = 0u64;
    let mut skipped_n = 0u64;
    while requests.len() < count {
        for line in &mut lines {
            line.clear();
            ensure!(
                input.read_line(line)? > 0,
                "PROBE FASTQ exhausted at {} requests",
                requests.len()
            );
        }
        visited += 1;
        let sequence = lines[1].trim_end().as_bytes();
        ensure!(
            lines[0].starts_with('@')
                && lines[2].starts_with('+')
                && lines[3].trim_end().len() == sequence.len(),
            "PROBE malformed FASTQ at read {visited}"
        );
        ensure!(
            !sequence.is_empty() && sequence.len() <= 4096,
            "PROBE unsupported read length at read {visited}"
        );
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        if state & 7 != 0 {
            continue;
        } // fixed-seed 1/8 sampling of the source prefix
        let encoded: Option<Vec<u8>> = sequence
            .iter()
            .map(|b| match b {
                b'A' | b'a' => Some(0),
                b'C' | b'c' => Some(1),
                b'G' | b'g' => Some(2),
                b'T' | b't' => Some(3),
                _ => None,
            })
            .collect();
        let Some(encoded) = encoded else {
            skipped_n += 1;
            continue;
        };
        sampled += 1;
        let offset = reads.len() as u64;
        let len = encoded.len() as u64;
        reads.extend_from_slice(&encoded);
        reads.extend(encoded.iter().map(|b| 3 - b)); // position-wise complement, never reversed
        for s in (0..len).step_by(20) {
            for dir in [1, 0] {
                if requests.len() == count {
                    break;
                }
                requests.push(ProbeRequest {
                    s0: offset,
                    s1: offset + len,
                    read_len: len,
                    start: if dir == 1 { s } else { len - 1 - s },
                    length: len - s,
                    high: n_sa - 1,
                    dir,
                    ..Default::default()
                });
            }
        }
    }
    let mut out = BufWriter::new(File::create(output)?);
    out.write_all(MAGIC)?;
    for v in [count as u64, reads.len() as u64, seed] {
        out.write_all(&v.to_le_bytes())?;
    }
    for r in requests {
        for w in words(r) {
            out.write_all(&w.to_le_bytes())?;
        }
    }
    out.write_all(&reads)?;
    out.flush()?;
    let provenance = format!(
        "PROBE synthetic SA_SEARCH_FULL (not captured STAR scheduler requests)\ncount={count}\nseed={seed}\nvisited_reads={visited}\nsampled_reads={sampled}\nskipped_non_acgt={skipped_n}\nsource_fastq={}\nsource_sha256={}\nindex_parameters_sha256={}\nrequest_sha256={}\nseedSearchLmax=0 unlimited (pinned parametersDefault:575); N=remaining read length; grid=20; s1=position-wise complement\n",
        fastq.display(),
        probe_sha256(fastq)?,
        probe_sha256(&index.join("genomeParameters.txt"))?,
        probe_sha256(output)?
    );
    fs::write(output.with_extension("probe-provenance.txt"), &provenance)?;
    eprintln!("{provenance}");
    Ok(())
}

pub struct ProbeRequests {
    pub requests: Buf<Ro>,
    pub reads: Buf<Ro>,
    pub hash: String,
    pub count: usize,
    pub provenance: String,
    /// Captured STAR `(length, low, high, count)` oracle, when this is a real
    /// inner-request artifact. Synthetic artifacts intentionally have none.
    pub star_tuples: Option<Vec<umgpu::ProbeOutput>>,
}

fn star_tuple_path(path: &Path) -> std::path::PathBuf {
    path.with_extension("star-tuples.bin")
}

fn read_star_tuples(path: &Path, count: usize) -> Result<Option<Vec<umgpu::ProbeOutput>>> {
    let path = star_tuple_path(path);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    ensure!(
        bytes.len() >= 16 && &bytes[..8] == STAR_TUPLES_MAGIC,
        "STAR tuple magic/version mismatch"
    );
    let found = u64::from_le_bytes(bytes[8..16].try_into().unwrap()) as usize;
    ensure!(
        found == count,
        "STAR tuple count differs from request count"
    );
    ensure!(bytes.len() == 16 + found * 32, "STAR tuple extent mismatch");
    let mut tuples = Vec::with_capacity(found);
    let (tuple_bytes, remainder) = bytes[16..].as_chunks::<32>();
    ensure!(remainder.is_empty(), "STAR tuple extent mismatch");
    for chunk in tuple_bytes {
        tuples.push(umgpu::ProbeOutput {
            length: u64::from_le_bytes(chunk[0..8].try_into().unwrap()),
            low: u64::from_le_bytes(chunk[8..16].try_into().unwrap()),
            high: u64::from_le_bytes(chunk[16..24].try_into().unwrap()),
            count: u64::from_le_bytes(chunk[24..32].try_into().unwrap()),
            status: 0,
        });
    }
    Ok(Some(tuples))
}
pub fn probe_read(path: &Path, small: bool) -> Result<ProbeRequests> {
    let mut f = BufReader::new(File::open(path)?);
    let mut head = [0u8; 32];
    f.read_exact(&mut head)?;
    ensure!(&head[..8] == MAGIC, "PROBE request magic/version mismatch");
    let count = u64::from_le_bytes(head[8..16].try_into().unwrap()) as usize;
    let bytes = u64::from_le_bytes(head[16..24].try_into().unwrap()) as usize;
    ensure!(
        count > 0 && count <= 4_000_000 && bytes > 0 && bytes <= 4 << 30,
        "PROBE request file exceeds caps"
    );
    ensure!(
        fs::metadata(path)?.len() == (32 + count * 80 + bytes) as u64,
        "PROBE request extent mismatch"
    );
    let mut requests = probe_allocate(count * 80, small, "PROBE-requests")?;
    // Explicit LE decode, independent of native struct dumps.
    for r in requests.as_pod_mut_slice::<ProbeRequest>() {
        let mut w = [0u64; 10];
        for item in &mut w {
            let mut b = [0u8; 8];
            f.read_exact(&mut b)?;
            *item = u64::from_le_bytes(b);
        }
        *r = request(w);
    }
    let mut reads = probe_allocate(bytes, small, "PROBE-reads")?;
    f.read_exact(reads.as_mut_slice())?;
    // SSIR captures preserve both STAR buffers byte-for-byte.  Bytes outside an
    // observed eligible span may be STAR's N/separator values (4/11), so this
    // is intentionally not a synthetic-complement invariant.
    ensure!(
        reads.as_slice().iter().all(|b| *b <= 4 || *b == 11),
        "PROBE read arena contains unsupported STAR alphabet"
    );
    // Validate all request metadata and every potentially selected observed span
    // ONCE before timing. `s1` is never reconstructed from `s0`.
    for (i, r) in requests.as_pod_slice::<ProbeRequest>().iter().enumerate() {
        ensure!(
            r.tag == 0
                && r.dir <= 1
                && r.read_len > 0
                && r.read_len <= 4096
                && r.length > 0
                && r.prefix <= r.length
                && r.start < r.read_len
                && r.s0 <= bytes as u64
                && r.read_len <= bytes as u64 - r.s0
                && r.s1 <= bytes as u64
                && r.read_len <= bytes as u64 - r.s1
                && if r.dir == 1 {
                    r.length <= r.read_len - r.start
                } else {
                    r.length <= r.start + 1
                },
            "PROBE unsupported request/bounds at {i}"
        );
        for k in 0..r.length {
            let p = if r.dir == 1 { r.start + k } else { r.start - k };
            ensure!(
                reads.as_slice()[(r.s0 + p) as usize] <= 3
                    && reads.as_slice()[(r.s1 + p) as usize] <= 3,
                "PROBE non-ACGT byte in observed eligible request span {i}"
            );
        }
    }
    let hash = probe_sha256(path)?;
    let provenance = fs::read_to_string(path.with_extension("probe-provenance.txt"))
        .context("PROBE missing request source provenance")?;
    ensure!(
        provenance
            .lines()
            .any(|line| line == format!("request_sha256={hash}")),
        "PROBE request hash differs from generation provenance"
    );
    let star_tuples = read_star_tuples(path, count)?;
    let real = provenance
        .lines()
        .any(|line| line == "source_kind=STAR_INNER_CAPTURE");
    ensure!(
        !real || star_tuples.is_some(),
        "real STAR capture requires adjacent star-tuples.bin"
    );
    Ok(ProbeRequests {
        requests: requests.freeze(),
        reads: reads.freeze(),
        hash,
        provenance,
        count,
        star_tuples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_request_wire_roundtrip() {
        let r = ProbeRequest {
            s0: 123,
            s1: 456,
            read_len: 151,
            start: 150,
            length: 151,
            high: 6_000_000_000,
            dir: 0,
            ..Default::default()
        };
        assert_eq!(request(words(r)), r);
        assert_eq!(PROBE_COUNTS, [64000, 256000, 1000000, 4000000]);
    }

    #[test]
    fn real_tuple_sidecar_requires_exact_count() {
        let root = std::env::temp_dir().join(format!("umprobe-tuples-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let p = root.join("requests.bin");
        fs::write(&p, b"x").unwrap();
        let mut sidecar = STAR_TUPLES_MAGIC.to_vec();
        sidecar.extend(2u64.to_le_bytes());
        sidecar.extend([0u8; 64]);
        fs::write(star_tuple_path(&p), sidecar).unwrap();
        assert!(read_star_tuples(&p, 1).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
