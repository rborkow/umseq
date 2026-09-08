//! Three-arm V2 replay over a STAR-shaped, caller-owned index allocation.
//! This is a measurement harness, not an upstream correctness gate.
use anyhow::{Context, Result, anyhow, ensure};
use clap::Parser;
use std::{fs, io::Read, path::PathBuf, time::Instant};
use umem::{AnyBuf, Buf, Ro, Rw};
use umgpu::{ProbeConfig, ProbeConfigV2, ProbeOutputV2, ProbeRequestV2, ProbeStats};
use umseed_probe::{
    index::{probe_allocate, probe_load},
    requests::probe_read,
};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    index: PathBuf,
    #[arg(long)]
    requests: PathBuf,
    #[arg(long)]
    config: PathBuf,
    #[arg(long, conflicts_with_all=["borrowed_madvise", "umem"])]
    borrowed: bool,
    #[arg(long, conflicts_with_all=["borrowed", "umem"])]
    borrowed_madvise: bool,
    #[arg(long, conflicts_with_all=["borrowed", "borrowed_madvise"])]
    umem: bool,
    /// Mac/stub check: the raw-host launch must return Unsupported, never a fake rate.
    #[arg(long)]
    cpu_only: bool,
}
fn config(path: &PathBuf) -> Result<ProbeConfigV2> {
    let b = fs::read(path)?;
    ensure!(b.len() == 224, "expected 224-byte prefix config");
    let w: Vec<u64> = b
        .as_chunks::<8>()
        .0
        .iter()
        .map(|x| u64::from_le_bytes(*x))
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
    Ok(c)
}
fn vm_rss() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|x| {
            x.lines()
                .find_map(|l| l.strip_prefix("VmRSS:"))
                .and_then(|x| x.split_whitespace().next()?.parse::<u64>().ok())
        })
        .unwrap_or(0)
        * 1024
}
fn anon_huge(ptr: *const u8) -> u64 {
    let Ok(s) = fs::read_to_string("/proc/self/smaps") else {
        return 0;
    };
    let a = ptr as usize;
    let mut hit = false;
    for l in s.lines() {
        if let Some((r, _)) = l.split_once(' ')
            && let Some((x, y)) = r.split_once('-')
            && let (Ok(x), Ok(y)) = (usize::from_str_radix(x, 16), usize::from_str_radix(y, 16))
        {
            hit = x <= a && a < y;
            continue;
        }
        if hit && let Some(v) = l.strip_prefix("AnonHugePages:") {
            return v
                .split_whitespace()
                .next()
                .and_then(|x| x.parse::<u64>().ok())
                .unwrap_or(0)
                * 1024;
        }
    }
    0
}
fn advise_touch(v: &mut [u8]) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let p = 4096usize;
        let lo = (v.as_ptr() as usize + p - 1) & !(p - 1);
        let hi = (v.as_ptr() as usize + v.len()) & !(p - 1);
        if hi > lo {
            let r = unsafe { libc::madvise(lo as *mut libc::c_void, hi - lo, libc::MADV_HUGEPAGE) };
            if r != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
    }
    for x in v.iter_mut().step_by(4096) {
        std::hint::black_box(*x);
    }
    Ok(())
}
fn v2(r: umgpu::ProbeRequest) -> ProbeRequestV2 {
    let mut q = r;
    q.tag = 1;
    q.prefix = u64::MAX;
    q.low = u64::MAX;
    q.high = u64::MAX;
    ProbeRequestV2 {
        inner: q,
        distance: 0,
    }
}
fn ro(x: AnyBuf) -> Buf<Ro> {
    match x {
        AnyBuf::Ro(x) => x,
        _ => unreachable!(),
    }
}
fn rw(x: AnyBuf) -> Buf<Rw> {
    match x {
        AnyBuf::Rw(x) => x,
        _ => unreachable!(),
    }
}
fn main() -> Result<()> {
    let a = Args::parse();
    ensure!(
        a.cpu_only || a.borrowed || a.borrowed_madvise || a.umem,
        "select one arm"
    );
    if a.cpu_only {
        // Only meaningful against the stub backend (Mac): the CUDA backend has
        // no `Unsupported` variant and a real context would succeed.
        #[cfg(feature = "cuda")]
        return Err(anyhow!(
            "cpu-only is a stub-backend check; not applicable with --features cuda"
        ));
        #[cfg(not(feature = "cuda"))]
        {
            let e = match umgpu::Context::new(0, umgpu::ContextOptions::default()) {
                Err(e) => e,
                Ok(_) => return Err(anyhow!("cpu-only requires the CUDA stub")),
            };
            ensure!(
                matches!(e, umgpu::Error::Unsupported),
                "stub must return Unsupported"
            );
            println!("arm\tcpu-only\tresult\tUnsupported");
            return Ok(());
        }
    }
    let mut c = config(&a.config)?;
    let cap = probe_read(&a.requests, false)?;
    let want = cap
        .star_tuples
        .as_ref()
        .context("STAR tuple sidecar required")?;
    let qs: Vec<_> = cap
        .requests
        .as_pod_slice::<umgpu::ProbeRequest>()
        .iter()
        .copied()
        .map(v2)
        .collect();
    let ctx = umgpu::Context::new(0, umgpu::ContextOptions::default())
        .map_err(|e| anyhow!(e.to_string()))?;
    let uc = ctx.umem_context();
    let started = Instant::now();
    let mut total_ms = 0f32;
    let mut gathers = 0u64;
    if a.umem {
        let ix = probe_load(&a.index, false)?;
        c.sai_bytes = ix.sai.len() as u64;
        let mut genome = ix.genome;
        let mut sa_buf = ix.sa;
        let mut sai_buf = ix.sai;
        for (bi, q) in qs.chunks(262144).enumerate() {
            let n = q.len();
            let mut rb = probe_allocate(cap.reads.len(), false, "borrowed-reads")?;
            rb.as_mut_slice().copy_from_slice(cap.reads.as_slice());
            let mut qb = probe_allocate(n * 88, false, "borrowed-requests")?;
            qb.as_pod_mut_slice::<ProbeRequestV2>()[..n].copy_from_slice(q);
            let ob = probe_allocate(n * 48, false, "borrowed-output")?;
            let sb = probe_allocate(n * 48, false, "borrowed-stats")?;
            let g = genome.lease(&uc);
            let sa = sa_buf.lease(&uc);
            let si = sai_buf.lease(&uc);
            let r = rb.freeze().lease(&uc);
            let qq = qb.freeze().lease(&uc);
            let o = ob.lease(&uc);
            let s = sb.lease(&uc);
            let z =
                umgpu::seed_probe_v2(&ctx, &g, &sa, &si, &r, cap.reads.len(), &qq, &o, &s, c, n);
            let x = umgpu::seed_probe_reclaim(
                &ctx,
                vec![
                    g.erase(),
                    sa.erase(),
                    si.erase(),
                    r.erase(),
                    qq.erase(),
                    o.erase(),
                    s.erase(),
                ],
            )
            .map_err(|e| anyhow!(e))?;
            let mut x = x.into_iter();
            genome = ro(x.next().unwrap());
            sa_buf = ro(x.next().unwrap());
            sai_buf = ro(x.next().unwrap());
            let _r = ro(x.next().unwrap());
            let _q = ro(x.next().unwrap());
            let o = rw(x.next().unwrap());
            let s = rw(x.next().unwrap());
            total_ms += z.map_err(|e| anyhow!(e.to_string()))?;
            for (j, out) in o.as_pod_slice::<ProbeOutputV2>()[..n].iter().enumerate() {
                ensure!(
                    out.inner == want[bi * 262144 + j],
                    "GPU tuple mismatch at {}",
                    bi * 262144 + j
                );
            }
            gathers += s.as_pod_slice::<ProbeStats>()[..n]
                .iter()
                .map(|x| x.gathers)
                .sum::<u64>();
        }
    } else {
        let mut g = vec![5; c.inner.n_genome as usize + 400];
        fs::File::open(a.index.join("Genome"))?
            .read_exact(&mut g[200..200 + c.inner.n_genome as usize])?;
        let mut sa = fs::read(a.index.join("SA"))?;
        sa.resize(
            ((c.inner.n_sa - 1) * (c.inner.strand_bit + 1) / 8 + 8) as usize,
            0,
        );
        let mut si = fs::read(a.index.join("SAindex"))?;
        ensure!(c.sai_offset as usize <= si.len(), "SAi header extent");
        si.drain(..c.sai_offset as usize);
        c.sai_offset = 0;
        c.sai_bytes = si.len() as u64;
        if a.borrowed_madvise {
            advise_touch(&mut g)?;
            advise_touch(&mut sa)?;
            advise_touch(&mut si)?;
        }
        for (bi, q) in qs.chunks(262144).enumerate() {
            let n = q.len();
            let mut rb = probe_allocate(cap.reads.len(), false, "borrowed-reads")?;
            rb.as_mut_slice().copy_from_slice(cap.reads.as_slice());
            let mut qb = probe_allocate(n * 88, false, "borrowed-requests")?;
            qb.as_pod_mut_slice::<ProbeRequestV2>()[..n].copy_from_slice(q);
            let ob = probe_allocate(n * 48, false, "borrowed-output")?;
            let sb = probe_allocate(n * 48, false, "borrowed-stats")?;
            let r = rb.freeze().lease(&uc);
            let qq = qb.freeze().lease(&uc);
            let o = ob.lease(&uc);
            let s = sb.lease(&uc);
            let z = unsafe {
                umgpu::seed_probe_v2_raw_host(
                    &ctx,
                    (g.as_ptr(), g.len()),
                    (sa.as_ptr(), sa.len()),
                    (si.as_ptr(), si.len()),
                    &r,
                    cap.reads.len(),
                    &qq,
                    &o,
                    &s,
                    c,
                    n,
                )
            };
            let x =
                umgpu::seed_probe_reclaim(&ctx, vec![r.erase(), qq.erase(), o.erase(), s.erase()])
                    .map_err(|e| anyhow!(e))?;
            let mut x = x.into_iter();
            let _r = ro(x.next().unwrap());
            let _q = ro(x.next().unwrap());
            let o = rw(x.next().unwrap());
            let s = rw(x.next().unwrap());
            total_ms += z.map_err(|e| anyhow!(e.to_string()))?;
            for (j, out) in o.as_pod_slice::<ProbeOutputV2>()[..n].iter().enumerate() {
                ensure!(
                    out.inner == want[bi * 262144 + j],
                    "GPU tuple mismatch at {}",
                    bi * 262144 + j
                );
            }
            gathers += s.as_pod_slice::<ProbeStats>()[..n]
                .iter()
                .map(|x| x.gathers)
                .sum::<u64>();
        }
        println!(
            "index_anon_huge_bytes\t{}",
            anon_huge(g.as_ptr()) + anon_huge(sa.as_ptr()) + anon_huge(si.as_ptr())
        );
    }
    println!(
        "arm\t{}\tgathers\t{}\tevent_ms\t{total_ms:.3}\tgathers_per_s\t{:.4e}\twall_s\t{:.3}\tvm_rss_bytes\t{}",
        if a.umem {
            "umem"
        } else if a.borrowed_madvise {
            "borrowed-madvise"
        } else {
            "borrowed"
        },
        gathers,
        gathers as f64 / (total_ms as f64 / 1000.),
        started.elapsed().as_secs_f64(),
        vm_rss()
    );
    Ok(())
}
