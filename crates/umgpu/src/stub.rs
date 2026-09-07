//! Non-CUDA implementation used by ordinary development hosts.

use thiserror::Error;

/// Backend errors.
#[derive(Debug, Error)]
pub enum Error {
    /// This binary was built without the `cuda` feature.
    #[error("CUDA support is unavailable; rebuild umgpu with --features cuda")]
    Unsupported,
}

/// Options controlling CUDA context setup.
#[derive(Clone, Copy, Debug, Default)]
pub struct ContextOptions {
    /// Request CUDA host registration for leased mappings.
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

/// Placeholder CUDA context.
pub struct Context;
impl Context {
    /// Always reports that CUDA is unavailable in this build.
    pub fn new(_device: i32, _options: ContextOptions) -> Result<Self, Error> {
        Err(Error::Unsupported)
    }
}

/// Unavailable without CUDA.
pub fn markdup_temp_size(_n: usize) -> Result<usize, Error> {
    Err(Error::Unsupported)
}

/// Unavailable without CUDA.
#[allow(clippy::too_many_arguments)]
pub fn markdup(
    _ctx: &Context,
    _table: &umem::GpuLease<umem::Ro>,
    _arena: &umem::GpuLease<umem::Ro>,
    _order: &umem::GpuLease<umem::Ro>,
    _work: &umem::GpuLease<umem::Rw>,
    _temp: &umem::GpuLease<umem::Rw>,
    _control: &umem::GpuLease<umem::Rw>,
    _n: usize,
) -> Result<(), Error> {
    Err(Error::Unsupported)
}

impl Context {
    pub fn umem_context(&self) -> umem::Context {
        umem::Context::new(umem::ContextId(0))
    }
    pub fn device_props(&self) -> DeviceProps {
        DeviceProps::default()
    }
}
#[allow(clippy::too_many_arguments)]
pub fn seed_probe<T: Send>(
    _ctx: &Context,
    _genome: &umem::GpuLease<umem::Ro>,
    _sa: &umem::GpuLease<umem::Ro>,
    _reads: &umem::GpuLease<umem::Ro>,
    _requests: &umem::GpuLease<umem::Ro>,
    _output: &umem::GpuLease<umem::Rw>,
    _stats: &umem::GpuLease<umem::Rw>,
    _config: crate::ProbeConfig,
    _start: usize,
    _n: usize,
    _cpu: impl FnOnce(crate::ProbeSlices<'_>) -> T + Send,
) -> Result<(f32, f64, T), Error> {
    seed_probe_variant(
        _ctx,
        _genome,
        _sa,
        _reads,
        _requests,
        _output,
        _stats,
        _config,
        _start,
        _n,
        crate::ProbeVariant::Thread,
        _cpu,
    )
}

/// PROBE selected variant, with the same checked leases and synchronous drain.
#[allow(clippy::too_many_arguments)]
pub fn seed_probe_variant<T: Send>(
    _ctx: &Context,
    _genome: &umem::GpuLease<umem::Ro>,
    _sa: &umem::GpuLease<umem::Ro>,
    _reads: &umem::GpuLease<umem::Ro>,
    _requests: &umem::GpuLease<umem::Ro>,
    _output: &umem::GpuLease<umem::Rw>,
    _stats: &umem::GpuLease<umem::Rw>,
    _config: crate::ProbeConfig,
    _start: usize,
    _n: usize,
    _variant: crate::ProbeVariant,
    _cpu: impl FnOnce(crate::ProbeSlices<'_>) -> T + Send,
) -> Result<(f32, f64, T), Error> {
    Err(Error::Unsupported)
}

pub fn seed_probe_lease_address<M: umem::Mode>(lease: &umem::GpuLease<M>) -> usize {
    // SAFETY: address observation only; no device or host dereference.
    unsafe { lease.as_ptr() as usize }
}

pub fn seed_probe_reclaim(
    _ctx: &Context,
    _leases: Vec<umem::AnyLease>,
) -> Result<Vec<umem::AnyBuf>, String> {
    Err("PROBE CUDA unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_is_unsupported_without_cuda() {
        assert!(matches!(
            Context::new(0, ContextOptions::default()),
            Err(Error::Unsupported)
        ));
    }
}
