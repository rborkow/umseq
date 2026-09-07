// PROBE focused collective/bounds tests. This is host emulation, not CUDA.
#include "probe_warp_host.h"
#include <cassert>
#include <cstdio>
struct Case {
  std::vector<uint8_t> genome, sa, reads;
  ProbeConfig c{256, 17, 32};
  ProbeRequest r{};
};
int main() {
  for (size_t n : {0, 1, 2, 3, 4, 5, 31, 32, 33, 127, 128, 129}) {
    std::vector<unsigned> visits(n), writes(n);
    const size_t start = 13;
    std::vector<unsigned> request_visits(start + n);
    for (size_t block = 0; block < probe_warp_blocks(n); ++block) {
      for (unsigned thread = 0; thread < 128; ++thread) {
        const size_t i = probe_warp_request(block, thread);
        // Verify all lanes of each warp take the same early-return decision.
        assert((i < n) == (probe_warp_request(block, thread & ~31u) < n));
        if (i >= n)
          continue;
        ++visits[i];
        ++request_visits[start + i];
        if ((thread & 31) == 0)
          ++writes[i];
      }
    }
    for (size_t i = 0; i < n; ++i) {
      assert(visits[i] == 32 && writes[i] == 1);
      assert(request_visits[start + i] == 32);
    }
    for (size_t i = 0; i < start; ++i)
      assert(request_visits[i] == 0);
  }
  std::vector<Case> cases;
  for (ProbeU64 dir : {0, 1}) {
    for (bool reverse : {false, true}) {
      for (ProbeU64 length : {1, 31, 32, 33, 63, 64, 65, 97}) {
        for (ProbeU64 prefix : {0, 3, 32}) {
          if (prefix > length || (dir == 0 && prefix == length))
            continue;
          for (ProbeU64 mismatch : {0, 1, 30, 31, 32, 33, 63, 64, 96, 97}) {
            if (mismatch < prefix || mismatch > length)
              continue;
            for (uint8_t symbol : {1, 4, 5}) {
              Case t;
              t.genome.assign(656, 5);
              t.reads.assign(length * 2, 0);
              t.sa.assign(16 * 33 / 8 + 8, 0);
              t.r = {0,      0,      length, length, dir ? 0 : length - 1,
                     length, prefix, 0,      16,     dir};
              // Fill the actually selected read strand, keeping complements.
              const ProbeU64 offset = ((dir == 1) != reverse) ? 0 : length;
              for (ProbeU64 k = 0; k < length; ++k) {
                t.reads[offset + k] = 0;
                t.reads[(length - offset) + k] = 3;
                const ProbeU64 gp = reverse ? 200 + 128 - k : 200 + 128 + k;
                t.genome[gp] = k == mismatch ? symbol : 0;
              }
              const ProbeU64 encoded =
                  reverse ? (ProbeU64(1) << 32) | 127 : 128;
              for (unsigned i = 0; i < 17; ++i)
                for (unsigned bit = 0; bit < 33; ++bit)
                  if (encoded & (ProbeU64(1) << bit))
                    t.sa[(i * 33 + bit) / 8] |=
                        uint8_t(1 << ((i * 33 + bit) % 8));
              cases.push_back(std::move(t));
            }
          }
        }
      }
    }
  }
  // Invalid requests must reject uniformly before/after the packed broadcast.
  for (unsigned kind = 0; kind < 6; ++kind) {
    Case t = cases.front();
    switch (kind) {
    case 0:
      t.r.tag = 1;
      break;
    case 1:
      t.r.dir = 2;
      break;
    case 2:
      t.r.high = t.c.n_sa;
      break;
    case 3:
      t.r.s1 = t.reads.size();
      break;
    case 4:
      t.r.prefix = t.r.length + 1;
      break;
    case 5:
      t.sa[0] = 255;
      t.sa[1] = 255;
      break;
    }
    cases.push_back(std::move(t));
  }
  // Prefix==length empty comparisons and reverse source-pointer rejection.
  for (bool invalid : {false, true}) {
    Case t = cases.front();
    t.r.prefix = t.r.length;
    t.r.dir = invalid ? 0 : 1;
    cases.push_back(std::move(t));
  }
  // Highest supported packed width, reverse leading padding, and a reverse
  // genome starting-offset failure even though the comparison is empty.
  for (unsigned kind = 0; kind < 5; ++kind) {
    Case t = cases.front();
    t.c.strand_bit = kind < 2 ? 53 : 32;
    t.r.dir = 1;
    t.r.start = 0;
    if (kind == 3)
      t.r.prefix = t.r.length;
    if (kind == 4) {
      t.r = {0, 0, 2, 2, 1, 1, 1, 0, 16, 0};
      t.reads = {0, 0, 3, 3}; // valid empty reverse read comparison
    }
    const ProbeU64 encoded =
        kind == 0 || kind == 4 ? 128 : (ProbeU64(1) << t.c.strand_bit) | 255;
    const unsigned width = unsigned(t.c.strand_bit + 1);
    t.sa.assign(16 * width / 8 + 8, 0);
    for (unsigned i = 0; i < 17; ++i)
      for (unsigned bit = 0; bit < width; ++bit)
        if (encoded & (ProbeU64(1) << bit))
          t.sa[(i * width + bit) / 8] |= uint8_t(1 << ((i * width + bit) % 8));
    cases.push_back(std::move(t));
  }
  std::vector<std::thread> lanes;
  for (unsigned lane = 0; lane < 32; ++lane) {
    lanes.emplace_back([&, lane] {
      ProbeHostWarp::id = lane;
      for (const auto &t : cases) {
        ProbeSearch scalar{t.genome.data(), t.sa.data(), t.reads.data(),
                           t.reads.size(),  t.c,         t.r};
        ProbeSearchImpl<ProbeHostWarp> warp{
            t.genome.data(), t.sa.data(), t.reads.data(),
            t.reads.size(),  t.c,         t.r};
        const auto expected = scalar.run(), actual = warp.run();
        assert(std::memcmp(&expected, &actual, sizeof(actual)) == 0);
        assert(std::memcmp(&scalar.stats, &warp.stats, sizeof(ProbeStats)) ==
               0);
        ProbeHostWarp::barrier();
      }
    });
  }
  for (auto &lane : lanes)
    lane.join();
  printf(
      "PROBE host warp: %zu cases x 32 lanes, partial-block mapping passed\n",
      cases.size());
}
