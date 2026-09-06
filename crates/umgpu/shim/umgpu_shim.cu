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
extern "C" const char* umgpu_error_string(int c) { return cudaGetErrorString((cudaError_t)c); }
