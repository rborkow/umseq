//! PROBE resident pinned STAR layout loader. SA_SEARCH_FULL keeps annotation SJ bytes.
use crate::cpu::probe_packed_at;
use anyhow::{Context, Result, bail, ensure};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::Path,
    process::Command,
    time::Instant,
};
use umem::{Allocation, Buf, Ro, Rw};
use umgpu::ProbeConfig;

pub struct ProbeResident {
    pub genome: Buf<Ro>,
    pub sa: Buf<Ro>,
    pub sai: Buf<Ro>,
    pub config: ProbeConfig,
    pub hashes: String,
    pub load_seconds: f64,
}

pub fn probe_sha256(path: &Path) -> Result<String> {
    let output = match Command::new("sha256sum").arg(path).output() {
        Ok(o) => o,
        Err(_) => Command::new("shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()?,
    };
    ensure!(
        output.status.success(),
        "PROBE SHA256 failed for {}",
        path.display()
    );
    let hash = String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_owned();
    ensure!(
        hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "PROBE invalid SHA256 output"
    );
    Ok(hash)
}

pub fn probe_allocate(bytes: usize, small: bool, name: &str) -> Result<Buf<Rw>> {
    let b = Buf::<Rw>::allocate(
        bytes,
        Allocation::Anon {
            huge: !small,
            require_huge: !small,
        },
    )?;
    let p = b.page_report();
    ensure!(
        if small {
            p.huge_bytes == 0
        } else {
            p.huge_bytes == p.len
        },
        "PROBE {name}: page backing failed: {p:?}"
    );
    eprintln!(
        "PROBE allocation name={name} cpu_ptr={:p} logical_bytes={bytes} report={p:?}",
        b.as_slice().as_ptr()
    );
    Ok(b)
}
fn extent(n: u64, width: u64) -> Result<usize> {
    ensure!(n > 0, "PROBE empty packed array");
    Ok(usize::try_from(
        (n - 1)
            .checked_mul(width)
            .context("PROBE packed overflow")?
            / 8
            + 8,
    )?)
}
fn number(p: &BTreeMap<String, String>, key: &str) -> Result<u64> {
    p.get(key)
        .with_context(|| format!("PROBE missing parameter {key}"))?
        .parse()
        .with_context(|| format!("PROBE invalid {key}"))
}

pub fn probe_load(dir: &Path, small: bool) -> Result<ProbeResident> {
    let started = Instant::now();
    let text = fs::read_to_string(dir.join("genomeParameters.txt"))?;
    let mut p = BTreeMap::new();
    let mut strand = None;
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.first() == Some(&"###") {
            if fields.get(1) == Some(&"GstrandBit") {
                ensure!(
                    strand.is_none() && fields.len() == 3,
                    "PROBE duplicate/malformed GstrandBit"
                );
                strand = Some(fields[2].parse::<u64>()?);
            }
            continue;
        }
        if fields.is_empty() || fields[0].starts_with('#') {
            continue;
        }
        ensure!(
            p.insert(fields[0].to_owned(), fields[1..].join(" "))
                .is_none(),
            "PROBE duplicate parameter {}",
            fields[0]
        );
    }
    ensure!(
        p.get("versionGenome").map(String::as_str) == Some("2.7.4a"),
        "PROBE unsupported index format versionGenome {:?}; expected 2.7.4a (not executable version)",
        p.get("versionGenome")
    );
    for (key, expected) in [
        ("genomeType", "Full"),
        ("genomeTransformType", "None"),
        ("genomeTransformVCF", "-"),
    ] {
        ensure!(
            p.get(key).is_none_or(|v| v == expected),
            "PROBE unsupported {key}={:?}",
            p.get(key)
        );
    }
    ensure!(
        number(&p, "genomeSAsparseD")? == 1,
        "PROBE unsupported sparse SA: genomeSAsparseD != 1"
    );
    if p.get("twopassMode").is_some_and(|v| v != "None") {
        bail!("PROBE unsupported mapping-time two-pass state");
    }
    let sizes: Vec<u64> = p
        .get("genomeFileSizes")
        .context("PROBE missing genomeFileSizes")?
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        sizes.len() == 2
            && sizes[0] > 0
            && sizes[0] <= 8 << 30
            && sizes[1] > 0
            && sizes[1] <= 64 << 30,
        "PROBE unsupported file sizes"
    );
    let ng = sizes[0];
    let bit = strand.unwrap_or((64 - ng.leading_zeros() as u64).max(32));
    ensure!(
        (32..=53).contains(&bit) && ng <= 1 << bit,
        "PROBE unsupported GstrandBit={bit}"
    );
    let ns = sizes[1] * 8 / (bit + 1);
    let sa_extent = extent(ns, bit + 1)?;
    ensure!(
        sa_extent >= sizes[1] as usize && sa_extent - sizes[1] as usize <= 8,
        "PROBE packed SA tail extent"
    );
    for (name, size) in [("Genome", ng), ("SA", sizes[1])] {
        ensure!(
            fs::metadata(dir.join(name))?.len() == size,
            "PROBE {name} file size disagrees with genomeFileSizes"
        );
    }
    let prefix = number(&p, "genomeSAindexNbases")?;
    ensure!(
        (1..=15).contains(&prefix),
        "PROBE prefix bases outside 1..15"
    );
    let mut sai_file = File::open(dir.join("SAindex"))?;
    let mut head = vec![0u8; (8 * (prefix + 2)) as usize];
    sai_file.read_exact(&mut head)?;
    let words: Vec<u64> = head
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| u64::from_le_bytes(*b))
        .collect();
    ensure!(
        words[0] == prefix && words[1] == 0,
        "PROBE SAindex header mismatch"
    );
    for i in 0..prefix as usize {
        ensure!(
            words[i + 2] == words[i + 1] + (1u64 << (2 * (i + 1))),
            "PROBE SAindex prefix offsets inconsistent with pinned layout"
        );
    }
    let n_sai = *words.last().unwrap();
    let sai_extent = extent(n_sai, bit + 3)?;
    ensure!(
        sai_file.metadata()?.len() == (head.len() + sai_extent) as u64,
        "PROBE SAindex extent mismatch"
    );
    // Largest random-access array first; all bytes are read directly into umem.
    let mut sa = probe_allocate(sa_extent, small, "PROBE-SA")?;
    File::open(dir.join("SA"))?.read_exact(&mut sa.as_mut_slice()[..sizes[1] as usize])?;
    let mut genome = probe_allocate(ng as usize + 400, small, "PROBE-Genome")?;
    genome.as_mut_slice()[..200].fill(5);
    genome.as_mut_slice()[ng as usize + 200..].fill(5);
    File::open(dir.join("Genome"))?
        .read_exact(&mut genome.as_mut_slice()[200..ng as usize + 200])?;
    let mut sai = probe_allocate(head.len() + sai_extent, small, "PROBE-SAindex")?;
    sai.as_mut_slice()[..head.len()].copy_from_slice(&head);
    sai_file.read_exact(&mut sai.as_mut_slice()[head.len()..])?;
    // Once-only full storage validation, never inside per-request timing.
    ensure!(
        genome.as_slice().iter().all(|&v| v <= 5),
        "PROBE invalid genome alphabet"
    );
    for i in 0..ns {
        ensure!(
            probe_packed_at(sa.as_slice(), i, bit + 1) & !(1 << bit) < ng,
            "PROBE SA[{i}] address outside genome"
        );
    }
    let n_mask = 1u64 << (bit + 1);
    let absent_mask = 1u64 << (bit + 2);
    for i in 0..n_sai {
        let v = probe_packed_at(&sai.as_slice()[head.len()..], i, bit + 3);
        ensure!(
            v & !(n_mask | absent_mask) <= ns,
            "PROBE SAindex[{i}] coordinate outside SA"
        );
    }
    let starts: Vec<u64> = fs::read_to_string(dir.join("chrStart.txt"))?
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    ensure!(
        starts.first() == Some(&0) && starts.windows(2).all(|x| x[0] < x[1]),
        "PROBE chromosome geometry"
    );
    let lengths: Vec<u64> = fs::read_to_string(dir.join("chrLength.txt"))?
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    let names = fs::read_to_string(dir.join("chrName.txt"))?;
    let names: Vec<_> = names.split_whitespace().collect();
    let name_lengths = fs::read_to_string(dir.join("chrNameLength.txt"))?;
    let name_lengths: Vec<_> = name_lengths.lines().collect();
    ensure!(
        lengths.len() + 1 == starts.len()
            && names.len() == lengths.len()
            && name_lengths.len() == lengths.len(),
        "PROBE chromosome metadata count mismatch"
    );
    for (i, &length) in lengths.iter().enumerate() {
        ensure!(
            length > 0 && length <= starts[i + 1] - starts[i],
            "PROBE chromosome {i} span exceeds bin"
        );
        let row: Vec<_> = name_lengths[i].split_whitespace().collect();
        ensure!(
            row.len() == 2 && row[0] == names[i] && row[1].parse::<u64>()? == length,
            "PROBE chromosome {i} name/length mismatch"
        );
    }
    let terminal = *starts.last().context("PROBE empty chrStart")?;
    ensure!(terminal <= ng, "PROBE chromosome terminal exceeds genome");
    let overhang = number(&p, "sjdbOverhang")?;
    if terminal < ng {
        let sj = fs::read_to_string(dir.join("sjdbInfo.txt"))
            .context("PROBE annotation SJ bytes present but missing sjdbInfo.txt")?;
        let mut rows = sj.lines();
        let header: Vec<u64> = rows
            .next()
            .context("PROBE missing SJ header")?
            .split_whitespace()
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()?;
        ensure!(
            header.len() == 2
                && header[1] == overhang
                && overhang > 0
                && overhang <= 4096
                && header[0] <= 1_000_000,
            "PROBE unsupported SJ header/overhang"
        );
        ensure!(
            terminal + header[0] * (2 * overhang + 1) <= ng,
            "PROBE annotation SJ extent exceeds genome tail"
        );
        let mut sj_count = 0;
        for (i, row) in rows.filter(|l| !l.trim().is_empty()).enumerate() {
            let x: Vec<u64> = row
                .split_whitespace()
                .map(str::parse)
                .collect::<std::result::Result<_, _>>()?;
            ensure!(
                x.len() == 6 && x[2] <= 6 && x[3] <= 255 && x[4] <= 255 && x[5] <= 2,
                "PROBE SJ row {i} field/range unsupported"
            );
            ensure!(
                x[0] >= overhang && x[0] <= x[1] && x[1] < terminal,
                "PROBE SJ row {i} coordinate unsupported"
            );
            let shift = if x[2] == 0 { x[3] } else { 0 };
            let donor = x[0] - overhang + shift;
            let acceptor = x[1] + 1 + shift;
            ensure!(
                donor <= terminal
                    && acceptor <= terminal
                    && overhang <= terminal - donor
                    && overhang <= terminal - acceptor,
                "PROBE SJ row {i} translated span outside pre-SJ genome"
            );
            sj_count += 1;
        }
        ensure!(sj_count == header[0], "PROBE SJ row count mismatch");
        for i in 0..header[0] {
            let spacer = terminal + (i + 1) * (2 * overhang + 1) - 1;
            ensure!(
                genome.as_slice()[200 + spacer as usize] == 5,
                "PROBE SJ terminal spacer mismatch at {i}"
            );
        }
        eprintln!(
            "PROBE annotation_SJ_count={} overhang={overhang} sjdbInsertSave={:?}; retained on-disk bytes, no mapping-time insertion",
            header[0],
            p.get("sjdbInsertSave")
        );
    }
    let mut hashes = Vec::new();
    for (name, bytes) in [
        ("Genome", &genome.as_slice()[200..200 + ng as usize]),
        ("SA", &sa.as_slice()[..sizes[1] as usize]),
        ("SAindex", sai.as_slice()),
    ] {
        let resident_hash = probe_sha256_bytes(bytes)?;
        ensure!(
            resident_hash == probe_sha256(&dir.join(name))?,
            "PROBE loaded snapshot/file SHA256 mismatch: {name}; source changed during load"
        );
        eprintln!("PROBE resident_snapshot_sha256 {name}={resident_hash}");
    }
    for name in [
        "Genome",
        "SA",
        "SAindex",
        "genomeParameters.txt",
        "chrStart.txt",
        "chrLength.txt",
        "chrName.txt",
        "chrNameLength.txt",
    ] {
        hashes.push(format!("{name}:{}", probe_sha256(&dir.join(name))?));
    }
    if dir.join("sjdbInfo.txt").exists() {
        hashes.push(format!(
            "sjdbInfo.txt:{}",
            probe_sha256(&dir.join("sjdbInfo.txt"))?
        ));
    }
    eprintln!(
        "PROBE nGenome={ng} nSA={ns} GstrandBit={bit} GstrandMask={:#x} SAiMarkNmask={:#x} SAiMarkAbsentMask={:#x} SA_SEARCH_FULL; seedSearchLmax=0 unlimited",
        !(1u64 << bit),
        !n_mask,
        !absent_mask
    );
    Ok(ProbeResident {
        genome: genome.freeze(),
        sa: sa.freeze(),
        sai: sai.freeze(),
        config: ProbeConfig {
            n_genome: ng,
            n_sa: ns,
            strand_bit: bit,
        },
        hashes: hashes.join(","),
        load_seconds: started.elapsed().as_secs_f64(),
    })
}

/// PROBE hashes the actual loaded snapshot through the local SHA256 utility.
/// This setup-only host pipe is not GPU staging; no second resident index is built.
pub fn probe_sha256_bytes(bytes: &[u8]) -> Result<String> {
    use std::{io::Write, process::Stdio};
    let mut command = Command::new("sha256sum");
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_) => Command::new("shasum")
            .args(["-a", "256"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?,
    };
    child
        .stdin
        .take()
        .context("PROBE hash pipe unavailable")?
        .write_all(bytes)?;
    let output = child.wait_with_output()?;
    ensure!(output.status.success(), "PROBE resident SHA256 failed");
    let hash = String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_owned();
    ensure!(
        hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
        "PROBE resident hash malformed"
    );
    Ok(hash)
}
