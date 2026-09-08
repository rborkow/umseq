//! V2 owning prefix path, separate from Terra's V1 initialization and coordinator.
use super::*;
use umgpu::{
    ProbeConfigV2, ProbeOutputV2, ProbeOutputV3, ProbeRequestV2, ProbeRequestV3, ProbeVariant,
};

pub struct UsiPrefixContext {
    session: Mutex<PrefixSession>,
}
/// Reusable resident prefix runner shared by the C boundary and corpus replay.
pub struct PrefixSession {
    gpu: umgpu::Context,
    index: PrefixIndex,
    config: ProbeConfigV2,
    epoch: u64,
    owned: Owned,
    disabled: bool,
}
enum PrefixIndex {
    Owned(Option<ProbeResident>),
    /// STAR owns these allocations.  They must outlive this session and every
    /// successful or quarantined device drain (the C ABI documents that lease).
    Borrowed {
        genome: (*const u8, usize),
        sa: (*const u8, usize),
        sai: (*const u8, usize),
    },
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
            index: PrefixIndex::Owned(Some(index)),
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
    /// # Safety
    /// The three raw index ranges are STAR-owned, immutable, readable, and
    /// remain live until this session is destroyed (including a quarantined
    /// device completion).
    pub unsafe fn new_borrowed(
        genome: (*const u8, usize),
        sa: (*const u8, usize),
        sai: (*const u8, usize),
        config: ProbeConfigV2,
        epoch: u64,
    ) -> Result<Self, String> {
        validate_borrowed_config(genome, sa, sai, config, epoch)?;
        let gpu =
            umgpu::Context::new(0, umgpu::ContextOptions::default()).map_err(|e| e.to_string())?;
        Ok(Self {
            gpu,
            index: PrefixIndex::Borrowed { genome, sa, sai },
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
        self.search_timed(epoch, reads, requests)
            .map(|(o, s, _)| (o, s))
    }
    pub fn search_timed(
        &mut self,
        epoch: u64,
        reads: &[u8],
        requests: &[ProbeRequestV2],
    ) -> Result<(Vec<ProbeOutputV2>, Vec<ProbeStats>, f32), String> {
        if self.disabled || epoch != self.epoch {
            return Err("prefix context disabled or epoch mismatch".into());
        }
        if requests.len() > MAX as usize {
            return Err("prefix batch exceeds cap".into());
        }
        if requests.is_empty() {
            return Ok((Vec::new(), Vec::new(), 0.0));
        }
        if reads.is_empty() {
            return Err("empty prefix read arena".into());
        }
        let n = requests.len();
        // Borrowed index: dispatch before taking the batch buffers (see V3 note).
        if let PrefixIndex::Borrowed { genome, sa, sai } = &self.index {
            return self.search_borrowed_v2(epoch, reads, requests, *genome, *sa, *sai);
        }
        let mut b = self.owned.prepare(reads.len(), n * 88, n * 48, n * 48)?;
        b.reads.as_mut_slice()[..reads.len()].copy_from_slice(reads);
        b.requests.as_pod_mut_slice::<ProbeRequestV2>()[..n].copy_from_slice(requests);
        let PrefixIndex::Owned(index) = &mut self.index else {
            unreachable!()
        };
        let index = index.take().ok_or("prefix resident unavailable")?;
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
        self.index = PrefixIndex::Owned(Some(ProbeResident {
            genome: ro(v.next().unwrap()),
            sa: ro(v.next().unwrap()),
            sai: ro(v.next().unwrap()),
            sa_file_bytes: index.sa_file_bytes,
            config: index.config,
            hashes: index.hashes,
            load_seconds: index.load_seconds,
        }));
        let b = BatchBuffers {
            reads: reclaim_writable(ro(v.next().unwrap())),
            requests: reclaim_writable(ro(v.next().unwrap())),
            output: rw(v.next().unwrap()),
            stats: rw(v.next().unwrap()),
        };
        let outputs = b.output.as_pod_slice::<ProbeOutputV2>()[..n].to_vec();
        let stats = b.stats.as_pod_slice::<ProbeStats>()[..n].to_vec();
        self.owned.restore(b);
        let ms = result.map_err(|e| e.to_string())?;
        self.disabled = false;
        Ok((outputs, stats, ms))
    }
    /// Whole-chain search returning owned vectors (tests, replay). The
    /// integration hot path uses [`Self::search_chains_into`], which writes
    /// the 472-byte outputs straight into the caller's records: `to_vec` here
    /// was a 75 MB allocation + copy per batch on the coordinator thread.
    pub fn search_chains(
        &mut self,
        epoch: u64,
        reads: &[u8],
        requests: &[ProbeRequestV3],
        variant: ProbeVariant,
    ) -> Result<(Vec<ProbeOutputV3>, Vec<ProbeStats>, f32), String> {
        let mut outputs = vec![ProbeOutputV3::default(); requests.len()];
        let mut stats = vec![ProbeStats::default(); requests.len()];
        let ms =
            self.search_chains_into(epoch, reads, requests, variant, &mut outputs, &mut stats)?;
        Ok((outputs, stats, ms))
    }
    /// Whole-chain search; `outputs`/`stats` must hold `requests.len()` records.
    pub fn search_chains_into(
        &mut self,
        epoch: u64,
        reads: &[u8],
        requests: &[ProbeRequestV3],
        variant: ProbeVariant,
        outputs: &mut [ProbeOutputV3],
        stats: &mut [ProbeStats],
    ) -> Result<f32, String> {
        if self.disabled || epoch != self.epoch {
            return Err("prefix context disabled or epoch mismatch".into());
        }
        if requests.len() > MAX as usize {
            return Err("prefix batch exceeds cap".into());
        }
        if outputs.len() < requests.len() || stats.len() < requests.len() {
            return Err("prefix result slices shorter than the batch".into());
        }
        if requests.is_empty() {
            return Ok(0.0);
        }
        if reads.is_empty() {
            return Err("empty prefix read arena".into());
        }
        let n = requests.len();
        // Borrowed index: dispatch before taking the batch buffers, so the
        // borrowed path's own `prepare` reuses them (taking them here and
        // dropping `b` on return re-allocated ~240 MB of THP per batch).
        let d = if let PrefixIndex::Borrowed { genome, sa, sai } = &self.index {
            let (genome, sa, sai) = (*genome, *sa, *sai);
            self.search_borrowed_v3(epoch, reads, requests, variant, genome, sa, sai)?
        } else {
            self.search_owned_v3(epoch, reads, requests, variant)?
        };
        outputs[..n].copy_from_slice(&d.b.output.as_pod_slice::<ProbeOutputV3>()[..n]);
        stats[..n].copy_from_slice(&d.b.stats.as_pod_slice::<ProbeStats>()[..n]);
        self.owned.restore(d.b);
        Ok(d.ms)
    }
    fn search_owned_v3(
        &mut self,
        epoch: u64,
        reads: &[u8],
        requests: &[ProbeRequestV3],
        variant: ProbeVariant,
    ) -> Result<Drained, String> {
        let n = requests.len();
        let _ = epoch;
        let mut b = self.owned.prepare(reads.len(), n * 88, n * 472, n * 48)?;
        b.reads.as_mut_slice()[..reads.len()].copy_from_slice(reads);
        b.requests.as_pod_mut_slice::<ProbeRequestV3>()[..n].copy_from_slice(requests);
        let PrefixIndex::Owned(index) = &mut self.index else {
            unreachable!()
        };
        let index = index.take().ok_or("prefix resident unavailable")?;
        let uc = self.gpu.umem_context();
        let g = index.genome.lease(&uc);
        let sa = index.sa.lease(&uc);
        let sai = index.sai.lease(&uc);
        let r = b.reads.freeze().lease(&uc);
        let q = b.requests.freeze().lease(&uc);
        let o = b.output.lease(&uc);
        let s = b.stats.lease(&uc);
        self.disabled = true; // restored only after confirmed drain and recovery
        let result = umgpu::seed_probe_v3(
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
            variant,
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
        self.index = PrefixIndex::Owned(Some(ProbeResident {
            genome: ro(v.next().unwrap()),
            sa: ro(v.next().unwrap()),
            sai: ro(v.next().unwrap()),
            sa_file_bytes: index.sa_file_bytes,
            config: index.config,
            hashes: index.hashes,
            load_seconds: index.load_seconds,
        }));
        let b = BatchBuffers {
            reads: reclaim_writable(ro(v.next().unwrap())),
            requests: reclaim_writable(ro(v.next().unwrap())),
            output: rw(v.next().unwrap()),
            stats: rw(v.next().unwrap()),
        };
        let ms = match result {
            Ok(ms) => ms,
            Err(e) => {
                self.owned.restore(b);
                return Err(e.to_string());
            }
        };
        self.disabled = false;
        Ok(Drained { b, ms })
    }
    fn search_borrowed_v2(
        &mut self,
        _epoch: u64,
        reads: &[u8],
        requests: &[ProbeRequestV2],
        genome: (*const u8, usize),
        sa: (*const u8, usize),
        sai: (*const u8, usize),
    ) -> Result<(Vec<ProbeOutputV2>, Vec<ProbeStats>, f32), String> {
        let n = requests.len();
        let mut b = self.owned.prepare(reads.len(), n * 88, n * 48, n * 48)?;
        b.reads.as_mut_slice()[..reads.len()].copy_from_slice(reads);
        b.requests.as_pod_mut_slice::<ProbeRequestV2>()[..n].copy_from_slice(requests);
        let uc = self.gpu.umem_context();
        let r = b.reads.freeze().lease(&uc);
        let q = b.requests.freeze().lease(&uc);
        let o = b.output.lease(&uc);
        let s = b.stats.lease(&uc);
        self.disabled = true;
        // SAFETY: `usi_init_v2_borrowed` requires STAR's G/SA/SAi allocations
        // to remain immutable and live through this synchronous drain.
        let result = unsafe {
            umgpu::seed_probe_v2_raw_host(
                &self.gpu,
                genome,
                sa,
                sai,
                &r,
                reads.len(),
                &q,
                &o,
                &s,
                self.config,
                n,
            )
        };
        let recovered =
            umgpu::seed_probe_reclaim(&self.gpu, vec![r.erase(), q.erase(), o.erase(), s.erase()])?;
        let mut v = recovered.into_iter();
        let ro = |v: AnyBuf| match v {
            AnyBuf::Ro(b) => b,
            _ => unreachable!(),
        };
        let rw = |v: AnyBuf| match v {
            AnyBuf::Rw(b) => b,
            _ => unreachable!(),
        };
        let b = BatchBuffers {
            reads: reclaim_writable(ro(v.next().unwrap())),
            requests: reclaim_writable(ro(v.next().unwrap())),
            output: rw(v.next().unwrap()),
            stats: rw(v.next().unwrap()),
        };
        let outputs = b.output.as_pod_slice::<ProbeOutputV2>()[..n].to_vec();
        let stats = b.stats.as_pod_slice::<ProbeStats>()[..n].to_vec();
        self.owned.restore(b);
        let ms = result.map_err(|e| e.to_string())?;
        self.disabled = false;
        Ok((outputs, stats, ms))
    }
    #[allow(clippy::too_many_arguments)]
    fn search_borrowed_v3(
        &mut self,
        _epoch: u64,
        reads: &[u8],
        requests: &[ProbeRequestV3],
        variant: ProbeVariant,
        genome: (*const u8, usize),
        sa: (*const u8, usize),
        sai: (*const u8, usize),
    ) -> Result<Drained, String> {
        let n = requests.len();
        let mut b = self.owned.prepare(reads.len(), n * 88, n * 472, n * 48)?;
        b.reads.as_mut_slice()[..reads.len()].copy_from_slice(reads);
        b.requests.as_pod_mut_slice::<ProbeRequestV3>()[..n].copy_from_slice(requests);
        let uc = self.gpu.umem_context();
        let r = b.reads.freeze().lease(&uc);
        let q = b.requests.freeze().lease(&uc);
        let o = b.output.lease(&uc);
        let s = b.stats.lease(&uc);
        self.disabled = true;
        // SAFETY: the borrowed STAR index allocation outlives the context and drain.
        let result = unsafe {
            umgpu::seed_probe_v3_raw_host(
                &self.gpu,
                genome,
                sa,
                sai,
                &r,
                reads.len(),
                &q,
                &o,
                &s,
                self.config,
                n,
                variant,
            )
        };
        let recovered =
            umgpu::seed_probe_reclaim(&self.gpu, vec![r.erase(), q.erase(), o.erase(), s.erase()])?;
        let mut v = recovered.into_iter();
        let ro = |v: AnyBuf| match v {
            AnyBuf::Ro(b) => b,
            _ => unreachable!(),
        };
        let rw = |v: AnyBuf| match v {
            AnyBuf::Rw(b) => b,
            _ => unreachable!(),
        };
        let b = BatchBuffers {
            reads: reclaim_writable(ro(v.next().unwrap())),
            requests: reclaim_writable(ro(v.next().unwrap())),
            output: rw(v.next().unwrap()),
            stats: rw(v.next().unwrap()),
        };
        let ms = match result {
            Ok(ms) => ms,
            Err(e) => {
                self.owned.restore(b);
                return Err(e.to_string());
            }
        };
        self.disabled = false;
        Ok(Drained { b, ms })
    }
}

/// A drained V3 batch: results still sit in the umem batch buffers.
struct Drained {
    b: BatchBuffers,
    ms: f32,
}
fn validate_borrowed_config(
    genome: (*const u8, usize),
    sa: (*const u8, usize),
    sai: (*const u8, usize),
    config: ProbeConfigV2,
    epoch: u64,
) -> Result<(), String> {
    let c = config.inner;
    let sa_need = c
        .n_sa
        .checked_sub(1)
        .and_then(|n| n.checked_mul(c.strand_bit + 1))
        .and_then(|n| n.checked_div(8))
        .and_then(|n| n.checked_add(8))
        .ok_or("borrowed SA extent overflow")?;
    if epoch == 0
        || c.n_genome == 0
        || c.n_sa == 0
        || !(1..=15).contains(&config.index_bases)
        || config.sai_width != c.strand_bit + 3
        || config.sai_offset != 0
        || config.sai_bytes == 0
        || genome.0.is_null()
        || sa.0.is_null()
        || sai.0.is_null()
        || genome.1 < c.n_genome as usize + 400
        || sa.1 < sa_need as usize
        || sai.1 < config.sai_bytes as usize
    {
        return Err("invalid borrowed prefix index/configuration".into());
    }
    if config.n_mask != !config.n_mask_c
        || !config.n_mask_c.is_power_of_two()
        || !config.absent_mask.is_power_of_two()
        || config.n_mask_c == config.absent_mask
    {
        return Err("invalid loaded Genome SAindex masks".into());
    }
    Ok(())
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
/// Initializes V2 over STAR's already-loaded index without copying or registering it.
/// # Safety
/// `g_minus_200`, `sa`, and `sai_payload` name immutable STAR allocations of the
/// supplied readable extents. They remain live until `usi_destroy_v2`; on an
/// uncertain completion the caller must retain them through device teardown.
pub unsafe extern "C" fn usi_init_v2_borrowed(
    g_minus_200: *const u8,
    g_len: u64,
    sa: *const u8,
    sa_len: u64,
    sai_payload: *const u8,
    sai_len: u64,
    identity: *const UsiIdentityV1,
    config: *const ProbeConfigV2,
    epoch: u64,
    out: *mut *mut UsiPrefixContext,
    error: *mut UsiErrorV1,
) -> i32 {
    std::panic::catch_unwind(|| unsafe {
        if !valid_range(error, size_of::<UsiErrorV1>())
            || !valid_range(out, size_of::<*mut UsiPrefixContext>())
        {
            return BAD;
        }
        let lens = match (
            usize::try_from(g_len),
            usize::try_from(sa_len),
            usize::try_from(sai_len),
        ) {
            (Ok(g), Ok(sa), Ok(sai)) => (g, sa, sai),
            _ => return err(error, BAD, "borrowed index extent overflow"),
        };
        let inputs = [
            (g_minus_200, lens.0),
            (sa, lens.1),
            (sai_payload, lens.2),
            (identity.cast::<u8>(), size_of::<UsiIdentityV1>()),
            (config.cast::<u8>(), size_of::<ProbeConfigV2>()),
        ];
        if inputs.iter().any(|&(p, n)| !valid_range(p, n))
            || inputs.iter().any(|&(p, n)| {
                overlaps(p, n, out.cast(), size_of::<*mut UsiPrefixContext>())
                    || overlaps(p, n, error.cast(), size_of::<UsiErrorV1>())
            })
            || overlaps(
                out.cast(),
                size_of::<*mut UsiPrefixContext>(),
                error.cast(),
                size_of::<UsiErrorV1>(),
            )
        {
            return BAD;
        }
        if !(*out).is_null() {
            return err(error, BAD, "out must initially be null");
        }
        *out = ptr::null_mut();
        let c = *config;
        let ranges = ((g_minus_200, lens.0), (sa, lens.1), (sai_payload, lens.2));
        if let Err(e) = validate_borrowed_config(ranges.0, ranges.1, ranges.2, c, epoch) {
            return err(error, BAD, &e);
        }
        let actual = match borrowed_identity(ranges.0, ranges.1, ranges.2, c) {
            Ok(x) => x,
            Err(e) => return err(error, ID, &e),
        };
        if !same_identity(&actual, &*identity) {
            return err(error, ID, "borrowed STAR index identity mismatch");
        }
        match PrefixSession::new_borrowed(ranges.0, ranges.1, ranges.2, c, epoch) {
            Ok(session) => {
                *out = Box::into_raw(Box::new(UsiPrefixContext {
                    session: Mutex::new(session),
                }));
                err(error, OK, "")
            }
            Err(e) => err(error, GPU, &e),
        }
    })
    .unwrap_or_else(|_| err(error, ALLOC, "panic at borrowed prefix init boundary"))
}

fn borrowed_identity(
    genome: (*const u8, usize),
    sa: (*const u8, usize),
    sai: (*const u8, usize),
    config: ProbeConfigV2,
) -> Result<UsiIdentityV1, String> {
    let logical =
        usize::try_from(config.inner.n_genome).map_err(|_| "borrowed genome extent overflow")?;
    // SAFETY: init validated the three readable caller extents; this sampling is
    // synchronous and does not retain slices or pointers beyond initialization.
    let (g, sa_bytes, sai_bytes) = unsafe {
        (
            std::slice::from_raw_parts(genome.0.add(200), logical),
            std::slice::from_raw_parts(sa.0, sa.1),
            std::slice::from_raw_parts(sai.0, sai.1),
        )
    };
    let mut sha = [0; 96];
    sha[..32].copy_from_slice(&sampled_hash(g));
    sha[32..64].copy_from_slice(&sampled_hash(sa_bytes));
    sha[64..].copy_from_slice(&sampled_hash(sai_bytes));
    Ok(UsiIdentityV1 {
        genome_file_bytes: logical as u64,
        sa_file_bytes: sa.1 as u64,
        sai_file_bytes: sai.1 as u64,
        n_sa: config.inner.n_sa,
        strand_bit: config.inner.strand_bit,
        sparse: 1,
        sha256: sha,
    })
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

#[unsafe(no_mangle)]
/// # Safety
/// Context must be live; caller input/output slices must be valid and disjoint.
pub unsafe extern "C" fn usi_search_batch_v3(
    ctx: *mut UsiPrefixContext,
    epoch: u64,
    reads: *const u8,
    read_bytes: u64,
    requests: *const ProbeRequestV3,
    n: u64,
    output: *mut ProbeOutputV3,
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
                    || !valid_range(output, n as usize * 472)
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
            (output.cast::<u8>(), n as usize * 472),
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
            let (o, s) = if n == 0 {
                (&mut [][..], &mut [][..])
            } else {
                (
                    std::slice::from_raw_parts_mut(output, n as usize),
                    std::slice::from_raw_parts_mut(stats, n as usize),
                )
            };
            match session.search_chains_into(epoch, r, q, ProbeVariant::Thread, o, s) {
                Ok(_) => err(error, OK, ""),
                Err(e) => err(error, GPU, &e),
            }
        }
    })
    .unwrap_or_else(|_| err(error, UNCERTAIN, "panic at prefix batch boundary"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn borrowed_prefix_stub_is_unsupported() {
        let genome = [0u8; 401];
        let sa = [0u8; 8];
        let sai = [0u8; 8];
        let config = ProbeConfigV2 {
            inner: ProbeConfig {
                n_genome: 1,
                n_sa: 1,
                strand_bit: 32,
            },
            index_bases: 1,
            sai_width: 35,
            absent_mask: 1 << 34,
            n_mask: !(1 << 33),
            n_mask_c: 1 << 33,
            sai_bytes: sai.len() as u64,
            sai_offset: 0,
            ..Default::default()
        };
        // SAFETY: these synthetic arrays meet the exact raw-host readable
        // extents and live for the constructor call. Mac's stub must reject
        // the device, never emulate a prefix result.
        let result = unsafe {
            PrefixSession::new_borrowed(
                (genome.as_ptr(), genome.len()),
                (sa.as_ptr(), sa.len()),
                (sai.as_ptr(), sai.len()),
                config,
                1,
            )
        };
        #[cfg(not(feature = "cuda"))]
        match result {
            Err(_) => assert!(matches!(
                umgpu::Context::new(0, umgpu::ContextOptions::default()),
                Err(umgpu::Error::Unsupported)
            )),
            Ok(_) => panic!("stub must return Unsupported"),
        }
        #[cfg(feature = "cuda")]
        let _ = result;
    }
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
