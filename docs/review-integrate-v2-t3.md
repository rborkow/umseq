# Independent review — P2C-INTEGRATE-V2-T3

2026-09-07. Independent read-only `prefix_review` agent reviewed the device prefix body, V2 ABI and safe CUDA transport, owning FFI context, independent oracle, and replay against pinned STAR 2.7.11b source. No SSH, measurements, or reviewer edits.

The reviewer found no admitted-profile STAR semantic mismatch: forward/reverse prefix bytes, absent step-down, next-entry/end-table handling, unmasked prefix-only results, unique comparison from Lind, and inner-search prefix selection match the source. The lease review found index and batch leases remain live through launch/drain, with failed recovery disabling the session. V1 layouts and path remain unchanged.

The review identified missing explicit V2 FFI overlap checks. Those were added for init, batch inputs/outputs/error/context, and destroy. A follow-up caught an early malformed-input error write preceding overlap checks; the implementation now returns BAD without writing on that path. `malformed_batch_does_not_write_aliased_error` passes in the final workspace run. This final correction was verified by the implementing agent's regression, not a third independent review pass.

The reviewer confirmed the synthetic grid reaches all three prefix branches and Lind==0, and the corpus replay requires all 999,914 exact tuples without filtering. The suggested legacy-tag/profile regression rows were added and checked on follow-up. Malformed configuration is not exhaustively tested; this is a research implementation awaiting its external correctness gate.

No CUDA compilation, actual device grid, real-corpus replay, strict 20M parity, Tier 0 GPU comparison, or performance result was reviewed or claimed. Those remain mandatory before accepting T3 as GPU-validated. See `bench/PHASE2C-integrate-v2-t3.md` for exact ABI, ownership, Terra handoff, commands, and evidence.
