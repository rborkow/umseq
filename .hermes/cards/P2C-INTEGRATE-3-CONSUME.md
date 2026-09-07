# P2C-INTEGRATE-3-CONSUME — Terra — make hooked STAR consume the window's prep instead of redoing it

Workdir `/Users/rborkows/projects/uni-rnaseq`. Read `AGENTS.md`, then
`bench/PHASE2C-integrate-1.md` **all three rounds** (the "Where the ~157 CPU-s now sits" table
is your work list), then `bench/star-integrate/star_integrate_window.cpp`,
`star_integrate.cpp`, `make_star_integrate.py`. **You are the only worker; you own every file
under `bench/star-integrate/` and `crates/umgpu/ffi/`.** No SSH, no commits.

## Where we are

Three rounds of the 20M gate: parity byte-identical every time, 96% of inner seed requests
served by the GPU, mapping wall within 8% of stock. CPU-s: stock 755, GPU arm **912 (+20.8%)**.
The GPU saves ~130 CPU-s of seed search; the lookahead spends ~185 doing STAR's per-read
prep twice. Every remaining cost is duplicated CPU work, enumerated below. This card
removes as much of it as parity allows, in strict value order. Stop where the clock stops;
each item lands independently.

## Item 1 (~75 CPU-s, do first): hand `Read1` from the frame

Stock `ReadAlign_oneRead.cpp:14-71`: per read, `readLoad` → `convertNucleotidesToNumbers`
(+ clip) per mate, then pair-combine into `Read1[0]` (spacer + complement + reverse of mate
2), then `Read1[1]` = complement of `Read1[0]`, `Read1[2]` = reverse of `Read1[1]`.
`star_integrate_window.cpp:199-266` does exactly the same `readLoad` + combine to build
`num[0]` (= `Read1[0]` byte-for-byte, `frame.a`) and `complement` (= `Read1[1]`).

Change: `prepare_window` runs before `oneRead` for the same reads (generator line 42-43).
Generate a hook at the top of `oneRead` that asks the window for the frame at `iReadAll`
(the frame already stores `ordinal`); on hit, `readLoad` still runs for the stream
position / names / quals / `readLength*` / `clipMates` side effects **but** skip the
numeric conversion+combine and `memcpy` `Read1[0]` from the frame, then `Read1[1]`/`[2]`
from the frame's complement (add a reversed copy to the frame or compute `Read1[2]` in
place — it's one reverse of `Lread` bytes, cheap). The window must have stored the
*post-clip* numeric sequence (it does: `readLoad` clips before returning). On a miss fall
through to stock unchanged.

Parity is the proof: `Read1` identical ⇒ every downstream byte identical. Add a unit test
in `test_window_prefix.py` that runs stock conversion and the frame path on 1,000 random
paired reads with random clips and asserts `Read1[0..2]` equal. If any clip mode makes the
stored bytes diverge from stock's, exclude that mode by count (sidecar `read1_fallback`)
rather than approximating.

## Item 2 (~40 CPU-s `sys`): coordinator sleeps, doesn't poll

`star_integrate.cpp:476` `s.cv.wait_for(lock, 100 µs)` in a loop for the life of the run.
Round 3 `sys` was 64–75 s vs stock 28. Replace with: producers `notify_one` when a
queue crosses `submit_floor` (an atomic counter, one notify per crossing, not per push)
and when a window is retired; the coordinator waits on the cv with a **bounded** timeout
equal to `FILL_MAX_US` only while `filling`, and indefinitely (`wait`) otherwise. Keep
`stopping`. Measure with the existing tests; add one asserting the coordinator makes no
more than N wakeups for a K-window run with a fake backend.

## Item 3 (~70 CPU-s, riskiest, only if 1–2 are green with ≥45 min left): skip STAR's prefix work on hit

`prepare_window` computes `(Lind, iSA1, iSA2, L_in)` per candidate; STAR recomputes them
in `ReadAlign_maxMappableLength2strands.cpp` before the generated `lookup` call. Under the
admitted profile (see `docs/STAR-INTEGRATE-DESIGN.md` §c) they are determined by
`(read, S, dirR)`. Move the hook *above* STAR's prefix block, key on `(frame, S, N, dirR)`,
on hit skip the prefix block and the search; on miss run stock unchanged. **Strict mode
must still call the stock function (which does its own prefix work) and compare all four
outputs** — that's the proof, keep it. If strict shows any mismatch, don't ship item 3;
report the mismatching case.

## Not in scope

Kernel, transport, `umem`, parity checker, gate script, `run_timing_host.sh`. Don't touch
`CONTRACT.md` semantics except to document item 1's `Read1` hand-off if landed.

## Finish

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, `python3 -B bench/star-integrate/test_*.py` (all four), clang-format
on every C++ file you touched. `TDD.md` gets a line per item (RED→GREEN). Report: per item
done/not-done, the profile symbol it removes, and any parity risk you know of but couldn't
test locally.
