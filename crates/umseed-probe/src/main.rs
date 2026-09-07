//! PROBE-only CLI: no downstream gate or performance decision is automated here.
use anyhow::{Result, anyhow, ensure};
use clap::{Parser, Subcommand, ValueEnum};
use rayon::prelude::*;
use std::{
    fs::File,
    io::{BufWriter, Write},
    path::PathBuf,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use umem::{AnyBuf, Buf, Ro, Rw};
use umgpu::{ProbeConfig, ProbeOutput, ProbeRequest, ProbeStats};
use umseed_probe::{
    cpu::{ProbeIndex, probe_search},
    index::{probe_allocate, probe_load},
    requests::{PROBE_COUNTS, probe_generate, probe_read},
};
#[derive(Parser)]
#[command(about = "PROBE synthetic seed-search performance; NOT an upstream correctness gate")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    /// PROBE deterministic request file from real mate1 bytes; no GPU/index load.
    Generate {
        #[arg(long)]
        fastq: PathBuf,
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 4_000_000)]
        count: usize,
        #[arg(long, default_value_t = 188140)]
        seed: u64,
    },
    /// PROBE CPU20/GPU smoke + warm throughput, subject to the orchestrator's SPLIT gate.
    Run {
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        requests: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, value_enum)]
        variant: Variant,
        #[arg(long, value_enum, default_value = "huge")]
        pages: Pages,
        #[arg(
            long,
            value_delimiter = ',',
            default_value = "64000,256000,1000000,4000000"
        )]
        counts: Vec<usize>,
        #[arg(long, default_value_t = 3)]
        repeats: usize,
        /// Optional untimed per-request shape report.  The searches used for
        /// this file are the same CPU search used for the oracle, but this
        /// sidecar is written before warm-up and never folded into a timed row.
        #[arg(long)]
        diagnostics: Option<PathBuf>,
        #[arg(long)]
        overlap: bool,
        /// Must describe the orchestrator's boundary decision, not a guessed measurement.
        #[arg(long)]
        split_provenance: String,
        /// Absolute Unix cutoff; bounded by the explicitly renewed authorization.
        #[arg(long)]
        cutoff_unix: u64,
    },
}
#[derive(Clone, Copy, ValueEnum)]
enum Variant {
    Thread,
    Warp,
}
#[derive(Clone, Copy, ValueEnum)]
enum Pages {
    Huge,
    Small4k,
}
const PROBE_CUTOFF: u64 = 1788756895; // 2026-09-06 21:54:55 PDT; renewed three-hour grant
fn cutoff(t: u64) -> Result<()> {
    ensure!(t <= PROBE_CUTOFF, "PROBE cutoff exceeds authorization");
    ensure!(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() < t,
        "PROBE absolute cutoff reached"
    );
    Ok(())
}

struct Inputs {
    genome: Buf<Ro>,
    sa: Buf<Ro>,
    reads: Buf<Ro>,
    requests: Buf<Ro>,
}
struct State {
    input: Option<Inputs>,
    output: Option<Buf<Rw>>,
    stats: Option<Buf<Rw>>,
    config: ProbeConfig,
    logged: bool,
    variant: umgpu::ProbeVariant,
}
struct GpuTime {
    event_ms: f32,
    device_wall: f64,
    total: f64,
    cpu: f64,
}
fn cpu(
    pool: &rayon::ThreadPool,
    ix: ProbeIndex<'_>,
    reads: &[u8],
    requests: &[ProbeRequest],
    output: &mut [(ProbeOutput, ProbeStats)],
) -> f64 {
    let t = Instant::now();
    pool.install(|| {
        output
            .par_iter_mut()
            .zip(requests.par_iter())
            .for_each(|(out, &r)| *out = probe_search(ix, reads, r))
    });
    t.elapsed().as_secs_f64()
}
impl State {
    fn cpu(
        &self,
        pool: &rayon::ThreadPool,
        start: usize,
        out: &mut [(ProbeOutput, ProbeStats)],
    ) -> f64 {
        let i = self.input.as_ref().unwrap();
        cpu(
            pool,
            ProbeIndex {
                genome: i.genome.as_slice(),
                sa: i.sa.as_slice(),
                config: self.config,
            },
            i.reads.as_slice(),
            &i.requests.as_pod_slice::<ProbeRequest>()[start..start + out.len()],
            out,
        )
    }
    fn gpu(
        &mut self,
        ctx: &umgpu::Context,
        start: usize,
        n: usize,
        pool: &rayon::ThreadPool,
        overlap: &mut [(ProbeOutput, ProbeStats)],
    ) -> Result<GpuTime> {
        let t = Instant::now();
        let i = self.input.take().unwrap();
        let c = ctx.umem_context();
        let cpu_addresses = [
            i.genome.as_slice().as_ptr() as usize,
            i.sa.as_slice().as_ptr() as usize,
            i.reads.as_slice().as_ptr() as usize,
            i.requests.as_slice().as_ptr() as usize,
        ];
        let g = i.genome.lease(&c);
        let sa = i.sa.lease(&c);
        let r = i.reads.lease(&c);
        let q = i.requests.lease(&c);
        let o = self.output.take().unwrap().lease(&c);
        let s = self.stats.take().unwrap().lease(&c);
        for (j, (name, lease)) in [
            ("PROBE-genome", &g),
            ("PROBE-sa", &sa),
            ("PROBE-reads", &r),
            ("PROBE-requests", &q),
        ]
        .into_iter()
        .enumerate()
        {
            let address = umgpu::seed_probe_lease_address(lease);
            ensure!(
                address == cpu_addresses[j],
                "PROBE CPU/GPU address differs for {name}"
            );
            if !self.logged {
                eprintln!(
                    "PROBE lease name={name} cpu_ptr={:#x} gpu_ptr={address:#x} context={:?} bytes={} mode=Ro ATS host_register=false",
                    cpu_addresses[j],
                    lease.context(),
                    lease.len()
                );
            }
        }
        self.logged = true;
        let config = self.config;
        let result = umgpu::seed_probe_variant(
            ctx,
            &g,
            &sa,
            &r,
            &q,
            &o,
            &s,
            config,
            start,
            n,
            self.variant,
            |view| {
                if overlap.is_empty() {
                    0.0
                } else {
                    cpu(
                        pool,
                        ProbeIndex {
                            genome: view.genome,
                            sa: view.sa,
                            config,
                        },
                        view.reads,
                        &view.requests[..overlap.len()],
                        overlap,
                    )
                }
            },
        );
        let mut recovered = umgpu::seed_probe_reclaim(
            ctx,
            vec![
                g.erase(),
                sa.erase(),
                r.erase(),
                q.erase(),
                o.erase(),
                s.erase(),
            ],
        )
        .map_err(|e| anyhow!(e))?;
        fn rw(v: AnyBuf) -> Buf<Rw> {
            if let AnyBuf::Rw(b) = v {
                b
            } else {
                panic!("PROBE mode")
            }
        }
        fn ro(v: AnyBuf) -> Buf<Ro> {
            if let AnyBuf::Ro(b) = v {
                b
            } else {
                panic!("PROBE mode")
            }
        }
        self.stats = Some(rw(recovered.pop().unwrap()));
        self.output = Some(rw(recovered.pop().unwrap()));
        let requests = ro(recovered.pop().unwrap());
        let reads = ro(recovered.pop().unwrap());
        let sa = ro(recovered.pop().unwrap());
        let genome = ro(recovered.pop().unwrap());
        self.input = Some(Inputs {
            genome,
            sa,
            reads,
            requests,
        });
        let (event_ms, device_wall, cpu) = result?;
        Ok(GpuTime {
            event_ms,
            device_wall,
            total: t.elapsed().as_secs_f64(),
            cpu,
        })
    }
    fn check(&self, expected: &[(ProbeOutput, ProbeStats)]) -> Result<u64> {
        let out = self.output.as_ref().unwrap().as_pod_slice::<ProbeOutput>();
        let stats = self.stats.as_ref().unwrap().as_pod_slice::<ProbeStats>();
        for (i, (e, s)) in expected.iter().enumerate() {
            ensure!(
                e.status == 0 && out[i].status == 0,
                "PROBE unsupported/bounds request {i}: CPU={e:?} GPU={:?}",
                out[i]
            );
            ensure!(
                *e == out[i],
                "PROBE tuple mismatch request {i}: CPU={e:?} GPU={:?}; STOP",
                out[i]
            );
            ensure!(
                *s == stats[i],
                "PROBE instrumentation mismatch request {i}: CPU={s:?} GPU={:?}; STOP",
                stats[i]
            );
        }
        Ok(checksum(out[..expected.len()].iter().copied()))
    }
}
fn checksum(outputs: impl Iterator<Item = ProbeOutput>) -> u64 {
    outputs.fold(0xcbf29ce484222325, |mut h, o| {
        for v in [o.length, o.low, o.high, o.count, o.status] {
            h = (h ^ v).wrapping_mul(0x100000001b3);
        }
        h
    })
}
fn validate_cpu(out: &[(ProbeOutput, ProbeStats)]) -> Result<u64> {
    for (i, (o, _)) in out.iter().enumerate() {
        ensure!(
            o.status == 0,
            "PROBE CPU unsupported/bounds request {i}: {o:?}; STOP"
        );
    }
    Ok(checksum(out.iter().map(|x| x.0)))
}
fn validate_star_tuples(out: &[(ProbeOutput, ProbeStats)], star: &[ProbeOutput]) -> Result<()> {
    ensure!(
        out.len() <= star.len(),
        "STAR tuple oracle shorter than selected batch"
    );
    for (i, ((actual, _), expected)) in out.iter().zip(star).enumerate() {
        ensure!(
            *actual == *expected,
            "PROBE captured STAR tuple mismatch request {i}: STAR={expected:?} CPU={actual:?}; STOP"
        );
    }
    Ok(())
}
fn distribution(values: impl IntoIterator<Item = u64>) -> String {
    let mut v: Vec<u64> = values.into_iter().collect();
    v.sort_unstable();
    let q = |numerator: usize, denominator: usize| v[(v.len() - 1) * numerator / denominator];
    let mut bins = [0usize; 65];
    for value in &v {
        bins[if *value == 0 {
            0
        } else {
            64 - value.leading_zeros() as usize
        }] += 1;
    }
    let histogram = bins
        .iter()
        .enumerate()
        .filter(|(_, count)| **count != 0)
        .map(|(bin, count)| {
            if bin == 0 {
                format!("0:{count}")
            } else {
                format!("[2^{},2^{}):{count}", bin - 1, bin)
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "mean={:.3},median={},p90={},p99={},max={},log2_histogram={histogram}",
        v.iter().sum::<u64>() as f64 / v.len() as f64,
        q(1, 2),
        q(9, 10),
        q(99, 100),
        v[v.len() - 1]
    )
}
fn distribution_json(values: impl IntoIterator<Item = u64>) -> serde_json::Value {
    let mut v: Vec<u64> = values.into_iter().collect();
    v.sort_unstable();
    let q = |n: usize, d: usize| v[(v.len() - 1) * n / d];
    let mut histogram = std::collections::BTreeMap::<String, usize>::new();
    for value in &v {
        let label = if *value == 0 {
            "0".to_owned()
        } else {
            let b = 64 - value.leading_zeros() as usize;
            format!("[2^{},2^{})", b - 1, b)
        };
        *histogram.entry(label).or_default() += 1;
    }
    serde_json::json!({
        "mean": v.iter().sum::<u64>() as f64 / v.len() as f64,
        "median": q(1, 2), "p90": q(9, 10), "p99": q(99, 100),
        "max": v[v.len() - 1], "log2_histogram": histogram,
    })
}
#[allow(clippy::too_many_arguments)]
fn row(
    w: &mut impl Write,
    mode: &str,
    n: usize,
    repeat: usize,
    cpu_s: f64,
    g: &GpuTime,
    combined: f64,
    out: &[(ProbeOutput, ProbeStats)],
    hash: u64,
    baseline_cpu: f64,
    baseline_gpu: f64,
) -> Result<()> {
    let gathers: u64 = out.iter().map(|x| x.1.gathers).sum();
    let bytes: u64 = out.iter().map(|x| x.1.bytes).sum();
    let loops: u64 = out.iter().map(|x| x.1.loops).sum();
    let compares: u64 = out.iter().map(|x| x.1.comparisons).sum();
    let max_bytes = out.iter().map(|x| x.1.bytes).max().unwrap_or(0);
    let max_loops = out.iter().map(|x| x.1.loops).max().unwrap_or(0);
    let max_compare = out.iter().map(|x| x.1.max_compare).max().unwrap_or(0);
    let mut imbalance = 0.0;
    let mut max_imbalance = 0.0f64;
    let mut warps = 0;
    for warp in out.chunks(32) {
        let sum: u64 = warp.iter().map(|x| x.1.bytes).sum();
        let max = warp.iter().map(|x| x.1.bytes).max().unwrap();
        let b = if sum > 0 {
            max as f64 * warp.len() as f64 / sum as f64
        } else {
            1.0
        };
        imbalance += b;
        max_imbalance = max_imbalance.max(b);
        warps += 1;
    }
    let gpu_s = if mode == "PROBE-overlap" {
        g.device_wall
    } else {
        g.total
    };
    writeln!(
        w,
        "{mode}\t{n}\t{repeat}\t20\t{cpu_s:.9}\t{gpu_s:.9}\t{:.6}\t{:.9}\t{combined:.9}\t{:.3}\t{:.3}\t{:.6}\t{hash:016x}\t{gathers}\t{bytes}\t{loops}\t{compares}\t{:.3}\t{max_bytes}\t{:.3}\t{max_loops}\t{:.3}\t{max_compare}\t{:.6}\t{max_imbalance:.6}\t{:.6}\t{:.6}\t{:.6}\t{:.6}",
        g.event_ms,
        g.device_wall,
        n as f64 / cpu_s,
        n as f64 / gpu_s,
        cpu_s / gpu_s,
        bytes as f64 / out.len() as f64,
        loops as f64 / out.len() as f64,
        bytes as f64 / compares.max(1) as f64,
        imbalance / warps as f64,
        (gathers * 8 + bytes) as f64 / gpu_s / 1e9,
        if combined > 0.0 {
            2.0 * n as f64 / combined
        } else {
            0.0
        },
        if baseline_cpu > 0.0 {
            cpu_s / baseline_cpu
        } else {
            0.0
        },
        if baseline_gpu > 0.0 {
            gpu_s / baseline_gpu
        } else {
            0.0
        }
    )?;
    w.flush()?;
    Ok(())
}
fn main() -> Result<()> {
    match Cli::parse().command {
        Commands::Generate {
            fastq,
            index,
            output,
            count,
            seed,
        } => probe_generate(&fastq, &output, &index, count, seed),
        Commands::Run {
            index,
            requests,
            output,
            variant,
            pages,
            counts,
            repeats,
            diagnostics,
            overlap,
            split_provenance,
            cutoff_unix,
        } => {
            cutoff(cutoff_unix)?;
            ensure!(
                repeats >= 3 && !counts.is_empty() && counts.iter().all(|n| *n > 0),
                "PROBE requires >=3 repeats and positive request counts"
            );
            ensure!(
                !split_provenance.trim().is_empty(),
                "PROBE requires orchestrator SPLIT boundary provenance"
            );
            ensure!(!output.exists(), "PROBE refusing to overwrite TSV");
            let setup = Instant::now();
            let ctx = umgpu::Context::new(
                0,
                umgpu::ContextOptions {
                    host_register: false,
                },
            )?;
            eprintln!("PROBE device={:?}", ctx.device_props());
            let small = matches!(pages, Pages::Small4k);
            let resident = probe_load(&index, small)?;
            cutoff(cutoff_unix)?;
            let req = probe_read(&requests, small)?;
            let parameters_hash =
                umseed_probe::index::probe_sha256(&index.join("genomeParameters.txt"))?;
            ensure!(
                req.provenance
                    .lines()
                    .any(|line| line == format!("index_parameters_sha256={parameters_hash}")),
                "PROBE request generation index parameters differ from resident index"
            );
            let max = *counts.iter().max().unwrap();
            ensure!(max <= req.count, "PROBE request file too small");
            let real_capture = req
                .provenance
                .lines()
                .any(|line| line == "source_kind=STAR_INNER_CAPTURE");
            if real_capture {
                ensure!(
                    counts.len() == 1 && counts[0] == req.count,
                    "real capture must run its actual complete captured count once; do not repeat/truncate requests"
                );
                ensure!(
                    req.star_tuples.is_some(),
                    "real capture lacks STAR tuple oracle"
                );
            } else {
                ensure!(
                    counts.iter().all(|n| PROBE_COUNTS.contains(n)),
                    "synthetic PROBE supports only its established decimal count matrix"
                );
            }
            for (i, r) in req
                .requests
                .as_pod_slice::<ProbeRequest>()
                .iter()
                .enumerate()
            {
                ensure!(
                    r.low <= r.high && r.high < resident.config.n_sa,
                    "PROBE SA interval invalid at request {i}"
                );
            }
            let pool = rayon::ThreadPoolBuilder::new().num_threads(20).build()?;
            ensure!(
                pool.current_num_threads() == 20,
                "PROBE CPU20 actual worker count !=20"
            );
            let mut state = State {
                input: Some(Inputs {
                    genome: resident.genome,
                    sa: resident.sa,
                    reads: req.reads,
                    requests: req.requests,
                }),
                output: Some(probe_allocate(max * 40, small, "PROBE-output")?),
                stats: Some(probe_allocate(max * 48, small, "PROBE-stats")?),
                config: resident.config,
                logged: false,
                variant: match variant {
                    Variant::Thread => umgpu::ProbeVariant::Thread,
                    Variant::Warp => umgpu::ProbeVariant::Warp,
                },
            };
            let mut reference = vec![(ProbeOutput::default(), ProbeStats::default()); max];
            if let Some(path) = diagnostics {
                ensure!(!path.exists(), "PROBE refusing to overwrite diagnostics");
                let n = if real_capture { req.count } else { max };
                state.cpu(&pool, 0, &mut reference[..n]);
                validate_cpu(&reference[..n])?;
                if let Some(star) = &req.star_tuples {
                    validate_star_tuples(&reference[..n], &star[..n])?;
                }
                let rs = &state
                    .input
                    .as_ref()
                    .unwrap()
                    .requests
                    .as_pod_slice::<ProbeRequest>()[..n];
                let json = serde_json::json!({
                    "timed": false,
                    "source_kind": if real_capture { "STAR_INNER_CAPTURE" } else { "SYNTHETIC_SA_SEARCH_FULL" },
                    "count": n,
                    "loop_definition": "scalar search main binary loop plus expand loops",
                    "gather_definition": "logical comparator/SA-gather counter; not physical DRAM traffic",
                    "N": distribution_json(rs.iter().map(|r| r.length)),
                    "L_in": distribution_json(rs.iter().map(|r| r.prefix)),
                    "interval_width": distribution_json(rs.iter().map(|r| r.high-r.low+1)),
                    "logical_loop_trips": distribution_json(reference[..n].iter().map(|x| x.1.loops)),
                    "logical_dependent_gathers": distribution_json(reference[..n].iter().map(|x| x.1.gathers)),
                });
                std::fs::write(path, serde_json::to_vec_pretty(&json)?)?;
            }
            let mut w = BufWriter::new(File::create(output)?);
            writeln!(
                w,
                "# PROBE ONLY; same-algorithm agreement != upstream oracle; workload={}; variant={}; pages={}; split={split_provenance}",
                if real_capture {
                    "captured STAR prefix-narrowed INNER requests"
                } else {
                    "SA_SEARCH_FULL synthetic grid, remaining read length"
                },
                match variant {
                    Variant::Thread => "thread",
                    Variant::Warp => "warp",
                },
                if small {
                    "4K verified zero huge bytes"
                } else {
                    "100% huge required"
                }
            )?;
            writeln!(
                w,
                "# PROBE index_hashes={} request_sha256={} index_load_validation_s={:.6} setup_s={:.6} workers={} bytes_copied_constant={} (not a dynamic detector); index_SAindex_resident_bytes={}",
                resident.hashes,
                req.hash,
                resident.load_seconds,
                setup.elapsed().as_secs_f64(),
                pool.current_num_threads(),
                umgpu::stats::bytes_copied(),
                resident.sai.len()
            )?;
            writeln!(
                w,
                "# PROBE source_kind={}; {}",
                if real_capture {
                    "STAR_INNER_CAPTURE"
                } else {
                    "SYNTHETIC_SA_SEARCH_FULL"
                },
                if real_capture {
                    "captured STAR tuples are a required CPU/GPU oracle; no pipeline-gain claim"
                } else {
                    "synthetic/optimistic proof-of-capability only; not a correctness gate or SEED-CUDA"
                }
            )?;
            writeln!(
                w,
                "# PROBE compared_bytes and logical_gpu_GBs count scalar-equivalent logical work; warp speculative sequence reads after the first mismatch are actual extra loads excluded from these metrics"
            )?;
            writeln!(
                w,
                "# PROBE mean/max_warp_byte_imbalance are software group-of-32 request-cost proxies, not hardware activity counters or actual intra-warp imbalance for warp-per-request"
            )?;
            for line in req.provenance.lines() {
                writeln!(w, "# PROBE request provenance: {line}")?;
            }
            for (name, b) in [
                ("genome", &state.input.as_ref().unwrap().genome),
                ("sa", &state.input.as_ref().unwrap().sa),
                ("reads", &state.input.as_ref().unwrap().reads),
                ("requests", &state.input.as_ref().unwrap().requests),
            ] {
                writeln!(
                    w,
                    "# PROBE CPU pointer {name}={:p} bytes={} page_report={:?}",
                    b.as_slice().as_ptr(),
                    b.len(),
                    b.page_report()
                )?;
            }
            writeln!(
                w,
                "mode\trequests_per_arm\trepeat\tcpu_workers\tcpu_s\tgpu_wall_s\tgpu_event_ms\tdevice_call_wall_s\tcombined_s\tcpu_rps\tgpu_rps\tGPU_RPS_over_CPU20_RPS\tcpu_gpu_output_checksum\tsa_gathers\tcompared_bytes\tloop_trips\tcomparisons\tmean_request_bytes\tmax_request_bytes\tmean_loops\tmax_loops\tmean_compare_bytes\tmax_compare_bytes\tmean_warp_byte_imbalance\tmax_warp_byte_imbalance\tlogical_gpu_GBs\tcombined_rps\tcpu_slowdown\tgpu_slowdown"
            )?;
            for n in counts {
                cutoff(cutoff_unix)?;
                let expected = &mut reference[..n];
                state.cpu(&pool, 0, expected);
                validate_cpu(expected)?;
                if let Some(star) = &req.star_tuples {
                    validate_star_tuples(expected, &star[..n])?;
                }
                let request_slice = &state
                    .input
                    .as_ref()
                    .unwrap()
                    .requests
                    .as_pod_slice::<ProbeRequest>()[..n];
                writeln!(
                    w,
                    "# PROBE distribution requests={n} N(length): {}; L_in(prefix): {}; interval_width: {}; logical_loop_trips: {}; logical_dependent_gathers: {}",
                    distribution(request_slice.iter().map(|r| r.length)),
                    distribution(request_slice.iter().map(|r| r.prefix)),
                    distribution(request_slice.iter().map(|r| r.high - r.low + 1)),
                    distribution(expected.iter().map(|x| x.1.loops)),
                    distribution(expected.iter().map(|x| x.1.gathers))
                )?;
                let warm = Instant::now();
                let mut rounds = 0;
                let mut warm_event_ms = 0.0;
                while warm_event_ms < 1000.0 {
                    cutoff(cutoff_unix)?;
                    let timing = state.gpu(&ctx, 0, n, &pool, &mut [])?;
                    warm_event_ms += timing.event_ms;
                    state.check(expected)?;
                    rounds += 1;
                }
                writeln!(
                    w,
                    "# PROBE warmup requests={n} rounds={rounds} event_ms={warm_event_ms} elapsed_s={:.6}; checks included, every GPU output consumed",
                    warm.elapsed().as_secs_f64()
                )?;
                for rep in 1..=repeats {
                    cutoff(cutoff_unix)?;
                    let c = state.cpu(&pool, 0, expected);
                    let h = validate_cpu(expected)?;
                    let g = state.gpu(&ctx, 0, n, &pool, &mut [])?;
                    ensure!(h == state.check(expected)?, "PROBE checksum mismatch");
                    row(
                        &mut w,
                        "PROBE-isolated",
                        n,
                        rep,
                        c,
                        &g,
                        0.0,
                        expected,
                        h,
                        0.0,
                        0.0,
                    )?;
                }
                if overlap {
                    let half = n / 2;
                    let (first, second) = expected.split_at_mut(half);
                    state.cpu(&pool, 0, first);
                    validate_cpu(first)?;
                    state.cpu(&pool, half, second);
                    validate_cpu(second)?;
                    let mut concurrent =
                        vec![(ProbeOutput::default(), ProbeStats::default()); half];
                    for rep in 1..=repeats {
                        cutoff(cutoff_unix)?;
                        let base_cpu = state.cpu(&pool, 0, first);
                        let cpu_hash = validate_cpu(first)?;
                        let base_gpu = state.gpu(&ctx, half, half, &pool, &mut [])?;
                        let gpu_hash = state.check(second)?;
                        writeln!(
                            w,
                            "# PROBE overlap controls repeat={rep} half={half} CPU_first_half_s={base_cpu:.9} GPU_second_half_device_s={:.9} GPU_second_half_total_s={:.9} cpu_checksum={cpu_hash:016x} gpu_checksum={gpu_hash:016x}",
                            base_gpu.device_wall, base_gpu.total
                        )?;
                        let g = state.gpu(&ctx, half, half, &pool, &mut concurrent)?;
                        let h = state.check(second)?;
                        validate_cpu(&concurrent)?;
                        ensure!(concurrent == first, "PROBE overlap CPU mismatch");
                        row(
                            &mut w,
                            "PROBE-overlap",
                            half,
                            rep,
                            g.cpu,
                            &g,
                            g.total,
                            second,
                            h,
                            base_cpu,
                            base_gpu.device_wall,
                        )?;
                    }
                }
            }
            writeln!(
                w,
                "# PROBE COMPLETE agreement; same algorithm, not upstream oracle"
            )?;
            w.flush()?;
            eprintln!("PROBE complete: agreement only. No capacity or upstream correctness claim.");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_cli_variant_protocol() {
        let base = [
            "umseed-probe",
            "run",
            "--index",
            "index",
            "--requests",
            "requests",
            "--output",
            "out.tsv",
            "--split-provenance",
            "inner SPLIT verified",
            "--cutoff-unix",
            "1788756895",
        ];
        // Variant selection remains explicit; malformed/missing variants reject.
        assert!(Cli::try_parse_from(base).is_err());
        for name in ["thread", "warp", "unknown"] {
            let args: Vec<_> = base.into_iter().chain(["--variant", name]).collect();
            let parsed = Cli::try_parse_from(args);
            if name == "unknown" {
                assert!(parsed.is_err());
                continue;
            }
            let Commands::Run {
                variant,
                repeats,
                counts,
                cutoff_unix,
                ..
            } = parsed.unwrap().command
            else {
                panic!("run command");
            };
            assert_eq!(matches!(variant, Variant::Warp), name == "warp");
            assert_eq!(repeats, 3);
            assert_eq!(counts, PROBE_COUNTS);
            assert_eq!(cutoff_unix, PROBE_CUTOFF);
        }
    }

    #[test]
    fn captured_star_tuple_mismatch_is_fatal() {
        let actual = [(
            ProbeOutput {
                length: 7,
                low: 2,
                high: 3,
                count: 2,
                status: 0,
            },
            ProbeStats::default(),
        )];
        let expected = [ProbeOutput {
            length: 7,
            low: 2,
            high: 4,
            count: 3,
            status: 0,
        }];
        assert!(validate_star_tuples(&actual, &expected).is_err());
    }
}
