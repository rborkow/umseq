//! nvCOMP-backed planned BGZF writer. The CPU writer remains the reference path.

use super::*;

#[cfg(feature = "nvcomp")]
pub(super) fn write_bam_gpu(
    path: &Path,
    resident: &Resident,
    duplicates: &HashSet<usize>,
    algorithm: i32,
    timing_name: &str,
) -> Result<BamLayout> {
    use umem::AnyBuf;

    let total_start = Instant::now();
    let mut header = resident.header.clone();
    header
        .header_mut()
        .get_or_insert_with(Map::<SamHeader>::default)
        .other_fields_mut()
        .insert(SORT_ORDER, COORDINATE.into());
    let mut header_bytes = Vec::new();
    bam::io::Writer::from(&mut header_bytes).write_header(&header)?;

    let (ranges, raw_records, raw_offset) = plan_blocks(resident, &header_bytes)?;
    let chunks = ranges.len();
    anyhow::ensure!(chunks > 0, "BAM BGZF plan has no blocks");
    let ctx = umgpu::Context::new(0, umgpu::ContextOptions::default())?;
    let align = umgpu::deflate_alignments(algorithm)?;
    let input_stride = aligned(OUTPUT_BLOCK_SIZE, align.input.max(1))?;
    let max_output = umgpu::deflate_max_output(OUTPUT_BLOCK_SIZE, algorithm)?;
    let output_stride = aligned(max_output, align.output.max(1))?;
    let allocation = Allocation::Anon {
        huge: true,
        require_huge: false,
    };
    let alloc = |bytes| Buf::<Rw>::allocate(bytes.max(1), allocation.clone());
    let mut raw = alloc(
        chunks
            .checked_mul(input_stride)
            .context("GPU input size overflow")?,
    )?;
    let mut in_ptrs = alloc(
        chunks
            .checked_mul(std::mem::size_of::<usize>())
            .context("GPU input pointer size overflow")?,
    )?;
    let mut in_bytes = alloc(
        chunks
            .checked_mul(std::mem::size_of::<usize>())
            .context("GPU input size size overflow")?,
    )?;
    let mut output = alloc(
        chunks
            .checked_mul(output_stride)
            .context("GPU output size overflow")?,
    )?;
    let mut out_ptrs = alloc(
        chunks
            .checked_mul(std::mem::size_of::<usize>())
            .context("GPU output pointer size overflow")?,
    )?;
    let out_bytes = alloc(
        chunks
            .checked_mul(std::mem::size_of::<usize>())
            .context("GPU output length size overflow")?,
    )?;
    let statuses = alloc(
        chunks
            .checked_mul(std::mem::size_of::<i32>())
            .context("GPU status size overflow")?,
    )?;
    let temp_bytes = umgpu::deflate_temp_size(chunks, OUTPUT_BLOCK_SIZE, algorithm)?;
    let temp = alloc(temp_bytes)?;

    let raw_sizes = ranges
        .iter()
        .map(|&(first, last, _, includes_header)| {
            (includes_header as usize * header_bytes.len())
                + resident.order[first..last]
                    .iter()
                    .map(|&index| resident.headers()[index as usize].len as usize + 4)
                    .sum::<usize>()
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        raw_sizes.iter().all(|&n| n <= OUTPUT_BLOCK_SIZE),
        "planned BGZF chunk exceeds nvCOMP limit"
    );
    // Borrowing the mapping as disjoint mutable slices permits the existing Rayon encode
    // parallelism without exposing a raw pointer outside umem.
    raw.as_mut_slice()
        .par_chunks_mut(input_stride)
        .zip(ranges.par_iter())
        .try_for_each(
            |(raw_block, &(first, last, _, includes_header))| -> Result<()> {
                let mut used = 0;
                if includes_header {
                    raw_block[..header_bytes.len()].copy_from_slice(&header_bytes);
                    used += header_bytes.len();
                }
                for &index in &resident.order[first..last] {
                    let fixed = resident.headers()[index as usize];
                    let bytes = resident.record_bytes(fixed);
                    raw_block[used..used + 4].copy_from_slice(&fixed.len.to_le_bytes());
                    used += 4;
                    if duplicates.contains(&(index as usize)) {
                        raw_block[used..used + 14].copy_from_slice(&bytes[..14]);
                        raw_block[used + 14..used + 16]
                            .copy_from_slice(&(fixed.flag | 0x400).to_le_bytes());
                        raw_block[used + 16..used + bytes.len()].copy_from_slice(&bytes[16..]);
                    } else {
                        raw_block[used..used + bytes.len()].copy_from_slice(bytes);
                    }
                    used += bytes.len();
                }
                anyhow::ensure!(
                    used <= OUTPUT_BLOCK_SIZE,
                    "planned BGZF chunk exceeds nvCOMP limit"
                );
                Ok(())
            },
        )?;
    in_bytes.as_pod_mut_slice::<usize>()[..chunks].copy_from_slice(&raw_sizes);
    let raw_base = raw.as_slice().as_ptr() as usize;
    let output_base = output.as_slice().as_ptr() as usize;
    for block in 0..chunks {
        in_ptrs.as_pod_mut_slice::<usize>()[block] = raw_base + block * input_stride;
        out_ptrs.as_pod_mut_slice::<usize>()[block] = output_base + block * output_stride;
    }

    let uctx = ctx.umem_context();
    let raw = raw.lease(&uctx);
    let in_ptrs = in_ptrs.lease(&uctx);
    let in_bytes = in_bytes.lease(&uctx);
    let temp = temp.lease(&uctx);
    let output = output.lease(&uctx);
    let out_ptrs = out_ptrs.lease(&uctx);
    let out_bytes = out_bytes.lease(&uctx);
    let statuses = statuses.lease(&uctx);
    let gpu_start = Instant::now();
    let launch = umgpu::deflate_batch(
        &ctx,
        ctx.default_stream(),
        &raw,
        &in_ptrs,
        &in_bytes,
        &temp,
        &output,
        &out_ptrs,
        &out_bytes,
        &statuses,
        chunks,
        OUTPUT_BLOCK_SIZE,
        output_stride,
        algorithm,
    );
    let buffers = umgpu::submit(
        &ctx,
        ctx.default_stream(),
        vec![
            raw.erase(),
            in_ptrs.erase(),
            in_bytes.erase(),
            temp.erase(),
            output.erase(),
            out_ptrs.erase(),
            out_bytes.erase(),
            statuses.erase(),
        ],
    )?
    .wait()?;
    let gpu_time = gpu_start.elapsed();
    launch.context("nvCOMP Deflate launch")?;
    let mut buffers = buffers.into_iter();
    let rw = |b| match b {
        AnyBuf::Rw(b) => b,
        AnyBuf::Ro(_) => unreachable!("all deflate leases are writable"),
    };
    let raw = rw(buffers.next().expect("raw"));
    let _in_ptrs = rw(buffers.next().expect("in_ptrs"));
    let in_bytes = rw(buffers.next().expect("in_bytes"));
    let _temp = rw(buffers.next().expect("temp"));
    let output = rw(buffers.next().expect("output"));
    let _out_ptrs = rw(buffers.next().expect("out_ptrs"));
    let out_bytes = rw(buffers.next().expect("out_bytes"));
    let statuses = rw(buffers.next().expect("statuses"));
    for (block, &status) in statuses.as_pod_slice::<i32>()[..chunks].iter().enumerate() {
        anyhow::ensure!(status == 0, "nvCOMP Deflate block {block} status {status}");
    }

    let frame_start = Instant::now();
    let input_lengths = &in_bytes.as_pod_slice::<usize>()[..chunks];
    let output_lengths = &out_bytes.as_pod_slice::<usize>()[..chunks];
    let framed = (0..chunks)
        .into_par_iter()
        .map(|block| -> Result<Vec<u8>> {
            let raw_block =
                &raw.as_slice()[block * input_stride..block * input_stride + input_lengths[block]];
            let deflate = &output.as_slice()
                [block * output_stride..block * output_stride + output_lengths[block]];
            frame_bgzf(raw_block, deflate)
        })
        .collect::<Result<Vec<_>>>()?;
    let framing_time = frame_start.elapsed();
    let write_start = Instant::now();
    let mut file = io::BufWriter::new(File::create(path)?);
    let mut blocks = Vec::with_capacity(chunks + 1);
    let mut raw_blocks = Vec::with_capacity(chunks + 1);
    let mut compressed = 0u64;
    for ((_, _, raw_start, _), bytes) in ranges.iter().zip(framed) {
        blocks.push(compressed);
        raw_blocks.push(*raw_start);
        file.write_all(&bytes)?;
        compressed += bytes.len() as u64;
    }
    blocks.push(compressed);
    raw_blocks.push(raw_offset);
    file.write_all(&bgzf::io::Writer::new(Vec::new()).finish()?)?;
    file.flush()?;
    let write_time = write_start.elapsed();
    eprintln!(
        "{timing_name}\t{:.6} s\n{timing_name}_gpu_compression\t{:.6} s\n{timing_name}_host_crc_framing\t{:.6} s\n{timing_name}_file_write\t{:.6} s",
        total_start.elapsed().as_secs_f64(),
        gpu_time.as_secs_f64(),
        framing_time.as_secs_f64(),
        write_time.as_secs_f64()
    );
    let records = raw_records
        .iter()
        .map(|&offset| {
            if offset == raw_offset {
                compressed << 16
            } else {
                let block = raw_blocks.partition_point(|&start| start <= offset) - 1;
                (blocks[block] << 16) | (offset - raw_blocks[block])
            }
        })
        .collect();
    Ok(BamLayout {
        records,
        raw_records,
        blocks,
        raw_blocks,
    })
}

#[cfg(feature = "nvcomp")]
fn plan_blocks(
    resident: &Resident,
    header: &[u8],
) -> Result<(Vec<(usize, usize, u64, bool)>, Vec<u64>, u64)> {
    let mut ranges = Vec::new();
    let mut records = Vec::with_capacity(resident.order.len() + 1);
    let mut offset = 0u64;
    let mut first = 0usize;
    let mut used = header.len();
    for (rank, &index) in resident.order.iter().enumerate() {
        let len = resident.headers()[index as usize].len as usize + 4;
        anyhow::ensure!(
            len <= OUTPUT_BLOCK_SIZE,
            "BAM record ({len} bytes) exceeds BGZF output block size"
        );
        if used + len > OUTPUT_BLOCK_SIZE && (rank != first || !header.is_empty()) {
            ranges.push((first, rank, offset, offset == 0));
            offset += used as u64;
            first = rank;
            used = 0;
        }
        records.push(offset + used as u64);
        used += len;
    }
    ranges.push((first, resident.order.len(), offset, offset == 0));
    offset += used as u64;
    records.push(offset);
    Ok((ranges, records, offset))
}

#[cfg(feature = "nvcomp")]
fn aligned(value: usize, alignment: usize) -> Result<usize> {
    value
        .checked_add(alignment - 1)
        .map(|n| n / alignment * alignment)
        .context("nvCOMP alignment overflow")
}

#[cfg(feature = "nvcomp")]
fn frame_bgzf(raw: &[u8], deflate: &[u8]) -> Result<Vec<u8>> {
    let total = 18usize
        .checked_add(deflate.len())
        .and_then(|n| n.checked_add(8))
        .context("BGZF block length overflow")?;
    anyhow::ensure!(
        total <= 65_536,
        "nvCOMP Deflate output ({total} bytes framed) exceeds BGZF block limit"
    );
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&[
        0x1f, 0x8b, 0x08, 0x04, 0, 0, 0, 0, 0, 0xff, 6, 0, b'B', b'C', 2, 0,
    ]);
    out.extend_from_slice(&((total - 1) as u16).to_le_bytes());
    out.extend_from_slice(deflate);
    out.extend_from_slice(&crc32(raw).to_le_bytes());
    out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
    Ok(out)
}

#[cfg(feature = "nvcomp")]
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

#[cfg(not(feature = "nvcomp"))]
pub(super) fn write_bam_gpu(
    _path: &Path,
    _resident: &Resident,
    _duplicates: &HashSet<usize>,
    _algorithm: i32,
    _timing_name: &str,
) -> Result<BamLayout> {
    bail!("--gpu deflate requires rebuilding umbam with --features nvcomp")
}
