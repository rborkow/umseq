use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
use umgpu::{ProbeConfig, ProbeConfigV2, ProbeRequest, ProbeRequestV2, ProbeRequestV3};
use umstar::prefix_oracle::prefix_oracle;
fn pack(values: &[u64], width: u64) -> Vec<u8> {
    let mut bytes = vec![0; ((values.len() as u64 - 1) * width / 8 + 8) as usize];
    for (i, &v) in values.iter().enumerate() {
        for bit in 0..width {
            if v & (1 << bit) != 0 {
                let b = i as u64 * width + bit;
                bytes[b as usize / 8] |= 1 << (b % 8);
            }
        }
    }
    bytes
}
fn words(bytes: &mut Vec<u8>, values: impl IntoIterator<Item = u64>) {
    for v in values {
        bytes.extend(v.to_le_bytes());
    }
}
#[test]
fn every_prefix_branch_matches_independent_star_transcription() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let temp = std::env::temp_dir().join(format!("star-prefix-oracle-{}", std::process::id()));
    fs::create_dir_all(&temp).unwrap();
    let exe = temp.join("host");
    assert!(
        Command::new("c++")
            .args(["-std=c++17", "-O2", "-Wall", "-Wextra", "-Werror"])
            .arg("-include")
            .arg("cstdlib")
            .arg("-I")
            .arg(root.join("../umgpu/shim"))
            .arg(root.join("tests/prefix_host.cpp"))
            .arg("-o")
            .arg(&exe)
            .status()
            .unwrap()
            .success()
    );
    let nmark = 1 << 33;
    let absent = 1 << 34;
    let mut entries = vec![0, 2, 4, 6];
    entries.extend([
        0,
        1,
        2 | absent,
        2 | absent,
        2,
        4,
        4 | absent,
        4 | absent,
        4 | nmark,
        5,
        6 | absent,
        6 | absent,
        6,
        7,
        8 | absent,
        8 | absent,
    ]);
    let mut c = ProbeConfigV2 {
        inner: ProbeConfig {
            n_genome: 64,
            n_sa: 8,
            strand_bit: 32,
        },
        index_bases: 2,
        sai_width: 35,
        absent_mask: absent,
        n_mask: !nmark,
        n_mask_c: nmark,
        sparse: 1,
        sai_offset: 32,
        ..Default::default()
    };
    c.starts[..3].copy_from_slice(&[0, 4, 20]);
    let mut g = vec![5; 464];
    for i in 0..64 {
        g[200 + i] = (i % 4) as u8;
    }
    g[218] = 4;
    let sa = pack(&[0, 4, 8, 12, 16, (1 << 32) | 20, 24, (1 << 32) | 28], 33);
    let mut branches = [0usize; 4];
    let mut rejected_zero = 0;
    // Second table removes the entire A prefix, forcing Lind==0 for that grid subset.
    for (all_absent, sparse, lmax) in [(false, 1, 0), (true, 1, 0), (false, 2, 0), (false, 1, 1)] {
        c.sparse = sparse;
        c.seed_search_lmax = lmax;
        let mut e = entries.clone();
        if all_absent {
            e[0] |= absent;
            for v in &mut e[4..8] {
                *v |= absent;
            }
        }
        let mut sai = Vec::new();
        words(&mut sai, [2, 0, 4, 20]);
        sai.extend(pack(&e, 35));
        c.sai_bytes = sai.len() as u64;
        let mut reads = Vec::new();
        let mut requests = Vec::new();
        for sequence in 0..256u64 {
            let offset = reads.len() as u64;
            let bases: Vec<_> = (0..4).map(|k| ((sequence >> (2 * k)) & 3) as u8).collect();
            reads.extend(&bases);
            reads.extend(bases.iter().map(|b| 3 - b));
            for start in 0..4 {
                for dir in 0..2 {
                    for length in 1..=if dir == 1 { 4 - start } else { start + 1 } {
                        requests.push(ProbeRequestV2 {
                            inner: ProbeRequest {
                                tag: 1,
                                s0: offset,
                                s1: offset + 4,
                                read_len: 4,
                                start,
                                length,
                                dir,
                                prefix: u64::MAX,
                                low: u64::MAX,
                                high: u64::MAX,
                            },
                            distance: 0,
                        });
                    }
                }
            }
        }
        let mut legacy = requests[0];
        legacy.inner.tag = 0;
        legacy.inner.prefix = 0;
        legacy.inner.low = 0;
        legacy.inner.high = 7;
        requests.push(legacy);
        // Profile rejection must precede consumption of the old inner fields.
        let mut rejected = requests[0];
        rejected.distance = 1;
        requests.push(rejected);
        let expected: Vec<_> = requests
            .iter()
            .map(|&r| prefix_oracle(&g, &sa, &sai, &reads, c, r))
            .collect();
        for o in &expected {
            if o.inner.status == 0 {
                branches[o.branch as usize] += 1;
            }
            if o.inner.status == 6 {
                rejected_zero += 1;
            }
        }
        let mut input = Vec::new();
        words(
            &mut input,
            [
                64,
                8,
                32,
                2,
                35,
                absent,
                !nmark,
                nmark,
                sparse,
                lmax,
                32,
                c.sai_bytes,
            ],
        );
        words(&mut input, c.starts);
        words(
            &mut input,
            [g.len(), sa.len(), sai.len(), reads.len(), requests.len()].map(|n| n as u64),
        );
        input.extend(&g);
        input.extend(&sa);
        input.extend(&sai);
        input.extend(&reads);
        for r in &requests {
            let q = r.inner;
            words(
                &mut input,
                [
                    q.tag, q.s0, q.s1, q.read_len, q.start, q.length, q.prefix, q.low, q.high,
                    q.dir, r.distance,
                ],
            );
        }
        let mut child = Command::new(&exe)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
        let output = child.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout.len(), expected.len() * 48);
        for (i, (row, want)) in output
            .stdout
            .as_chunks::<48>()
            .0
            .iter()
            .zip(&expected)
            .enumerate()
        {
            let w: Vec<_> = row
                .as_chunks::<8>()
                .0
                .iter()
                .map(|b| u64::from_le_bytes(*b))
                .collect();
            let o = want.inner;
            assert_eq!(
                w,
                [o.length, o.low, o.high, o.count, o.status, want.branch],
                "request {i}: {:?}",
                requests[i]
            );
        }
        check_chains(
            &exe,
            &g,
            &sa,
            &sai,
            &reads,
            c,
            !all_absent && sparse == 1 && lmax == 0,
        );
        #[cfg(feature = "cuda")]
        check_device(&g, &sa, &sai, &reads, &requests, c, &expected);
    }
    assert!(branches[1..].iter().all(|&n| n > 0), "{branches:?}");
    assert!(rejected_zero > 0);
    fs::remove_dir_all(temp).unwrap();
}
#[cfg(feature = "cuda")]
fn check_device(
    g: &[u8],
    sa: &[u8],
    sai: &[u8],
    reads: &[u8],
    requests: &[ProbeRequestV2],
    c: ProbeConfigV2,
    expected: &[umgpu::ProbeOutputV2],
) {
    let alloc = |bytes: &[u8]| {
        let mut b = umseed_probe::index::probe_allocate(bytes.len(), false, "prefix-grid").unwrap();
        b.as_mut_slice().copy_from_slice(bytes);
        b.freeze()
    };
    let index = umseed_probe::index::ProbeResident {
        genome: alloc(g),
        sa: alloc(sa),
        sai: alloc(sai),
        sa_file_bytes: sa.len() as u64,
        config: c.inner,
        hashes: String::new(),
        load_seconds: 0.0,
    };
    let mut session = umstar::PrefixSession::new(index, c, 1).unwrap();
    let (outputs, _) = session.search(1, reads, requests).unwrap();
    assert_eq!(outputs, expected);
}

fn chain_words(o: &umgpu::ProbeOutputV3) -> Vec<u64> {
    let mut w = Vec::new();
    for s in o.steps {
        w.extend([s.shift, s.max_l, s.nrep, s.low, s.high, s.branch, s.status]);
    }
    w.extend([o.n_steps, o.flag_dir_map_cleared, o.status]);
    w
}
fn config_words(c: ProbeConfigV2) -> Vec<u64> {
    let mut w = vec![
        c.inner.n_genome,
        c.inner.n_sa,
        c.inner.strand_bit,
        c.index_bases,
        c.sai_width,
        c.absent_mask,
        c.n_mask,
        c.n_mask_c,
        c.sparse,
        c.seed_search_lmax,
        c.sai_offset,
        c.sai_bytes,
    ];
    w.extend(c.starts);
    w
}
#[allow(clippy::too_many_arguments)]
fn check_chains(
    exe: &std::path::Path,
    g: &[u8],
    sa: &[u8],
    sai: &[u8],
    reads: &[u8],
    c: ProbeConfigV2,
    require_coverage: bool,
) {
    let mut requests = Vec::new();
    for offset in (0..reads.len() as u64).step_by(8) {
        for piece_start in 0..4 {
            for piece_length in 1..=4 - piece_start {
                for nstart in 1..=piece_length + 1 {
                    let lstart = piece_length / nstart;
                    for istart in 0..nstart {
                        for dir in 0..2 {
                            for seed_map_min in 0..=piece_length {
                                for max_steps in [0, 1, 8, 9] {
                                    requests.push(ProbeRequestV3 {
                                        s0: offset,
                                        s1: offset + 4,
                                        read_len: 4,
                                        piece_start,
                                        piece_length,
                                        istart,
                                        nstart,
                                        lstart,
                                        dir,
                                        seed_map_min,
                                        max_steps,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Repeated A forces two-base unique steps in this synthetic table.
    // Sixteen bases takes eight steps; longer spans overflow.
    let mut reads = reads.to_vec();
    let offset = reads.len() as u64;
    reads.extend([0; 24]);
    reads.extend([3; 24]);
    for piece_length in [16, 18, 24] {
        for dir in 0..2 {
            requests.push(ProbeRequestV3 {
                s0: offset,
                s1: offset + 24,
                read_len: 24,
                piece_length,
                nstart: 1,
                lstart: piece_length,
                dir,
                max_steps: 8,
                ..Default::default()
            });
        }
    }
    let mut malformed = requests[0];
    malformed.istart = u64::MAX - 1;
    malformed.nstart = u64::MAX;
    malformed.lstart = 2;
    requests.push(malformed);
    malformed = requests[0];
    malformed.s0 = u64::MAX;
    requests.push(malformed);
    malformed = requests[0];
    malformed.dir = 2;
    requests.push(malformed);
    let expected: Vec<_> = requests
        .iter()
        .map(|&q| umstar::prefix_oracle::chain_oracle(g, sa, sai, &reads, c, q))
        .collect();
    if require_coverage {
        assert!(expected.iter().any(|o| o.flag_dir_map_cleared == 1));
        assert!(expected.iter().any(|o| o.status == 0 && o.n_steps == 0));
        assert!(expected.iter().any(|o| o.status == 0 && o.n_steps > 1));
        assert!(expected.iter().any(|o| o.status == 10 && o.n_steps == 1));
        assert!(expected.iter().any(|o| o.status == 9));
        assert!(expected.iter().any(|o| o.status == 0 && o.n_steps == 8));
        assert!(
            expected
                .iter()
                .any(|o| o.status == 0 && o.flag_dir_map_cleared == 0)
        );
        assert!(requests.iter().zip(&expected).any(|(q, o)| q.istart == 0
            && q.seed_map_min > 0
            && o.status == 0
            && o.n_steps == 1
            && o.steps[0].max_l < q.piece_length));
    }
    let mut input = Vec::new();
    words(&mut input, config_words(c));
    words(
        &mut input,
        [g.len(), sa.len(), sai.len(), reads.len(), requests.len()].map(|v| v as u64),
    );
    input.extend(g);
    input.extend(sa);
    input.extend(sai);
    input.extend(&reads);
    for q in &requests {
        words(
            &mut input,
            [
                q.s0,
                q.s1,
                q.read_len,
                q.piece_start,
                q.piece_length,
                q.istart,
                q.nstart,
                q.lstart,
                q.dir,
                q.seed_map_min,
                q.max_steps,
            ],
        );
    }
    let mut child = Command::new(exe)
        .arg("chains")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(&input).unwrap());
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout.len(), expected.len() * 472);
    for (i, (row, want)) in output
        .stdout
        .as_chunks::<472>()
        .0
        .iter()
        .zip(&expected)
        .enumerate()
    {
        let w: Vec<_> = row
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect();
        assert_eq!(w, chain_words(want), "chain {i}: {:?}", requests[i]);
    }
    #[cfg(feature = "cuda")]
    check_chain_device(
        g,
        sa,
        sai,
        &reads,
        &requests,
        c,
        &expected,
        if require_coverage {
            "ordinary"
        } else {
            "missing_A"
        },
    );
}
#[cfg(feature = "cuda")]
#[allow(clippy::too_many_arguments)]
fn check_chain_device(
    g: &[u8],
    sa: &[u8],
    sai: &[u8],
    reads: &[u8],
    requests: &[ProbeRequestV3],
    c: ProbeConfigV2,
    expected: &[umgpu::ProbeOutputV3],
    dataset: &str,
) {
    let alloc = |bytes: &[u8]| {
        let mut b = umseed_probe::index::probe_allocate(bytes.len(), false, "chain-grid").unwrap();
        b.as_mut_slice().copy_from_slice(bytes);
        b.freeze()
    };
    let index = umseed_probe::index::ProbeResident {
        genome: alloc(g),
        sa: alloc(sa),
        sai: alloc(sai),
        sa_file_bytes: sa.len() as u64,
        config: c.inner,
        hashes: String::new(),
        load_seconds: 0.0,
    };
    let mut session = umstar::PrefixSession::new(index, c, 1).unwrap();
    // The exhaustive grid exceeds the coordinator's 262,144-request batch cap
    // (a real limit, enforced only by the CUDA session). Submit in cap-sized
    // slices; every request is still checked, in order.
    const CAP: usize = 262_144;
    for variant in [umgpu::ProbeVariant::Thread, umgpu::ProbeVariant::Warp] {
        let mut outputs = Vec::with_capacity(requests.len());
        for slice in requests.chunks(CAP) {
            let (o, _, _) = session.search_chains(1, reads, slice, variant).unwrap();
            outputs.extend(o);
        }
        assert_eq!(outputs, expected, "{variant:?}");
    }
    if std::env::var_os("UMSTAR_CHAIN_BENCH").is_some() {
        bench_chains(&mut session, reads, requests, expected, dataset);
    }
}

// Explicit benchmark only: correctness above covers ALL requests, including rejections.
// Flatten exactly the steps actually executed by V3, including rejected-chain prefixes.
#[cfg(feature = "cuda")]
fn bench_chains(
    session: &mut umstar::PrefixSession,
    reads: &[u8],
    requests: &[ProbeRequestV3],
    expected: &[umgpu::ProbeOutputV3],
    dataset: &str,
) {
    let mut flat = Vec::new();
    let mut wants = Vec::new();
    for (q, o) in requests.iter().zip(expected) {
        let mut mapped = 0;
        for step in &o.steps[..o.n_steps as usize] {
            flat.push(ProbeRequestV2 {
                inner: ProbeRequest {
                    tag: 1,
                    s0: q.s0,
                    s1: q.s1,
                    read_len: q.read_len,
                    start: step.shift,
                    length: q.piece_length - q.istart * q.lstart - mapped,
                    dir: q.dir,
                    ..Default::default()
                },
                distance: 0,
            });
            wants.push(umgpu::ProbeOutputV2 {
                inner: umgpu::ProbeOutput {
                    length: step.max_l,
                    low: step.low,
                    high: step.high,
                    count: step.nrep,
                    status: step.status,
                },
                branch: step.branch,
            });
            mapped += step.max_l;
        }
    }
    if flat.is_empty() {
        return;
    }
    println!(
        "chain_bench,dataset,repeat,arm,chains,steps,overflow,max_steps,event_ms,gathers,gathers_per_s"
    );
    let mut control = None;
    // Warm-up round (-1), then five rotated repeats. Only CUDA event time is reported.
    for repeat in -1i32..5 {
        for turn in 0..3 {
            let arm = (turn + repeat.max(0)) % 3;
            let (stats, ms) = if arm == 0 {
                let (outputs, stats, ms) = session.search_timed(1, reads, &flat).unwrap();
                assert_eq!(outputs, wants);
                (stats, ms)
            } else {
                let variant = if arm == 1 {
                    umgpu::ProbeVariant::Thread
                } else {
                    umgpu::ProbeVariant::Warp
                };
                let (outputs, stats, ms) =
                    session.search_chains(1, reads, requests, variant).unwrap();
                assert_eq!(outputs, expected);
                (stats, ms)
            };
            let total = stats.iter().fold(umgpu::ProbeStats::default(), |mut a, b| {
                a.gathers += b.gathers;
                a.bytes += b.bytes;
                a.loops += b.loops;
                a.comparisons += b.comparisons;
                a.max_compare = a.max_compare.max(b.max_compare);
                a.directions |= b.directions;
                a
            });
            if let Some(want) = control {
                assert_eq!(total, want);
            } else {
                control = Some(total);
            }
            if repeat >= 0 {
                assert!(ms > 0.0);
                println!(
                    "chain_bench,{dataset},{repeat},{},{},{},{},{},{ms:.6},{},{:.3}",
                    ["v2", "v3_thread", "v3_warp"][arm as usize],
                    requests.len(),
                    flat.len(),
                    expected.iter().filter(|o| o.status == 9).count(),
                    expected.iter().filter(|o| o.status == 10).count(),
                    total.gathers,
                    total.gathers as f64 / (f64::from(ms) * 0.001)
                );
            }
        }
    }
}
