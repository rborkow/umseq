# P2C-INTEGRATE-V2-T5C — borrowed integrated STAR index

Status: implementation and Mac-local gates complete. No Spark run or timing was
performed (this card forbids SSH), so this report makes no parity or throughput
claim beyond the already accepted T5B measurement.

`setup()` passes these caller-owned ranges to `usi_init_v2_borrowed`:

- Genome: `(uint8_t *)mapGen.G - 200`, `mapGen.nGenome + 400` bytes.
- SA: `mapGen.SA.charArray`, `((mapGen.nSA - 1) * (mapGen.GstrandBit + 1) / 8) + 8` bytes; setup bypasses if `SA.lengthByte` is shorter.
- SAindex: `mapGen.SAi.charArray`, `mapGen.SAi.lengthByte` bytes, with
  `config.sai_offset = 0`.

The borrowed context retains no umem index buffers and dispatches V3 via
`seed_probe_v3_raw_host`; reads, requests, results, and stats remain umem
buffers. The sampled identity hashes these same supplied ranges, so it is now a
self-consistency check of pointer extents/lengths rather than a cross-copy
comparison. `index_mode: "borrowed"`, the sum of `AnonHugePages:` for every
mapping overlapping the three ranges, and `setup_wall_s` are emitted in the
sidecar.

The generator injects the allocation advice in the pinned private-tree shapes:

- `Genome_genomeLoad.cpp`: immediately after each `G1 = new char[...]`, before
  the corresponding `fstreamReadBig(GenomeIn, ...)`.
- `PackedArray.cpp`: immediately after `charArray = new char[lengthByte]`,
  before its first `memset`; this covers `SA.allocateArray()` and
  `SAi.allocateArray()`.
- `Genome_genomeLoad.cpp`: after `SAiIn.close()`, opens `Genome`, `SA`, and
  `SAindex` solely to issue `posix_fadvise(..., POSIX_FADV_DONTNEED)`.

All hooks are compiled only under `STAR_INTEGRATE` (and Linux for the Linux
memory-advice APIs); they do not alter output bytes.

Validated locally: `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
`python3 -B bench/star-integrate/test_source_patch.py`,
`python3 -B bench/star-integrate/test_coordinator.py`,
`bench/star-integrate/test_abi.sh`, and clang-format dry-run. The borrowed
synthetic prefix test asserts the Mac stub's Unsupported path. CUDA-grid replay
and integrated STAR parity remain Spark-only work.
