//! Resident CUDA markdup. The CPU consumes only the compact, index-sorted result.
use super::*;

/// Runs the CUDA markdup pipeline, including allocations and compact-result construction.
#[cfg(feature = "cuda")]
pub fn mark_duplicates_gpu(resident: &mut Resident) -> Result<MarkdupResult> {
    use umem::AnyBuf;
    let start = Instant::now();
    let n = resident.headers().len();
    if n == 0 {
        return Ok(MarkdupResult {
            duplicates: HashSet::new(),
            metrics: DupMetrics::default(),
        });
    }
    // The packed fast path has a 16-bit reference ID. Preserve the oracle for exotic
    // dictionaries or mapped records with a negative tid instead of truncating keys.
    if resident
        .headers()
        .par_iter()
        .any(|h| h.flag & 0x904 == 0 && !(0..=65535).contains(&h.tid))
    {
        eprintln!("markdup GPU: reference ID outside packed domain; using CPU oracle");
        return mark_duplicates(resident);
    }
    let ctx = umgpu::Context::new(0, umgpu::ContextOptions::default())?;
    let allocation = Allocation::Anon {
        huge: true,
        require_huge: false,
    };
    let alloc = |bytes| Buf::<Rw>::allocate(bytes, allocation.clone());
    let work = alloc(
        n.checked_mul(umgpu::MARKDUP_WORK_BYTES)
            .context("markdup workspace overflow")?,
    )?;
    let temp = alloc(umgpu::markdup_temp_size(n)?)?;
    let control = alloc(umgpu::MARKDUP_CONTROL_BYTES)?;
    let mut order = alloc(n * 4)?;
    order
        .as_pod_mut_slice::<u32>()
        .copy_from_slice(&resident.order);
    let placeholder = || {
        Buf::<Ro>::allocate(
            1,
            Allocation::Anon {
                huge: false,
                require_huge: false,
            },
        )
    };
    // Allocate both placeholders before taking either resident mapping.
    let table_placeholder = placeholder()?;
    let arena_placeholder = placeholder()?;
    let table = std::mem::replace(&mut resident.table, table_placeholder);
    let arena = std::mem::replace(&mut resident.arena, arena_placeholder);
    let uctx = ctx.umem_context();
    let table = table.lease(&uctx);
    let arena = arena.lease(&uctx);
    let order = order.freeze().lease(&uctx);
    let work = work.lease(&uctx);
    let temp = temp.lease(&uctx);
    let control = control.lease(&uctx);
    let status = umgpu::markdup(&ctx, &table, &arena, &order, &work, &temp, &control, n);
    // The synchronous shim drains on error too. Return the resident inputs before
    // propagating a kernel error; normal completion still uses the standard CudaFence.
    let buffers = umgpu::submit(
        &ctx,
        ctx.default_stream(),
        vec![
            table.erase(),
            arena.erase(),
            order.erase(),
            work.erase(),
            temp.erase(),
            control.erase(),
        ],
    )?
    .wait()?;
    let mut buffers = buffers.into_iter();
    let ro = |b| match b {
        AnyBuf::Ro(b) => b,
        _ => unreachable!("Ro lease returned Rw"),
    };
    let rw = |b| match b {
        AnyBuf::Rw(b) => b,
        _ => unreachable!("Rw lease returned Ro"),
    };
    resident.table = ro(buffers.next().expect("table"));
    resident.arena = ro(buffers.next().expect("arena"));
    let _order = ro(buffers.next().expect("order"));
    let work = rw(buffers.next().expect("work"));
    let _temp = rw(buffers.next().expect("temp"));
    let control = rw(buffers.next().expect("control"));
    status.context("CUDA markdup (invalid BAM layout, coordinate overflow or kernel failure)")?;
    let m = control.as_pod_slice::<u64>();
    let count = control.as_pod_slice::<u32>()[12] as usize;
    anyhow::ensure!(count <= n, "GPU duplicate count exceeds record count");
    let at = umgpu::MARKDUP_INDICES_OFFSET * n / 4;
    let indices = &work.as_pod_slice::<u32>()[at..at + count];
    let build = Instant::now();
    let duplicates = indices.iter().map(|&i| i as usize).collect();
    let build_seconds = build.elapsed().as_secs_f64();
    let metrics = DupMetrics {
        unpaired_examined: m[0],
        pairs_examined: m[1],
        secondary_or_supplementary: m[2],
        unmapped: m[3],
        unpaired_duplicates: m[4],
        pair_duplicates: m[5],
    };
    let populations = &m[10..14];
    let [examined, pairs, singles, ends] = <[u64; 4]>::try_from(populations)?;
    // Logical traffic model: charge the arena once and one read/write per sort.
    // Excludes CUB digit passes/scratch, caches, and repeated name comparisons;
    // these estimates are not hardware bytes or an achieved DRAM bandwidth measurement.
    let bytes = [
        n as u64 * (48 + 4 + 4 + 16 + 7 * 4) + resident.arena_used as u64,
        n as u64 * 8 + examined * (8 + 24 + 8),
        n as u64 * 12 + pairs * (4 * (32 + 24) + 48 + 32) + ends * 8,
        n as u64 * 12 + singles * (3 * (24 + 24) + 16),
        n as u64 * (6 * 12 + 4) + count as u64 * 4,
    ];
    let times = &control.as_pod_slice::<f32>()[14..19];
    for ((name, &ms), bytes) in [
        "K1_derive",
        "K2_names",
        "K3_pairs",
        "K4_singles",
        "K5_output",
    ]
    .iter()
    .zip(times)
    .zip(bytes)
    {
        eprintln!("markdup_gpu\t{name}\t{ms:.3} ms\t{bytes} logical_bytes_estimate");
    }
    eprintln!(
        "markdup_gpu\thost_result\t{build_seconds:.6} s\ntotal_markdup_gpu\t{:.6} s",
        start.elapsed().as_secs_f64()
    );
    assert_eq!(umgpu::stats::bytes_copied(), 0);
    Ok(MarkdupResult {
        duplicates,
        metrics,
    })
}

/// Reports missing CUDA support on ordinary development hosts.
#[cfg(not(feature = "cuda"))]
pub fn mark_duplicates_gpu(_resident: &mut Resident) -> Result<MarkdupResult> {
    bail!("--gpu requires rebuilding umbam with --features cuda")
}

/// Integration gate: compare CPU and GPU duplicate index sets and every metric.
#[cfg(feature = "cuda")]
pub fn verify_markdup_gpu(input: &Path, threads: usize) -> Result<()> {
    let mut resident = decode(input, threads)?;
    let mut order = resident.order.clone();
    order.par_sort_unstable_by(|a, b| {
        compare_headers(
            &resident.headers()[*a as usize],
            &resident.headers()[*b as usize],
            *a,
            *b,
        )
    });
    resident.order = order;
    let cpu = mark_duplicates(&resident)?;
    let gpu = mark_duplicates_gpu(&mut resident)?;
    anyhow::ensure!(
        cpu.duplicates == gpu.duplicates,
        "CPU/GPU duplicate index sets differ"
    );
    anyhow::ensure!(
        cpu.metrics == gpu.metrics,
        "CPU/GPU metrics differ: {:?} vs {:?}",
        cpu.metrics,
        gpu.metrics
    );
    anyhow::ensure!(
        umgpu::stats::bytes_copied() == 0,
        "CUDA copied resident data"
    );
    Ok(())
}
