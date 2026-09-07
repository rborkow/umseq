//! V2 owning prefix path, separate from Terra's V1 initialization and coordinator.
use super::*;
use umgpu::{ProbeConfigV2, ProbeOutputV2, ProbeRequestV2};

pub struct UsiPrefixContext {
    session: Mutex<PrefixSession>,
}
/// Reusable resident prefix runner shared by the C boundary and corpus replay.
pub struct PrefixSession {
    gpu: umgpu::Context,
    index: Option<ProbeResident>,
    config: ProbeConfigV2,
    epoch: u64,
    owned: Owned,
    disabled: bool,
}
impl PrefixSession {
    pub fn new(index: ProbeResident, config: ProbeConfigV2, epoch: u64) -> Result<Self, String> {
        if epoch == 0
            || config.inner.n_genome != index.config.n_genome
            || config.inner.n_sa != index.config.n_sa
            || config.inner.strand_bit != index.config.strand_bit
            || !(1..=15).contains(&config.index_bases)
            || config.sai_width != index.config.strand_bit + 3
            || config.sai_bytes != index.sai.len() as u64
            || config.sai_offset != 8 * (config.index_bases + 2)
            || config.sai_offset > config.sai_bytes
        {
            return Err("prefix configuration differs from resident index".into());
        }
        let header = &index.sai.as_slice()[..config.sai_offset as usize];
        let words: Vec<_> = header
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| u64::from_le_bytes(*b))
            .collect();
        if words[0] != config.index_bases
            || words[1..] != config.starts[..=config.index_bases as usize]
        {
            return Err("prefix starts differ from resident SAindex header".into());
        }
        // Masks come from the loaded Genome, never inferred from disk metadata.
        // Their internal relationships are validated before trusting coordinates.
        if config.n_mask != !config.n_mask_c
            || !config.n_mask_c.is_power_of_two()
            || !config.absent_mask.is_power_of_two()
            || config.n_mask_c == config.absent_mask
            || config.n_mask_c.trailing_zeros() as u64 >= config.sai_width
            || config.absent_mask.trailing_zeros() as u64 >= config.sai_width
        {
            return Err("invalid loaded Genome SAindex masks".into());
        }
        let gpu =
            umgpu::Context::new(0, umgpu::ContextOptions::default()).map_err(|e| e.to_string())?;
        Ok(Self {
            gpu,
            index: Some(index),
            config,
            epoch,
            owned: Owned {
                reads: None,
                requests: None,
                output: None,
                stats: None,
            },
            disabled: false,
        })
    }
    pub fn search(
        &mut self,
        epoch: u64,
        reads: &[u8],
        requests: &[ProbeRequestV2],
    ) -> Result<(Vec<ProbeOutputV2>, Vec<ProbeStats>), String> {
        if self.disabled || epoch != self.epoch {
            return Err("prefix context disabled or epoch mismatch".into());
        }
        if requests.len() > MAX as usize {
            return Err("prefix batch exceeds cap".into());
        }
        if requests.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        if reads.is_empty() {
            return Err("empty prefix read arena".into());
        }
        let n = requests.len();
        let mut b = self.owned.prepare(reads.len(), n * 88, n * 48, n * 48)?;
        b.reads.as_mut_slice()[..reads.len()].copy_from_slice(reads);
        b.requests.as_pod_mut_slice::<ProbeRequestV2>()[..n].copy_from_slice(requests);
        let index = self.index.take().ok_or("prefix resident unavailable")?;
        let uc = self.gpu.umem_context();
        let g = index.genome.lease(&uc);
        let sa = index.sa.lease(&uc);
        let sai = index.sai.lease(&uc);
        let r = b.reads.freeze().lease(&uc);
        let q = b.requests.freeze().lease(&uc);
        let o = b.output.lease(&uc);
        let s = b.stats.lease(&uc);
        self.disabled = true; // restored only after confirmed drain and recovery
        let result = umgpu::seed_probe_v2(
            &self.gpu,
            &g,
            &sa,
            &sai,
            &r,
            reads.len(),
            &q,
            &o,
            &s,
            self.config,
            n,
        );
        let recovered = umgpu::seed_probe_reclaim(
            &self.gpu,
            vec![
                g.erase(),
                sa.erase(),
                sai.erase(),
                r.erase(),
                q.erase(),
                o.erase(),
                s.erase(),
            ],
        )?;
        let mut v = recovered.into_iter();
        let ro = |v: AnyBuf| match v {
            AnyBuf::Ro(b) => b,
            _ => unreachable!(),
        };
        let rw = |v: AnyBuf| match v {
            AnyBuf::Rw(b) => b,
            _ => unreachable!(),
        };
        self.index = Some(ProbeResident {
            genome: ro(v.next().unwrap()),
            sa: ro(v.next().unwrap()),
            sai: ro(v.next().unwrap()),
            sa_file_bytes: index.sa_file_bytes,
            config: index.config,
            hashes: index.hashes,
            load_seconds: index.load_seconds,
        });
        let b = BatchBuffers {
            reads: reclaim_writable(ro(v.next().unwrap())),
            requests: reclaim_writable(ro(v.next().unwrap())),
            output: rw(v.next().unwrap()),
            stats: rw(v.next().unwrap()),
        };
        let outputs = b.output.as_pod_slice::<ProbeOutputV2>()[..n].to_vec();
        let stats = b.stats.as_pod_slice::<ProbeStats>()[..n].to_vec();
        self.owned.restore(b);
        result.map_err(|e| e.to_string())?;
        self.disabled = false;
        Ok((outputs, stats))
    }
}

#[unsafe(no_mangle)]
/// # Safety
/// All pointers must name live aligned storage for the duration of this call.
pub unsafe extern "C" fn usi_init_v2(
    path: *const c_char,
    identity: *const UsiIdentityV1,
    config: *const ProbeConfigV2,
    epoch: u64,
    out: *mut *mut UsiPrefixContext,
    error: *mut UsiErrorV1,
) -> i32 {
    std::panic::catch_unwind(|| {
        if !valid_range(error, size_of::<UsiErrorV1>())
            || !valid_range(out, size_of::<*mut UsiPrefixContext>())
        {
            return BAD;
        }
        let inputs = [
            (identity.cast::<u8>(), size_of::<UsiIdentityV1>()),
            (config.cast::<u8>(), size_of::<ProbeConfigV2>()),
        ];
        if overlaps(
            out.cast(),
            size_of::<*mut UsiPrefixContext>(),
            error.cast(),
            size_of::<UsiErrorV1>(),
        ) || inputs.iter().any(|&(p, n)| {
            overlaps(p, n, out.cast(), size_of::<*mut UsiPrefixContext>())
                || overlaps(p, n, error.cast(), size_of::<UsiErrorV1>())
        }) {
            return BAD;
        }
        // SAFETY: caller owns the validated output/error slots and input structs;
        // C string and configuration are borrowed only during initialization.
        unsafe {
            *out = ptr::null_mut();
            if path.is_null()
                || !valid_range(identity, size_of::<UsiIdentityV1>())
                || !valid_range(config, size_of::<ProbeConfigV2>())
            {
                return err(error, BAD, "prefix init input");
            }
            let path = match CStr::from_ptr(path).to_str() {
                Ok(p) => p,
                Err(_) => return err(error, BAD, "prefix path UTF8"),
            };
            let index = match probe_load(Path::new(path), false) {
                Ok(x) => x,
                Err(e) => return err(error, ID, &e.to_string()),
            };
            match identity_for(&index) {
                Ok(actual) if same_identity(&actual, &*identity) => (),
                _ => return err(error, ID, "prefix index identity mismatch"),
            }
            match PrefixSession::new(index, *config, epoch) {
                Ok(session) => {
                    *out = Box::into_raw(Box::new(UsiPrefixContext {
                        session: Mutex::new(session),
                    }));
                    err(error, OK, "")
                }
                Err(e) => err(error, ID, &e),
            }
        }
    })
    .unwrap_or_else(|_| err(error, ALLOC, "panic at prefix init boundary"))
}
#[unsafe(no_mangle)]
/// # Safety
/// Context must be live; caller input/output slices must be valid and disjoint.
pub unsafe extern "C" fn usi_search_batch_v2(
    ctx: *mut UsiPrefixContext,
    epoch: u64,
    reads: *const u8,
    read_bytes: u64,
    requests: *const ProbeRequestV2,
    n: u64,
    output: *mut ProbeOutputV2,
    stats: *mut ProbeStats,
    error: *mut UsiErrorV1,
) -> i32 {
    std::panic::catch_unwind(|| {
        if !valid_range(error, size_of::<UsiErrorV1>()) {
            return BAD;
        }
        if !valid_range(ctx, size_of::<UsiPrefixContext>())
            || n > MAX
            || read_bytes > isize::MAX as u64
            || (n != 0
                && (!valid_range(reads, read_bytes as usize)
                    || !valid_range(requests, n as usize * 88)
                    || !valid_range(output, n as usize * 48)
                    || !valid_range(stats, n as usize * 48)))
        {
            // Error storage may alias a malformed input: no write before overlap checks.
            return BAD;
        }
        let ro: [(*const u8, usize); 3] = [
            (ctx.cast::<u8>(), size_of::<UsiPrefixContext>()),
            (reads, if n == 0 { 0 } else { read_bytes as usize }),
            (requests.cast::<u8>(), n as usize * 88),
        ];
        let rw: [(*const u8, usize); 3] = [
            (error.cast::<u8>(), size_of::<UsiErrorV1>()),
            (output.cast::<u8>(), n as usize * 48),
            (stats.cast::<u8>(), n as usize * 48),
        ];
        for (i, &(p, bytes)) in rw.iter().enumerate() {
            if ro
                .iter()
                .chain(rw[..i].iter())
                .any(|&(q, len)| overlaps(p, bytes, q, len))
            {
                return BAD; // error can alias live input/output: do not write it
            }
        }
        // SAFETY: caller guarantees live context and disjoint slices; validated
        // extents bound the views, all borrowed until synchronous search returns.
        unsafe {
            let mut session = match (*ctx).session.lock() {
                Ok(x) => x,
                Err(_) => return err(error, UNCERTAIN, "prefix poisoned"),
            };
            let (r, q) = if n == 0 {
                (&[][..], &[][..])
            } else {
                (
                    std::slice::from_raw_parts(reads, read_bytes as usize),
                    std::slice::from_raw_parts(requests, n as usize),
                )
            };
            match session.search(epoch, r, q) {
                Ok((o, s)) => {
                    if n != 0 {
                        ptr::copy_nonoverlapping(o.as_ptr(), output, n as usize);
                        ptr::copy_nonoverlapping(s.as_ptr(), stats, n as usize);
                    }
                    err(error, OK, "")
                }
                Err(e) => err(error, GPU, &e),
            }
        }
    })
    .unwrap_or_else(|_| err(error, UNCERTAIN, "panic at prefix batch boundary"))
}
#[unsafe(no_mangle)]
/// # Safety
/// Handle/error slots must be live and aligned; handle is null or from usi_init_v2.
pub unsafe extern "C" fn usi_destroy_v2(
    ctx: *mut *mut UsiPrefixContext,
    error: *mut UsiErrorV1,
) -> i32 {
    if !valid_range(error, size_of::<UsiErrorV1>())
        || !valid_range(ctx, size_of::<*mut UsiPrefixContext>())
    {
        return BAD;
    }
    if overlaps(
        ctx.cast(),
        size_of::<*mut UsiPrefixContext>(),
        error.cast(),
        size_of::<UsiErrorV1>(),
    ) {
        return BAD;
    }
    // SAFETY: exclusive caller-owned handle, and every batch synchronously drained
    // or quarantined its leases before returning. No resident device access remains.
    unsafe {
        let p = *ctx;
        *ctx = ptr::null_mut();
        if !p.is_null() {
            drop(Box::from_raw(p));
        }
    }
    err(error, OK, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_batch_does_not_write_aliased_error() {
        let mut error = UsiErrorV1 {
            code: 99,
            reserved: 77,
            message: [42; 248],
        };
        let pointer = &mut error as *mut UsiErrorV1;
        // SAFETY: the error slot is live; malformed count/null context must be
        // rejected before any input dereference or write to aliased output.
        let code = unsafe {
            usi_search_batch_v2(
                ptr::null_mut(),
                1,
                ptr::null(),
                0,
                ptr::null(),
                MAX + 1,
                pointer.cast(),
                ptr::null_mut(),
                pointer,
            )
        };
        assert_eq!(code, BAD);
        assert_eq!(error.code, 99);
        assert_eq!(error.reserved, 77);
        assert_eq!(error.message, [42; 248]);
    }
}
