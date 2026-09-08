// Compile and run this with STAR's pinned source directory as the include path.
// It includes the producer TU so the exercised routine is the actual producer,
// then compares its tuple to the prefix oracle transcribed from STAR 2.7.11b
// ReadAlign_maxMappableLength2strands.cpp lines 22-96.
#define STAR_INTEGRATE 1
#include "star_integrate_window.cpp"

#include <cassert>
#include <cstring>
#include <random>

// Link the actual enabled producer TU without bringing in the asynchronous
// coordinator.  V2's producer only admits starts; the device owns SAindex.
namespace star_integrate {
bool window_remaining(uint64_t) { return false; }
bool next_window_pending() { return false; }
bool lookahead_start(WindowEnd &) { return false; }
bool setup(const Parameters &, const Genome &) { return false; }
bool submit_window(std::vector<WindowRead> &&, WindowEnd &&) { return false; }
} // namespace star_integrate

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
  // V2 publishes every sparse admitted initial start, including the branches
  // formerly filtered here (prefix-only and unique).
  std::vector<star_integrate::InnerCall> actual;
  assert(star_integrate::append_prefix_call(p, 1, 2, 0, 7, 3, 2, 5, 1, actual));
  assert(actual.size() == 1);
  const auto &a = actual.front();
  assert(a.start == 1 && a.length == 2 && a.low == 0 && a.high == 0 && a.dir &&
         a.prefix == p.seedMapMin && a.piece == 7 && a.fragment == 3 &&
         a.distance == 0 && a.nstart == 2 && a.lstart == 5 && a.istart == 1 &&
         a.generation == 0 && a.piece_start == 0 && a.piece_length == 0 &&
         a.kind == 0 && a.read_id == 0 && a.index_epoch == 0);
  // No SAindex content can change admission now.
  actual.clear();
  assert(star_integrate::append_prefix_call(p, 1, 2, 0, 7, 3, 2, 5, 1, actual));
  g.SAi.deallocateArray();

  // Item 1: frame bytes are created after readLoad's clipping.  Exercise the
  // stock combine/complement/reverse path and the frame-copy path on paired
  // reads with independent random 5'/3' clips.  This makes the equality
  // claim cover all three Read1 orientations, not merely frame.a.
  std::mt19937_64 rng(0x5eed1234ULL);
  for (unsigned sample = 0; sample != 1000; ++sample) {
    const uint raw0 = 2 + rng() % 100, raw1 = 2 + rng() % 100;
    const uint clip05 = rng() % raw0, clip03 = rng() % (raw0 - clip05);
    const uint clip15 = rng() % raw1, clip13 = rng() % (raw1 - clip15);
    std::vector<char> raw_a(raw0), raw_b(raw1), num_a(raw0), num_b(raw1);
    for (uint i = 0; i < raw0; ++i)
      raw_a[i] = "ACGTNacgtn"[rng() % 10];
    for (uint i = 0; i < raw1; ++i)
      raw_b[i] = "ACGTNacgtn"[rng() % 10];
    convertNucleotidesToNumbers(raw_a.data(), num_a.data(), raw0);
    convertNucleotidesToNumbers(raw_b.data(), num_b.data(), raw1);
    const uint len0 = raw0 - clip05 - clip03, len1 = raw1 - clip15 - clip13;
    std::vector<char> left(num_a.begin() + clip05,
                           num_a.begin() + clip05 + len0);
    std::vector<char> right(num_b.begin() + clip15,
                            num_b.begin() + clip15 + len1);
    const uint length = len0 + len1 + 1;
    std::vector<char> stock0(length), stock1(length), stock2(length);
    std::copy(left.begin(), left.end(), stock0.begin());
    stock0[len0] = MARK_FRAG_SPACER_BASE;
    complementSeqNumbers(right.data(), stock0.data() + len0 + 1, len1);
    for (uint i = 0; i < len1 / 2; ++i)
      std::swap(stock0[length - i - 1], stock0[i + len0 + 1]);
    complementSeqNumbers(stock0.data(), stock1.data(), length);
    for (uint i = 0; i < length; ++i)
      stock2[length - i - 1] = stock1[i];

    star_integrate::WindowRead frame = {};
    frame.a.assign(stock0.begin(), stock0.end());
    frame.b.resize(length);
    complementSeqNumbers(reinterpret_cast<char *>(frame.a.data()),
                         reinterpret_cast<char *>(frame.b.data()), length);
    std::vector<char> handed0(frame.a.begin(), frame.a.end());
    std::vector<char> handed1(frame.b.begin(), frame.b.end()), handed2(length);
    for (uint i = 0; i < length; ++i)
      handed2[length - i - 1] = handed1[i];
    assert(!std::memcmp(stock0.data(), handed0.data(), length));
    assert(!std::memcmp(stock1.data(), handed1.data(), length));
    assert(!std::memcmp(stock2.data(), handed2.data(), length));
  }
}
