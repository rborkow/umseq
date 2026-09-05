//! Reproduce the `require_huge` → HugeTLB fallback seen in `thp_probe`.
use umem::{Allocation, Buf, Rw};

fn main() {
    let len: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(4096)
        << 20;
    let order = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "plain,require".into());
    for step in order.split(',') {
        let alloc = match step {
            "require" => Allocation::Anon {
                huge: true,
                require_huge: true,
            },
            "plain" => Allocation::Anon {
                huge: true,
                require_huge: false,
            },
            _ => Allocation::Anon {
                huge: false,
                require_huge: false,
            },
        };
        match Buf::<Rw>::allocate(len, alloc) {
            Ok(b) => {
                let r = b.page_report();
                println!(
                    "{step:<8} {:>6.1}% {:?}",
                    100.0 * r.huge_bytes as f64 / r.len as f64,
                    r.kind
                );
                std::mem::forget(b); // keep it alive, like a real index would be
            }
            Err(e) => println!("{step:<8} ERR {e}"),
        }
    }
}
