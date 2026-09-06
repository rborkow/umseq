use std::{
    collections::HashSet,
    ffi::{CStr, c_char, c_int, c_void},
    ptr,
    sync::{Arc, Mutex},
};

use thiserror::Error;
use umem::{AnyLease, ContextId, Fence, FenceError, GpuLease, Rw, Submission};

#[allow(improper_ctypes)]
unsafe extern "C" {
    fn cudaSetDevice(device: c_int) -> c_int;
    fn umgpu_init(props: *mut c_int) -> c_int;
    fn umgpu_stream_create(stream: *mut *mut c_void) -> c_int;
    fn umgpu_stream_destroy(stream: *mut c_void) -> c_int;
    fn umgpu_event_create(event: *mut *mut c_void) -> c_int;
    fn umgpu_event_destroy(event: *mut c_void) -> c_int;
    fn umgpu_event_record(event: *mut c_void, stream: *mut c_void) -> c_int;
    fn umgpu_event_query(event: *mut c_void) -> c_int;
    fn umgpu_event_sync(event: *mut c_void) -> c_int;
    fn umgpu_host_register(pointer: *mut c_void, len: usize) -> c_int;
    fn umgpu_host_unregister(pointer: *mut c_void) -> c_int;
    fn umgpu_radix_sort_pairs_u64_u32_temp_size(n: usize, bytes: *mut usize) -> c_int;
    fn umgpu_radix_sort_pairs_u64_u32(
        temp: *mut c_void,
        temp_bytes: usize,
        keys_in: *const u64,
        keys_out: *mut u64,
        vals_in: *const u32,
        vals_out: *mut u32,
        n: usize,
        begin_bit: c_int,
        end_bit: c_int,
        stream: *mut c_void,
    ) -> c_int;
    fn umgpu_rle_u64_temp_size(n: usize, bytes: *mut usize) -> c_int;
    fn umgpu_rle_u64(
        temp: *mut c_void,
        temp_bytes: usize,
        keys: *const u64,
        unique: *mut u64,
        counts: *mut u32,
        runs: *mut u32,
        n: usize,
        stream: *mut c_void,
    ) -> c_int;
    fn umgpu_exclusive_scan_u32_temp_size(n: usize, bytes: *mut usize) -> c_int;
    fn umgpu_exclusive_scan_u32(
        temp: *mut c_void,
        temp_bytes: usize,
        input: *const u32,
        output: *mut u32,
        n: usize,
        stream: *mut c_void,
    ) -> c_int;
    fn umgpu_inc_u64(input: *const u64, output: *mut u64, n: usize, stream: *mut c_void) -> c_int;
    fn umgpu_dup_keys(
        headers: *const c_void,
        arena: *const u8,
        arena_len: usize,
        n: usize,
        mode: c_int,
        keys_out: *mut u64,
        vals_out: *mut u32,
        stream: *mut c_void,
    ) -> c_int;
    fn umgpu_error_string(code: c_int) -> *const c_char;
}

#[cfg(feature = "nvcomp")]
unsafe extern "C" {
    fn umgpu_deflate_alignments(
        algorithm: c_int,
        input: *mut usize,
        output: *mut usize,
        temp: *mut usize,
    ) -> c_int;
    fn umgpu_deflate_temp_size(
        num_chunks: usize,
        max_chunk: usize,
        algorithm: c_int,
        temp_bytes: *mut usize,
    ) -> c_int;
    fn umgpu_deflate_max_output(max_chunk: usize, algorithm: c_int, max_out: *mut usize) -> c_int;
    fn umgpu_deflate_batch(
        in_ptrs: *const *const c_void,
        in_bytes: *const usize,
        max_chunk: usize,
        num_chunks: usize,
        temp: *mut c_void,
        temp_bytes: usize,
        out_ptrs: *const *mut c_void,
        out_bytes: *mut usize,
        algorithm: c_int,
        statuses: *mut c_int,
        stream: *mut c_void,
    ) -> c_int;
    fn umgpu_nvcomp_error_string(code: c_int) -> *const c_char;
}

/// Backend errors.
#[derive(Debug, Error)]
pub enum Error {
    /// Invalid or overlapping pipeline buffers.
    #[error("invalid GPU input: {0}")]
    InvalidInput(&'static str),
    /// CUDA reported a failure.
    #[error("CUDA error {code}: {message}")]
    Cuda { code: i32, message: String },
    /// A lease belongs to another CUDA context.
    #[error("lease context {found:?} does not match CUDA context {expected:?}")]
    Context {
        expected: ContextId,
        found: ContextId,
    },
    /// A lease cannot cover the requested typed range.
    #[error("{name} is {actual} bytes; need at least {needed}")]
    TooShort {
        name: &'static str,
        actual: usize,
        needed: usize,
    },
}

/// Options controlling CUDA context setup.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContextOptions {
    /// Register each submitted mapping with CUDA. Off by default for THP/HMM operation.
    pub host_register: bool,
}

/// CUDA device properties recorded at context initialization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeviceProps {
    /// CUDA pageable-memory access capability.
    pub pageable_memory_access: i32,
    /// CUDA host page-table access capability.
    pub pageable_memory_access_uses_host_page_tables: i32,
    /// CUDA direct managed-memory host access capability.
    pub direct_managed_mem_access_from_host: i32,
    /// CUDA host-registration capability.
    pub host_register_supported: i32,
    /// CUDA concurrent managed-access capability.
    pub concurrent_managed_access: i32,
    /// Device compute major version.
    pub major: i32,
    /// Device compute minor version.
    pub minor: i32,
    /// Device SM count.
    pub multi_processor_count: i32,
}

/// CUDA context and its default stream.
pub struct Context {
    id: ContextId,
    props: DeviceProps,
    host_register: bool,
    registered: Arc<Mutex<HashSet<usize>>>,
    stream: Stream,
}

impl Context {
    /// Selects `device`, records its capabilities, and creates the default stream.
    pub fn new(device: i32, options: ContextOptions) -> Result<Self, Error> {
        // SAFETY: CUDA runtime accepts a device ordinal; no Rust pointer is passed.
        check(unsafe { cudaSetDevice(device) })?;
        let mut raw = [0_i32; 8];
        // SAFETY: `raw` has exactly the eight writable i32 slots required by the shim.
        check(unsafe { umgpu_init(raw.as_mut_ptr()) })?;
        let stream = Stream::create(device)?;
        Ok(Self {
            id: ContextId(device as u64),
            props: DeviceProps {
                pageable_memory_access: raw[0],
                pageable_memory_access_uses_host_page_tables: raw[1],
                direct_managed_mem_access_from_host: raw[2],
                host_register_supported: raw[3],
                concurrent_managed_access: raw[4],
                major: raw[5],
                minor: raw[6],
                multi_processor_count: raw[7],
            },
            host_register: options.host_register,
            registered: Arc::new(Mutex::new(HashSet::new())),
            stream,
        })
    }

    /// Returns the `umem` context used when leasing buffers to this backend.
    pub const fn umem_context(&self) -> umem::Context {
        umem::Context::new(self.id)
    }
    /// Returns the backend context identifier.
    pub const fn id(&self) -> ContextId {
        self.id
    }
    /// Returns the properties captured at initialization.
    pub const fn device_props(&self) -> DeviceProps {
        self.props
    }
    /// Returns the context's default CUDA stream.
    pub const fn default_stream(&self) -> &Stream {
        &self.stream
    }
    /// Creates an additional stream on this context's selected CUDA device.
    pub fn create_stream(&self) -> Result<Stream, Error> {
        self.activate()?;
        Stream::create(self.id.0 as i32)
    }

    fn activate(&self) -> Result<(), Error> {
        // SAFETY: the stored ContextId originated from a non-negative CUDA device ordinal.
        check(unsafe { cudaSetDevice(self.id.0 as c_int) })
    }

    fn check_lease<M: umem::Mode>(
        &self,
        lease: &GpuLease<M>,
        name: &'static str,
        bytes: usize,
    ) -> Result<(), Error> {
        if lease.context() != self.id {
            return Err(Error::Context {
                expected: self.id,
                found: lease.context(),
            });
        }
        if lease.len() < bytes {
            return Err(Error::TooShort {
                name,
                actual: lease.len(),
                needed: bytes,
            });
        }
        self.register(lease)
    }
    fn register<M: umem::Mode>(&self, lease: &GpuLease<M>) -> Result<(), Error> {
        if !self.host_register {
            return Ok(());
        }
        // SAFETY: the lease owns this mapping, its length is valid, and registration occurs before its kernel launch.
        let p = unsafe { lease.as_ptr() } as usize;
        let mut set = self.registered.lock().expect("registration mutex poisoned");
        if set.insert(p) {
            // SAFETY: `p` and `lease.len()` came from the live lease and identify its mapping.
            if let Err(e) = check(unsafe { umgpu_host_register(p as *mut c_void, lease.len()) }) {
                set.remove(&p);
                return Err(e);
            }
        }
        Ok(())
    }
    fn take_registered(&self, leases: &[AnyLease]) -> Vec<usize> {
        let mut set = self.registered.lock().expect("registration mutex poisoned");
        leases
            .iter()
            .filter_map(|l| {
                // SAFETY: `l` is still owned by this submission setup and exposes its backing address.
                let p = unsafe { l.as_ptr() } as usize;
                set.remove(&p).then_some(p)
            })
            .collect()
    }
}

/// An owned CUDA stream.
pub struct Stream {
    raw: *mut c_void,
    device: i32,
}
impl Stream {
    fn create(device: i32) -> Result<Self, Error> {
        // SAFETY: `device` was accepted by Context construction or is its retained ordinal.
        check(unsafe { cudaSetDevice(device) })?;
        let mut raw = ptr::null_mut();
        // SAFETY: `raw` is a valid out-pointer for the shim to initialize.
        check(unsafe { umgpu_stream_create(&mut raw) })?;
        Ok(Self { raw, device })
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        if self.raw.is_null() {
            return;
        }
        // SAFETY: `device` is the ordinal that created this stream.
        let select = unsafe { cudaSetDevice(self.device) };
        if select != 0 {
            eprintln!(
                "umgpu: could not select stream device: {}",
                error_message(select)
            );
        }
        let mut event = ptr::null_mut();
        // SAFETY: this stream is owned here; recording and syncing an event drains prior work before destruction.
        let status = unsafe { umgpu_event_create(&mut event) };
        if status == 0 {
            unsafe {
                umgpu_event_record(event, self.raw);
                umgpu_event_sync(event);
                umgpu_event_destroy(event);
            }
        }
        // SAFETY: this `Stream` owns `raw` and destroys it exactly once.
        let status = unsafe { umgpu_stream_destroy(self.raw) };
        if status != 0 {
            eprintln!("umgpu: stream destroy failed: {}", error_message(status));
        }
        self.raw = ptr::null_mut();
    }
}

/// Event-backed completion fence.
pub struct CudaFence {
    event: *mut c_void,
    device: i32,
    registered: Vec<usize>,
}
// SAFETY: a cudaEvent_t is an opaque handle valid from any host thread (CUDA runtime
// handles are not thread-affine); the fence is the sole owner and destroys it on drop.
// The registered-pointer list is plain data.
unsafe impl Send for CudaFence {}
impl CudaFence {
    fn unregister(&mut self) {
        for p in self.registered.drain(..) {
            // SAFETY: this fence observed its event completed, so CUDA no longer accesses this registered mapping.
            let status = unsafe { umgpu_host_unregister(p as *mut c_void) };
            if status != 0 {
                eprintln!("umgpu: host unregister failed: {}", error_message(status));
            }
        }
    }
}
impl Fence for CudaFence {
    fn wait(&mut self) -> Result<(), FenceError> {
        // SAFETY: `device` is the ordinal that created this event.
        let select = unsafe { cudaSetDevice(self.device) };
        if select != 0 {
            return Err(fence_error(select));
        }
        // SAFETY: `event` is owned by this fence and remains valid until Drop.
        let status = unsafe { umgpu_event_sync(self.event) };
        if status == 0 {
            self.unregister();
            Ok(())
        } else {
            Err(fence_error(status))
        }
    }
    fn try_wait(&mut self) -> Result<bool, FenceError> {
        // SAFETY: `device` is the ordinal that created this event.
        let select = unsafe { cudaSetDevice(self.device) };
        if select != 0 {
            return Err(fence_error(select));
        }
        // SAFETY: `event` is owned by this fence and remains valid until Drop.
        match unsafe { umgpu_event_query(self.event) } {
            0 => {
                self.unregister();
                Ok(true)
            }
            1 => Ok(false),
            status => Err(fence_error(status)),
        }
    }
}
impl Drop for CudaFence {
    fn drop(&mut self) {
        if !self.event.is_null() {
            // SAFETY: `device` is the ordinal that created this event.
            let select = unsafe { cudaSetDevice(self.device) };
            if select != 0 {
                eprintln!(
                    "umgpu: could not select event device: {}",
                    error_message(select)
                );
            }
            // SAFETY: this fence owns the event and destroys it once after Submission has waited it.
            let status = unsafe { umgpu_event_destroy(self.event) };
            if status != 0 {
                eprintln!("umgpu: event destroy failed: {}", error_message(status));
            }
        }
    }
}

/// Records a completion event and transfers `leases` into a `umem` submission.
pub fn submit(
    ctx: &Context,
    stream: &Stream,
    leases: Vec<AnyLease>,
) -> Result<Submission<CudaFence>, Error> {
    ctx.activate()?;
    for lease in &leases {
        if lease.context() != ctx.id {
            return Err(Error::Context {
                expected: ctx.id,
                found: lease.context(),
            });
        }
    }
    let mut event = ptr::null_mut();
    // SAFETY: `event` is a valid out-pointer for a newly created CUDA event.
    check(unsafe { umgpu_event_create(&mut event) })?;
    // SAFETY: event and stream are live handles owned by this function/context.
    if let Err(e) = check(unsafe { umgpu_event_record(event, stream.raw) }) {
        // SAFETY: event was successfully created and has not escaped.
        unsafe {
            umgpu_event_destroy(event);
        }
        return Err(e);
    }
    let fence = CudaFence {
        event,
        device: ctx.id.0 as i32,
        registered: ctx.take_registered(&leases),
    };
    // SAFETY: the event was recorded after all caller-enqueued work on `stream`; leases are context-checked above.
    match unsafe { Submission::new(&ctx.umem_context(), leases, fence) } {
        Ok(s) => Ok(s),
        Err((_leases, _fence, mismatch)) => Err(Error::Context {
            expected: mismatch.expected,
            found: mismatch.found,
        }),
    }
}

/// Returns CUB scratch bytes for radix sorting `n` u64/u32 pairs.
pub fn radix_sort_pairs_u64_u32_temp_size(n: usize) -> Result<usize, Error> {
    temp_size(n, umgpu_radix_sort_pairs_u64_u32_temp_size)
}
/// Returns CUB scratch bytes for run-length encoding `n` u64 keys.
pub fn rle_u64_temp_size(n: usize) -> Result<usize, Error> {
    temp_size(n, umgpu_rle_u64_temp_size)
}
/// Returns CUB scratch bytes for exclusively scanning `n` u32 values.
pub fn exclusive_scan_u32_temp_size(n: usize) -> Result<usize, Error> {
    temp_size(n, umgpu_exclusive_scan_u32_temp_size)
}
fn temp_size(
    n: usize,
    f: unsafe extern "C" fn(usize, *mut usize) -> c_int,
) -> Result<usize, Error> {
    let mut bytes = 0;
    // SAFETY: `bytes` is a valid out-pointer and CUB receives no data pointers during a sizing query.
    check(unsafe { f(n, &mut bytes) })?;
    Ok(bytes)
}

/// Enqueues an ascending radix sort over `[begin_bit, end_bit)`.
pub fn radix_sort_pairs_u64_u32(
    ctx: &Context,
    stream: &Stream,
    keys: &GpuLease<Rw>,
    vals: &GpuLease<Rw>,
    keys_out: &GpuLease<Rw>,
    vals_out: &GpuLease<Rw>,
    temp: &GpuLease<Rw>,
    n: usize,
    bits: std::ops::Range<i32>,
) -> Result<(), Error> {
    ctx.activate()?;
    let kb = n.checked_mul(8).ok_or(Error::TooShort {
        name: "keys",
        actual: 0,
        needed: usize::MAX,
    })?;
    let vb = n.checked_mul(4).ok_or(Error::TooShort {
        name: "vals",
        actual: 0,
        needed: usize::MAX,
    })?;
    for (l, name, bytes) in [
        (keys, "keys", kb),
        (keys_out, "keys_out", kb),
        (vals, "vals", vb),
        (vals_out, "vals_out", vb),
    ] {
        ctx.check_lease(l, name, bytes)?;
    }
    let needed = radix_sort_pairs_u64_u32_temp_size(n)?;
    ctx.check_lease(temp, "temp", needed)?;
    // SAFETY: each pointer comes from the checked live lease; byte checks cover n elements and temp storage; Rw leases permit kernel writes.
    check(unsafe {
        umgpu_radix_sort_pairs_u64_u32(
            temp.as_ptr().cast(),
            temp.len(),
            keys.as_ptr().cast(),
            keys_out.as_ptr().cast(),
            vals.as_ptr().cast(),
            vals_out.as_ptr().cast(),
            n,
            bits.start,
            bits.end,
            stream.raw,
        )
    })
}

/// Enqueues run-length encoding of sorted u64 keys.
pub fn rle_u64(
    ctx: &Context,
    stream: &Stream,
    keys: &GpuLease<Rw>,
    unique: &GpuLease<Rw>,
    counts: &GpuLease<Rw>,
    num_runs: &GpuLease<Rw>,
    temp: &GpuLease<Rw>,
    n: usize,
) -> Result<(), Error> {
    ctx.activate()?;
    let kb = n.checked_mul(8).ok_or(Error::TooShort {
        name: "keys",
        actual: 0,
        needed: usize::MAX,
    })?;
    let cb = n.checked_mul(4).ok_or(Error::TooShort {
        name: "counts",
        actual: 0,
        needed: usize::MAX,
    })?;
    for (l, name, bytes) in [
        (keys, "keys", kb),
        (unique, "unique", kb),
        (counts, "counts", cb),
        (num_runs, "num_runs", 4),
    ] {
        ctx.check_lease(l, name, bytes)?;
    }
    let needed = rle_u64_temp_size(n)?;
    ctx.check_lease(temp, "temp", needed)?;
    // SAFETY: checked leases provide all CUB input/output ranges and caller retains them until submit's event completes.
    check(unsafe {
        umgpu_rle_u64(
            temp.as_ptr().cast(),
            temp.len(),
            keys.as_ptr().cast(),
            unique.as_ptr().cast(),
            counts.as_ptr().cast(),
            num_runs.as_ptr().cast(),
            n,
            stream.raw,
        )
    })
}

/// Enqueues an exclusive u32 sum scan.
pub fn exclusive_scan_u32(
    ctx: &Context,
    stream: &Stream,
    input: &GpuLease<Rw>,
    output: &GpuLease<Rw>,
    temp: &GpuLease<Rw>,
    n: usize,
) -> Result<(), Error> {
    ctx.activate()?;
    let bytes = n.checked_mul(4).ok_or(Error::TooShort {
        name: "input",
        actual: 0,
        needed: usize::MAX,
    })?;
    ctx.check_lease(input, "input", bytes)?;
    ctx.check_lease(output, "output", bytes)?;
    let needed = exclusive_scan_u32_temp_size(n)?;
    ctx.check_lease(temp, "temp", needed)?;
    // SAFETY: checked leases cover input, output, and CUB scratch for n u32 elements.
    check(unsafe {
        umgpu_exclusive_scan_u32(
            temp.as_ptr().cast(),
            temp.len(),
            input.as_ptr().cast(),
            output.as_ptr().cast(),
            n,
            stream.raw,
        )
    })
}

/// Enqueues `output[i] = input[i] + 1` for `n` u64 elements.
pub fn inc_u64(
    ctx: &Context,
    stream: &Stream,
    input: &GpuLease<Rw>,
    output: &GpuLease<Rw>,
    n: usize,
) -> Result<(), Error> {
    ctx.activate()?;
    let bytes = n.checked_mul(8).ok_or(Error::TooShort {
        name: "input",
        actual: 0,
        needed: usize::MAX,
    })?;
    ctx.check_lease(input, "input", bytes)?;
    ctx.check_lease(output, "output", bytes)?;
    // SAFETY: checked input/output leases cover n u64 elements and output is Rw.
    check(unsafe { umgpu_inc_u64(input.as_ptr().cast(), output.as_ptr().cast(), n, stream.raw) })
}

/// Duplication-key flavour accepted by [`dup_keys`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DupKeyMode {
    Position,
    Sequence,
}

/// Enqueues one 64-bit key and record index per 48-byte resident header.
/// The CUDA shim reads documented byte offsets rather than a C++ Rust-struct analogue.
pub fn dup_keys(
    ctx: &Context,
    stream: &Stream,
    headers: &GpuLease<umem::Ro>,
    arena: &GpuLease<umem::Ro>,
    keys_out: &GpuLease<Rw>,
    vals_out: &GpuLease<Rw>,
    n: usize,
    mode: DupKeyMode,
) -> Result<(), Error> {
    ctx.activate()?;
    let hb = n.checked_mul(48).ok_or(Error::TooShort {
        name: "headers",
        actual: 0,
        needed: usize::MAX,
    })?;
    let kb = n.checked_mul(8).ok_or(Error::TooShort {
        name: "keys_out",
        actual: 0,
        needed: usize::MAX,
    })?;
    let vb = n.checked_mul(4).ok_or(Error::TooShort {
        name: "vals_out",
        actual: 0,
        needed: usize::MAX,
    })?;
    ctx.check_lease(headers, "headers", hb)?;
    // Every body access is span-checked by the kernel against this live arena lease.
    ctx.check_lease(arena, "arena", 0)?;
    ctx.check_lease(keys_out, "keys_out", kb)?;
    ctx.check_lease(vals_out, "vals_out", vb)?;
    let mode = match mode {
        DupKeyMode::Position => 0,
        DupKeyMode::Sequence => 1,
    };
    // SAFETY: checked live leases cover the fixed input and outputs; submit retains them through the CUDA event.
    check(unsafe {
        umgpu_dup_keys(
            headers.as_ptr().cast(),
            arena.as_ptr(),
            arena.len(),
            n,
            mode,
            keys_out.as_ptr().cast(),
            vals_out.as_ptr().cast(),
            stream.raw,
        )
    })
}

fn check(code: i32) -> Result<(), Error> {
    if code == 0 {
        Ok(())
    } else {
        Err(Error::Cuda {
            code,
            message: error_message(code),
        })
    }
}
fn error_message(code: i32) -> String {
    // SAFETY: CUDA returns a static NUL-terminated diagnostic string for every error code.
    let p = unsafe { umgpu_error_string(code) };
    if p.is_null() {
        format!("unknown CUDA error {code}")
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}
fn fence_error(code: i32) -> FenceError {
    FenceError {
        message: error_message(if code < 0 { -code } else { code }),
    }
}

/// nvCOMP pointer alignment requirements for a batched Deflate launch.
#[cfg(feature = "nvcomp")]
#[derive(Clone, Copy, Debug)]
pub struct DeflateAlignments {
    /// Required input-address alignment.
    pub input: usize,
    /// Required output-address alignment.
    pub output: usize,
    /// Required temporary-storage alignment.
    pub temp: usize,
}

/// Gets nvCOMP's Deflate pointer-alignment requirements.
#[cfg(feature = "nvcomp")]
pub fn deflate_alignments(algorithm: i32) -> Result<DeflateAlignments, Error> {
    let mut input = 0;
    let mut output = 0;
    let mut temp = 0;
    // SAFETY: all three pointers name writable stack slots for nvCOMP's sizing query.
    check_nvcomp(unsafe {
        umgpu_deflate_alignments(algorithm, &mut input, &mut output, &mut temp)
    })?;
    Ok(DeflateAlignments {
        input,
        output,
        temp,
    })
}

/// Gets nvCOMP scratch bytes for a Deflate batch.
#[cfg(feature = "nvcomp")]
pub fn deflate_temp_size(
    num_chunks: usize,
    max_chunk: usize,
    algorithm: i32,
) -> Result<usize, Error> {
    let mut bytes = 0;
    // SAFETY: `bytes` is a writable output slot; nvCOMP dereferences no batch pointers here.
    check_nvcomp(unsafe { umgpu_deflate_temp_size(num_chunks, max_chunk, algorithm, &mut bytes) })?;
    Ok(bytes)
}

/// Gets nvCOMP's maximum raw-Deflate output size for one input chunk.
#[cfg(feature = "nvcomp")]
pub fn deflate_max_output(max_chunk: usize, algorithm: i32) -> Result<usize, Error> {
    let mut bytes = 0;
    // SAFETY: `bytes` is a writable output slot for nvCOMP's sizing query.
    check_nvcomp(unsafe { umgpu_deflate_max_output(max_chunk, algorithm, &mut bytes) })?;
    Ok(bytes)
}

/// Enqueues a batched raw-Deflate compression. All arrays and payloads must be `umem`
/// leases and stay in the returned submission until its fence completes.
#[cfg(feature = "nvcomp")]
#[allow(clippy::too_many_arguments)]
pub fn deflate_batch(
    ctx: &Context,
    stream: &Stream,
    input: &GpuLease<Rw>,
    in_ptrs: &GpuLease<Rw>,
    in_bytes: &GpuLease<Rw>,
    temp: &GpuLease<Rw>,
    output: &GpuLease<Rw>,
    out_ptrs: &GpuLease<Rw>,
    out_bytes: &GpuLease<Rw>,
    statuses: &GpuLease<Rw>,
    first_chunk: usize,
    num_chunks: usize,
    max_chunk: usize,
    max_output: usize,
    algorithm: i32,
) -> Result<(), Error> {
    if num_chunks == 0 || max_chunk > 65_536 {
        return Err(Error::InvalidInput(
            "Deflate chunks must be 1..=65536 bytes",
        ));
    }
    ctx.activate()?;
    let end_chunk = first_chunk
        .checked_add(num_chunks)
        .ok_or(Error::InvalidInput("Deflate chunk range overflow"))?;
    let ptr_bytes = end_chunk
        .checked_mul(std::mem::size_of::<usize>())
        .ok_or(Error::InvalidInput("Deflate pointer array overflow"))?;
    let status_bytes = end_chunk
        .checked_mul(std::mem::size_of::<i32>())
        .ok_or(Error::InvalidInput("Deflate status array overflow"))?;
    ctx.check_lease(input, "deflate input", 1)?;
    ctx.check_lease(in_ptrs, "deflate input pointers", ptr_bytes)?;
    ctx.check_lease(in_bytes, "deflate input sizes", ptr_bytes)?;
    ctx.check_lease(
        temp,
        "deflate temp",
        deflate_temp_size(num_chunks, max_chunk, algorithm)?,
    )?;
    let output_bytes = end_chunk
        .checked_mul(max_output)
        .ok_or(Error::InvalidInput("Deflate output size overflow"))?;
    ctx.check_lease(output, "deflate output", output_bytes)?;
    ctx.check_lease(out_ptrs, "deflate output pointers", ptr_bytes)?;
    ctx.check_lease(out_bytes, "deflate output sizes", ptr_bytes)?;
    ctx.check_lease(statuses, "deflate statuses", status_bytes)?;
    // SAFETY: each address comes from a checked, live `umem` lease, and the sub-batch
    // window `[first_chunk, first_chunk + num_chunks)` is bounds-checked against every
    // array above. The caller fills pointer arrays only with ranges in `input`/`output`;
    // submission retains every lease through the stream fence, preventing CPU/GPU races
    // and dangling pointers.
    check_nvcomp(unsafe {
        umgpu_deflate_batch(
            in_ptrs.as_ptr().cast::<*const c_void>().add(first_chunk),
            in_bytes.as_ptr().cast::<usize>().add(first_chunk),
            max_chunk,
            num_chunks,
            temp.as_ptr().cast(),
            temp.len(),
            out_ptrs.as_ptr().cast::<*mut c_void>().add(first_chunk),
            out_bytes.as_ptr().cast::<usize>().add(first_chunk),
            algorithm,
            statuses.as_ptr().cast::<c_int>().add(first_chunk),
            stream.raw,
        )
    })
}

#[cfg(feature = "nvcomp")]
fn check_nvcomp(code: i32) -> Result<(), Error> {
    if code == 0 {
        return Ok(());
    }
    // SAFETY: nvCOMP returns a static NUL-terminated error string for status values.
    let p = unsafe { umgpu_nvcomp_error_string(code) };
    let message = if p.is_null() {
        format!("unknown nvCOMP error {code}")
    } else {
        // SAFETY: `p` is the static nvCOMP diagnostic just checked for null.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    };
    Err(Error::Cuda { code, message })
}

unsafe extern "C" {
    fn umgpu_markdup_temp_size(n: usize, bytes: *mut usize) -> c_int;
    fn umgpu_markdup(
        headers: *const c_void,
        arena: *const u8,
        arena_len: usize,
        order: *const u32,
        n: usize,
        work: *mut c_void,
        temp: *mut c_void,
        temp_bytes: usize,
        control: *mut c_void,
        stream: *mut c_void,
    ) -> c_int;
}

/// CUB scratch requirement for the resident markdup pipeline.
pub fn markdup_temp_size(n: usize) -> Result<usize, Error> {
    temp_size(n, umgpu_markdup_temp_size)
}

/// Runs resident markdup synchronously. The shim drains the stream on every exit,
/// including errors, before returning; all input/output ranges remain leased throughout.
/// `order` must contain record indices (out-of-range indices are rejected on device).
/// Duplicate indices are returned in workspace at `MARKDUP_INDICES_OFFSET * n`.
/// Control layout: six u64 metrics, u32 duplicate count, u32 error, five f32 times,
/// four bytes padding, then four u64 populations (examined, pairs, singles, pair ends).
#[allow(clippy::too_many_arguments)]
pub fn markdup(
    ctx: &Context,
    table: &GpuLease<umem::Ro>,
    arena: &GpuLease<umem::Ro>,
    order: &GpuLease<umem::Ro>,
    work: &GpuLease<Rw>,
    temp: &GpuLease<Rw>,
    control: &GpuLease<Rw>,
    n: usize,
) -> Result<(), Error> {
    ctx.activate()?;
    // CUB counts and the 2*n pair-end stream must fit its signed item count.
    if n == 0 || n > i32::MAX as usize / 2 {
        return Err(Error::InvalidInput("record count must be 1..=i32::MAX/2"));
    }
    ctx.check_lease(table, "table", n * 48)?;
    ctx.check_lease(arena, "arena", 1)?;
    ctx.check_lease(order, "order", n * 4)?;
    ctx.check_lease(work, "markdup workspace", n * crate::MARKDUP_WORK_BYTES)?;
    ctx.check_lease(temp, "markdup scratch", markdup_temp_size(n)?)?;
    ctx.check_lease(control, "markdup control", crate::MARKDUP_CONTROL_BYTES)?;
    // SAFETY: live, context-checked leases guarantee these addresses. Only compare
    // their ranges here; the shim is the sole code that dereferences the pointers.
    let ranges = unsafe {
        [
            (table.as_ptr() as usize, table.len()),
            (arena.as_ptr() as usize, arena.len()),
            (order.as_ptr() as usize, order.len()),
            (work.as_ptr() as usize, work.len()),
            (temp.as_ptr() as usize, temp.len()),
            (control.as_ptr() as usize, control.len()),
        ]
    };
    for i in 3..ranges.len() {
        for j in 0..i {
            let (a, alen) = ranges[i];
            let (b, blen) = ranges[j];
            if a < b.saturating_add(blen) && b < a.saturating_add(alen) {
                return Err(Error::InvalidInput("writable markdup leases overlap"));
            }
        }
    }
    // SAFETY: table, arena and order Ro leases cover the validated read ranges;
    // work, temp and control Rw leases cover all writable ranges. The shim checks
    // arena offsets and order indices, and synchronizes even on failure, so none
    // of these pointers can outlive its guaranteeing lease.
    check(unsafe {
        umgpu_markdup(
            table.as_ptr().cast(),
            arena.as_ptr(),
            arena.len(),
            order.as_ptr().cast(),
            n,
            work.as_ptr().cast(),
            temp.as_ptr().cast(),
            temp.len(),
            control.as_ptr().cast(),
            ctx.default_stream().raw,
        )
    })
}
