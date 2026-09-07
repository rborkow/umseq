// SPDX-License-Identifier: MIT
// Stable PROBE transport shared by C, C++, CUDA and Rust repr(C) records.
#ifndef UMGPU_SEED_PROBE_ABI_H
#define UMGPU_SEED_PROBE_ABI_H
#include <stddef.h>
#include <stdint.h>

typedef uint64_t ProbeU64;
typedef struct ProbeRequest {
  ProbeU64 tag, s0, s1, read_len, start, length, prefix, low, high, dir;
} ProbeRequest;
typedef struct ProbeOutput {
  ProbeU64 length, low, high, count, status;
} ProbeOutput;
typedef struct ProbeStats {
  ProbeU64 gathers, bytes, loops, comparisons, max_compare, directions;
} ProbeStats;
typedef struct ProbeConfig {
  ProbeU64 n_genome, n_sa, strand_bit;
} ProbeConfig;

// Request: tag=0; dir=1 forward, 0 reverse; s0/s1 are read-arena offsets.
// Output: inclusive low/high. status: 0 success, 1 tag/direction,
// 2 request bounds, 3 comparator bounds, 4 result bounds. Only 0 is usable.
// Stats: loops counts main AND expansion iterations; bytes are logical examined
// bytes, gathers/comparisons count comparator invocations, max_compare is the
// largest examined-byte count, directions is a four-direction bit mask.
#ifdef __cplusplus
#define PROBE_ABI_ASSERT(c) static_assert(c, "PROBE ABI: " #c)
#define PROBE_ABI_ALIGNOF(t) alignof(t)
#else
#define PROBE_ABI_ASSERT(c) _Static_assert(c, "PROBE ABI: " #c)
#define PROBE_ABI_ALIGNOF(t) _Alignof(t)
#endif
PROBE_ABI_ASSERT(sizeof(ProbeU64) == 8);
PROBE_ABI_ASSERT(sizeof(unsigned) == 4);
PROBE_ABI_ASSERT(sizeof(ProbeRequest) == 80);
PROBE_ABI_ASSERT(PROBE_ABI_ALIGNOF(ProbeRequest) == 8);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, tag) == 0);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, s0) == 8);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, s1) == 16);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, read_len) == 24);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, start) == 32);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, length) == 40);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, prefix) == 48);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, low) == 56);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, high) == 64);
PROBE_ABI_ASSERT(offsetof(ProbeRequest, dir) == 72);
PROBE_ABI_ASSERT(sizeof(ProbeOutput) == 40);
PROBE_ABI_ASSERT(PROBE_ABI_ALIGNOF(ProbeOutput) == 8);
PROBE_ABI_ASSERT(offsetof(ProbeOutput, length) == 0);
PROBE_ABI_ASSERT(offsetof(ProbeOutput, low) == 8);
PROBE_ABI_ASSERT(offsetof(ProbeOutput, high) == 16);
PROBE_ABI_ASSERT(offsetof(ProbeOutput, count) == 24);
PROBE_ABI_ASSERT(offsetof(ProbeOutput, status) == 32);
PROBE_ABI_ASSERT(sizeof(ProbeStats) == 48);
PROBE_ABI_ASSERT(PROBE_ABI_ALIGNOF(ProbeStats) == 8);
PROBE_ABI_ASSERT(offsetof(ProbeStats, gathers) == 0);
PROBE_ABI_ASSERT(offsetof(ProbeStats, bytes) == 8);
PROBE_ABI_ASSERT(offsetof(ProbeStats, loops) == 16);
PROBE_ABI_ASSERT(offsetof(ProbeStats, comparisons) == 24);
PROBE_ABI_ASSERT(offsetof(ProbeStats, max_compare) == 32);
PROBE_ABI_ASSERT(offsetof(ProbeStats, directions) == 40);
PROBE_ABI_ASSERT(sizeof(ProbeConfig) == 24);
PROBE_ABI_ASSERT(PROBE_ABI_ALIGNOF(ProbeConfig) == 8);
PROBE_ABI_ASSERT(offsetof(ProbeConfig, n_genome) == 0);
PROBE_ABI_ASSERT(offsetof(ProbeConfig, n_sa) == 8);
PROBE_ABI_ASSERT(offsetof(ProbeConfig, strand_bit) == 16);
#undef PROBE_ABI_ASSERT
#undef PROBE_ABI_ALIGNOF

// CUDA runtime's opaque stream type; no CUDA SDK required to include this file.
struct CUstream_st;
#ifdef __cplusplus
extern "C" {
#endif
// Existing low-level symbol: caller-managed live device-accessible leases.
// This is NOT the owning STAR backend. variant 0=thread, 1=warp.
int umgpu_seed_probe(const uint8_t *g, const uint8_t *sa, const uint8_t *reads,
                     size_t read_bytes, const ProbeRequest *requests,
                     size_t start, size_t n, ProbeConfig config,
                     ProbeOutput *out, ProbeStats *stats, unsigned variant,
                     float *event_ms, struct CUstream_st *stream);
#ifdef __cplusplus
}
#endif
#endif
