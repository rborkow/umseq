//! Allocate N MiB with `require_huge`, print the page report, time it.
use std::time::Instant;
use umem::{Allocation, Buf, Rw};

fn main() {
    let mib: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4096);
    let len = mib << 20;
    for (label, alloc) in [
        (
            "Anon{huge,require}",
            Allocation::Anon {
                huge: true,
                require_huge: true,
            },
        ),
        (
            "Anon{huge}",
            Allocation::Anon {
                huge: true,
                require_huge: false,
            },
        ),
        (
            "Anon{small}",
            Allocation::Anon {
                huge: false,
                require_huge: false,
            },
        ),
        ("Hugetlb", Allocation::Hugetlb),
    ] {
        let t = Instant::now();
        match Buf::<Rw>::allocate(len, alloc) {
            Ok(mut b) => {
                b.as_mut_slice()[len - 1] = 7;
                let r = b.page_report();
                println!(
                    "{label:<22} {mib:>6} MiB  {:>6.1}% {:?}  {:.2}s",
                    100.0 * r.huge_bytes as f64 / r.len as f64,
                    r.kind,
                    t.elapsed().as_secs_f64()
                );
            }
            Err(e) => println!("{label:<22} {mib:>6} MiB  ERR {e}"),
        }
    }
}
