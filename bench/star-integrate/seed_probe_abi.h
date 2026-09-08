// Single source of truth for all probe ABI versions.
#pragma once
#include "../../crates/umgpu/shim/seed_probe_abi.h"
#include "usi.h"
#ifdef __cplusplus
extern "C" {
#endif
typedef struct UsiPrefixContext UsiPrefixContext;
int32_t usi_init_v2(const char *, const UsiIdentityV1 *, const ProbeConfigV2 *,
                    uint64_t, UsiPrefixContext **, UsiErrorV1 *);
int32_t usi_init_v2_borrowed(const uint8_t *, uint64_t, const uint8_t *,
                             uint64_t, const uint8_t *, uint64_t,
                             const UsiIdentityV1 *, const ProbeConfigV2 *,
                             uint64_t, UsiPrefixContext **, UsiErrorV1 *);
int32_t usi_search_batch_v2(UsiPrefixContext *, uint64_t, const uint8_t *,
                            uint64_t, const ProbeRequestV2 *, uint64_t,
                            ProbeOutputV2 *, ProbeStats *, UsiErrorV1 *);
int32_t usi_destroy_v2(UsiPrefixContext **, UsiErrorV1 *);
// Reuse usi_init_v2 / usi_destroy_v2 and its single resident index for V3.
int32_t usi_search_batch_v3(UsiPrefixContext *, uint64_t, const uint8_t *,
                            uint64_t, const ProbeRequestV3 *, uint64_t,
                            ProbeOutputV3 *, ProbeStats *, UsiErrorV1 *);
#ifdef __cplusplus
}
#endif
