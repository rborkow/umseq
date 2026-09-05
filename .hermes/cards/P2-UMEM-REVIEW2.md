# Card P2-UMEM-REVIEW2 — second adversarial pass on the `umem` implementation

You reviewed the *design* of this crate earlier (`docs/review-umem-astra.md`); v2 of the design
(`docs/design-umem.md`) addressed your blocking issues. The crate is now implemented at
`crates/umem/` and has been through one orchestrator review. Review the **implementation**
against the v2 design and your own earlier findings. Do NOT write code. Do NOT modify any file
except creating `docs/review-umem-impl-astra.md`.

Read: `docs/design-umem.md`, `docs/review-umem-astra.md`, then every file under `crates/umem/`
(`src/lib.rs`, `src/backing.rs`, `src/smaps.rs`, `src/quarantine.rs`, `tests/*`, `examples/*`).

Verified facts you can rely on: `cargo test` passes on macOS and on the Linux target (DGX
Spark, kernel 6.17, THP=madvise, 32 GiB HugeTLB pool); the ignored THP test passes there;
`thp_probe` shows 100% THP coverage at 4 GiB in ~0.06 s across allocation orders and 100%
HugeTLB via `Allocation::Hugetlb`.

Answer specifically, citing file:line:
1. **Soundness of the ownership model as implemented.** Can safe code obtain a `&[u8]` or
   `&mut [u8]` over a `Backing` that is also referenced by a live `GpuLease` or `Submission`?
   Consider `Arc` cloning paths, `Submission::try_wait`'s `Err((self, _))` return, `Drop`
   ordering, panics inside `Fence::wait`, and `mem::forget` on each type. Are the `unsafe impl
   Send/Sync for Mapping` justified given what `Buf<Rw>`/`Buf<Ro>` promise?
2. **`Submission::drop` blocks on the fence.** The design says this is the one permitted
   blocking wait and it must never run from a completion handler. Is there any path where it
   can? Is blocking in `Drop` during unwinding a problem?
3. **Linux allocation path** (`backing.rs`): alignment math, guard-page reasoning, error
   cleanup (is every early return leak-free and double-free-free?), `MADV_POPULATE_WRITE`
   fallback, `MADV_COLLAPSE` loop, the `require_huge` → HugeTLB fallback with a rounded length
   but the caller's logical length. Anything that breaks at `len` = 1 byte, `len` = exactly
   2 MiB, `len` = 2 MiB + 1, or `len` near `isize::MAX`?
4. **smaps parser** (`smaps.rs`): exact-bounds match — can the kernel report our region as two
   VMAs (e.g. after partial `MADV_COLLAPSE` or `mprotect`)? Is refusing to report in that case
   the right behavior, and does the caller handle it?
5. **`map_file_unchecked`**: the safety contract is stated; is it sufficient? What about
   `MAP_PRIVATE` + page-cache eviction + re-read after the file changed?
6. **Tests**: which of your T1–T6 are actually covered, which are claimed but weak, and what
   is the single most valuable missing test?
7. **API ergonomics for `umgpu`**: `Submission` owns exactly one lease. Real kernels take
   several buffers (input records, output records, index). Does the one-lease design force
   `umgpu` into something unsound or awkward? Propose the minimal change.

Output: `docs/review-umem-impl-astra.md`, structured as Blocking / Should fix / Nits /
Answers 1–7 / Suggested tests. Under ~200 lines. No preamble.
