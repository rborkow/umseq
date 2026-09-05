//! OS-backed virtual-memory regions.

use std::{fs::File, io, os::fd::AsRawFd, path::Path, ptr::NonNull};

use thiserror::Error;

#[cfg(target_os = "linux")]
use crate::smaps::PageKind;
use crate::smaps::{self, PageReport};

#[cfg(target_os = "linux")]
const HUGE: usize = 2 * 1024 * 1024;

/// Allocation policy for an anonymous buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Allocation {
    /// Anonymous memory, optionally requesting transparent huge pages.
    Anon {
        /// Ask the operating system for huge pages where available.
        huge: bool,
        /// Reject the allocation unless its huge coverage is complete.
        require_huge: bool,
    },
    /// Explicit 2 MiB HugeTLB pages.
    Hugetlb,
}

/// Failure to allocate or verify a backing region.
#[derive(Debug, Error)]
pub enum Error {
    /// A requested length cannot be represented as a mapping.
    #[error("invalid allocation length {0}")]
    InvalidLength(usize),
    /// An operating-system memory operation failed.
    #[error("memory operation failed: {0}")]
    Io(#[from] io::Error),
    /// Huge-page coverage was incomplete.
    #[error("huge-page coverage {covered} of {len} bytes")]
    NotHuge {
        /// Bytes observed as huge-page backed.
        covered: usize,
        /// Requested addressable bytes.
        len: usize,
    },
    /// HugeTLB allocation failed because the reserved pool is unavailable.
    #[error("HugeTLB allocation failed; reserve 2 MiB pages with vm.nr_hugepages")]
    HugepagePool,
}

/// An owned mapped virtual-memory region.
pub(crate) struct Mapping {
    ptr: NonNull<u8>,
    len: usize,
    mapped_len: usize,
    /// (low guard addr, high guard addr, guard len) — PROT_NONE pages we own on each side.
    guard: Option<(usize, usize, usize)>,
}

// SAFETY: Mapping is only exposed through ownership-typed Buf/GpuLease state machines; raw
// mapping access never escapes those states as a safe shared mutable capability.
unsafe impl Send for Mapping {}
// SAFETY: Mapping is only exposed through ownership-typed Buf/GpuLease state machines; raw
// mapping access never escapes those states as a safe shared mutable capability.
unsafe impl Sync for Mapping {}

impl Mapping {
    pub(crate) fn allocate(len: usize, backing: Allocation) -> Result<(Self, PageReport), Error> {
        if len == 0 || len > isize::MAX as usize {
            return Err(Error::InvalidLength(len));
        }
        match backing {
            Allocation::Hugetlb => Self::hugetlb(len),
            Allocation::Anon { huge, require_huge } => {
                #[cfg(target_os = "linux")]
                {
                    match Self::anonymous(len, huge, require_huge) {
                        Err(Error::NotHuge { .. }) if require_huge => {
                            // DESIGN: HugeTLB fallback maps a 2 MiB-rounded region but keeps the
                            // caller's logical length.
                            let rounded = round_up(len, HUGE).ok_or(Error::InvalidLength(len))?;
                            Self::hugetlb(rounded).map(|(mut m, mut r)| {
                                m.len = len;
                                r.len = len;
                                r.huge_bytes = len;
                                r.fallback = true;
                                (m, r)
                            })
                        }
                        result => result,
                    }
                }
                #[cfg(not(target_os = "linux"))]
                {
                    Self::anonymous(len, huge, require_huge)
                }
            }
        }
    }

    pub(crate) unsafe fn map_file(path: impl AsRef<Path>) -> Result<(Self, PageReport), Error> {
        let file = File::open(path)?;
        let len: usize = file
            .metadata()?
            .len()
            .try_into()
            .map_err(|_| Error::InvalidLength(usize::MAX))?;
        if len == 0 || len > isize::MAX as usize {
            return Err(Error::InvalidLength(len));
        }
        // SAFETY: caller provides the design's no-external-writer contract; this Mapping owns
        // the resulting read-only mapping and unmaps the exact length.
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len, // kernel rounds to a page; see mapped_len below
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(io::Error::last_os_error().into());
        }
        // The kernel rounds the VMA up to a page boundary; report over that extent, keep the
        // logical (EOF) length for slices.
        let mapped_len = round_up(len, page_size()).ok_or(Error::InvalidLength(len))?;
        let mapping = Self {
            ptr: NonNull::new(raw as *mut u8).expect("mmap is non-null"),
            len,
            mapped_len,
            guard: None,
        };
        let mut report = smaps::report(mapping.ptr.as_ptr(), mapped_len)?;
        report.len = len;
        report.huge_bytes = report.huge_bytes.min(len);
        Ok((mapping, report))
    }

    #[cfg(target_os = "linux")]
    fn anonymous(len: usize, huge: bool, require_huge: bool) -> Result<(Self, PageReport), Error> {
        let logical_len = len;
        // DESIGN: when huge pages are requested, the mapped length is rounded to 2 MiB so the final
        // extent is collapse-eligible; otherwise a non-multiple length can never reach 100% coverage.
        let unit = if huge { HUGE } else { page_size() };
        let len = round_up(len, unit).ok_or(Error::InvalidLength(len))?;
        // 2 MiB alignment slack + guard pages on both sides.
        let map_len = len.checked_add(2 * HUGE).ok_or(Error::InvalidLength(len))?;
        Self::anonymous_with_slack(logical_len, len, huge, require_huge, map_len)
    }

    #[cfg(target_os = "linux")]
    fn anonymous_with_slack(
        logical_len: usize,
        len: usize,
        huge: bool,
        require_huge: bool,
        map_len: usize,
    ) -> Result<(Self, PageReport), Error> {
        // SAFETY: mmap creates a region owned by this Mapping; map_len is page-aligned after rounding.
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                map_len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(io::Error::last_os_error().into());
        }
        let raw_addr = raw as usize;
        let page = page_size();
        // Align up to 2 MiB. Keep one PROT_NONE guard page immediately below and above the
        // region, *owned by us*, so the kernel can never merge our VMA with a neighbour
        // (different protection ⇒ no merge). Exact VMA bounds are what make the per-range smaps
        // report attributable. Everything else in the over-allocation is released.
        let mut aligned = (raw_addr + HUGE - 1) & !(HUGE - 1);
        if aligned == raw_addr {
            aligned += HUGE;
        }
        let head = aligned - raw_addr;
        let tail = map_len.saturating_sub(head + len);
        if head < page || tail < page {
            // Not enough room for guards on both sides; retry with more slack.
            // SAFETY: `raw`/`map_len` are exactly what mmap returned and nothing else owns them.
            unsafe { libc::munmap(raw, map_len) };
            return Self::anonymous_with_slack(
                logical_len,
                len,
                huge,
                require_huge,
                map_len + HUGE,
            );
        }
        let lo_guard = aligned - page;
        let hi_guard = aligned + len;
        // SAFETY (all munmap/mprotect below): owned, page-aligned subregions of the mmap above.
        // Until `mapping` exists nothing owns `raw`; release the whole region on any failure.
        let fail = |e: io::Error| -> Error {
            unsafe { libc::munmap(raw, map_len) };
            e.into()
        };
        if head > page && unsafe { libc::munmap(raw, head - page) } != 0 {
            return Err(fail(io::Error::last_os_error()));
        }
        if unsafe { libc::mprotect(lo_guard as *mut libc::c_void, page, libc::PROT_NONE) } != 0 {
            return Err(fail(io::Error::last_os_error()));
        }
        if tail > page
            && unsafe { libc::munmap((hi_guard + page) as *mut libc::c_void, tail - page) } != 0
        {
            return Err(fail(io::Error::last_os_error()));
        }
        if unsafe { libc::mprotect(hi_guard as *mut libc::c_void, page, libc::PROT_NONE) } != 0 {
            return Err(fail(io::Error::last_os_error()));
        }
        let mapping = Self {
            ptr: NonNull::new(aligned as *mut u8).expect("mmap is non-null"),
            len: logical_len,
            mapped_len: len,
            guard: Some((lo_guard, hi_guard, page)),
        };
        // `huge: false` is an explicit small-page request (a dependable benchmark control),
        // not merely "no advice".
        mapping.advise(if huge {
            libc::MADV_HUGEPAGE
        } else {
            libc::MADV_NOHUGEPAGE
        })?;
        mapping.populate()?;
        let mut report = smaps::report(mapping.ptr.as_ptr(), len)?;
        report.len = logical_len;
        report.huge_bytes = report.huge_bytes.min(logical_len);
        // DESIGN: under memory pressure MADV_COLLAPSE triggers synchronous compaction that can
        // take seconds per round and still fail (observed: 3-15 s with a 40 GB neighbour resident).
        // With require_huge the HugeTLB pool is a cheaper guaranteed fallback, so try one round;
        // without it, keep trying — best effort is all the caller asked for.
        let rounds = if require_huge { 1 } else { 3 };
        if huge && report.huge_bytes < logical_len {
            for _ in 0..rounds {
                mapping.collapse_uncovered()?;
                report = smaps::report(mapping.ptr.as_ptr(), len)?;
                report.len = logical_len;
                report.huge_bytes = report.huge_bytes.min(logical_len);
                if report.huge_bytes >= logical_len {
                    break;
                }
            }
        }
        if require_huge && report.huge_bytes < logical_len {
            return Err(Error::NotHuge {
                covered: report.huge_bytes,
                len: logical_len,
            });
        }
        Ok((mapping, report))
    }

    #[cfg(not(target_os = "linux"))]
    fn anonymous(
        len: usize,
        _huge: bool,
        _require_huge: bool,
    ) -> Result<(Self, PageReport), Error> {
        // DESIGN: macOS has no THP contract, so require_huge is accepted as a no-op.
        Self::plain_anon(len)
    }

    #[cfg(target_os = "linux")]
    fn hugetlb(len: usize) -> Result<(Self, PageReport), Error> {
        if !len.is_multiple_of(HUGE) {
            return Err(Error::InvalidLength(len));
        }
        // SAFETY: mmap creates a region owned by this Mapping and len is 2 MiB aligned.
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE
                    | libc::MAP_ANON
                    | libc::MAP_HUGETLB
                    | (21 << libc::MAP_HUGE_SHIFT),
                -1,
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            let error = io::Error::last_os_error();
            return if error.raw_os_error() == Some(libc::ENOMEM) {
                Err(Error::HugepagePool)
            } else {
                Err(error.into())
            };
        }
        let mapping = Self {
            ptr: NonNull::new(raw as *mut u8).expect("mmap is non-null"),
            len,
            mapped_len: len,
            guard: None,
        };
        mapping.populate()?;
        Ok((
            mapping,
            PageReport {
                len,
                huge_bytes: len,
                kind: PageKind::Hugetlb,
                observed_at: std::time::SystemTime::now(),
                fallback: false,
            },
        ))
    }

    #[cfg(not(target_os = "linux"))]
    fn hugetlb(_len: usize) -> Result<(Self, PageReport), Error> {
        Err(Error::HugepagePool)
    }

    #[cfg(not(target_os = "linux"))]
    fn plain_anon(len: usize) -> Result<(Self, PageReport), Error> {
        // SAFETY: mmap creates a region owned by this Mapping; len is supplied to munmap unchanged.
        let raw = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANON,
                -1,
                0,
            )
        };
        if raw == libc::MAP_FAILED {
            return Err(io::Error::last_os_error().into());
        }
        let mapping = Self {
            ptr: NonNull::new(raw as *mut u8).expect("mmap is non-null"),
            len,
            mapped_len: len,
            guard: None,
        };
        mapping.populate()?;
        let report = smaps::report(mapping.ptr.as_ptr(), len)?;
        Ok((mapping, report))
    }

    #[cfg(target_os = "linux")]
    fn advise(&self, advice: libc::c_int) -> Result<(), Error> {
        // SAFETY: this region is ours, page-aligned, and pointer came from our mmap.
        if unsafe { libc::madvise(self.ptr.as_ptr().cast(), self.mapped_len, advice) } != 0 {
            return Err(io::Error::last_os_error().into());
        }
        Ok(())
    }

    fn populate(&self) -> Result<(), Error> {
        #[cfg(target_os = "linux")]
        {
            const MADV_POPULATE_WRITE_VALUE: libc::c_int = 23;
            match self.advise(MADV_POPULATE_WRITE_VALUE) {
                Ok(()) => return Ok(()),
                // EINVAL: advice unsupported on this kernel → fall back to touching pages.
                Err(Error::Io(e)) if e.raw_os_error() == Some(libc::EINVAL) => {}
                // ENOMEM / EFAULT / EHWPOISON etc. are real failures; do not turn them into SIGBUS.
                Err(e) => return Err(e),
            }
        }
        let page = page_size();
        for offset in (0..self.mapped_len).step_by(page) {
            // SAFETY: the mapping is ours and offset is strictly within its initialized range.
            unsafe { self.ptr.as_ptr().add(offset).write_volatile(0) };
        }
        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn collapse_uncovered(&self) -> Result<(), Error> {
        const MADV_COLLAPSE_VALUE: libc::c_int = 25;
        for offset in (0..self.mapped_len).step_by(HUGE) {
            let extent = (self.mapped_len - offset).min(HUGE);
            if extent == HUGE {
                // SAFETY: each extent is an owned 2 MiB-aligned subregion of this mapping.
                let result = unsafe {
                    libc::madvise(
                        self.ptr.as_ptr().add(offset).cast(),
                        extent,
                        MADV_COLLAPSE_VALUE,
                    )
                };
                if result != 0 {
                    let _ = io::Error::last_os_error();
                }
            }
        }
        Ok(())
    }

    pub(crate) fn ptr(&self) -> *mut u8 {
        self.ptr.as_ptr()
    }
    pub(crate) fn len(&self) -> usize {
        self.len
    }
}

impl Drop for Mapping {
    fn drop(&mut self) {
        // SAFETY: this region is ours, page-aligned, and pointer came from our mmap.
        let _ = unsafe { libc::munmap(self.ptr.as_ptr().cast(), self.mapped_len) };
        if let Some((lo, hi, page)) = self.guard {
            // SAFETY: the guard pages are ours (mprotect'ed subregions of the original mmap).
            let _ = unsafe { libc::munmap(lo as *mut libc::c_void, page) };
            let _ = unsafe { libc::munmap(hi as *mut libc::c_void, page) };
        }
    }
}

fn page_size() -> usize {
    // SAFETY: sysconf has no pointer or allocation invariants.
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

fn round_up(value: usize, alignment: usize) -> Option<usize> {
    value
        .checked_add(alignment - 1)
        .map(|value| value / alignment * alignment)
}
