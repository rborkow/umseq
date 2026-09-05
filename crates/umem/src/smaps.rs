//! Page-backing observations.

use std::time::SystemTime;

/// The observed page backing kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageKind {
    /// Transparent huge pages.
    Thp,
    /// Explicit HugeTLB pages.
    Hugetlb,
    /// Ordinary base pages.
    Small,
}

/// A timestamped observation of a mapped region's page backing.
#[derive(Clone, Debug)]
pub struct PageReport {
    /// Addressable byte length.
    pub len: usize,
    /// Bytes reported as huge-page backed.
    pub huge_bytes: usize,
    /// Observed backing kind.
    pub kind: PageKind,
    /// Time at which this observation was made.
    pub observed_at: SystemTime,
    /// True when a `require_huge` request could not be satisfied with THP and the reserved
    /// HugeTLB pool was used instead.
    pub fallback: bool,
}

#[cfg(target_os = "linux")]
pub(crate) fn report(ptr: *const u8, len: usize) -> std::io::Result<PageReport> {
    let text = std::fs::read_to_string("/proc/self/smaps")?;
    let lo = ptr as usize;
    let hi = lo
        .checked_add(len)
        .ok_or_else(|| std::io::Error::other("range overflow"))?;
    // Walk VMAs in address order. Our region must be covered by an exact, contiguous tiling of
    // VMAs that each lie wholly inside [lo, hi). A VMA that overlaps the boundary means the
    // kernel merged us with a neighbour and its counters are not attributable.
    let mut cursor = lo;
    let mut anon_huge = 0usize;
    let mut hugetlb = 0usize;
    let mut file_pmd = 0usize;
    let mut in_ours = false;
    for line in text.lines() {
        if let Some((range, _)) = line.split_once(' ')
            && let Some((a, b)) = range.split_once('-')
            && let (Ok(start), Ok(end)) =
                (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16))
        {
            in_ours = false;
            if end <= lo || start >= hi {
                continue;
            }
            if start < lo || end > hi {
                return Err(std::io::Error::other(format!(
                    "mapping {lo:#x}-{hi:#x} overlaps merged VMA {start:#x}-{end:#x}; \
                     per-range huge-page coverage cannot be attributed"
                )));
            }
            if start != cursor {
                return Err(std::io::Error::other(format!(
                    "gap in VMA tiling of {lo:#x}-{hi:#x} at {cursor:#x}"
                )));
            }
            cursor = end;
            in_ours = true;
            continue;
        }
        if !in_ours {
            continue;
        }
        if let Some(v) = line.strip_prefix("AnonHugePages:") {
            anon_huge = anon_huge.saturating_add(kib(v)?);
        } else if let Some(v) = line.strip_prefix("Private_Hugetlb:") {
            hugetlb = hugetlb.saturating_add(kib(v)?);
        } else if let Some(v) = line.strip_prefix("Shared_Hugetlb:") {
            hugetlb = hugetlb.saturating_add(kib(v)?);
        } else if let Some(v) = line.strip_prefix("FilePmdMapped:") {
            file_pmd = file_pmd.saturating_add(kib(v)?);
        }
    }
    if cursor != hi {
        return Err(std::io::Error::other(format!(
            "no VMA tiling covers {lo:#x}-{hi:#x} (reached {cursor:#x})"
        )));
    }
    let (huge_bytes, kind) = if hugetlb != 0 {
        (hugetlb, PageKind::Hugetlb)
    } else if anon_huge != 0 {
        (anon_huge, PageKind::Thp)
    } else if file_pmd != 0 {
        (file_pmd, PageKind::Thp)
    } else {
        (0, PageKind::Small)
    };
    Ok(PageReport {
        len,
        huge_bytes,
        kind,
        observed_at: SystemTime::now(),
        fallback: false,
    })
}

#[cfg(target_os = "linux")]
fn kib(value: &str) -> std::io::Result<usize> {
    value
        .split_whitespace()
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .and_then(|v| v.checked_mul(1024))
        .ok_or_else(|| std::io::Error::other(format!("malformed smaps counter: {value:?}")))
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn report(_ptr: *const u8, len: usize) -> std::io::Result<PageReport> {
    Ok(PageReport {
        len,
        huge_bytes: 0,
        kind: PageKind::Small,
        observed_at: SystemTime::now(),
        fallback: false,
    })
}
