use umem::{Allocation, Buf, Context, ContextId, Rw};

fn main() {
    let buffer = Buf::<Rw>::allocate(4096, Allocation::Anon { huge: false, require_huge: false }).unwrap();
    let lease = buffer.lease(&Context::new(ContextId(1)));
    let _slice = buffer.as_slice();
    drop(lease);
}
