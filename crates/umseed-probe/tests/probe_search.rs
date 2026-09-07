use umgpu::{ProbeConfig, ProbeRequest};
use umseed_probe::cpu::{ProbeIndex, probe_packed_at, probe_search};

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

#[test]
fn probe_packed_unaligned_final_word() {
    let values = [0, 1u64 << 32, (1u64 << 33) - 1, 7, 123456789];
    let bytes = pack(&values, 33);
    for (i, v) in values.into_iter().enumerate() {
        assert_eq!(probe_packed_at(&bytes, i as u64, 33), v);
    }
}

#[test]
fn probe_four_directions_and_status() {
    for dir in [0, 1] {
        for reverse_genome in [false, true] {
            let mut genome = vec![5; 416];
            let base = if reverse_genome { 13 } else { 2 };
            let s0 = [0, 1, 2, 3, 0, 1];
            let mut reads = s0.to_vec();
            reads.extend(s0.map(|b| 3 - b));
            let req = ProbeRequest {
                s1: 6,
                read_len: 6,
                start: if dir == 1 { 1 } else { 4 },
                length: 3,
                dir,
                ..Default::default()
            };
            for k in 0..3 {
                let rp = if dir == 1 {
                    req.start + k
                } else {
                    req.start - k
                };
                let rb = if (dir == 1) != reverse_genome { 0 } else { 6 };
                let gp = if reverse_genome { base - k } else { base + k };
                genome[200 + gp as usize] = reads[rb + rp as usize];
            }
            let sa = pack(&[2 | if reverse_genome { 1 << 32 } else { 0 }], 33);
            let ix = ProbeIndex {
                genome: &genome,
                sa: &sa,
                config: ProbeConfig {
                    n_genome: 16,
                    n_sa: 1,
                    strand_bit: 32,
                },
            };
            let (out, stats) = probe_search(ix, &reads, req);
            assert_eq!(
                (out.length, out.low, out.high, out.count, out.status),
                (3, 0, 0, 1, 0)
            );
            assert_eq!(stats.bytes, 6);
            assert_eq!(
                probe_search(ix, &reads, ProbeRequest { tag: 1, ..req })
                    .0
                    .status,
                1
            );
            assert_eq!(
                probe_search(ix, &reads, ProbeRequest { high: 1, ..req })
                    .0
                    .status,
                2
            );
        }
    }
}

#[test]
fn probe_zero_match_tie_retains_full_range() {
    let mut genome = vec![5; 416];
    genome[200..216].fill(2);
    let sa = pack(&[0, 2, 4, 6, 8], 33);
    let ix = ProbeIndex {
        genome: &genome,
        sa: &sa,
        config: ProbeConfig {
            n_genome: 16,
            n_sa: 5,
            strand_bit: 32,
        },
    };
    let req = ProbeRequest {
        s1: 1,
        read_len: 1,
        length: 1,
        high: 4,
        dir: 1,
        ..Default::default()
    };
    let (out, _) = probe_search(ix, &[0, 3], req);
    assert_eq!(
        (out.length, out.low, out.high, out.count, out.status),
        (0, 0, 4, 5, 0)
    );
}

#[test]
fn probe_host_kernel_transport_matches_cpu() {
    use std::{
        fs,
        process::{Command, Stdio},
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let temp = std::env::temp_dir().join(format!("umseed-probe-transport-{}", std::process::id()));
    fs::create_dir_all(&temp).unwrap();
    let exe = temp.join("probe-transport");
    let compile = Command::new("c++")
        .args(["-std=c++17", "-O2", "-Wall", "-Wextra", "-Werror", "-I"])
        .arg(root.join("../umgpu/shim"))
        .arg(root.join("tests/probe_transport.cpp"))
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let mut g = vec![5u8; 528];
    for i in 0..128 {
        g[200 + i] = ((i * 17 + i / 7) % 6) as u8;
    }
    let values: Vec<u64> = (0..128)
        .map(|i| (i * 37 % 128) | if i % 2 == 0 { 1 << 32 } else { 0 })
        .collect();
    let sa = pack(&values, 33);
    let mut reads: Vec<u8> = (0..64).map(|i| ((i * 7 + i / 11) % 4) as u8).collect();
    reads.extend(reads.clone().into_iter().map(|b| 3 - b));
    let mut reqs = Vec::new();
    for i in 0..2048u64 {
        let start = i % 64;
        let dir = i / 64 % 2;
        let length = if dir == 1 { 64 - start } else { start + 1 };
        reqs.push(ProbeRequest {
            s1: 64,
            read_len: 64,
            start,
            length,
            prefix: if i % 5 == 0 { length } else { 0 },
            low: i % 64,
            high: 64 + i % 64,
            dir,
            ..Default::default()
        });
    }
    let mut wire = Vec::new();
    for w in [128u64, 128, 32, 128, reqs.len() as u64] {
        wire.extend(w.to_le_bytes());
    }
    wire.extend(&g);
    wire.extend(&sa);
    wire.extend(&reads);
    let ix = ProbeIndex {
        genome: &g,
        sa: &sa,
        config: ProbeConfig {
            n_genome: 128,
            n_sa: 128,
            strand_bit: 32,
        },
    };
    let mut expected = Vec::new();
    for r in reqs {
        for v in [
            r.tag, r.s0, r.s1, r.read_len, r.start, r.length, r.prefix, r.low, r.high, r.dir,
        ] {
            wire.extend(v.to_le_bytes());
        }
        let (o, s) = probe_search(ix, &reads, r);
        for v in [
            o.length,
            o.low,
            o.high,
            o.count,
            o.status,
            s.gathers,
            s.bytes,
            s.loops,
            s.comparisons,
            s.max_compare,
            s.directions,
        ] {
            expected.extend(v.to_le_bytes());
        }
    }
    let input = temp.join("probe-wire.bin");
    fs::write(&input, wire).unwrap();
    let output = Command::new(&exe)
        .stdin(Stdio::from(fs::File::open(input).unwrap()))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, expected);
    let warp_exe = temp.join("probe-warp-transport");
    let compile = Command::new("c++")
        .args([
            "-std=c++17",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pthread",
            "-DPROBE_TEST_WARP",
            "-I",
        ])
        .arg(root.join("../umgpu/shim"))
        .arg(root.join("tests/probe_transport.cpp"))
        .arg("-o")
        .arg(&warp_exe)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let output = Command::new(warp_exe)
        .stdin(Stdio::from(
            fs::File::open(temp.join("probe-wire.bin")).unwrap(),
        ))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, expected);
    fs::remove_dir_all(temp).unwrap();
}

#[test]
fn probe_exact_prefix_does_not_form_genome_pointer() {
    let genome = vec![5; 416];
    let sa = pack(&[1 << 32 | 15], 33);
    let ix = ProbeIndex {
        genome: &genome,
        sa: &sa,
        config: ProbeConfig {
            n_genome: 16,
            n_sa: 1,
            strand_bit: 32,
        },
    };
    let r = ProbeRequest {
        s1: 4,
        read_len: 4,
        start: 2,
        length: 2,
        prefix: 2,
        dir: 0,
        ..Default::default()
    };
    let (out, stats) = probe_search(ix, &[0, 0, 0, 0, 3, 3, 3, 3], r);
    assert_eq!(
        (out.length, out.low, out.high, out.count, out.status),
        (2, 0, 0, 1, 0)
    );
    assert_eq!(stats.bytes, 0);
}

#[test]
fn probe_fully_known_reverse_prefix_can_exhaust_read() {
    let genome = vec![5; 464];
    let sa = pack(&[0, 1, 2, 3, 4], 33);
    let ix = ProbeIndex {
        genome: &genome,
        sa: &sa,
        config: ProbeConfig {
            n_genome: 64,
            n_sa: 5,
            strand_bit: 32,
        },
    };
    let reads = vec![0; 28];
    let request = ProbeRequest {
        s1: 14,
        read_len: 14,
        start: 13,
        length: 14,
        prefix: 14,
        high: 4,
        dir: 0,
        ..Default::default()
    };
    let (out, stats) = probe_search(ix, &reads, request);
    assert_eq!(
        (out.length, out.low, out.high, out.count, out.status),
        (14, 0, 4, 5, 0)
    );
    assert_eq!(
        (stats.bytes, stats.gathers, stats.comparisons, stats.loops),
        (0, 3, 3, 1)
    );
    assert_eq!(
        probe_search(
            ix,
            &reads,
            ProbeRequest {
                length: 15,
                ..request
            }
        )
        .0
        .status,
        2
    );
}

#[test]
fn probe_warp_collectives_and_tail_protocol() {
    use std::{fs, process::Command};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let exe = std::env::temp_dir().join(format!("probe-warp-focused-{}", std::process::id()));
    let compile = Command::new("c++")
        .args([
            "-std=c++17",
            "-O2",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-pthread",
            "-I",
        ])
        .arg(root.join("../umgpu/shim"))
        .arg(root.join("tests/probe_warp.cpp"))
        .arg("-o")
        .arg(&exe)
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "{}",
        String::from_utf8_lossy(&compile.stderr)
    );
    let result = Command::new(&exe).output().unwrap();
    fs::remove_file(exe).unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    eprint!("{}", String::from_utf8_lossy(&result.stdout));
}
