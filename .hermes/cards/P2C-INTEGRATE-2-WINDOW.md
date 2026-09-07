# P2C-INTEGRATE-2-WINDOW — Terra #1 — cut the lookahead's duplicated CPU work (you own the producer side)

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, `bench/PHASE2C-integrate-1.md`
(both rounds), then the profile below. **Another worker is concurrently editing
`bench/star-integrate/star_integrate.cpp` (the coordinator). You own:**
`star_integrate_window.cpp`, `star_integrate_work.{hpp,cpp}`, `make_star_integrate.py`,
`crates/umgpu/ffi/star_integrate.rs`, and the identity code path. You may add declarations to
`star_integrate.hpp` — append only, say exactly what. Do not edit `star_integrate.cpp`; if you
need a coordinator-side change, write it as a one-paragraph request in your final report.

## Measured (perf, 4M reads, enabled path, `bench/evidence/integrate-1-host/`)

Round 2: GPU arm 800 CPU-s = hooks-bypassed arm 800 = stock 753 + 47. The GPU saved ~130 CPU-s of
seed search; the lookahead spent it. Self-time, whole run, integration-attributable:

| symbol | % | what |
|---|---|---|
| `prepare_window` self | 9.6 | qualitySplit + SAindex prefix lookup + candidate build, per read, *duplicating what STAR does again at the call site* |
| `__memset_zva64` from `prepare_window` | 4.3 | zero-init of `WindowRead`/`InnerCall`/vectors per read |
| `complementSeqNumbers` + `convertNucleotidesToNumbers` | 10.2 | read re-preparation in the window (stock does it once in `oneRead`; we do it twice) |
| `submit_window` | 4.2 | per-window: moves + `unordered_multimap` inserts + a global mutex |
| `__aarch64_cas4_acq` | 3.8 | the same global `s.mu` taken in `submit_window` **and once per frame in `assign_frame_identity`** |
| memcpy / malloc | 4.5 | frame byte copies, per-candidate allocations |
| hooks-bypassed floor | +6.3% | `compared()` is called ~1.4 G times per run even when disabled; `set_chain`/`inner_call` per request |

Startup: `setup_wall_s` 65–91 s because the sampled identity re-reads 30 GB of index files.

## Deliverable, in priority order (stop where the clock stops; each item independently landable)

1. **Setup ≤ 3 s.** Bind identity resident-to-resident: sample STAR's loaded `G/SA/SAi` and the
   USI context's loaded copy with the same sampled scheme (first/last 1 MiB + 64 blocks) and
   compare; use `stat` file sizes for the length fields. No index file reads at setup.
2. **Hook floor → ~0 when disabled.** `compared(bytes)` must compile to a thread-local
   `+=` guarded by a `static bool` read once, or be behind `STAR_INTEGRATE_COUNTERS` so the
   timed build has no call. Same for `set_chain`/`inner_call`/`reverse_suppressed` on the
   disabled path. Target: bypass arm within 1% of stock CPU-s.
3. **Prepare reads once.** The window already produces `num[0]`/`num[1]` for lookahead; STAR
   then redoes `convertNucleotidesToNumbers`/`complementSeqNumbers` in `ReadAlign_oneRead.cpp`.
   Hook `oneRead` (via `make_star_integrate.py`) so that when a frame for `iReadAll` exists,
   `Read1[0..2]` are filled from the frame's bytes (memcpy) instead of reconverted. Parity
   gate proves equivalence. If clipping state makes this unsafe for some reads, fall back to
   stock conversion for those and count them.
4. **Stop zeroing and allocating per read.** Pool `WindowRead` and candidate vectors per worker
   (high-water mark, `clear()` not reconstruct); `InnerCall` built by the shared key builder
   without `= {}` then overwrite; frame bytes into one per-window arena, not a `vector<uint8_t>`
   per frame.
5. **Prefix lookup once (only if 1–4 land with time left):** the window computed
   `(Lind, iSA1, iSA2, L_in)`; at the call site STAR recomputes them before `lookup`. Under the
   admitted profile (sparse1, `seedSearchLmax=0`, no remap) the design §(c) proves
   `(read, S, dirR)` determines them. Move the generated hook *before* STAR's prefix
   computation in `maxMappableLength2strands`, key on `(frame, S, N, dirR)`, and on hit skip
   STAR's prefix work too; on miss fall through unchanged. **Strict mode must still compare
   all four outputs against the stock function**, which does its own prefix work — that's the
   proof. This is the largest remaining duplicate (~10%) and the riskiest; do it last.

Each item: local suites green (`cargo clippy/test --workspace`, `python3 -B
bench/star-integrate/test_*.py`, `cargo fmt`, clang-format), a line in `TDD.md`. No kernel,
transport, `umem`, or parity-checker changes. No SSH/commits. Report per item: done / not
done, and the profile symbol you expect it to remove.
