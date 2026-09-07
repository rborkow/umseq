// PROBE syntax-check declarations ONLY. Never include in a real CUDA build.
// No runtime implementations, linkage, device code generation, or GPU
// validation.
#pragma once
#include <cstddef>
#define __host__ __attribute__((host))
#define __device__ __attribute__((device))
#define __global__ __attribute__((global))
struct dim3 {
  unsigned x, y, z;
  __host__ __device__ constexpr dim3(unsigned a = 1, unsigned b = 1,
                                     unsigned c = 1)
      : x(a), y(b), z(c) {}
};
extern __device__ const dim3 threadIdx, blockIdx, blockDim;
using cudaStream_t = void *;
using cudaEvent_t = void *;
enum cudaError_t { cudaSuccess, cudaErrorInvalidValue };
cudaError_t cudaEventCreate(cudaEvent_t *);
cudaError_t cudaEventRecord(cudaEvent_t, cudaStream_t);
cudaError_t cudaStreamSynchronize(cudaStream_t);
cudaError_t cudaEventElapsedTime(float *, cudaEvent_t, cudaEvent_t);
cudaError_t cudaEventDestroy(cudaEvent_t);
cudaError_t cudaGetLastError();
__device__ unsigned long long __shfl_sync(unsigned, unsigned long long, int,
                                          int = 32);
__device__ unsigned __ballot_sync(unsigned, int);
__device__ int __ffs(int);
extern "C" int cudaConfigureCall(dim3, dim3, size_t = 0,
                                 cudaStream_t = nullptr);
extern "C" int cudaSetupArgument(const void *, size_t, size_t);
extern "C" int cudaLaunch(const void *);
