// chase.cu — random-access latency & throughput on each memory class.
#include <algorithm>
#include <chrono>
#include <cstdint>
#include <limits>
#include <random>
#include <vector>

#include "common.h"

__global__ void chase(const uint64_t* __restrict__ next, uint64_t n, int steps, int threadsTotal,
                      uint64_t* out) {
  uint64_t tid = (uint64_t)blockIdx.x * blockDim.x + threadIdx.x;
  if (tid >= (uint64_t)threadsTotal)
    return;
  uint64_t p = (tid * 2654435761ull) % n;
  for (int i = 0; i < steps; ++i)
    p = next[p];
  out[tid] = p;
}
__global__ void warm_all(const uint64_t* next, uint64_t n) {
  for (uint64_t i = (uint64_t)blockIdx.x * blockDim.x + threadIdx.x; i < n;
       i += (uint64_t)gridDim.x * blockDim.x) {
    volatile uint64_t x = next[i];
    (void)x;
  }
}
static void build_cycle(uint64_t* a, uint64_t n) {
  std::mt19937_64 rng(42);
  std::vector<uint64_t> p(n);
  for (uint64_t i = 0; i < n; ++i)
    p[i] = i;
  for (uint64_t i = n - 1; i > 0; --i) {
    uint64_t j = rng() % i;
    std::swap(p[i], p[j]);
  }
  for (uint64_t i = 0; i < n; ++i)
    a[p[i]] = p[(i + 1) % n];
}
struct Res {
  double lat_ns;
  double mlookups_s;
  uint64_t checksum;
};
static Res run(const uint64_t* p, uint64_t n, int threadsTotal, int steps, uint64_t* d_out, cudaEvent_t e0,
               cudaEvent_t e1) {
  int tb = 128, blocks = (threadsTotal + tb - 1) / tb;
  int warm_blocks = (int)std::min<uint64_t>((n + tb - 1) / tb, 2147483647ull);
  warm_all<<<warm_blocks, tb>>>(p, n);
  CK(cudaDeviceSynchronize());
  float best = std::numeric_limits<float>::max();
  for (int r = 0; r < 3; ++r) {
    CK(cudaEventRecord(e0));
    chase<<<blocks, tb>>>(p, n, steps, threadsTotal, d_out);
    CK(cudaEventRecord(e1));
    CK(cudaEventSynchronize(e1));
    float ms;
    CK(cudaEventElapsedTime(&ms, e0, e1));
    best = std::min(best, ms);
  }
  std::vector<uint64_t> out(threadsTotal);
  CK(cudaMemcpy(out.data(), d_out, (size_t)threadsTotal * sizeof(uint64_t), cudaMemcpyDeviceToHost));
  uint64_t checksum = 0;
  for (uint64_t x : out)
    checksum ^= x + 0x9e3779b97f4a7c15ull + (checksum << 6) + (checksum >> 2);
  (void)checksum;
  double total = (double)threadsTotal * steps;
  return {best * 1e6 / steps, total / (best / 1e3) / 1e6, checksum};
}
int main(int argc, char** argv) {
  size_t maxMB = argc > 1 ? strtoull(argv[1], nullptr, 10) : 2048;
  if (maxMB == 0 || maxMB > 32768) {
    fprintf(stderr, "working set must be >0 and <= 32768 MiB (32 GiB)\n");
    return 2;
  }
  uint64_t bytes = (uint64_t)maxMB << 20, n = bytes / sizeof(uint64_t);
  cudaDeviceProp prop;
  CK(cudaGetDeviceProperties(&prop, 0));
  printf("Device %s, %d SMs\n", prop.name, prop.multiProcessorCount);
  uint64_t* d_out;
  CK(cudaMalloc(&d_out, 1ull << 24));
  cudaEvent_t e0, e1;
  CK(cudaEventCreate(&e0));
  CK(cudaEventCreate(&e1));
  uint64_t* h = (uint64_t*)malloc(bytes);
  if (!h) {
    perror("malloc");
    return 1;
  }
  build_cycle(h, n);
  uint64_t *d_dev, *d_pin, *d_man;
  CK(cudaMalloc(&d_dev, bytes));
  CK(cudaMemcpy(d_dev, h, bytes, cudaMemcpyHostToDevice));
  CK(cudaMallocHost(&d_pin, bytes));
  memcpy(d_pin, h, bytes);
  CK(cudaMallocManaged(&d_man, bytes));
  memcpy(d_man, h, bytes);
  struct C {
    const char* name;
    const uint64_t* p;
  } cls[] = {{"cudaMalloc (device)", d_dev},
             {"cudaMallocHost (pinned)", d_pin},
             {"cudaMallocManaged", d_man},
             {"plain malloc (HMM/ATS)", h}};
  size_t sizesMB[] = {16, 64, 256, 1024, 4096, 16384};
  printf("\n%-26s %8s %20s %14s %14s\n", "memory class", "WS(MiB)", "lat ns/hop (1 chain)", "Mlk/s @64K T",
         "Mlk/s @1M T");
  for (size_t ws : sizesMB) {
    if (((uint64_t)ws << 20) > bytes)
      break;
    uint64_t nws = ((uint64_t)ws << 20) / sizeof(uint64_t);
    build_cycle(h, nws);
    CK(cudaMemcpy(d_dev, h, nws * sizeof(uint64_t), cudaMemcpyHostToDevice));
    memcpy(d_pin, h, nws * sizeof(uint64_t));
    memcpy(d_man, h, nws * sizeof(uint64_t));
    for (auto& c : cls) {
      Res lat = run(c.p, nws, 1, 20000, d_out, e0, e1), t64 = run(c.p, nws, 65536, 256, d_out, e0, e1),
          t1m = run(c.p, nws, 1 << 20, 64, d_out, e0, e1);
      printf("%-26s %8zu %20.0f %14.0f %14.0f [checksums %llx/%llx/%llx]\n", c.name, ws, lat.lat_ns,
             t64.mlookups_s, t1m.mlookups_s, (unsigned long long)lat.checksum,
             (unsigned long long)t64.checksum, (unsigned long long)t1m.checksum);
    }
  }
  uint64_t nws = std::min<uint64_t>(n, (uint64_t)1024 * 1024 * 1024 / sizeof(uint64_t));
  build_cycle(h, nws);
  uint64_t p = 0;
  auto t0 = std::chrono::steady_clock::now();
  for (int i = 0; i < 2000000; ++i)
    p = h[p];
  double ns = std::chrono::duration<double, std::nano>(std::chrono::steady_clock::now() - t0).count() / 2e6;
  printf("\nCPU single-thread dependent chase, 1024 MiB WS: %.0f ns/hop (sink %llu)\n", ns,
         (unsigned long long)p);
  return 0;
}
