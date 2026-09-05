#![cfg(target_os = "linux")]

use umem::{Allocation, Buf, PageKind, Rw};

#[test]
#[ignore]
fn thp_coverage_is_complete_when_requested() {
    if std::env::var_os("UMEM_THP_TEST").as_deref() != Some(std::ffi::OsStr::new("1")) {
        return;
    }
    let buffer = Buf::<Rw>::allocate(
        32 * 1024 * 1024,
        Allocation::Anon {
            huge: true,
            require_huge: true,
        },
    )
    .unwrap();
    let report = buffer.page_report();
    assert_eq!(report.kind, PageKind::Thp);
    assert_eq!(report.huge_bytes, buffer.len());
}
