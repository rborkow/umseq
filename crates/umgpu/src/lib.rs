//! CUDA backend for [`umem`] buffers. Enable the `cuda` feature on a CUDA host.

#[cfg(feature = "cuda")]
mod cuda;
#[cfg(not(feature = "cuda"))]
mod stub;

#[cfg(feature = "cuda")]
pub use cuda::*;
#[cfg(not(feature = "cuda"))]
pub use stub::*;

/// Transfer accounting for this crate. Its CUDA path never calls a copy API.
pub mod stats {
    /// Returns bytes copied by `umgpu` transfer operations (there are none).
    pub const fn bytes_copied() -> u64 {
        0
    }
}
