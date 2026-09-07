//! PROBE transport, all integers and no implicit padding. Tag 0 = inner search.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeRequest {
    pub tag: u64,
    pub s0: u64,
    pub s1: u64,
    pub read_len: u64,
    pub start: u64,
    pub length: u64,
    pub prefix: u64,
    pub low: u64,
    pub high: u64,
    pub dir: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeOutput {
    pub length: u64,
    pub low: u64,
    pub high: u64,
    pub count: u64,
    /// 0 success; 1 unsupported tag/direction; 2 request bounds; 3 comparator bounds; 4 result bounds.
    pub status: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeStats {
    pub gathers: u64,
    pub bytes: u64,
    pub loops: u64,
    pub comparisons: u64,
    pub max_compare: u64,
    pub directions: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ProbeConfig {
    pub n_genome: u64,
    pub n_sa: u64,
    pub strand_bit: u64,
}
// SAFETY: PROBE FFI records contain only consecutive u64 fields, no padding or invalid patterns.
unsafe impl umem::Pod for ProbeRequest {}
// SAFETY: PROBE FFI record contains five consecutive u64 fields.
unsafe impl umem::Pod for ProbeOutput {}
// SAFETY: PROBE FFI record contains six consecutive u64 fields.
unsafe impl umem::Pod for ProbeStats {}
// SAFETY: PROBE FFI record contains three consecutive u64 fields.
unsafe impl umem::Pod for ProbeConfig {}

/// Shared immutable views scoped to live PROBE Ro leases, for CPU/GPU overlap only.
pub struct ProbeSlices<'a> {
    pub genome: &'a [u8],
    pub sa: &'a [u8],
    pub reads: &'a [u8],
    pub requests: &'a [ProbeRequest],
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_abi_roundtrip() {
        assert_eq!(size_of::<ProbeRequest>(), 80);
        assert_eq!(size_of::<ProbeOutput>(), 40);
        assert_eq!(size_of::<ProbeStats>(), 48);
        assert_eq!(size_of::<ProbeConfig>(), 24);
        let mut b = umem::Buf::<umem::Rw>::allocate(
            80,
            umem::Allocation::Anon {
                huge: false,
                require_huge: false,
            },
        )
        .unwrap();
        b.as_pod_mut_slice::<ProbeRequest>()[0] = ProbeRequest {
            high: 0x123456789abcdef0,
            dir: 1,
            ..Default::default()
        };
        assert_eq!(
            u64::from_le_bytes(b.as_slice()[64..72].try_into().unwrap()),
            0x123456789abcdef0
        );
    }
}
