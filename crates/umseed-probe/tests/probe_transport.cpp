// PROBE host compilation of the exact CUDA algorithm header. Not CUDA
// execution.
#include "seed_probe.h"
#ifdef PROBE_TEST_WARP
#include "probe_warp_host.h"
#endif
#include <cstdio>
#include <vector>
int main() {
  ProbeConfig c{};
  ProbeU64 read_bytes = 0, count = 0;
  if (fread(&c, sizeof(c), 1, stdin) != 1 ||
      fread(&read_bytes, 8, 1, stdin) != 1 || fread(&count, 8, 1, stdin) != 1)
    return 2;
  std::vector<uint8_t> genome(c.n_genome + 400);
  std::vector<uint8_t> sa((c.n_sa - 1) * (c.strand_bit + 1) / 8 + 8);
  std::vector<uint8_t> reads(read_bytes);
  if (fread(genome.data(), 1, genome.size(), stdin) != genome.size() ||
      fread(sa.data(), 1, sa.size(), stdin) != sa.size() ||
      fread(reads.data(), 1, reads.size(), stdin) != reads.size())
    return 3;
#ifdef PROBE_TEST_WARP
  std::vector<ProbeRequest> requests(count);
  if (fread(requests.data(), sizeof(ProbeRequest), count, stdin) != count)
    return 4;
  std::vector<ProbeOutput> outputs(count);
  std::vector<ProbeStats> stats(count);
  probe_host_warp_batch(genome.data(), sa.data(), reads.data(), read_bytes, c,
                        requests, outputs, stats);
  for (ProbeU64 i = 0; i < count; ++i) {
    fwrite(&outputs[i], sizeof(ProbeOutput), 1, stdout);
    fwrite(&stats[i], sizeof(ProbeStats), 1, stdout);
  }
#else
  for (ProbeU64 i = 0; i < count; ++i) {
    ProbeRequest r{};
    if (fread(&r, sizeof(r), 1, stdin) != 1)
      return 4;
    ProbeSearch search{genome.data(), sa.data(), reads.data(),
                       read_bytes,    c,         r};
    ProbeOutput output = search.run();
    fwrite(&output, sizeof(output), 1, stdout);
    fwrite(&search.stats, sizeof(search.stats), 1, stdout);
  }
#endif
  return 0;
}
