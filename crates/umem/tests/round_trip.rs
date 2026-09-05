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

#[test]
fn pod_view_and_arena() {
    use umem::{Arena, Pod};
    #[repr(C)]
    #[derive(Clone, Copy, Debug, PartialEq, Default)]
    struct Rec {
        tid: i32,
        pos: i32,
        off: u64,
    }
    // SAFETY: repr(C), integers only, no padding (4+4+8 = 16, align 8).
    unsafe impl Pod for Rec {}

    let alloc = Allocation::Anon {
        huge: false,
        require_huge: false,
    };
    let mut table = Buf::<Rw>::allocate(16 * 3, alloc).unwrap();
    let recs = table.as_pod_mut_slice::<Rec>();
    assert_eq!(recs.len(), 3);
    recs[1] = Rec {
        tid: 7,
        pos: 42,
        off: 99,
    };
    assert_eq!(
        table.as_pod_slice::<Rec>()[1],
        Rec {
            tid: 7,
            pos: 42,
            off: 99
        }
    );
    assert_eq!(&table.as_slice()[16..20], &7i32.to_le_bytes());

    let mut arena = Arena::new(Buf::<Rw>::allocate(64, alloc).unwrap());
    let a = arena.push(b"hello").unwrap();
    let b = arena.push(b"world!!").unwrap();
    assert_eq!(arena.get(a), b"hello");
    assert_eq!(arena.get(b), b"world!!");
    assert_eq!(arena.used(), 12);
    assert!(arena.push(&[0u8; 60]).is_none(), "must not overflow");
    let (buf, used) = arena.into_buf();
    assert_eq!(used, 12);
    assert_eq!(&buf.as_slice()[..5], b"hello");
}
