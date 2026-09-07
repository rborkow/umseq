// Exercise the actual counter implementation, not a fabricated JSON sidecar.
#include <cstdlib>
#include <fstream>
#include <stdexcept>
#include <string>
#include <vector>
struct Parameters {
  unsigned long long seedSearchLmax = 0;
  struct {
    unsigned long long gSAsparseD = 1;
  } pGe;
};
namespace capture {
using Bytes = std::vector<unsigned char>;
void require(bool ok, const char *reason) {
  if (!ok)
    throw std::runtime_error(reason);
}
void exclusive(const std::string &path, const Bytes &bytes) {
  std::ofstream out(path, std::ios::binary);
  out.write(reinterpret_cast<const char *>(bytes.data()), bytes.size());
  require(bool(out), "fixture output failed");
}
} // namespace capture
#include "chain_position_impl.hpp"

void request(unsigned long long ip, unsigned long long start,
             unsigned long long dir, unsigned long long piece,
             unsigned long long lstart, unsigned long long mapped,
             unsigned gathers) {
  using namespace chain_position;
  const U offset = start * lstart + mapped;
  const U shift = dir == 0 ? 100 + offset : 100 + piece - 1 - offset;
  chain_opportunity(ip, start, dir, 100, piece, lstart);
  outer_begin(ip, start, dir, 100, piece, lstart, mapped, shift,
              piece - offset);
  inner_begin(shift, piece - offset, 2, 20, dir == 0, 0);
  for (unsigned i = 0; i < gathers; ++i) {
    compare_begin();
    compared(10);
  }
  inner_end();
  outer_end();
}
int main(int argc, char **argv) {
  if (argc != 2)
    return 2;
  setenv("SSIR_COUNTERS_DIRECTORY", argv[1], 1);
  chain_position::startup();
  chain_position::read_begin();
  request(0, 1, 0, 75, 37, 0,
          10); // Initial, offset37: still included in union.
  request(0, 1, 0, 75, 37, 3, 20);  // Adaptive offset40: grid-only.
  request(1, 1, 1, 75, 37, 20, 30); // Adaptive offset57: NOT grid20.
  request(1, 0, 1, 75, 37, 0, 40);  // Initial+grid overlap, counted once.
  request(2, 1, 0, 4200, 4090, 10,
          20); // Full offset4100: preserve grid class before bucketing.
  chain_position::chain_opportunity(3, 0, 1, 100, 75, 37);
  chain_position::reverse_suppressed(3, 0, 1, 100, 75, 37);
  chain_position::read_end();
  chain_position::finish();
}
