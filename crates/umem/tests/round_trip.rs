use umem::{Allocation, AnyBuf, Buf, Context, ContextId, Fence, FenceError, Rw, Submission};

struct Ready;
impl Fence for Ready {
    fn wait(&mut self) -> Result<(), FenceError> {
        Ok(())
    }
    fn try_wait(&mut self) -> Result<bool, FenceError> {
        Ok(true)
    }
}

#[test]
fn cpu_visibility_round_trip() {
    let mut buffer = Buf::<Rw>::allocate(
        16384,
        Allocation::Anon {
            huge: false,
            require_huge: false,
        },
    )
    .unwrap();
    for (index, byte) in buffer.as_mut_slice().iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(31);
    }
    let ctx = Context::new(ContextId(7));
    let lease = buffer.lease(&ctx);
    // A fake backend reads the CPU-written pattern and overwrites it, through the lease pointer.
    // SAFETY: this fake backend owns the lease and touches only its 16 KiB allocation.
    unsafe {
        let p = lease.as_ptr();
        for index in 0..16384usize {
            assert_eq!(*p.add(index), (index as u8).wrapping_mul(31));
        }
        std::ptr::write_bytes(p, 0xa5, 16384);
    }
    // SAFETY: the fake fence covers the (synchronous) fake work above.
    let sub = unsafe { Submission::new(&ctx, vec![lease.erase()], Ready) }
        .ok()
        .unwrap();
    let mut out = sub.wait().unwrap();
    let AnyBuf::Rw(buffer) = out.remove(0) else {
        panic!("expected Rw back")
    };
    for (index, byte) in buffer.as_slice().iter().enumerate() {
        assert_eq!(*byte, 0xa5, "byte {index}");
    }
}
