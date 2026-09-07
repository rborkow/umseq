#include "seed_probe.h"
#include <cstdio>
#include <vector>
static void read(void *p, size_t n) {
  if (fread(p, 1, n, stdin) != n)
    std::abort();
}
int main() {
  ProbeConfigV2 c{};
  read(&c, sizeof(c));
  size_t sizes[5];
  read(sizes, sizeof(sizes));
  std::vector<uint8_t> g(sizes[0]), sa(sizes[1]), sai(sizes[2]),
      reads(sizes[3]);
  read(g.data(), g.size());
  read(sa.data(), sa.size());
  read(sai.data(), sai.size());
  read(reads.data(), reads.size());
  for (size_t i = 0; i < sizes[4]; ++i) {
    ProbeRequestV2 r{};
    read(&r, sizeof(r));
    ProbePrefixSearch search{g.data(),     sa.data(), sai.data(), reads.data(),
                             reads.size(), c,         r};
    const auto o = search.run();
    if (fwrite(&o, sizeof(o), 1, stdout) != 1)
      return 2;
  }
}
