// bw_paths.cu — measure CUDA data paths on a unified-memory box.
#include <cstring>
#include <ctime>
#include <random>
#include <vector>

#include "common.h"

__global__ void read_sum(const uint4* p, size_t n16, unsigned long long* out) {
  unsigned long long a = 0;
  for (size_t i = (size_t)blockIdx.x * blockDim.x + threadIdx.x; i < n16;
       i += (size_t)gridDim.x * blockDim.x) {
    uint4 v = p[i];
    a += v.x + v.y + v.z + v.w;
  }
  out[blockIdx.x * blockDim.x + threadIdx.x] = a;
}
__global__ void gather(const unsigned* p, size_t n, unsigned long long* out, unsigned seed) {
  unsigned long long a = 0;
  unsigned s = seed ^ (blockIdx.x * 7919u + threadIdx.x * 104729u);
  for (int k = 0; k < 64; ++k) {
    s = s * 1664525u + 1013904223u;
    a += p[(size_t)s % n];
  }
  out[blockIdx.x * blockDim.x + threadIdx.x] = a;
}
struct T {
  double first_wall, best_dev, median_dev, median_wall;
  unsigned long long checksum;
};
static T kernel_time(const void* p, size_t bytes, size_t n16, size_t n4, int blocks, int threads, bool random,
                     unsigned seed, cudaEvent_t e0, cudaEvent_t e1, unsigned long long* out) {
  float dev[5], wall[5];
  for (int r = 0; r < 5; ++r) {
    double t = wall_now();
    CK(cudaEventRecord(e0));
    if (random)
      gather<<<blocks, threads>>>((const unsigned*)p, n4, out, seed + r);
    else
      read_sum<<<blocks, threads>>>((const uint4*)p, n16, out);
    CK(cudaEventRecord(e1));
    CK(cudaEventSynchronize(e1));
    wall[r] = (float)((wall_now() - t) * 1e3);
    CK(cudaEventElapsedTime(&dev[r], e0, e1));
  }
  CK(cudaDeviceSynchronize());
  size_t count = (size_t)blocks * threads;
  std::vector<unsigned long long> h(count);
  CK(cudaMemcpy(h.data(), out, count * sizeof(*out), cudaMemcpyDeviceToHost));
  unsigned long long sum = 0;
  for (auto x : h)
    sum += x;
  if (!random) {
    unsigned char b = 0;
    CK(cudaMemcpy(&b, p, 1, cudaMemcpyDefault));
    unsigned long long expected = (unsigned long long)n16 * 4 * (b * 0x01010101ull);
    if (sum != expected)
      fprintf(stderr, "stream checksum mismatch: got %llx expected %llx\n", sum, expected);
  }
  return {wall[0], *std::min_element(dev, dev + 5), median5(dev), median5(wall), sum};
}
static void clocks(const char* when) {
  FILE* f = popen(
      "nvidia-smi --query-gpu=clocks.sm,clocks.mem,temperature.gpu,power.draw --format=csv,noheader", "r");
  printf("nvidia-smi (%s):\n", when);
  if (!f) {
    perror("popen nvidia-smi");
    return;
  }
  char line[256];
  while (fgets(line, sizeof line, f))
    fputs(line, stdout);
  pclose(f);
}
static void print_kernel(const char* name, const T& t, size_t bytes, bool random) {
  double gb = bytes / (t.median_dev / 1e3) / 1e9;
  printf(
      "%-38s first-wall %8.3f ms  device best/median %8.3f/%8.3f ms  wall median %8.3f ms  %8.2f GB/s  [sum "
      "%llx]\n",
      name, t.first_wall, t.best_dev, t.median_dev, t.median_wall, random ? 0.0 : gb,
      (unsigned long long)t.checksum);
}
int main(int argc, char** argv) {
  size_t MB = argc > 1 ? strtoull(argv[1], nullptr, 10) : 1024;
  uint64_t bytes = (uint64_t)MB << 20;
  if (MB == 0 || MB > 32768) {
    fprintf(stderr, "size must be >0 and <= 32768 MiB\n");
    return 2;
  }
  uint64_t seed = argc > 2 ? strtoull(argv[2], nullptr, 10) : (uint64_t)time(nullptr);
  cudaDeviceProp prop;
  CK(cudaGetDeviceProperties(&prop, 0));
  printf(
      "Device: %s sm_%d%d pageableMemoryAccess=%d concurrentManagedAccess=%d hostPageTables=%d "
      "directManagedHost=%d\n",
      prop.name, prop.major, prop.minor, prop.pageableMemoryAccess, prop.concurrentManagedAccess,
      prop.pageableMemoryAccessUsesHostPageTables, prop.directManagedMemAccessFromHost);
  printf("Buffer: %zu MiB, seed %llu\n", MB, (unsigned long long)seed);
  clocks("before");
  int blocks = prop.multiProcessorCount * 8, threads = 256;
  size_t count = (size_t)blocks * threads;
  unsigned long long* out;
  CK(cudaMallocManaged(&out, count * sizeof(*out)));
  cudaEvent_t e0, e1;
  CK(cudaEventCreate(&e0));
  CK(cudaEventCreate(&e1));
  size_t n16 = bytes / 16, n4 = bytes / 4;
  void* dev;
  CK(cudaMalloc(&dev, bytes));
  CK(cudaMemset(dev, 1, bytes));
  void* pin;
  CK(cudaMallocHost(&pin, bytes));
  memset(pin, 3, bytes);
  void* man;
  CK(cudaMallocManaged(&man, bytes));
  memset(man, 4, bytes);
  void* plain = malloc(bytes);
  if (!plain) {
    perror("malloc");
    return 1;
  }
  memset(plain, 5, bytes);
  void* page = malloc(bytes);
  if (!page) {
    perror("malloc");
    return 1;
  }
  memset(page, 2, bytes);
  struct C {
    const char* name;
    void* p;
  };
  std::vector<C> cls = {{"cudaMalloc device", dev},
                        {"cudaMallocHost pinned", pin},
                        {"cudaMallocManaged", man},
                        {"plain malloc HMM/ATS", plain}};
  std::mt19937_64 rng(seed);
  std::shuffle(cls.begin(), cls.end(), rng);
  printf("memory-class order:");
  for (auto& c : cls)
    printf(" %s", c.name);
  printf("\n");
  for (auto& c : cls) {
    T r = kernel_time(c.p, bytes, n16, n4, blocks, threads, false, (unsigned)seed, e0, e1, out);
    print_kernel(c.name, r, bytes, false);
    T g = kernel_time(c.p, bytes, n16, n4, blocks, threads, true, (unsigned)seed, e0, e1, out);
    print_kernel("  random gather", g, bytes, true);
  }
  void* cpu = malloc(bytes);
  double best = 1e30;
  unsigned long long checksum = 0;
  for (int r = 0; r < 5; ++r) {
    double t = wall_now();
    memcpy(cpu, page, bytes);
    if (bytes >= sizeof(unsigned long long))
      checksum ^= ((unsigned long long*)cpu)[r % (bytes / sizeof(unsigned long long))];
    best = std::min(best, wall_now() - t);
  }
  printf("CPU memcpy (consumed checksum %llx): %.2f GB/s\n", checksum, 2 * bytes / best / 1e9);
  clocks("after");
  return 0;
}
