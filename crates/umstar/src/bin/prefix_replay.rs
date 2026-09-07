//! Untimed exact replay: no request filtering and no use of captured prefix/SA bounds.
use std::{error::Error, fs, path::Path};
use umgpu::{ProbeConfig, ProbeConfigV2, ProbeRequestV2};
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: prefix_replay INDEX_DIR real-requests.bin loaded-prefix-config.bin (requires --features cuda)".into());
    }
    let bytes = fs::read(&args[3])?;
    if bytes.len() != 224 {
        return Err("expected exactly 224 LE configuration bytes from loaded Genome".into());
    }
    let w: Vec<_> = bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| u64::from_le_bytes(*b))
        .collect();
    let mut c = ProbeConfigV2 {
        inner: ProbeConfig {
            n_genome: w[0],
            n_sa: w[1],
            strand_bit: w[2],
        },
        index_bases: w[3],
        sai_width: w[4],
        absent_mask: w[5],
        n_mask: w[6],
        n_mask_c: w[7],
        sparse: w[8],
        seed_search_lmax: w[9],
        sai_offset: w[10],
        sai_bytes: w[11],
        ..Default::default()
    };
    c.starts.copy_from_slice(&w[12..]);
    let captured = umseed_probe::requests::probe_read(Path::new(&args[2]), false)?;
    if captured.count != 999_914 {
        return Err(format!("expected all 999914 requests, found {}", captured.count).into());
    }
    let expected = captured
        .star_tuples
        .as_ref()
        .ok_or("captured STAR tuple sidecar required")?;
    let index = umseed_probe::index::probe_load(Path::new(&args[1]), false)?;
    let requests: Vec<_> = captured
        .requests
        .as_pod_slice::<umgpu::ProbeRequest>()
        .iter()
        .map(|q| {
            let mut inner = *q;
            inner.tag = 1;
            // Poison these to prove the device derives the interval and prefix itself.
            inner.prefix = u64::MAX;
            inner.low = u64::MAX;
            inner.high = u64::MAX;
            ProbeRequestV2 { inner, distance: 0 }
        })
        .collect();
    // CPU outer transcription also has to match every captured tuple before GPU work.
    for (i, (&r, want)) in requests.iter().zip(expected).enumerate() {
        let got = umstar::prefix_oracle::prefix_oracle(
            index.genome.as_slice(),
            index.sa.as_slice(),
            index.sai.as_slice(),
            captured.reads.as_slice(),
            c,
            r,
        );
        if got.inner != *want {
            return Err(format!("CPU prefix vs STAR mismatch at {i}: {got:?} != {want:?}").into());
        }
    }
    let mut session = umstar::PrefixSession::new(index, c, 1)?;
    let mut branches = [0u64; 4];
    let mut matched = 0usize;
    for (batch, chunk) in requests.chunks(262_144).enumerate() {
        let (out, _) = session.search(1, captured.reads.as_slice(), chunk)?;
        for (j, got) in out.iter().enumerate() {
            let i = batch * 262_144 + j;
            if got.inner != expected[i] {
                return Err(format!(
                    "GPU prefix vs STAR mismatch at {i}: {got:?} != {:?}",
                    expected[i]
                )
                .into());
            }
            if got.branch > 3 {
                return Err(format!("unknown branch at {i}: {}", got.branch).into());
            }
            branches[got.branch as usize] += 1;
            matched += 1;
        }
    }
    println!("matched\ttotal\tlegacy\tprefix_only\tunique\tsearched");
    println!(
        "{matched}\t{}\t{}\t{}\t{}\t{}",
        requests.len(),
        branches[0],
        branches[1],
        branches[2],
        branches[3]
    );
    Ok(())
}
