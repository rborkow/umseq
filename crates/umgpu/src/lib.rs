//! CUDA backend for [`umem`] buffers. Enable the `cuda` feature on a CUDA host.

#[cfg(feature = "cuda")]
mod cuda;
#[cfg(not(feature = "cuda"))]
mod stub;

#[cfg(feature = "cuda")]
pub use cuda::*;
#[cfg(not(feature = "cuda"))]
pub use stub::*;
