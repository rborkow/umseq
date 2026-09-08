# P2C-INTEGRATE-V2-T5B — borrowed STAR index probe

Status: measurement harness prepared; no Spark measurement has been run from this
workspace. Therefore Part 2 is deliberately **not enabled**: the required
`borrowed-madvise >= 90% * umem` decision has no measured value yet.

The new `umseed-probe` binary retains the existing exact oracle: every V2
`(length, low, high, count)` tuple is compared to the adjacent captured STAR
tuple sidecar. It reports event-time gathers/s, end-of-arm `VmRSS`, and the sum
of `AnonHugePages` for the three index allocations. `--borrowed` loads into
ordinary `Vec<u8>` storage; `--borrowed-madvise` issues `MADV_HUGEPAGE` before
the sequential first-touch pass; `--umem` uses the current `probe_load` control.
On a non-CUDA build, `--cpu-only` asserts `Unsupported` and prints no rate.

Run on Spark under the orchestrator-held resource lock (replace `EVIDENCE` with
the allocated evidence directory):

```sh
for arm in borrowed borrowed-madvise umem; do
  cargo run -p umseed-probe --release --features cuda --bin probe_borrowed -- \
    --index /path/to/STAR-index \
    --requests bench/evidence/EVIDENCE/real-requests.bin \
    --config bench/evidence/EVIDENCE/prefix-config.bin \
    --"$arm" | tee "bench/evidence/EVIDENCE/probe-borrowed-$arm.tsv"
done
```

The raw-host V2 boundary receives these exact ranges:

- Genome: pointer is STAR's `G-200`; readable length is `nGenome + 400`.
- SA: readable length is `((nSA - 1) * (GstrandBit + 1) / 8) + 8`.
- SAi: STAR payload starts at its `PackedArray::charArray`; `sai_offset = 0`
  and `sai_bytes` is the actual payload readable extent (the disk header is not
  part of STAR's loaded `SAi` allocation).

The V3 raw-host sibling has the same extent and lifetime checks so the eventual
whole-chain integrated path does not need an ABI or kernel change. It does not
copy, register, or retain index storage.

Part 2 remains pending precisely because its admission threshold has not been
measured. Once it passes, the follow-up must change `usi_init_v2` to retain the
three caller-owned STAR ranges rather than call `probe_load`, use the same
resident bytes on both sides of sampled identity, add loader `madvise` and
post-read `POSIX_FADV_DONTNEED` hooks through the generator, and emit
`index_mode`, `index_anon_huge_bytes`, and `setup_wall_s` in the sidecar.
