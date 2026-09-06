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
