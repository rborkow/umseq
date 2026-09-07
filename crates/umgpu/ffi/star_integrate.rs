//! Private, synchronous STAR integration ABI.  This crate deliberately has no
//! CPU emulation: a successful batch means the existing CUDA transport drained.
//! Compiled by the umstar link facade; unsafe remains at the GPU FFI boundary.
use std::{
    ffi::{CStr, c_char},
    mem::size_of,
    path::Path,
    ptr,
    sync::Mutex,
};
use umem::{AnyBuf, Buf, Ro};
use umgpu::{ProbeConfig, ProbeOutput, ProbeRequest, ProbeStats};
use umseed_probe::index::{ProbeResident, probe_allocate, probe_load, probe_sha256_bytes};

const OK: i32 = 0;
const BAD: i32 = 1;
const ID: i32 = 2;
const ALLOC: i32 = 3;
const GPU: i32 = 4;
const UNCERTAIN: i32 = 5;
const MAX: u64 = 262_144;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UsiIdentityV1 {
    pub genome_file_bytes: u64,
    pub sa_file_bytes: u64,
    pub sai_file_bytes: u64,
    pub n_sa: u64,
    pub strand_bit: u64,
    pub sparse: u64,
    pub sha256: [u8; 96],
}
#[repr(C)]
pub struct UsiErrorV1 {
    pub code: u32,
    pub reserved: u32,
    pub message: [c_char; 248],
}
pub struct UsiContext {
    inner: Mutex<Inner>,
}
struct Resident {
    genome: Buf<Ro>,
    sa: Buf<Ro>,
    config: ProbeConfig,
}
struct Inner {
    epoch: u64,
    identity: UsiIdentityV1,
    resident: Option<Resident>,
    gpu: umgpu::Context,
    owned: Owned,
    disabled: bool,
}
struct Owned {
    reads: Option<Buf<umem::Rw>>,
    requests: Option<Buf<umem::Rw>>,
    output: Option<Buf<umem::Rw>>,
    stats: Option<Buf<umem::Rw>>,
}
struct BatchBuffers {
    reads: Buf<umem::Rw>,
    requests: Buf<umem::Rw>,
    output: Buf<umem::Rw>,
    stats: Buf<umem::Rw>,
}
struct DisableOnPanic(*mut bool);
impl Drop for DisableOnPanic {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // SAFETY: created from the locked context's `disabled` field and dropped
            // before that lock guard; it is never moved or aliased as a reference.
            unsafe { *self.0 = true };
        }
    }
}
fn reclaim_writable(buf: Buf<Ro>) -> Buf<umem::Rw> {
    // SAFETY: `seed_probe_reclaim` returns this buffer only after synchronous
    // completion. `Buf` has identical ownership/layout for the two marker modes;
    // no aliases exist, so restoring its CPU write permission is sound.
    unsafe { std::mem::transmute(buf) }
}

fn err(e: *mut UsiErrorV1, code: i32, msg: &str) -> i32 {
    unsafe {
        if !e.is_null() {
            (*e).code = code as u32;
            (*e).reserved = 0;
            (*e).message.fill(0);
            for (i, b) in msg.as_bytes().iter().take(247).enumerate() {
                (*e).message[i] = *b as c_char;
            }
        }
    };
    code
}
fn clear(e: *mut UsiErrorV1) {
    let _ = err(e, OK, "");
}
fn hash(bytes: &[u8]) -> Result<[u8; 32], String> {
    let h = probe_sha256_bytes(bytes).map_err(|e| e.to_string())?;
    let mut out = [0; 32];
    if h.len() != 64 {
        return Err("malformed resident sha256".into());
    };
    let (pairs, tail) = h.as_bytes().as_chunks::<2>();
    if !tail.is_empty() {
        return Err("malformed resident sha256".into());
    }
    for (i, p) in pairs.iter().enumerate() {
        out[i] = u8::from_str_radix(std::str::from_utf8(p).map_err(|_| "non-UTF8 sha256")?, 16)
            .map_err(|_| "non-hex sha256")?;
    }
    Ok(out)
}
fn same_identity(a: &UsiIdentityV1, b: &UsiIdentityV1) -> bool {
    a.genome_file_bytes == b.genome_file_bytes
        && a.sa_file_bytes == b.sa_file_bytes
        && a.sai_file_bytes == b.sai_file_bytes
        && a.n_sa == b.n_sa
        && a.strand_bit == b.strand_bit
        && a.sparse == b.sparse
        && a.sha256 == b.sha256
}
fn identity_for(r: &ProbeResident) -> Result<UsiIdentityV1, String> {
    let mut sha = [0; 96];
    sha[..32].copy_from_slice(&hash(
        &r.genome.as_slice()[200..200 + r.config.n_genome as usize],
    )?);
    let sa_file = usize::try_from(r.sa_file_bytes).map_err(|_| "SA extent overflow")?;
    if sa_file > r.sa.len() {
        return Err("SA file exceeds resident extent".into());
    }
    sha[32..64].copy_from_slice(&hash(&r.sa.as_slice()[..sa_file])?);
    sha[64..].copy_from_slice(&hash(r.sai.as_slice())?);
    Ok(UsiIdentityV1 {
        genome_file_bytes: r.config.n_genome,
        sa_file_bytes: sa_file as u64,
        sai_file_bytes: r.sai.len() as u64,
        n_sa: r.config.n_sa,
        strand_bit: r.config.strand_bit,
        sparse: 1,
        sha256: sha,
    })
}
fn overlaps(a: *const u8, al: usize, b: *const u8, bl: usize) -> bool {
    let x = a as usize;
    let y = b as usize;
    x < y.saturating_add(bl) && y < x.saturating_add(al)
}
fn valid_range<T>(p: *const T, bytes: usize) -> bool {
    !p.is_null()
        && (p as usize).is_multiple_of(std::mem::align_of::<T>())
        && (p as usize)
            .checked_add(bytes)
            .is_some_and(|end| end <= isize::MAX as usize)
}
fn checked_bytes(n: u64, w: usize) -> Option<usize> {
    usize::try_from(n).ok()?.checked_mul(w)
}
impl Owned {
    fn prepare(
        &mut self,
        rb: usize,
        nb: usize,
        ob: usize,
        sb: usize,
    ) -> Result<BatchBuffers, String> {
        if self.reads.as_ref().is_none_or(|b| b.len() < rb) {
            self.reads = Some(probe_allocate(rb, false, "USI-reads").map_err(|e| e.to_string())?);
        }
        if self.requests.as_ref().is_none_or(|b| b.len() < nb) {
            self.requests =
                Some(probe_allocate(nb, false, "USI-requests").map_err(|e| e.to_string())?);
        }
        if self.output.as_ref().is_none_or(|b| b.len() < ob) {
            self.output = Some(probe_allocate(ob, false, "USI-output").map_err(|e| e.to_string())?);
        }
        if self.stats.as_ref().is_none_or(|b| b.len() < sb) {
            self.stats = Some(probe_allocate(sb, false, "USI-stats").map_err(|e| e.to_string())?);
        }
        Ok(BatchBuffers {
            reads: self.reads.take().expect("allocated reads"),
            requests: self.requests.take().expect("allocated requests"),
            output: self.output.take().expect("allocated output"),
            stats: self.stats.take().expect("allocated stats"),
        })
    }
    fn restore(&mut self, b: BatchBuffers) {
        self.reads = Some(b.reads);
        self.requests = Some(b.requests);
        self.output = Some(b.output);
        self.stats = Some(b.stats);
    }
}

/// Loads a duplicate immutable resident index.  `probe_load` independently
/// validates the on-disk snapshot before this checks the caller's STAR snapshot.
#[unsafe(no_mangle)]
/// # Safety
/// Every non-null pointer must name live, suitably aligned storage of the documented extent.
pub unsafe extern "C" fn usi_init_v1(
    dir: *const c_char,
    identity: *const UsiIdentityV1,
    epoch: u64,
    out: *mut *mut UsiContext,
    error: *mut UsiErrorV1,
) -> i32 {
    std::panic::catch_unwind(|| unsafe {
        if !valid_range(error, size_of::<UsiErrorV1>()) {
            return BAD;
        }
        if dir.is_null()
            || !valid_range(identity, size_of::<UsiIdentityV1>())
            || !valid_range(out, size_of::<*mut UsiContext>())
            || epoch == 0
        {
            return err(error, BAD, "null argument or zero epoch");
        }
        if overlaps(
            out.cast(),
            size_of::<*mut UsiContext>(),
            error.cast(),
            size_of::<UsiErrorV1>(),
        ) {
            // Neither writable range may be changed: an error write would itself
            // corrupt the caller's output slot.
            return BAD;
        }
        if overlaps(
            error.cast(),
            size_of::<UsiErrorV1>(),
            identity.cast(),
            size_of::<UsiIdentityV1>(),
        ) || overlaps(
            out.cast(),
            size_of::<*mut UsiContext>(),
            identity.cast(),
            size_of::<UsiIdentityV1>(),
        ) {
            return BAD;
        }
        let out_was_nonnull = !(*out).is_null();
        *out = ptr::null_mut();
        // All writable overlap checks precede the first mutation of either
        // `out` or `error`.
        clear(error);
        if out_was_nonnull {
            return err(error, BAD, "out must initially be null");
        }
        let path = match CStr::from_ptr(dir).to_str() {
            Ok(x) if !x.is_empty() => x,
            _ => return err(error, BAD, "invalid index directory"),
        };
        // Reject unsupported builds/devices before loading multi-gigabyte index files.
        let gpu = match umgpu::Context::new(
            0,
            umgpu::ContextOptions {
                host_register: false,
            },
        ) {
            Ok(x) => x,
            Err(x) => return err(error, GPU, &format!("CUDA unavailable or unsuitable: {x}")),
        };
        let resident = match probe_load(Path::new(path), false) {
            Ok(x) => x,
            Err(x) => return err(error, ID, &format!("index validation: {x}")),
        };
        let actual = match identity_for(&resident) {
            Ok(x) => x,
            Err(x) => return err(error, ID, &x),
        };
        if !same_identity(&actual, &*identity) {
            return err(
                error,
                ID,
                "STAR snapshot identity differs from loaded resident index",
            );
        }
        // SAindex was validated above, but is deliberately not retained by v1.
        let index = Resident {
            genome: resident.genome,
            sa: resident.sa,
            config: resident.config,
        };
        *out = Box::into_raw(Box::new(UsiContext {
            inner: Mutex::new(Inner {
                epoch,
                identity: actual,
                resident: Some(index),
                gpu,
                owned: Owned {
                    reads: None,
                    requests: None,
                    output: None,
                    stats: None,
                },
                disabled: false,
            }),
        }));
        OK
    })
    .unwrap_or_else(|_| err(error, ALLOC, "panic at init boundary"))
}

#[unsafe(no_mangle)]
/// # Safety
/// Every non-null pointer must name live, suitably aligned storage of the documented extent.
pub unsafe extern "C" fn usi_search_batch_v1(
    ctx: *mut UsiContext,
    epoch: u64,
    reads: *const u8,
    read_bytes: u64,
    req: *const ProbeRequest,
    n: u64,
    out: *mut ProbeOutput,
    stats: *mut ProbeStats,
    error: *mut UsiErrorV1,
) -> i32 {
    std::panic::catch_unwind(|| unsafe {
        if !valid_range(error, size_of::<UsiErrorV1>()) {
            return BAD;
        }
        if ctx.is_null() || epoch == 0 || n > MAX {
            return err(error, BAD, "invalid context, epoch, or count");
        };
        if n == 0 {
            clear(error);
            return OK;
        }
        let nb = match checked_bytes(n, size_of::<ProbeRequest>()) {
            Some(x) => x,
            None => return err(error, BAD, "request extent overflow"),
        };
        let ob = match checked_bytes(n, size_of::<ProbeOutput>()) {
            Some(x) => x,
            None => return err(error, BAD, "output extent overflow"),
        };
        let sb = match checked_bytes(n, size_of::<ProbeStats>()) {
            Some(x) => x,
            None => return err(error, BAD, "stats extent overflow"),
        };
        let rb = match usize::try_from(read_bytes) {
            Ok(x) => x,
            Err(_) => return err(error, BAD, "read extent overflow"),
        };
        if rb == 0
            || !valid_range(reads, rb)
            || !valid_range(req, nb)
            || !valid_range(out, ob)
            || !valid_range(stats, sb)
        {
            return err(error, BAD, "null or empty batch storage");
        };
        if overlaps(out.cast(), ob, stats.cast(), sb)
            || overlaps(error.cast(), size_of::<UsiErrorV1>(), out.cast(), ob)
            || overlaps(error.cast(), size_of::<UsiErrorV1>(), stats.cast(), sb)
            || overlaps(error.cast(), size_of::<UsiErrorV1>(), reads, rb)
            || overlaps(error.cast(), size_of::<UsiErrorV1>(), req.cast(), nb)
            || overlaps(out.cast(), ob, reads, rb)
            || overlaps(stats.cast(), sb, reads, rb)
            || overlaps(out.cast(), ob, req.cast(), nb)
            || overlaps(stats.cast(), sb, req.cast(), nb)
        {
            // Do not clear/write `error`: it can alias the range we were asked
            // to leave untouched.
            return BAD;
        }
        clear(error);
        let mut i = (*ctx)
            .inner
            .lock()
            .map_err(|_| ())
            .unwrap_or_else(|_| panic!());
        let _disable_on_panic = DisableOnPanic(&mut i.disabled);
        if i.epoch != epoch || i.disabled {
            return err(error, ID, "context disabled or epoch mismatch");
        };
        let rr = std::slice::from_raw_parts(reads, rb);
        let qr = std::slice::from_raw_parts(req, n as usize);
        for q in qr {
            if q.tag != 0
                || q.dir > 1
                || q.read_len == 0
                || q.read_len > 4096
                || q.length == 0
                || q.prefix > q.length
                || q.low > q.high
                || q.high >= i.identity.n_sa
                || q.s0.checked_add(q.read_len).is_none_or(|x| x > read_bytes)
                || q.s1.checked_add(q.read_len).is_none_or(|x| x > read_bytes)
                || (q.dir == 1 && q.start.checked_add(q.length).is_none_or(|x| x > q.read_len))
                || (q.dir == 0 && (q.start >= q.read_len || q.length > q.start.saturating_add(1)))
            {
                return err(error, BAD, "request outside frozen probe domain");
            }
        }
        let mut buffers = match i.owned.prepare(rb, nb, ob, sb) {
            Ok(x) => x,
            Err(x) => return err(error, ALLOC, &x),
        };
        // Capacity may exceed this call's logical extents; only these prefixes are initialized.
        buffers.reads.as_mut_slice()[..rb].copy_from_slice(rr);
        buffers.requests.as_pod_mut_slice::<ProbeRequest>()[..n as usize].copy_from_slice(qr);
        let resident = i.resident.take().unwrap();
        let uc = i.gpu.umem_context();
        let g = resident.genome.lease(&uc);
        let sa = resident.sa.lease(&uc);
        let r = buffers.reads.freeze().lease(&uc);
        let q = buffers.requests.freeze().lease(&uc);
        let o = buffers.output.lease(&uc);
        let s = buffers.stats.lease(&uc);
        let config = resident.config;
        let result = umgpu::seed_probe_with_read_bytes(
            &i.gpu,
            &g,
            &sa,
            &r,
            rb,
            &q,
            &o,
            &s,
            config,
            0,
            n as usize,
            |_| (),
        );
        let recovered = umgpu::seed_probe_reclaim(
            &i.gpu,
            vec![
                g.erase(),
                sa.erase(),
                r.erase(),
                q.erase(),
                o.erase(),
                s.erase(),
            ],
        );
        match recovered {
            Ok(mut v) => {
                let st = match v.pop().unwrap() {
                    AnyBuf::Rw(x) => x,
                    _ => unreachable!(),
                };
                let oo = match v.pop().unwrap() {
                    AnyBuf::Rw(x) => x,
                    _ => unreachable!(),
                };
                let q = match v.pop().unwrap() {
                    AnyBuf::Ro(x) => x,
                    _ => unreachable!(),
                };
                let r = match v.pop().unwrap() {
                    AnyBuf::Ro(x) => x,
                    _ => unreachable!(),
                };
                let sa = match v.pop().unwrap() {
                    AnyBuf::Ro(x) => x,
                    _ => unreachable!(),
                };
                let g = match v.pop().unwrap() {
                    AnyBuf::Ro(x) => x,
                    _ => unreachable!(),
                };
                i.resident = Some(Resident {
                    genome: g,
                    sa,
                    config,
                });
                buffers = BatchBuffers {
                    reads: reclaim_writable(r),
                    requests: reclaim_writable(q),
                    output: oo,
                    stats: st,
                };
                match result {
                    Ok((_event, _wall, ())) => {
                        let produced = buffers.output.as_pod_slice::<ProbeOutput>();
                        for (q, produced) in qr.iter().zip(&produced[..n as usize]) {
                            if produced.status == 0 && !valid_success(q, produced, i.identity.n_sa)
                            {
                                i.owned.restore(buffers);
                                i.disabled = true;
                                return err(error, GPU, "GPU returned an invalid successful tuple");
                            }
                        }
                        ptr::copy_nonoverlapping(produced.as_ptr(), out, n as usize);
                        ptr::copy_nonoverlapping(
                            buffers.stats.as_pod_slice::<ProbeStats>().as_ptr(),
                            stats,
                            n as usize,
                        );
                        i.owned.restore(buffers);
                        OK
                    }
                    Err(x) => {
                        i.owned.restore(buffers);
                        i.disabled = true;
                        err(error, GPU, &format!("drained GPU transport failure: {x}"))
                    }
                }
            }
            Err(x) => {
                // `seed_probe_reclaim` owns the leases in this branch; umem's
                // submission guard quarantines them on an unobserved completion.
                i.disabled = true;
                err(
                    error,
                    UNCERTAIN,
                    &format!("completion uncertain; context quarantined: {x}"),
                )
            }
        }
    })
    .unwrap_or_else(|_| {
        err(
            error,
            UNCERTAIN,
            "panic at batch boundary; context disabled",
        )
    })
}

fn valid_success(q: &ProbeRequest, produced: &ProbeOutput, n_sa: u64) -> bool {
    produced.length >= q.prefix
        && produced.length <= q.length
        && produced.low <= produced.high
        && produced.low >= q.low
        && produced.high <= q.high
        && produced.high < n_sa
        && produced.count == produced.high - produced.low + 1
}

#[unsafe(no_mangle)]
/// # Safety
/// `ctx` and `error`, when non-null, must name live, suitably aligned storage.
pub unsafe extern "C" fn usi_destroy_v1(ctx: *mut *mut UsiContext, error: *mut UsiErrorV1) -> i32 {
    std::panic::catch_unwind(|| unsafe {
        if !valid_range(error, size_of::<UsiErrorV1>())
            || !valid_range(ctx, size_of::<*mut UsiContext>())
        {
            return err(error, BAD, "null destroy argument");
        };
        if overlaps(
            ctx.cast(),
            size_of::<*mut UsiContext>(),
            error.cast(),
            size_of::<UsiErrorV1>(),
        ) {
            return BAD;
        }
        clear(error);
        let p = *ctx;
        *ctx = ptr::null_mut();
        if !p.is_null() {
            drop(Box::from_raw(p));
        }
        OK
    })
    .unwrap_or_else(|_| err(error, UNCERTAIN, "panic at destroy boundary"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_comparison_is_complete() {
        let a = UsiIdentityV1 {
            genome_file_bytes: 1,
            sa_file_bytes: 2,
            sai_file_bytes: 3,
            n_sa: 4,
            strand_bit: 32,
            sparse: 1,
            sha256: [1; 96],
        };
        assert!(same_identity(&a, &a));
        let mut b = a;
        b.sha256[95] = 2;
        assert!(!same_identity(&a, &b));
    }
    #[test]
    fn ranges_do_not_wrap() {
        assert!(checked_bytes(u64::MAX, 80).is_none());
    }
    #[test]
    fn successful_tuple_stays_in_its_request_interval() {
        let q = ProbeRequest {
            tag: 0,
            s0: 0,
            s1: 4,
            read_len: 4,
            start: 0,
            length: 4,
            prefix: 1,
            low: 10,
            high: 20,
            dir: 1,
        };
        let mut o = ProbeOutput {
            length: 2,
            count: 1,
            low: 10,
            high: 10,
            status: 0,
        };
        assert!(valid_success(&q, &o, 100));
        o.low = 9;
        o.high = 9;
        assert!(!valid_success(&q, &o, 100), "below request low");
        o.low = 21;
        o.high = 21;
        assert!(!valid_success(&q, &o, 100), "above request high");
    }
    #[test]
    fn identity_preserves_file_padding_not_reconstructed_sa_bits() {
        fn bytes(n: usize) -> Buf<Ro> {
            umem::Buf::<umem::Rw>::allocate(
                n,
                umem::Allocation::Anon {
                    huge: false,
                    require_huge: false,
                },
            )
            .unwrap()
            .freeze()
        }
        // Two 33-bit entries require a 12-byte readable allocation. The pinned
        // file extent can be 11 bytes, not ceil(2*33/8)=9.
        let resident = ProbeResident {
            sa_file_bytes: 11,
            genome: bytes(408),
            sa: bytes(12),
            sai: bytes(8),
            config: ProbeConfig {
                n_genome: 8,
                n_sa: 2,
                strand_bit: 32,
            },
            hashes: String::new(),
            load_seconds: 0.0,
        };
        assert_eq!(identity_for(&resident).unwrap().sa_file_bytes, 11);
    }

    #[test]
    fn ffi_rejects_null_context_before_touching_batch_arrays() {
        let mut error = UsiErrorV1 {
            code: 99,
            reserved: 99,
            message: [1; 248],
        };
        // SAFETY: `error` is live, aligned writable storage; all other pointers are
        // deliberately null because the null context must be rejected first.
        let code = unsafe {
            usi_search_batch_v1(
                ptr::null_mut(),
                1,
                ptr::null(),
                0,
                ptr::null(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                &mut error,
            )
        };
        assert_eq!(code, BAD);
        assert_eq!(error.code, BAD as u32);
        assert_eq!(error.reserved, 0);
        assert_eq!(error.message[247], 0);
    }

    #[test]
    fn ffi_init_without_cuda_publishes_no_context() {
        let identity = UsiIdentityV1 {
            genome_file_bytes: 0,
            sa_file_bytes: 0,
            sai_file_bytes: 0,
            n_sa: 0,
            strand_bit: 0,
            sparse: 0,
            sha256: [0; 96],
        };
        let directory = c"/definitely/not/an/index";
        let mut out = ptr::null_mut();
        let mut error = UsiErrorV1 {
            code: 99,
            reserved: 99,
            message: [1; 248],
        };
        // SAFETY: every supplied pointer names live, aligned storage for its ABI type.
        let code = unsafe { usi_init_v1(directory.as_ptr(), &identity, 1, &mut out, &mut error) };
        #[cfg(not(feature = "cuda"))]
        assert_eq!(code, GPU);
        assert!(out.is_null());
        assert_eq!(error.code, code as u32);
        assert_eq!(error.reserved, 0);
    }

    #[test]
    fn ffi_overlap_is_rejected_without_mutating_either_writable_range() {
        let mut storage = [0xa5a5_a5a5_a5a5_a5a5u64; 40];
        let error = storage.as_mut_ptr().cast::<UsiErrorV1>();
        // SAFETY: the deliberately overlapping handle slot points into the
        // live error object; destroy must reject it before any write.
        let code = unsafe { usi_destroy_v1(error.cast::<*mut UsiContext>(), error) };
        assert_eq!(code, BAD);
        assert!(storage.iter().all(|&x| x == 0xa5a5_a5a5_a5a5_a5a5));
    }
}
