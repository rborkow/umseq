// PROBE only. No registration, staging, or device index allocation.
#include "seed_probe.h"
#include <cuda_runtime.h>
__global__ void probe_thread_kernel(const uint8_t *g, const uint8_t *sa,
                                    const uint8_t *reads, size_t read_bytes,
                                    const ProbeRequest *requests, size_t start,
                                    size_t n, ProbeConfig config,
                                    ProbeOutput *out, ProbeStats *stats) {
  const size_t i = size_t(blockIdx.x) * blockDim.x + threadIdx.x;
  if (i >= n)
    return;
  ProbeSearch search{g, sa, reads, read_bytes, config, requests[start + i]};
  out[i] = search.run(); // unconditional host-consumed output
  stats[i] = search.stats;
}
// Device-only collective policy; all 32 lanes are active at every call.
struct ProbeDeviceWarp {
  static constexpr bool enabled = true;
  __device__ static unsigned lane() { return threadIdx.x & 31; }
  __device__ static ProbeU64 broadcast(ProbeU64 value) {
    return __shfl_sync(0xffffffffu, static_cast<unsigned long long>(value), 0);
  }
  __device__ static unsigned ballot(bool predicate) {
    return __ballot_sync(0xffffffffu, predicate);
  }
  __device__ static unsigned first(unsigned mask) { return __ffs(mask) - 1; }
};
__global__ void probe_warp_kernel(const uint8_t *g, const uint8_t *sa,
                                  const uint8_t *reads, size_t read_bytes,
                                  const ProbeRequest *requests, size_t start,
                                  size_t n, ProbeConfig config,
                                  ProbeOutput *out, ProbeStats *stats) {
  // Fixed block size 128 = four whole warps. Inactive final warps return as a
  // unit before reading requests; n need not be divisible by either 4 or 32.
  const size_t i = probe_warp_request(blockIdx.x, threadIdx.x);
  if (i >= n)
    return;
  ProbeSearchImpl<ProbeDeviceWarp> search{
      g, sa, reads, read_bytes, config, requests[start + i]};
  const ProbeOutput result = search.run();
  if (ProbeDeviceWarp::lane() == 0) {
    out[i] = result;
    stats[i] = search.stats;
  }
}
extern "C" int umgpu_seed_probe(const uint8_t *g, const uint8_t *sa,
                                const uint8_t *reads, size_t read_bytes,
                                const ProbeRequest *requests, size_t start,
                                size_t n, ProbeConfig config, ProbeOutput *out,
                                ProbeStats *stats, unsigned variant,
                                float *event_ms, cudaStream_t stream) {
  // Rust enum supplies 0/1; retain stream drain even for a malformed FFI call.
  if (variant > 1) {
    const cudaError_t drained = cudaStreamSynchronize(stream);
    return int(drained == cudaSuccess ? cudaErrorInvalidValue : drained);
  }
  cudaEvent_t begin = nullptr, end = nullptr;
  cudaError_t rc = cudaEventCreate(&begin);
  if (rc == cudaSuccess)
    rc = cudaEventCreate(&end);
  if (rc == cudaSuccess)
    rc = cudaEventRecord(begin, stream);
  if (rc == cudaSuccess) {
    if (variant == 0) {
      probe_thread_kernel<<<(n + 127) / 128, 128, 0, stream>>>(
          g, sa, reads, read_bytes, requests, start, n, config, out, stats);
    } else {
      probe_warp_kernel<<<probe_warp_blocks(n), 128, 0, stream>>>(
          g, sa, reads, read_bytes, requests, start, n, config, out, stats);
    }
    rc = cudaGetLastError();
  }
  if (rc == cudaSuccess)
    rc = cudaEventRecord(end, stream);
  // All exits drain the stream before any guaranteeing lease can be released.
  const cudaError_t drained = cudaStreamSynchronize(stream);
  if (rc == cudaSuccess)
    rc = drained;
  if (rc == cudaSuccess)
    rc = cudaEventElapsedTime(event_ms, begin, end);
  if (begin)
    cudaEventDestroy(begin);
  if (end)
    cudaEventDestroy(end);
  return int(rc);
}

__global__ void probe_prefix_kernel(const uint8_t *g, const uint8_t *sa,
                                    const uint8_t *sai, const uint8_t *reads,
                                    size_t read_bytes,
                                    const ProbeRequestV2 *requests, size_t n,
                                    ProbeConfigV2 config, ProbeOutputV2 *out,
                                    ProbeStats *stats) {
  const size_t i = size_t(blockIdx.x) * blockDim.x + threadIdx.x;
  if (i >= n)
    return;
  ProbePrefixSearch search{g, sa, sai, reads, read_bytes, config, requests[i]};
  out[i] = search.run();
  stats[i] = search.stats;
}
extern "C" int umgpu_seed_probe_v2(const uint8_t *g, const uint8_t *sa,
                                   const uint8_t *sai, const uint8_t *reads,
                                   size_t read_bytes,
                                   const ProbeRequestV2 *requests, size_t n,
                                   ProbeConfigV2 config, ProbeOutputV2 *out,
                                   ProbeStats *stats, float *event_ms,
                                   cudaStream_t stream) {
  cudaEvent_t begin = nullptr, end = nullptr;
  cudaError_t rc = cudaEventCreate(&begin);
  if (rc == cudaSuccess)
    rc = cudaEventCreate(&end);
  if (rc == cudaSuccess)
    rc = cudaEventRecord(begin, stream);
  if (rc == cudaSuccess) {
    probe_prefix_kernel<<<(n + 127) / 128, 128, 0, stream>>>(
        g, sa, sai, reads, read_bytes, requests, n, config, out, stats);
    rc = cudaGetLastError();
  }
  if (rc == cudaSuccess)
    rc = cudaEventRecord(end, stream);
  // The live genome/SA/SAi/read/request/output leases survive every drain path.
  const cudaError_t drained = cudaStreamSynchronize(stream);
  if (rc == cudaSuccess)
    rc = drained;
  if (rc == cudaSuccess)
    rc = cudaEventElapsedTime(event_ms, begin, end);
  if (begin)
    cudaEventDestroy(begin);
  if (end)
    cudaEventDestroy(end);
  return int(rc);
}

template <class Warp>
__global__ void probe_chain_kernel(const uint8_t *g, const uint8_t *sa,
                                   const uint8_t *sai, const uint8_t *reads,
                                   size_t read_bytes,
                                   const ProbeRequestV3 *requests, size_t n,
                                   ProbeConfigV2 config, ProbeOutputV3 *out,
                                   ProbeStats *stats) {
  const size_t i = Warp::enabled
                       ? probe_warp_request(blockIdx.x, threadIdx.x)
                       : size_t(blockIdx.x) * blockDim.x + threadIdx.x;
  if (i >= n)
    return;
  ProbeChainSearchImpl<Warp> search{g,          sa,     sai,        reads,
                                    read_bytes, config, requests[i]};
  const auto result = search.run();
  if constexpr (Warp::enabled) {
    if (Warp::lane() != 0)
      return;
  }
  out[i] = result;
  stats[i] = search.stats;
}
extern "C" int umgpu_seed_probe_v3(const uint8_t *g, const uint8_t *sa,
                                   const uint8_t *sai, const uint8_t *reads,
                                   size_t read_bytes,
                                   const ProbeRequestV3 *requests, size_t n,
                                   ProbeConfigV2 config, ProbeOutputV3 *out,
                                   ProbeStats *stats, unsigned variant,
                                   float *event_ms, cudaStream_t stream) {
  if (variant > 1) {
    const auto drained = cudaStreamSynchronize(stream);
    return int(drained == cudaSuccess ? cudaErrorInvalidValue : drained);
  }
  cudaEvent_t begin = nullptr, end = nullptr;
  cudaError_t rc = cudaEventCreate(&begin);
  if (rc == cudaSuccess)
    rc = cudaEventCreate(&end);
  if (rc == cudaSuccess)
    rc = cudaEventRecord(begin, stream);
  if (rc == cudaSuccess) {
    if (variant == 0)
      probe_chain_kernel<ProbeScalar><<<(n + 127) / 128, 128, 0, stream>>>(
          g, sa, sai, reads, read_bytes, requests, n, config, out, stats);
    else
      probe_chain_kernel<ProbeDeviceWarp>
          <<<probe_warp_blocks(n), 128, 0, stream>>>(
              g, sa, sai, reads, read_bytes, requests, n, config, out, stats);
    rc = cudaGetLastError();
  }
  if (rc == cudaSuccess)
    rc = cudaEventRecord(end, stream);
  // The live genome/SA/SAi/read/request/output leases survive every drain path.
  const cudaError_t drained = cudaStreamSynchronize(stream);
  if (rc == cudaSuccess)
    rc = drained;
  if (rc == cudaSuccess)
    rc = cudaEventElapsedTime(event_ms, begin, end);
  if (begin)
    cudaEventDestroy(begin);
  if (end)
    cudaEventDestroy(end);
  return int(rc);
}
