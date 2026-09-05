use criterion::{Criterion, criterion_group, criterion_main};
use umem::{Allocation, Buf, Rw};

fn allocation(c: &mut Criterion) {
    let mut group = c.benchmark_group("alloc_populate_verify_1gib");
    for (name, allocation) in [
        (
            "anon_huge",
            Allocation::Anon {
                huge: true,
                require_huge: false,
            },
        ),
        ("hugetlb", Allocation::Hugetlb),
    ] {
        group.bench_function(name, |b| {
            b.iter(|| {
                let buffer = Buf::<Rw>::allocate(1024 * 1024 * 1024, allocation);
                if let Ok(buffer) = buffer {
                    std::hint::black_box(buffer.page_report());
                }
            })
        });
    }
    group.finish();
}

criterion_group!(benches, allocation);
criterion_main!(benches);
