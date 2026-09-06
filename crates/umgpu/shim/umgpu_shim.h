#pragma once
#include <stddef.h>
#include <stdint.h>

extern "C" {
int umgpu_init(int* device_props_out);
int umgpu_stream_create(void** stream); int umgpu_stream_destroy(void* stream);
int umgpu_event_create(void** ev); int umgpu_event_destroy(void* ev);
int umgpu_event_record(void* ev, void* stream); int umgpu_event_query(void* ev);
int umgpu_event_sync(void* ev);
int umgpu_host_register(void* p, size_t len); int umgpu_host_unregister(void* p);
int umgpu_radix_sort_pairs_u64_u32_temp_size(size_t n, size_t* temp_bytes);
int umgpu_radix_sort_pairs_u64_u32(void* temp, size_t temp_bytes, const uint64_t* keys_in,
  uint64_t* keys_out, const uint32_t* vals_in, uint32_t* vals_out, size_t n,
  int begin_bit, int end_bit, void* stream);
int umgpu_rle_u64_temp_size(size_t n, size_t* temp_bytes);
int umgpu_rle_u64(void* temp, size_t temp_bytes, const uint64_t* keys, uint64_t* unique_out,
  uint32_t* counts_out, uint32_t* num_runs_out, size_t n, void* stream);
int umgpu_exclusive_scan_u32_temp_size(size_t n, size_t* temp_bytes);
int umgpu_exclusive_scan_u32(void* temp, size_t temp_bytes, const uint32_t* in,
  uint32_t* out, size_t n, void* stream);
int umgpu_inc_u64(const uint64_t* in, uint64_t* out, size_t n, void* stream);
// RecordHeader is passed as raw 48-byte entries: tid@0, pos@4, flag@8, mapq@10,
// offset@32, len@40. mode is 0 for position and 1 for sequence duplication.
int umgpu_dup_keys(const void* headers, const uint8_t* arena, size_t arena_len, size_t n, int mode,
  uint64_t* keys_out, uint32_t* vals_out, void* stream);
// Synchronous, drains stream on every exit. Workspace = 96*n bytes, control = 112.
int umgpu_markdup_temp_size(size_t n, size_t* bytes);
int umgpu_markdup(const void* headers, const uint8_t* arena, size_t arena_len,
  const uint32_t* order, size_t n, void* work, void* temp, size_t temp_bytes,
  void* control, void* stream);
const char* umgpu_error_string(int code);
}
