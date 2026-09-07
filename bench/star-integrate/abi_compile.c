// Compile as C11 and C++17; check declarations without requiring CUDA/backend.
#include "usi.h"
#include <stdio.h>
#ifdef __cplusplus
#include "../../crates/umgpu/shim/seed_probe.h"
#endif
#ifndef ABI_LAYOUT_ONLY
void check_declarations(void) {
  int (*probe)(const uint8_t *, const uint8_t *, const uint8_t *, size_t,
               const ProbeRequest *, size_t, size_t, ProbeConfig, ProbeOutput *,
               ProbeStats *, unsigned, float *, struct CUstream_st *) =
      &umgpu_seed_probe;
  int32_t (*init)(const char *, const UsiIdentityV1 *, uint64_t, UsiContext **,
                  UsiErrorV1 *) = &usi_init_v1;
  int32_t (*batch)(UsiContext *, uint64_t, const uint8_t *, uint64_t,
                   const ProbeRequest *, uint64_t, ProbeOutput *, ProbeStats *,
                   UsiErrorV1 *) = &usi_search_batch_v1;
  int32_t (*destroy)(UsiContext **, UsiErrorV1 *) = &usi_destroy_v1;
  (void)probe;
  (void)init;
  (void)batch;
  (void)destroy;
}
#endif
int main(void) {
  printf("ProbeRequest=%zu ProbeOutput=%zu ProbeStats=%zu ProbeConfig=%zu "
         "UsiIdentityV1=%zu UsiErrorV1=%zu; all field offsets asserted\n",
         sizeof(ProbeRequest), sizeof(ProbeOutput), sizeof(ProbeStats),
         sizeof(ProbeConfig), sizeof(UsiIdentityV1), sizeof(UsiErrorV1));
  return 0;
}
