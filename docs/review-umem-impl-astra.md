## Blocking

- **B1 — Panics can free storage before completion.** `Submission::drop` calls arbitrary
  `Fence::wait` before protecting the lease (`crates/umem/src/lib.rs:255–264`). If that call
  panics during an ordinary drop, unwinding drops the fields, including the lease's last Arc;
  `Mapping::drop` then unmaps (`crates/umem/src/backing.rs:379–383`). There is no completion
  proof and no quarantine. A second route is `quarantine`: it drops the fence **before**
  retaining storage (`crates/umem/src/lib.rs:247–251`). A panicking fence destructor leaves
  `fence=None`, so subsequent Submission cleanup releases the unquarantined lease.
  This violates design lines 76–79 and the earlier review's blocking issue 3.
  Establish a panic-safe retention guard before invoking backend code; uncertain completion
  must retain/leak storage even if waiting, fence destruction, or quarantine insertion panics.
  Dispose of backend resources only after securing that retention. Rust drops struct fields
  in declaration order; the lease precedes the fence here (`crates/umem/src/lib.rs:197–200`;
  [Rust destructor rules](https://doc.rust-lang.org/reference/destructors.html)).

- **B2 — The file safety contract excludes too few writers and ends too early.** It forbids
  only writes by “other process[es]” for the returned buffer's lifetime
  (`crates/umem/src/lib.rs:150–155`). Same-process threads/fds/mappings can still change or
  truncate the inode using safe I/O. Consuming the Buf into a lease also does not end the
  required immutability period. Require stable contents, size, and valid readable backing
  from before metadata/mapping through every derived lease, submission, returned Buf, and
  outstanding device use, regardless of writer identity. Cover truncation, hole punching,
  and other invalidation as well as ordinary writes. Forgotten live GPU work does not end
  this obligation. This is the earlier review's blocking issue 4, only partially addressed.

## Should fix

- **S1 — Every non-page-multiple file fails on Linux.** `map_file` passes logical EOF length
  directly to exact-bounds reporting (`crates/umem/src/backing.rs:100–128`), but the VMA ends
  at a page boundary. The parser diagnoses this ordinary padding as a merged VMA
  (`crates/umem/src/smaps.rs:49–57`). Track rounded mapped length separately; keep CPU/kernel
  bounds at logical EOF. Page-aligned file mappings can also merge and need explicit handling.
- **S2 — A terminal `try_wait` error returns an unusable Submission.** It quarantines and
  clears both fields, then returns `Err((self, error))` (`crates/umem/src/lib.rs:233–250`).
  Retrying `wait` or `try_wait` panics at the fence `expect` (lines 208, 223). Distinguish
  `Pending(Self)` from terminal failure, or retain an explicit terminal state with stable
  error behavior. Never make an ordinary device error a retry-triggered panic.
- **S3 — Population errors are indiscriminately suppressed.** Any `MADV_POPULATE_WRITE`
  failure triggers actual page writes (`crates/umem/src/backing.rs:333–346`). Unsupported
  advice warrants a compatibility fallback; ENOMEM, EFAULT, and poisoned-page failures do
  not. This can turn a reportable fault into SIGBUS/process failure. Preserve substantive
  errors and permit cleanup/fallback policy to act on them. The syscall explicitly reports
  faults without raising SIGBUS ([madvise](https://man7.org/linux/man-pages/man2/madvise.2.html)).
- **S4 — Callback exclusion and the trusted completion boundary are undocumented/unprotected.**
  Public `submit` accepts any safe `Fence` (`crates/umem/src/lib.rs:52–58,185–193`); the raw
  pointer contract does not explicitly require completion of every use on every queue or
  panic-safe retention between launch and Submission construction (lines 177–180).
  Define those obligations at the unsafe backend boundary, including context identity and
  CPU visibility. An unsafe submission constructor accepting responsibility for the fence's
  association is one option. A lying safe fence alone is not a safe-only UB exploit: safe
  callers cannot launch GPU access. Backend wrappers must not accept untrusted completion
  proofs for already-launched work. Document the callback restriction and enforce it in umgpu.
- **S5 — Unmapped gaps are not reserved guards.** Another allocation can occupy the trimmed
  head/tail and allow VMA merging (`crates/umem/src/backing.rs:168–200`). Exact matching keeps
  reporting conservative, but allocation can fail nondeterministically. Reserve PROT_NONE
  guards with ownership/cleanup covering the reservation if stable isolation is required.
  Verification errors currently propagate as Io and bypass HugeTLB fallback, which only
  catches NotHuge (`crates/umem/src/backing.rs:74–87,210,221`). Distinguish unavailable
  attribution from measured shortfall and explicitly choose whether either warrants fallback.
- **S6 — Restore multi-buffer ownership before umgpu integrates** (answer 7). The implemented
  singleton diverges from `docs/design-umem.md:60–66` and complicates atomic error handling.

## Nits

- `collapse_uncovered` actually visits every full extent and discards every errno
  (`crates/umem/src/backing.rs:350–368`). Rename it and record failures/round counts; whole-VMA
  counters cannot locate uncovered extents. One strict / three best-effort rounds are bounded
  and the stated latency tradeoff is reasonable, but differs from design lines 130–132.
- `huge:false` does not request MADV_NOHUGEPAGE (`crates/umem/src/backing.rs:206–209`); it means
  “no advice,” not a dependable small-page benchmark control. macOS silently ignores
  `require_huge` (lines 239–245), unlike the public “reject” documentation (lines 21–22).
- The allocation benchmark times failed allocations as successful iterations
  (`crates/umem/benches/allocation.rs:18–21`); report/skip unsupported policies explicitly.
  `smaps` ignores FilePmdMapped (`crates/umem/src/smaps.rs:71–89`), so its Small classification
  cannot establish that file-backed translations are small. Malformed counters silently
  become zero and KiB multiplication is unchecked (lines 100–107).

## Answers 1–7

1. **No safe duplication path to CPU slices was found.** Buf and GpuLease are not Clone;
   their Arcs are private, and lease/freeze/take_buf move the Arc rather than clone it
   (`crates/umem/src/lib.rs:92–95,123–128,139–143,239–245`). A live slice prevents consuming
   its Buf. Cloning an outer Arc<Buf> cannot extract its Buf while other owners/borrows live.
   PageReport cloning copies metadata, not storage (line 120). Pending try_wait returns the
   sole owner; terminal failure returns an empty owner, not a CPU alias (S2). Internally,
   quarantine's Arc is never re-exposed as a Buf (`crates/umem/src/quarantine.rs:7–16`).
   Forgetting a Buf, GpuLease, or populated Submission leaks ownership and cannot restore
   CPU access; forgetting an emptied terminal Submission changes nothing. Backing cannot
   be independently constructed/extracted by safe clients. Dropping an unsubmitted lease
   frees it: unsafe backend code must therefore retain it across encoding/launch failures.
   Successful completion removes the fence before returning the Buf; failed completion
   normally quarantines. Panic paths break lifetime safety (B1), not move exclusivity.
   Mapping's Send/Sync implementations (`crates/umem/src/backing.rs:57–62`) are justified
   for this private state machine: Rw's Cell marker excludes Sync, Ro permits shared reads,
   and leases/submissions retain Cell even for Ro (`crates/umem/src/lib.rs:27,167,197–200`).
   They do not excuse fence panics, invalid file contracts, or backend races. No native Metal
   owner/deallocator or CUDA registration exists here; earlier final-release concerns remain
   integration obligations, not implemented guarantees (`crates/umem/src/lib.rs:69–72`).

2. **Yes, it can run in a completion handler.** There are no handlers inside this crate,
   but a Send Submission can be captured by an external callback and dropped there. Dropping
   the Err result of a not-ready `try_wait` also invokes the blocking destructor
   (`crates/umem/src/lib.rs:227–232,255–264`); “non-blocking” describes polling, not disposal.
   The type system does not enforce design lines 76–78. Waiting on a fence whose completion
   depends on that callback can deadlock. Blocking during unwinding is not inherently UB,
   but can deadlock while locks remain held; a second panic aborts. An explicit wait/try_wait
   panic also invokes Drop and can retry the same panicking wait. Prefer conservative
   retention during unwinding and route callback disposal to a safe cleanup context.

3. **Ordinary alignment and ownership accounting are correct on the target.** With 2 MiB H,
   raw page alignment gives `page <= head <= H`; `map_len=len+2H` gives `H <= tail < 2H`
   (`crates/umem/src/backing.rs:137–176`). Both trims are page multiples. The tail-retry
   branch is unreachable under these invariants; its unchecked addition is unnecessary.
   Before Mapping construction, head failure cleans the original reservation, while tail
   failure cleans only the still-owned suffix (lines 191–199). Afterwards `?` and NotHuge
   drop Mapping exactly once, including smaps/advice failures (lines 201–235). No ordinary
   early-return double-free/leak was found, assuming cleanup munmap succeeds; cleanup errors
   are ignored, so absolute leak-freedom under syscall failure is not established. A valid
   address-zero mmap would also panic before RAII ownership (lines 124,276); handle explicitly.
   Population initializes writable anonymous pages; manual fallback touches each page,
   with the other bytes already OS-zeroed. See S3 for failure semantics. Collapse rechecks
   coverage after each bounded pass; it does not assume syscall success proves coverage.
   For huge anonymous requests, logical lengths 1, H, H+1 map H, H, 2H respectively; slices
   retain precisely the requested length. Direct Hugetlb rejects 1 and H+1, accepts H
   (lines 249–251). Strict fallback rounds first, retains rounded mapped_len, and restores
   logical/report length (lines 78–84): no tail exposure or wrong-length HugeTLB unmap.
   With 2 MiB coverage units, reaching logical length implies the entire rounded allocation
   is covered. Partial coverage clamped to logical length is not necessarily the exact huge
   byte count *within* that prefix: some counted bytes can belong to padding (lines 211–223).
   Rejecting zero and logical lengths above isize::MAX is correct (lines 65–67). Rounding
   and slack additions are checked, but rounded/slack lengths can exceed isize::MAX; the
   target kernel rejects such impossible VA requests. Reject them explicitly for deterministic
   InvalidLength errors and to avoid depending on kernel VA limits before pointer arithmetic.
   HugeTLB coverage is inferred from successful explicit allocation/population, not parsed
   from smaps (lines 275–290); that is a valid backing guarantee but a design deviation.

4. **Splitting is possible; partial collapse alone is not a VMA split.** Collapse changes
   page tables inside existing VMAs ([Linux 6.17 collapse implementation](https://raw.githubusercontent.com/torvalds/linux/v6.17/mm/khugepaged.c));
   changing protection on an interior subrange can split a VMA
   ([mprotect](https://man7.org/linux/man-pages/man2/mprotect.2.html)). This crate performs no
   mprotect. Split mappings fail “no exact bounds”; containing merged VMAs fail explicitly
   (`crates/umem/src/smaps.rs:49–57,79–82`). Refusing attribution is conservative and correct;
   an alternative is summing an exact, contiguous tiling of wholly contained VMAs, never
   prorating a larger one. The caller unmaps and returns Io, not silent success or fallback
   (S5). `page_report` is only the allocation-time snapshot, so later splits do not refresh
   it (`crates/umem/src/lib.rs:118–120`). File EOF padding is a separate deterministic bug (S1).

5. **Not sufficient as written; see B2.** MAP_PRIVATE + PROT_READ does not create a snapshot
   (`crates/umem/src/backing.rs:111–118`). Clean pages may be evicted and faulted back from
   changed file contents; initial population/checksums do not repair this. Even before
   eviction, later file-change visibility is unspecified
   ([mmap](https://man7.org/linux/man-pages/man2/mmap.2.html)). A strengthened whole-lifetime
   immutability/validity contract suffices for ordinary files; the safe alternative is copying
   into owned anonymous memory. Closing the fd after mapping neither invalidates the mapping
   nor protects the inode from writers.

6. **T1 partial; T2/T3 simulated fragments; T4 limited evidence; T5/T6 absent here.** T1 has
   exactly one compile-fail fixture, use-after-lease (`crates/umem/tests/ownership.rs:4`,
   `crates/umem/tests/ui/slice_after_lease.rs:4–7`). Thread-trait assertions and Weak retention
   after forgetting a Submission are real checks (`crates/umem/src/lib.rs:267–280,295–308`),
   but there is no delayed worker, Miri harness, panic/cancellation test, or other promised UI
   case. T2's “visibility” test is a same-thread synchronous CPU overwrite, with no GPU check
   of the input pattern (`crates/umem/tests/round_trip.rs:23–31`). T3 only checks quarantine
   list growth after error, not delayed completion or exactly-once cleanup (lib.rs:311–323).
   T4's ignored test checks one 32 MiB THP result and can silently skip without its environment
   flag (`crates/umem/tests/thp.rs:6–21`); fallback would fail its THP-only assertion.
   The supplied Linux/macOS passes and 4 GiB THP/HugeTLB probe observations are accepted;
   they do not cover pressure stability or GPU visibility. `thp_repro` retains allocations
   in caller-selected order (lines 13–36), while `thp_probe` uses fixed order (lines 11–49).
   Neither implements all T4 diagnostics. No ext4/GPU T5 or native CUDA/Metal T6 tests exist
   under this crate; the Criterion benchmark measures allocation, not kernel throughput.

7. **Awkward, not inevitably unsound.** Separate singleton submissions can share an underlying
   event if every fence instance covers all device uses and every lease survives partial
   construction/error paths, but this duplicates waits and fragments ownership recovery.
   Minimal design-aligned change: one Submission owns Vec<AnyLease>, one fence, and one
   validated context, returning Vec<AnyBuf> with Ro/Rw preserved. A sealed typed tuple lease
   bundle with a corresponding output tuple is an alternative that avoids mode erasure.
   Completion/quarantine must cover the entire bundle. Add safe length access on GpuLease
   for wrapper bounds checks; it currently exposes context and pointer but no length
   (`crates/umem/src/lib.rs:170–194`). Keep backend resource owners retained with the bundle.

## Suggested tests

- **Highest value:** a fake fence whose wait panics on ordinary Submission drop. Catch the
  unwind and use a Weak backing observer to assert storage remains alive without dereferencing
  freed memory. Extend to a fence destructor that panics after a failed wait: both currently
  bypass quarantine. This directly tests B1 without a GPU or deliberate use-after-free.
- Add pending → ready and terminal-error polling, ordinary/unwinding drop, forgotten leases,
  and exactly-once cleanup tests; add the missing Ro-mutation, lease-clone, live-borrow-transfer,
  and use-after-wait compile-fail cases. Then test multi-buffer completion/error as one unit.
- Linux: lengths 1/H/H+1 and arithmetic boundaries; immutable non-page-aligned files; smaps
  exact/merged/split fixtures; injected population/collapse failures and strict fallback.
  Separate host-independent parser/state-machine tests from hardware-gated coverage tests.
- Preserve the supplied performance evidence, then complete T2/T3 with delayed real GPU work,
  byte verification, allocation churn, and native-resource final-release counters; complete
  T4 pressure/fresh-process trials and T5/T6 at the backend integration stage.
