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

/// Order-preserving fragment key: bits 48..33 = unsigned reference ID (0..65535),
/// bits 32..1 = signed i32 position XOR 0x80000000, bit 0 = reverse strand.
/// Bits 63..49 are zero. Unsupported reference IDs are rejected, never truncated.
pub fn pack_fragment_end(tid: i32, pos: i32, reverse: bool) -> Option<u64> {
    let tid = u16::try_from(tid).ok()?;
    Some((u64::from(tid) << 33) | (u64::from((pos as u32) ^ 0x8000_0000) << 1) | u64::from(reverse))
}

/// Resident markdup workspace bytes per record, including both radix ping-pong buffers.
pub const MARKDUP_WORK_BYTES: usize = 96;
/// Metrics, count, error, five event durations, padding, and four population counts.
pub const MARKDUP_CONTROL_BYTES: usize = 112;
/// Byte offset per record of the compact duplicate-index output in the workspace.
pub const MARKDUP_INDICES_OFFSET: usize = 88;

#[cfg(test)]
mod packing_tests {
    use super::pack_fragment_end;

    #[test]
    fn fragment_packing_preserves_tuple_order() {
        let mut tuples = Vec::new();
        for tid in [0, 1, 32767, 32768, 65535] {
            for pos in [i32::MIN, -101, -1, 0, 1, 101, i32::MAX] {
                for reverse in [false, true] {
                    tuples.push((tid, pos, reverse));
                }
            }
        }
        let mut packed_order = tuples.clone();
        packed_order.reverse();
        packed_order.sort_by_key(|&(t, p, r)| pack_fragment_end(t, p, r).unwrap());
        assert_eq!(packed_order, tuples);
        assert_eq!(pack_fragment_end(-1, 0, false), None);
        assert_eq!(pack_fragment_end(65536, 0, false), None);
    }
}
