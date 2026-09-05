#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]
//! OS-backed unified-memory buffers with exclusive CPU/GPU ownership transfer.

mod backing;
mod quarantine;
mod smaps;

use std::{cell::Cell, marker::PhantomData, path::Path, sync::Arc};

pub use backing::{Allocation, Error};
pub use smaps::{PageKind, PageReport};

/// A type-level CPU access mode.
pub trait Mode: private::Sealed + Send + 'static {
    /// Runtime tag for this mode.
    const MODE: ModeTag;
}

mod private {
    pub trait Sealed {}
}

/// Read-only CPU access mode.
pub struct Ro;
impl private::Sealed for Ro {}
impl Mode for Ro {
    const MODE: ModeTag = ModeTag::Ro;
}

/// Read-write CPU access mode.
pub struct Rw(PhantomData<Cell<()>>);
impl private::Sealed for Rw {}
impl Mode for Rw {
    const MODE: ModeTag = ModeTag::Rw;
}

/// Identifies a backend device context.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ContextId(pub u64);

/// A backend context used to bind a lease to one device.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Context {
    id: ContextId,
}

impl Context {
    /// Creates a context wrapper for a backend-owned identifier.
    pub const fn new(id: ContextId) -> Self {
        Self { id }
    }
    /// Returns the backend-owned context identifier.
    pub const fn id(self) -> ContextId {
        self.id
    }
}

/// An opaque backend completion primitive.
pub trait Fence: Send {
    /// Blocks until the associated GPU work has completed or failed.
    fn wait(&mut self) -> Result<(), FenceError>;
    /// Reports completion without blocking.
    fn try_wait(&mut self) -> Result<bool, FenceError>;
}

/// A device-side completion failure.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("GPU fence failed: {message}")]
pub struct FenceError {
    /// Backend-provided failure description.
    pub message: String,
}

/// Allocation storage shared by a CPU owner or an in-flight submission.
pub struct Backing {
    mapping: backing::Mapping,
    report: PageReport,
}

impl Backing {
    fn new(len: usize, allocation: Allocation) -> Result<Self, Error> {
        let (mapping, report) = backing::Mapping::allocate(len, allocation)?;
        Ok(Self { mapping, report })
    }

    /// Returns the backend-only raw mapping address.
    ///
    /// # Safety
    /// The caller must be `umgpu` while building a submission and must uphold the kernel
    /// wrapper contracts described by this crate's ownership model.
    pub unsafe fn as_ptr(&self) -> *mut u8 {
        // SAFETY: the caller upholds the design's submission and kernel-wrapper invariant.
        self.mapping.ptr()
    }
}

/// Owned allocation. While held, only the CPU may access its bytes.
pub struct Buf<M: Mode> {
    inner: Arc<Backing>,
    _mode: PhantomData<M>,
}

impl<M: Mode> Buf<M> {
    /// Allocates anonymous memory according to `allocation`.
    pub fn allocate(len: usize, allocation: Allocation) -> Result<Self, Error> {
        Ok(Self {
            inner: Arc::new(Backing::new(len, allocation)?),
            _mode: PhantomData,
        })
    }
    /// Returns the addressable byte length.
    pub fn len(&self) -> usize {
        self.inner.mapping.len()
    }
    /// Returns whether this buffer has no bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Returns the CPU-readable byte slice.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: Buf exclusively owns CPU access, and mapping lives through the returned borrow.
        unsafe { std::slice::from_raw_parts(self.inner.mapping.ptr(), self.len()) }
    }
    /// Returns the timestamped page-backing observation made at allocation time.
    pub fn page_report(&self) -> PageReport {
        self.inner.report.clone()
    }
    /// Transfers exclusive ownership to a GPU context.
    pub fn lease(self, ctx: &Context) -> GpuLease<M> {
        GpuLease {
            inner: self.inner,
            ctx: ctx.id,
            _mode: PhantomData,
        }
    }
}

/// Types that are valid for any bit pattern and have no padding-dependent invariants, so a
/// `[u8]` region can be viewed as `[T]`. Implement only for `#[repr(C)]` types made of
/// integers/floats with no padding bytes that must be initialized (padding is zero-filled here).
///
/// # Safety
/// `Self` must be `#[repr(C)]` or `#[repr(transparent)]`, contain no references, pointers,
/// `bool`, `char`, enums, or other types with invalid bit patterns, and every byte of a valid
/// value must be a plain integer/float byte.
pub unsafe trait Pod: Copy + 'static {}
// SAFETY: primitive integers/floats are valid for all bit patterns.
unsafe impl Pod for u8 {}
// SAFETY: as above.
unsafe impl Pod for u16 {}
// SAFETY: as above.
unsafe impl Pod for u32 {}
// SAFETY: as above.
unsafe impl Pod for u64 {}
// SAFETY: as above.
unsafe impl Pod for i8 {}
// SAFETY: as above.
unsafe impl Pod for i16 {}
// SAFETY: as above.
unsafe impl Pod for i32 {}
// SAFETY: as above.
unsafe impl Pod for i64 {}
// SAFETY: as above.
unsafe impl Pod for f32 {}
// SAFETY: as above.
unsafe impl Pod for f64 {}

impl<M: Mode> Buf<M> {
    /// Views the buffer as a slice of `T`. Requires the mapping base to be aligned for `T`
    /// (always true: mappings are page-aligned) and returns the largest whole-`T` prefix.
    pub fn as_pod_slice<T: Pod>(&self) -> &[T] {
        let n = self.len() / std::mem::size_of::<T>();
        // SAFETY: T: Pod ⇒ any bytes are a valid T; base is page-aligned ⇒ aligned for T; the
        // CPU exclusively owns this Buf so no GPU write can race; length is a whole-T prefix.
        unsafe { std::slice::from_raw_parts(self.inner.mapping.ptr().cast::<T>(), n) }
    }
}

impl Buf<Rw> {
    /// Mutable typed view; see [`Buf::as_pod_slice`].
    pub fn as_pod_mut_slice<T: Pod>(&mut self) -> &mut [T] {
        let n = self.len() / std::mem::size_of::<T>();
        // SAFETY: as `as_pod_slice`, plus `&mut self` proves exclusive CPU access.
        unsafe { std::slice::from_raw_parts_mut(self.inner.mapping.ptr().cast::<T>(), n) }
    }

    /// Returns the CPU-writable byte slice.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: &mut Buf proves exclusive CPU access, and mapping lives through the borrow.
        unsafe { std::slice::from_raw_parts_mut(self.inner.mapping.ptr(), self.len()) }
    }
    /// Converts this buffer into a read-only CPU buffer.
    pub fn freeze(self) -> Buf<Ro> {
        Buf {
            inner: self.inner,
            _mode: PhantomData,
        }
    }
}

impl Buf<Ro> {
    /// Maps a file read-only without copying it.
    ///
    /// `MAP_PRIVATE` + `PROT_READ` is **not** a snapshot: clean pages can be evicted and
    /// re-read from the file, and visibility of later file changes is unspecified. The safe
    /// alternative is to read the file into an owned anonymous buffer.
    ///
    /// # Safety
    /// The caller guarantees that the file's contents, size, and readable backing are stable
    /// — no writes, truncation, hole-punching, or other invalidation by **any** writer in any
    /// process or thread (including this one, via other fds or mappings) — from before this
    /// call until the last of: the returned `Buf`, every lease derived from it, every
    /// `Submission` holding such a lease, every `Buf` returned from those submissions, and any
    /// outstanding (including forgotten) device work referencing the mapping.
    pub unsafe fn map_file_unchecked(_path: impl AsRef<Path>) -> Result<Self, Error> {
        // SAFETY: the caller upholds the design's no-external-writer contract for this inode.
        let (mapping, report) = unsafe { backing::Mapping::map_file(_path) }?;
        Ok(Self {
            inner: Arc::new(Backing { mapping, report }),
            _mode: PhantomData,
        })
    }
}

/// A buffer handed to a GPU backend. It is not a pointer and is not cloneable.
pub struct GpuLease<M: Mode> {
    inner: Arc<Backing>,
    ctx: ContextId,
    _mode: PhantomData<(M, Cell<()>)>,
}

impl<M: Mode> GpuLease<M> {
    /// Returns the context to which this lease is bound.
    pub const fn context(&self) -> ContextId {
        self.ctx
    }
    /// Returns the addressable byte length, for wrapper bounds checks.
    pub fn len(&self) -> usize {
        self.inner.mapping.len()
    }
    /// Returns whether this lease has no bytes.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Returns the backend-only raw mapping address for encoding reviewed GPU work.
    ///
    /// # Safety
    /// The caller must uphold the kernel-wrapper contract: bounds, mode permissions,
    /// initialization, and absence of GPU races. The pointer must not outlive this lease's
    /// submission, and every device use of it must be covered by the fence that submission
    /// is built with.
    pub unsafe fn as_ptr(&self) -> *mut u8 {
        // SAFETY: the caller upholds the design's submission and kernel-wrapper invariant.
        unsafe { self.inner.as_ptr() }
    }
    /// Erases the mode so leases of mixed modes can share one submission.
    pub fn erase(self) -> AnyLease {
        AnyLease {
            inner: self.inner,
            ctx: self.ctx,
            mode: M::MODE,
            _not_sync: PhantomData,
        }
    }
}

/// Runtime tag for an erased mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeTag {
    /// Read-only.
    Ro,
    /// Read-write.
    Rw,
}

/// A lease with its mode erased. Only produced by [`GpuLease::erase`].
pub struct AnyLease {
    inner: Arc<Backing>,
    ctx: ContextId,
    mode: ModeTag,
    _not_sync: PhantomData<Cell<()>>,
}

impl AnyLease {
    /// The context this lease is bound to.
    pub const fn context(&self) -> ContextId {
        self.ctx
    }
    /// The erased mode.
    pub const fn mode(&self) -> ModeTag {
        self.mode
    }
    /// Byte length.
    pub fn len(&self) -> usize {
        self.inner.mapping.len()
    }
    /// Whether the lease is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Raw pointer; see [`GpuLease::as_ptr`] for the contract.
    ///
    /// # Safety
    /// Same as [`GpuLease::as_ptr`].
    pub unsafe fn as_ptr(&self) -> *mut u8 {
        // SAFETY: the caller upholds the design's submission and kernel-wrapper invariant.
        unsafe { self.inner.as_ptr() }
    }
}

/// A CPU-owned buffer with its mode erased; returned by [`Submission::wait`].
pub enum AnyBuf {
    /// Read-only.
    Ro(Buf<Ro>),
    /// Read-write.
    Rw(Buf<Rw>),
}

impl AnyBuf {
    fn from_lease(lease: AnyLease) -> Self {
        match lease.mode {
            ModeTag::Ro => AnyBuf::Ro(Buf {
                inner: lease.inner,
                _mode: PhantomData,
            }),
            ModeTag::Rw => AnyBuf::Rw(Buf {
                inner: lease.inner,
                _mode: PhantomData,
            }),
        }
    }
}

/// Holds the leased storage for the whole life of a submission so that no panic in backend
/// code can run `Backing::drop` before completion has been observed.
///
/// Field order matters: `fence` is declared *after* `guard`, and Rust drops fields in
/// declaration order, so on unwinding the guard's `Drop` runs first and quarantines.
struct RetentionGuard {
    leases: Vec<AnyLease>,
    /// Set to `true` only after a successful `wait`; anything else quarantines.
    completed: bool,
}

impl Drop for RetentionGuard {
    fn drop(&mut self) {
        if !self.completed {
            for lease in self.leases.drain(..) {
                quarantine::retain(lease.inner);
            }
        }
    }
}

/// An in-flight GPU submission that owns one or more leased buffers and one fence.
///
/// Invariants:
/// - While this value exists, every leased `Backing` is held by `guard` and cannot be freed.
/// - Ownership returns to the CPU **only** through [`Submission::wait`] / [`Submission::try_wait`]
///   observing fence completion. Forgetting a submission leaks; it never frees or returns access.
/// - Dropping a pending submission blocks on the fence. **Do not drop a `Submission` inside a
///   backend completion callback** — the wait may depend on that callback returning.
/// - If the fence fails, panics, or its destructor panics, the storage is quarantined, never freed.
pub struct Submission<F: Fence> {
    guard: RetentionGuard,
    ctx: ContextId,
    fence: Option<F>,
    terminal: Option<FenceError>,
}

/// Result of a non-blocking poll.
pub enum Poll<F: Fence> {
    /// Complete; ownership returned.
    Ready(Vec<AnyBuf>),
    /// Not yet complete; the submission is handed back unchanged.
    Pending(Submission<F>),
    /// The device reported failure; storage is quarantined and the submission is spent.
    Failed(FenceError),
}

impl<F: Fence> Submission<F> {
    /// Builds a submission from leases that all belong to `ctx` and a fence that covers every
    /// device use of every lease.
    ///
    /// # Safety
    /// The caller (a `umgpu` backend) guarantees that `fence` completes only after all GPU work
    /// touching any of `leases` has completed on every queue, and that no device work references
    /// the leases' memory after that point.
    pub unsafe fn new(
        ctx: &Context,
        leases: Vec<AnyLease>,
        fence: F,
    ) -> Result<Self, (Vec<AnyLease>, F, ContextMismatch)> {
        if let Some(bad) = leases.iter().find(|l| l.ctx != ctx.id) {
            let mismatch = ContextMismatch {
                expected: ctx.id,
                found: bad.ctx,
            };
            return Err((leases, fence, mismatch));
        }
        Ok(Self {
            guard: RetentionGuard {
                leases,
                completed: false,
            },
            ctx: ctx.id,
            fence: Some(fence),
            terminal: None,
        })
    }

    /// The context every lease in this submission belongs to.
    pub const fn context(&self) -> ContextId {
        self.ctx
    }

    /// Blocks for completion and returns CPU ownership of every buffer on success.
    pub fn wait(mut self) -> Result<Vec<AnyBuf>, FenceError> {
        if let Some(e) = self.terminal.take() {
            return Err(e);
        }
        let Some(fence) = self.fence.as_mut() else {
            return Err(FenceError {
                message: "submission already spent".into(),
            });
        };
        match fence.wait() {
            Ok(()) => Ok(self.release()),
            Err(e) => {
                // guard.completed stays false → Drop quarantines.
                self.terminal = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Checks completion without blocking.
    pub fn try_wait(mut self) -> Poll<F> {
        if let Some(e) = self.terminal.take() {
            return Poll::Failed(e);
        }
        let Some(fence) = self.fence.as_mut() else {
            return Poll::Failed(FenceError {
                message: "submission already spent".into(),
            });
        };
        match fence.try_wait() {
            Ok(true) => Poll::Ready(self.release()),
            Ok(false) => Poll::Pending(self),
            Err(e) => {
                self.terminal = Some(e.clone());
                Poll::Failed(e)
            }
        }
    }

    /// Only called after the fence reported success.
    fn release(&mut self) -> Vec<AnyBuf> {
        self.guard.completed = true;
        // Drop the fence first: if its destructor panics, `guard.completed` is already true and
        // the leases are still held by `guard`, which will then be dropped normally (not
        // quarantined) — that is correct, completion *was* observed.
        self.fence = None;
        self.guard
            .leases
            .drain(..)
            .map(AnyBuf::from_lease)
            .collect()
    }
}

impl<F: Fence> Drop for Submission<F> {
    fn drop(&mut self) {
        if self.guard.completed || self.terminal.is_some() {
            return; // already resolved; guard handles the rest
        }
        if let Some(fence) = self.fence.as_mut()
            && fence.wait().is_ok()
        {
            self.guard.completed = true;
        }
        // Any other outcome (Err, no fence, or a panic inside `wait` — which unwinds through
        // here and then drops `guard` with `completed == false`) quarantines.
    }
}

/// A lease was bound to a different context than the submission.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("lease context {found:?} does not match submission context {expected:?}")]
pub struct ContextMismatch {
    /// Context the submission was built for.
    pub expected: ContextId,
    /// Context found on the offending lease.
    pub found: ContextId,
}

/// A bump allocator over a `Buf<Rw>`: append byte payloads, get back `(offset, len)` handles
/// that remain valid for the life of the arena and are meaningful to a GPU consumer of the
/// same buffer. Never reallocates; `push` fails when full. Not thread-safe by design — fill it
/// from one thread (or shard into several arenas) and read it from many.
pub struct Arena {
    buf: Buf<Rw>,
    used: usize,
}

/// A handle into an [`Arena`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    /// Byte offset from the arena base.
    pub offset: u64,
    /// Byte length.
    pub len: u32,
}

impl Arena {
    /// Wraps a buffer; existing contents are ignored and overwritten.
    pub fn new(buf: Buf<Rw>) -> Self {
        Self { buf, used: 0 }
    }
    /// Capacity in bytes.
    pub fn capacity(&self) -> usize {
        self.buf.len()
    }
    /// Bytes used so far.
    pub fn used(&self) -> usize {
        self.used
    }
    /// Appends `bytes`, returning a handle, or `None` if it would not fit or exceeds `u32`.
    pub fn push(&mut self, bytes: &[u8]) -> Option<Span> {
        let len = u32::try_from(bytes.len()).ok()?;
        let end = self.used.checked_add(bytes.len())?;
        if end > self.buf.len() {
            return None;
        }
        self.buf.as_mut_slice()[self.used..end].copy_from_slice(bytes);
        let span = Span {
            offset: self.used as u64,
            len,
        };
        self.used = end;
        Some(span)
    }
    /// Resolves a handle. Panics on an out-of-range span (a logic error, not user input).
    pub fn get(&self, span: Span) -> &[u8] {
        let start = span.offset as usize;
        &self.buf.as_slice()[start..start + span.len as usize]
    }
    /// Consumes the arena, returning the underlying buffer (and the used length) so it can be
    /// leased to a GPU. Bytes past `used` are unspecified.
    pub fn into_buf(self) -> (Buf<Rw>, usize) {
        (self.buf, self.used)
    }
}

static_assertions::assert_impl_all!(Buf<Ro>: Send, Sync);
static_assertions::assert_impl_all!(Buf<Rw>: Send);
static_assertions::assert_not_impl_any!(Buf<Rw>: Sync);
static_assertions::assert_impl_all!(GpuLease<Ro>: Send);
static_assertions::assert_not_impl_any!(GpuLease<Ro>: Sync);
static_assertions::assert_impl_all!(AnyLease: Send);
static_assertions::assert_not_impl_any!(AnyLease: Sync);
static_assertions::assert_not_impl_any!(AnyLease: Clone);

#[cfg(test)]
mod tests {
    use super::*;

    fn buf() -> Buf<Rw> {
        Buf::<Rw>::allocate(
            4096,
            Allocation::Anon {
                huge: false,
                require_huge: false,
            },
        )
        .unwrap()
    }
    fn ctx() -> Context {
        Context::new(ContextId(1))
    }

    struct Never;
    impl Fence for Never {
        fn wait(&mut self) -> Result<(), FenceError> {
            Err(FenceError {
                message: "never".into(),
            })
        }
        fn try_wait(&mut self) -> Result<bool, FenceError> {
            Ok(false)
        }
    }
    struct Done;
    impl Fence for Done {
        fn wait(&mut self) -> Result<(), FenceError> {
            Ok(())
        }
        fn try_wait(&mut self) -> Result<bool, FenceError> {
            Ok(true)
        }
    }
    struct PanicsOnWait;
    impl Fence for PanicsOnWait {
        fn wait(&mut self) -> Result<(), FenceError> {
            panic!("backend exploded");
        }
        fn try_wait(&mut self) -> Result<bool, FenceError> {
            Ok(false)
        }
    }
    struct PanicsOnDrop;
    impl Fence for PanicsOnDrop {
        fn wait(&mut self) -> Result<(), FenceError> {
            Err(FenceError {
                message: "failed".into(),
            })
        }
        fn try_wait(&mut self) -> Result<bool, FenceError> {
            Ok(false)
        }
    }
    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            if !std::thread::panicking() {
                panic!("fence destructor exploded");
            }
        }
    }

    fn submit<F: Fence>(leases: Vec<AnyLease>, f: F) -> Submission<F> {
        // SAFETY: test fences do no device work.
        unsafe { Submission::new(&ctx(), leases, f) }.ok().unwrap()
    }

    #[test]
    fn modes_have_required_thread_traits() {
        static_assertions::assert_impl_all!(Buf<Ro>: Send, Sync);
        static_assertions::assert_impl_all!(Buf<Rw>: Send);
        static_assertions::assert_not_impl_any!(Buf<Rw>: Sync);
    }

    #[test]
    fn forgotten_submission_keeps_its_backing_alive() {
        let lease = buf().lease(&ctx());
        let weak = Arc::downgrade(&lease.inner);
        std::mem::forget(submit(vec![lease.erase()], Never));
        assert!(weak.upgrade().is_some());
    }

    #[test]
    fn failed_drop_quarantines_storage() {
        let before = quarantine::len();
        let lease = buf().lease(&ctx());
        let weak = Arc::downgrade(&lease.inner);
        drop(submit(vec![lease.erase()], Never));
        assert!(quarantine::len() > before);
        assert!(
            weak.upgrade().is_some(),
            "quarantined storage must stay alive"
        );
    }

    #[test]
    fn successful_wait_returns_every_buffer_with_mode() {
        let a = buf().lease(&ctx()).erase();
        let b = buf().freeze().lease(&ctx()).erase();
        let out = submit(vec![a, b], Done).wait().unwrap();
        assert!(matches!(out[0], AnyBuf::Rw(_)));
        assert!(matches!(out[1], AnyBuf::Ro(_)));
    }

    #[test]
    fn panic_in_fence_wait_during_drop_quarantines_not_frees() {
        let before = quarantine::len();
        let lease = buf().lease(&ctx());
        let weak = Arc::downgrade(&lease.inner);
        let sub = submit(vec![lease.erase()], PanicsOnWait);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(sub)));
        assert!(r.is_err(), "panic must propagate");
        assert!(weak.upgrade().is_some(), "storage freed during unwind (B1)");
        assert!(quarantine::len() > before, "not quarantined");
    }

    #[test]
    fn panic_in_fence_destructor_after_failure_still_quarantines() {
        let before = quarantine::len();
        let lease = buf().lease(&ctx());
        let weak = Arc::downgrade(&lease.inner);
        let sub = submit(vec![lease.erase()], PanicsOnDrop);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _ = sub.wait();
        }));
        assert!(r.is_err());
        assert!(weak.upgrade().is_some(), "storage freed during unwind (B1)");
        assert!(quarantine::len() > before, "not quarantined");
    }

    #[test]
    fn terminal_error_is_reported_not_panicked() {
        struct FailsPoll;
        impl Fence for FailsPoll {
            fn wait(&mut self) -> Result<(), FenceError> {
                Ok(())
            }
            fn try_wait(&mut self) -> Result<bool, FenceError> {
                Err(FenceError {
                    message: "device lost".into(),
                })
            }
        }
        match submit(vec![buf().lease(&ctx()).erase()], FailsPoll).try_wait() {
            Poll::Failed(e) => assert_eq!(e.message, "device lost"),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn pending_poll_hands_submission_back() {
        let sub = submit(vec![buf().lease(&ctx()).erase()], Never);
        match sub.try_wait() {
            Poll::Pending(sub) => {
                // dropping a pending submission with a failing fence quarantines
                let before = quarantine::len();
                drop(sub);
                assert!(quarantine::len() > before);
            }
            _ => panic!("expected Pending"),
        }
    }

    #[test]
    fn context_mismatch_is_rejected_and_leases_returned() {
        let lease = buf().lease(&Context::new(ContextId(99))).erase();
        // SAFETY: test fence does no device work.
        let r = unsafe { Submission::new(&ctx(), vec![lease], Done) };
        let (leases, _f, e) = r.err().unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(e.found, ContextId(99));
    }
}
