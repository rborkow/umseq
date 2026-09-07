// Compile and run this with STAR's pinned source directory as the include path.
// It includes the producer TU so the exercised routine is the actual producer,
// then compares its tuple to the prefix oracle transcribed from STAR 2.7.11b
// ReadAlign_maxMappableLength2strands.cpp lines 22-96.
#define STAR_INTEGRATE 1
#include "star_integrate_window.cpp"

#include <cassert>
#include <cstring>

// Link the actual enabled producer TU without bringing in the asynchronous
// coordinator.  The prefix tests below exercise its real sparse-prefix leaf.
namespace star_integrate {
bool window_remaining() { return false; }
bool setup(const Parameters &, const Genome &) { return false; }
void submit_window(std::vector<WindowRead> &&) {}
} // namespace star_integrate

namespace {
bool upstream_prefix_oracle(const Parameters &p, Genome &g, const char *read,
                            uint start, uint length, uint idir,
                            star_integrate::InnerCall &got) {
  const bool dir_r = idir == 0;
  const uint lmax = std::min(p.pGe.gSAindexNbases, length);
  uint ind1 = 0;
  if (dir_r)
    for (uint ii = 0; ii < lmax; ++ii)
      ind1 = (ind1 << 2LLU) + static_cast<uint>(read[start + ii]);
  else
    for (uint ii = 0; ii < lmax; ++ii)
      ind1 = (ind1 << 2LLU) + 3 - static_cast<uint>(read[start - ii]);
  uint lind = lmax, isa1 = 0;
  while (lind > 0) {
    isa1 = g.SAi[g.genomeSAindexStart[lind - 1] + ind1];
    if ((isa1 & g.SAiMarkAbsentMaskC) == 0)
      break;
    --lind;
    ind1 >>= 2;
  }
  if (!lind)
    return false;
  uint isa2;
  bool isa2_good = true;
  if (g.genomeSAindexStart[lind - 1] + ind1 + 1 < g.genomeSAindexStart[lind]) {
    isa2 = g.SAi[g.genomeSAindexStart[lind - 1] + ind1 + 1];
    if ((isa2 & g.SAiMarkAbsentMaskC) == 0)
      isa2 = (isa2 & g.SAiMarkNmask) - 1;
    else {
      isa2 = g.nSA - 1;
      isa2_good = false;
    }
  } else {
    isa2 = g.nSA - 1;
    isa2_good = false;
  }
  const bool isa1_no_n = (isa1 & g.SAiMarkNmaskC) == 0;
  if ((lind < p.pGe.gSAindexNbases && isa1_no_n && isa2_good) ||
      (isa1 == isa2 && isa1_no_n && isa2_good))
    return false;
  got = {start, length, isa1 & g.SAiMarkNmask,
         isa2,  dir_r,  isa2_good && isa1_no_n ? lind : 0,
         7,     3,      0,
         2,     5,      1,
         0};
  return true;
}
} // namespace

int main() {
  Parameters p;
  p.pGe.gSAsparseD = 1;
  p.pGe.gSAindexNbases = 2;
  Genome g(p, p.pGe);
  g.nSA = 20;
  g.SAiMarkAbsentMaskC = 0x80000000U;
  g.SAiMarkNmask = 0x7fffffffU;
  g.SAiMarkNmaskC = 0x80000000U;
  uint starts[] = {0, 4, 20, 20};
  g.genomeSAindexStart = starts;
  g.SAi.defineBits(32, 32);
  g.SAi.allocateArray();
  // Read 01 has L=2 prefix index 1. Its index entries form [5,9), forcing
  // STAR's inner search branch rather than either direct branch.
  g.SAi.writePacked(5, 5);
  g.SAi.writePacked(6, 9);
  const char read[] = {0, 0, 1, 2, 3};
  star_integrate::InnerCall expected{};
  assert(upstream_prefix_oracle(p, g, read, 1, 2, 0, expected));
  std::vector<star_integrate::InnerCall> actual;
  assert(star_integrate::append_prefix_call(p, g, read, 1, 2, 0, 7, 3, 2, 5, 1,
                                            actual));
  assert(actual.size() == 1);
  const auto &a = actual.front();
  assert(a.start == expected.start && a.length == expected.length &&
         a.low == expected.low && a.high == expected.high &&
         a.dir == expected.dir && a.prefix == expected.prefix &&
         a.piece == expected.piece && a.fragment == expected.fragment &&
         a.distance == expected.distance && a.nstart == expected.nstart &&
         a.lstart == expected.lstart && a.istart == expected.istart &&
         a.generation == 0 && a.piece_start == 0 && a.piece_length == 0 &&
         a.kind == 0 && a.read_id == 0 && a.index_epoch == 0);
  // The producer must never publish either direct branch. Equality is STAR's
  // unique-direct branch; a short known prefix is its other direct branch.
  g.SAi.writePacked(6, 6);
  actual.clear();
  assert(!upstream_prefix_oracle(p, g, read, 1, 2, 0, expected));
  assert(!star_integrate::append_prefix_call(p, g, read, 1, 2, 0, 7, 3, 2, 5, 1,
                                             actual));
  g.SAi.writePacked(6, 9);
  p.pGe.gSAindexNbases = 3;
  actual.clear();
  assert(!upstream_prefix_oracle(p, g, read, 1, 2, 0, expected));
  assert(!star_integrate::append_prefix_call(p, g, read, 1, 2, 0, 7, 3, 2, 5, 1,
                                             actual));
  g.SAi.deallocateArray();
}
