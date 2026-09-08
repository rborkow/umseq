// SPDX-License-Identifier: MIT
// INTEGRATE-1 private synchronous owning backend; implementation follows.
#ifndef STAR_INTEGRATE_USI_H
#define STAR_INTEGRATE_USI_H
#include "../../crates/umgpu/shim/seed_probe_abi.h"

typedef struct UsiContext UsiContext;
typedef struct UsiPrefixContext UsiPrefixContext;
typedef struct UsiIdentityV1 {
  uint64_t genome_file_bytes, sa_file_bytes, sai_file_bytes;
  uint64_t n_sa, strand_bit, sparse;
  uint8_t sha256[96]; // Reserved ABI name: FNV-1a sample digests, G/SA/SAindex.
} UsiIdentityV1;
typedef struct UsiErrorV1 {
  uint32_t code, reserved; // reserved=0; code equals function return value.
  char message[248];       // NUL-terminated, empty on success.
} UsiErrorV1;

#define USI_OK 0
#define USI_BAD_INPUT 1
#define USI_IDENTITY 2
#define USI_ALLOCATION 3
#define USI_GPU 4
#define USI_UNCERTAIN_COMPLETION 5
#define USI_MAX_REQUESTS UINT64_C(262144)

#ifdef __cplusplus
#define USI_ASSERT(c) static_assert(c, "USI ABI: " #c)
#else
#define USI_ASSERT(c) _Static_assert(c, "USI ABI: " #c)
#endif
USI_ASSERT(sizeof(UsiIdentityV1) == 144);
USI_ASSERT(offsetof(UsiIdentityV1, genome_file_bytes) == 0);
USI_ASSERT(offsetof(UsiIdentityV1, sa_file_bytes) == 8);
USI_ASSERT(offsetof(UsiIdentityV1, sai_file_bytes) == 16);
USI_ASSERT(offsetof(UsiIdentityV1, n_sa) == 24);
USI_ASSERT(offsetof(UsiIdentityV1, strand_bit) == 32);
USI_ASSERT(offsetof(UsiIdentityV1, sparse) == 40);
USI_ASSERT(offsetof(UsiIdentityV1, sha256) == 48);
USI_ASSERT(sizeof(UsiErrorV1) == 256);
USI_ASSERT(offsetof(UsiErrorV1, code) == 0);
USI_ASSERT(offsetof(UsiErrorV1, reserved) == 4);
USI_ASSERT(offsetof(UsiErrorV1, message) == 8);
#undef USI_ASSERT

#ifdef __cplusplus
extern "C" {
#endif
// All caller storage borrowed only during the call; no caller pointer retained.
// Required error storage on every call. Init sets *out=NULL before work.
int32_t usi_init_v1(const char *index_dir, const UsiIdentityV1 *identity,
                    uint64_t index_epoch, UsiContext **out, UsiErrorV1 *error);
// Serial coordinator only. Same nonzero epoch as init. Reuse exact Probe
// records. On return 0, results[j] and stats[j] correspond to requests[j]. On
// nonzero, consume no slot. n=0 validates ctx/epoch, then succeeds without
// array access. n>0: arrays required, each output array has n slots; reads has
// read_bytes bytes.
int32_t usi_search_batch_v1(UsiContext *ctx, uint64_t index_epoch,
                            const uint8_t *reads, uint64_t read_bytes,
                            const ProbeRequest *requests, uint64_t n,
                            ProbeOutput *results, ProbeStats *stats,
                            UsiErrorV1 *error);
// ctx required; *ctx=NULL is a successful no-op. Always nulls a valid handle;
// drain or quarantine backend allocations before releasing safe ownership.
int32_t usi_destroy_v1(UsiContext **ctx, UsiErrorV1 *error);
int32_t usi_init_v2(const char *index_dir, const UsiIdentityV1 *identity,
                    const ProbeConfigV2 *config, uint64_t index_epoch,
                    UsiPrefixContext **out, UsiErrorV1 *error);
int32_t usi_search_batch_v2(UsiPrefixContext *ctx, uint64_t index_epoch,
                            const uint8_t *reads, uint64_t read_bytes,
                            const ProbeRequestV2 *requests, uint64_t n,
                            ProbeOutputV2 *results, ProbeStats *stats,
                            UsiErrorV1 *error);
int32_t usi_destroy_v2(UsiPrefixContext **ctx, UsiErrorV1 *error);
// V3 (whole-chain) reuses the V2 context and resident index.
int32_t usi_search_batch_v3(UsiPrefixContext *ctx, uint64_t index_epoch,
                            const uint8_t *reads, uint64_t read_bytes,
                            const ProbeRequestV3 *requests, uint64_t n,
                            ProbeOutputV3 *results, ProbeStats *stats,
                            UsiErrorV1 *error);
#ifdef __cplusplus
}
#endif
#endif
