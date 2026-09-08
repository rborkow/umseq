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

/// Separate V2 transport; V1 layouts and entry points remain unchanged.
pub const PROBE_ABI_VERSION: u64 = 3;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeRequestV2 {
    pub inner: ProbeRequest,
    pub distance: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeOutputV2 {
    pub inner: ProbeOutput,
    /// 0 legacy, 1 prefix only, 2 unique, 3 inner search after prefix walk.
    pub branch: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ProbeConfigV2 {
    pub inner: ProbeConfig,
    pub index_bases: u64,
    pub sai_width: u64,
    pub absent_mask: u64,
    pub n_mask: u64,
    pub n_mask_c: u64,
    pub sparse: u64,
    pub seed_search_lmax: u64,
    pub sai_offset: u64,
    pub sai_bytes: u64,
    pub starts: [u64; 16],
}
// SAFETY: FFI records consist solely of consecutive u64 words, with no padding.
unsafe impl umem::Pod for ProbeRequestV2 {}
// SAFETY: FFI record consists solely of consecutive u64 words, with no padding.
unsafe impl umem::Pod for ProbeOutputV2 {}
// SAFETY: FFI record consists solely of consecutive u64 words, with no padding.
unsafe impl umem::Pod for ProbeConfigV2 {}
const _: () = {
    assert!(size_of::<ProbeRequestV2>() == 88);
    assert!(std::mem::offset_of!(ProbeRequestV2, distance) == 80);
    assert!(size_of::<ProbeOutputV2>() == 48);
    assert!(std::mem::offset_of!(ProbeOutputV2, branch) == 40);
    assert!(size_of::<ProbeConfigV2>() == 224);
    assert!(std::mem::offset_of!(ProbeConfigV2, index_bases) == 24);
    assert!(std::mem::offset_of!(ProbeConfigV2, starts) == 96);
    assert!(std::mem::offset_of!(ProbeConfigV2, sai_width) == 32);
    assert!(std::mem::offset_of!(ProbeConfigV2, absent_mask) == 40);
    assert!(std::mem::offset_of!(ProbeConfigV2, n_mask) == 48);
    assert!(std::mem::offset_of!(ProbeConfigV2, n_mask_c) == 56);
    assert!(std::mem::offset_of!(ProbeConfigV2, sparse) == 64);
    assert!(std::mem::offset_of!(ProbeConfigV2, seed_search_lmax) == 72);
    assert!(std::mem::offset_of!(ProbeConfigV2, sai_offset) == 80);
    assert!(std::mem::offset_of!(ProbeConfigV2, sai_bytes) == 88);
    assert!(align_of::<ProbeConfigV2>() == 8);
};

pub const PROBE_CHAIN_CAPACITY: usize = 8;
pub const PROBE_CHAIN_OVERFLOW: u64 = 9;
pub const PROBE_CHAIN_MAX_STEPS: u64 = 10;
pub const PROBE_CHAIN_NO_PROGRESS: u64 = 11;
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeRequestV3 {
    pub s0: u64,
    pub s1: u64,
    pub read_len: u64,
    pub piece_start: u64,
    pub piece_length: u64,
    pub istart: u64,
    pub nstart: u64,
    pub lstart: u64,
    pub dir: u64,
    pub seed_map_min: u64,
    pub max_steps: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeStepV3 {
    pub shift: u64,
    pub max_l: u64,
    pub nrep: u64,
    pub low: u64,
    pub high: u64,
    pub branch: u64,
    pub status: u64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProbeOutputV3 {
    pub steps: [ProbeStepV3; PROBE_CHAIN_CAPACITY],
    pub n_steps: u64,
    pub flag_dir_map_cleared: u64,
    /// Only status zero is consumable; otherwise discard every step.
    pub status: u64,
}
// SAFETY: FFI record is consecutive u64 storage with no padding or invalid patterns.
unsafe impl umem::Pod for ProbeRequestV3 {}
const _: () = {
    assert!(size_of::<ProbeRequestV3>() == 88);
    assert!(align_of::<ProbeRequestV3>() == 8);
    assert!(std::mem::offset_of!(ProbeRequestV3, s0) == 0);
    assert!(std::mem::offset_of!(ProbeRequestV3, s1) == 8);
    assert!(std::mem::offset_of!(ProbeRequestV3, read_len) == 16);
    assert!(std::mem::offset_of!(ProbeRequestV3, piece_start) == 24);
    assert!(std::mem::offset_of!(ProbeRequestV3, piece_length) == 32);
    assert!(std::mem::offset_of!(ProbeRequestV3, istart) == 40);
    assert!(std::mem::offset_of!(ProbeRequestV3, nstart) == 48);
    assert!(std::mem::offset_of!(ProbeRequestV3, lstart) == 56);
    assert!(std::mem::offset_of!(ProbeRequestV3, dir) == 64);
    assert!(std::mem::offset_of!(ProbeRequestV3, seed_map_min) == 72);
    assert!(std::mem::offset_of!(ProbeRequestV3, max_steps) == 80);
};
// SAFETY: FFI record is consecutive u64 storage with no padding or invalid patterns.
unsafe impl umem::Pod for ProbeStepV3 {}
const _: () = {
    assert!(size_of::<ProbeStepV3>() == 56);
    assert!(align_of::<ProbeStepV3>() == 8);
    assert!(std::mem::offset_of!(ProbeStepV3, shift) == 0);
    assert!(std::mem::offset_of!(ProbeStepV3, max_l) == 8);
    assert!(std::mem::offset_of!(ProbeStepV3, nrep) == 16);
    assert!(std::mem::offset_of!(ProbeStepV3, low) == 24);
    assert!(std::mem::offset_of!(ProbeStepV3, high) == 32);
    assert!(std::mem::offset_of!(ProbeStepV3, branch) == 40);
    assert!(std::mem::offset_of!(ProbeStepV3, status) == 48);
};
// SAFETY: FFI record is consecutive u64 storage with no padding or invalid patterns.
unsafe impl umem::Pod for ProbeOutputV3 {}
const _: () = {
    assert!(size_of::<ProbeOutputV3>() == 472);
    assert!(align_of::<ProbeOutputV3>() == 8);
    assert!(std::mem::offset_of!(ProbeOutputV3, steps) == 0);
    assert!(std::mem::offset_of!(ProbeOutputV3, n_steps) == 448);
    assert!(std::mem::offset_of!(ProbeOutputV3, flag_dir_map_cleared) == 456);
    assert!(std::mem::offset_of!(ProbeOutputV3, status) == 464);
};

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
