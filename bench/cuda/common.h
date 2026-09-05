#ifndef UNI_RNASEQ_BENCH_COMMON_H
#define UNI_RNASEQ_BENCH_COMMON_H
#include <cuda_runtime.h>

#include <algorithm>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#define CK(x)                                                                                 \
  do {                                                                                        \
    cudaError_t e = (x);                                                                      \
    if (e != cudaSuccess) {                                                                   \
      fprintf(stderr, "CUDA error %s at %s:%d\n", cudaGetErrorString(e), __FILE__, __LINE__); \
      exit(1);                                                                                \
    }                                                                                         \
  } while (0)
inline double wall_now() {
  return std::chrono::duration<double>(std::chrono::steady_clock::now().time_since_epoch()).count();
}
inline float median5(float a[5]) {
  float v[5];
  std::memcpy(v, a, sizeof v);
  std::sort(v, v + 5);
  return v[2];
}
inline long smaps_anon_huge_kb(const void* begin, size_t length) {
  uintptr_t lo = (uintptr_t)begin, hi = lo + length;
  FILE* f = fopen("/proc/self/smaps", "r");
  if (!f) {
    perror("fopen /proc/self/smaps");
    return -1;
  }
  char line[512];
  uintptr_t vl = 0, vh = 0;
  long huge = -1;
  bool in = false, found = false;
  while (fgets(line, sizeof line, f)) {
    unsigned long long a, b;
    if (sscanf(line, "%llx-%llx", &a, &b) == 2) {
      if (in && vl == lo && vh == hi) {
        found = true;
        break;
      }
      vl = (uintptr_t)a;
      vh = (uintptr_t)b;
      in = lo >= vl && hi <= vh;
      huge = -1;
    } else if (in) {
      long x;
      if (sscanf(line, "AnonHugePages: %ld kB", &x) == 1)
        huge = x;
    }
  }
  if (!found && in && vl == lo && vh == hi)
    found = true;
  fclose(f);
  return found ? huge : -1;
}
#endif
