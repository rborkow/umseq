// One source of truth for the private ABI; the module boundary preserves its
// crate-level documentation while this crate supplies the static-link facade.
#[path = "../../umgpu/ffi/star_integrate.rs"]
mod implementation;

pub use implementation::{UsiContext, UsiErrorV1, UsiIdentityV1};
