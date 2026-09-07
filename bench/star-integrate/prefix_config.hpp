// Loaded Genome is the authority for all SAindex masks and starts.
#pragma once
#include "seed_probe_abi.h"
#include <fstream>
#include <stdexcept>
template <class Genome>
ProbeConfigV2 probe_config_v2(const Genome &g, uint64_t seed_search_lmax,
                              uint64_t sai_file_bytes) {
  ProbeConfigV2 c{};
  c.inner = {g.nGenome, g.nSA, g.GstrandBit};
  c.index_bases = g.pGe.gSAindexNbases;
  if (c.index_bases == 0 || c.index_bases > 15)
    throw std::runtime_error("unsupported prefix index bases");
  c.sai_width = g.SAi.wordLength;
  c.absent_mask = g.SAiMarkAbsentMaskC;
  c.n_mask = g.SAiMarkNmask;
  c.n_mask_c = g.SAiMarkNmaskC;
  c.sparse = g.pGe.gSAsparseD;
  c.seed_search_lmax = seed_search_lmax;
  c.sai_offset = 8 * (c.index_bases + 2);
  c.sai_bytes = sai_file_bytes;
  for (uint64_t i = 0; i <= c.index_bases; ++i)
    c.starts[i] = g.genomeSAindexStart[i];
  return c;
}
// Replay input is the 28-word config encoded explicitly LE, not an inferred
// mask.
inline void write_probe_config_v2(const char *path, const ProbeConfigV2 &c) {
  std::ofstream out(path, std::ios::binary);
  const uint64_t fields[] = {
      c.inner.n_genome, c.inner.n_sa,       c.inner.strand_bit, c.index_bases,
      c.sai_width,      c.absent_mask,      c.n_mask,           c.n_mask_c,
      c.sparse,         c.seed_search_lmax, c.sai_offset,       c.sai_bytes};
  auto word = [&](uint64_t v) {
    for (unsigned i = 0; i < 8; ++i)
      out.put(char((v >> (8 * i)) & 255));
  };
  for (auto v : fields)
    word(v);
  for (auto v : c.starts)
    word(v);
  out.close();
  if (!out)
    throw std::runtime_error("prefix configuration dump failed");
}
