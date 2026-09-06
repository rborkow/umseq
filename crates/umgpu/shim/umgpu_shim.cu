#include "umgpu_shim.h"
#include <cuda_runtime.h>
#include <cub/cub.cuh>

static int result(cudaError_t e) { return static_cast<int>(e); }
extern "C" int umgpu_init(int* p) {
  int device = 0; cudaError_t e = cudaGetDevice(&device); if (e != cudaSuccess) return result(e);
  cudaDeviceProp d; e = cudaGetDeviceProperties(&d, device); if (e != cudaSuccess) return result(e);
  int a = 0; e = cudaDeviceGetAttribute(&a, cudaDevAttrPageableMemoryAccess, device); if (e != cudaSuccess) return result(e); p[0] = a;
  e = cudaDeviceGetAttribute(&a, cudaDevAttrPageableMemoryAccessUsesHostPageTables, device); if (e != cudaSuccess) return result(e); p[1] = a;
  e = cudaDeviceGetAttribute(&a, cudaDevAttrDirectManagedMemAccessFromHost, device); if (e != cudaSuccess) return result(e); p[2] = a;
  e = cudaDeviceGetAttribute(&a, cudaDevAttrHostRegisterSupported, device); if (e != cudaSuccess) return result(e); p[3] = a;
  e = cudaDeviceGetAttribute(&a, cudaDevAttrConcurrentManagedAccess, device); if (e != cudaSuccess) return result(e); p[4] = a;
  p[5] = d.major; p[6] = d.minor; p[7] = d.multiProcessorCount; return result(cudaSuccess);
}
extern "C" int umgpu_stream_create(void** s) { return result(cudaStreamCreate((cudaStream_t*)s)); }
extern "C" int umgpu_stream_destroy(void* s) { return result(cudaStreamDestroy((cudaStream_t)s)); }
extern "C" int umgpu_event_create(void** e) { return result(cudaEventCreateWithFlags((cudaEvent_t*)e, cudaEventDisableTiming)); }
extern "C" int umgpu_event_destroy(void* e) { return result(cudaEventDestroy((cudaEvent_t)e)); }
extern "C" int umgpu_event_record(void* e, void* s) { return result(cudaEventRecord((cudaEvent_t)e, (cudaStream_t)s)); }
extern "C" int umgpu_event_query(void* e) { cudaError_t x = cudaEventQuery((cudaEvent_t)e); return x == cudaSuccess ? 0 : (x == cudaErrorNotReady ? 1 : -result(x)); }
extern "C" int umgpu_event_sync(void* e) { return result(cudaEventSynchronize((cudaEvent_t)e)); }
extern "C" int umgpu_host_register(void* p, size_t n) { return result(cudaHostRegister(p, n, cudaHostRegisterDefault)); }
extern "C" int umgpu_host_unregister(void* p) { return result(cudaHostUnregister(p)); }
extern "C" int umgpu_radix_sort_pairs_u64_u32_temp_size(size_t n, size_t* b) { return result(cub::DeviceRadixSort::SortPairs(nullptr, *b, (const uint64_t*)nullptr, (uint64_t*)nullptr, (const uint32_t*)nullptr, (uint32_t*)nullptr, n)); }
extern "C" int umgpu_radix_sort_pairs_u64_u32(void* t, size_t b, const uint64_t* ki, uint64_t* ko, const uint32_t* vi, uint32_t* vo, size_t n, int begin, int end, void* s) { return result(cub::DeviceRadixSort::SortPairs(t, b, ki, ko, vi, vo, n, begin, end, (cudaStream_t)s)); }
extern "C" int umgpu_rle_u64_temp_size(size_t n, size_t* b) { return result(cub::DeviceRunLengthEncode::Encode(nullptr, *b, (const uint64_t*)nullptr, (uint64_t*)nullptr, (uint32_t*)nullptr, (uint32_t*)nullptr, n)); }
extern "C" int umgpu_rle_u64(void* t, size_t b, const uint64_t* k, uint64_t* u, uint32_t* c, uint32_t* r, size_t n, void* s) { return result(cub::DeviceRunLengthEncode::Encode(t, b, k, u, c, r, n, (cudaStream_t)s)); }
extern "C" int umgpu_exclusive_scan_u32_temp_size(size_t n, size_t* b) { return result(cub::DeviceScan::ExclusiveSum(nullptr, *b, (const uint32_t*)nullptr, (uint32_t*)nullptr, n)); }
extern "C" int umgpu_exclusive_scan_u32(void* t, size_t b, const uint32_t* i, uint32_t* o, size_t n, void* s) { return result(cub::DeviceScan::ExclusiveSum(t, b, i, o, n, (cudaStream_t)s)); }
__global__ static void inc(const uint64_t* in, uint64_t* out, size_t n) { size_t i = blockIdx.x * (size_t)blockDim.x + threadIdx.x; if (i < n) out[i] = in[i] + 1; }
extern "C" int umgpu_inc_u64(const uint64_t* in, uint64_t* out, size_t n, void* s) { inc<<<(n + 255) / 256, 256, 0, (cudaStream_t)s>>>(in, out, n); return result(cudaGetLastError()); }
// Deliberately use byte offsets rather than a C++ RecordHeader: Rust owns the ABI.
__device__ static uint32_t rd32(const uint8_t* p) { return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24); }
__device__ static uint64_t rd64(const uint8_t* p) { return (uint64_t)rd32(p) | ((uint64_t)rd32(p + 4) << 32); }
__device__ static uint64_t fnv(uint64_t h, uint8_t b) { return (h ^ b) * 0x100000001b3ULL; }
__device__ static uint64_t fnv_i32(uint64_t h, int32_t x) { const uint8_t* p = (const uint8_t*)&x; for (int i = 0; i != 4; ++i) h = fnv(h, p[i]); return h; }
__global__ static void dup_keys(const uint8_t* headers, const uint8_t* arena, size_t arena_len, size_t n, int mode, uint64_t* keys, uint32_t* vals) {
  size_t i = blockIdx.x * (size_t)blockDim.x + threadIdx.x; if (i >= n) return;
  const uint8_t* h = headers + i * 48; uint16_t flag = (uint16_t)h[8] | ((uint16_t)h[9] << 8); uint8_t mapq = h[10];
  bool keep = mode == 0 ? ((flag & 0x204) == 0 && mapq >= 30) : ((flag & 0x904) == 0 && mapq >= 30);
  vals[i] = (uint32_t)i; if (!keep) { keys[i] = ~0ULL; return; }
  uint64_t offset = rd64(h + 32); uint32_t l = rd32(h + 40);
  if (offset > arena_len || (uint64_t)l > arena_len - offset) { keys[i] = ~0ULL; return; }
  const uint8_t* b = arena + offset;
  // Malformed records are excluded here; the CPU reduction will reject them too.
  if (l < 32) { keys[i] = ~0ULL; return; }
  uint64_t out = 0xcbf29ce484222325ULL;
  uint8_t name = b[8]; uint16_t ncigar = (uint16_t)b[12] | ((uint16_t)b[13] << 8);
  if (mode) {
    int32_t seq_len = (int32_t)rd32(b + 16); size_t at = 32 + (size_t)name + (size_t)ncigar * 4;
    if (seq_len < 0 || at + ((size_t)seq_len + 1) / 2 > l) { keys[i] = ~0ULL; return; }
    out = fnv((out ^ (uint64_t)seq_len), 0); // replaced below to mirror Rust's initial multiply
    out = (0xcbf29ce484222325ULL ^ (uint64_t)seq_len) * 0x100000001b3ULL;
    size_t bytes = ((size_t)seq_len + 1) / 2; for (size_t j = 0; j < bytes; ++j) { uint8_t x = b[at+j]; if (j+1 == bytes && (seq_len & 1)) x &= 0xf0; out = fnv(out, x); }
  } else {
    int32_t pos = (int32_t)rd32(h + 4), tid = (int32_t)rd32(h); out = fnv_i32(fnv_i32(out, tid), pos); int32_t ref = pos;
    size_t at = 32 + (size_t)name; if (at + (size_t)ncigar * 4 > l) { keys[i] = ~0ULL; return; }
    for (uint16_t j = 0; j < ncigar; ++j) { uint32_t c = rd32(b + at + (size_t)j * 4); int32_t len = (int32_t)(c >> 4); uint32_t op = c & 15;
      if (op == 0) { out = fnv_i32(fnv_i32(out, ref), ref + len); ref += len; } else if (op == 2 || op == 3 || op == 4) ref += len;
    }
  }
  keys[i] = out;
}
extern "C" int umgpu_dup_keys(const void* h, const uint8_t* a, size_t alen, size_t n, int mode, uint64_t* k, uint32_t* v, void* s) { dup_keys<<<(n + 255) / 256, 256, 0, (cudaStream_t)s>>>((const uint8_t*)h, a, alen, n, mode, k, v); return result(cudaGetLastError()); }
extern "C" const char* umgpu_error_string(int c) { return cudaGetErrorString((cudaError_t)c); }
