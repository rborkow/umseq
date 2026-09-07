// Build this with the unmodified SSIRv1 format.cpp and sha256.cpp copied from
// the approved tooling snapshot. It deliberately exposes no parser internals.
#include "format.hpp"
#include <array>
#include <cstdio>
#include <stdexcept>
static ssir::Digest hex(const char *s) {
  ssir::Digest d{};
  unsigned n = 0;
  for (; *s; ++s) {
    unsigned x;
    if (*s >= '0' && *s <= '9')
      x = *s - '0';
    else if (*s >= 'a' && *s <= 'f')
      x = *s - 'a' + 10;
    else
      throw std::runtime_error("bad hex");
    if (n >= 64)
      throw std::runtime_error("digest length");
    if (!(n & 1))
      d[n / 2] = x << 4;
    else
      d[n / 2] |= x;
    ++n;
  }
  if (n != 64)
    throw std::runtime_error("digest length");
  return d;
}
int main(int argc, char **argv) {
  try {
    if (argc != 5)
      throw std::runtime_error("usage: FILE SOURCE_SHA INDEX_SHA RUNTIME_SHA");
    auto r =
        ssir::parse_file(argv[1], {hex(argv[2]), hex(argv[3]), hex(argv[4])});
    if (!r.ok)
      throw std::runtime_error(r.diagnostic);
    std::printf("SSIRv1 OK records=%llu reads=%llu inners=%llu\n",
                (unsigned long long)r.records, (unsigned long long)r.reads,
                (unsigned long long)r.inners);
  } catch (const std::exception &e) {
    std::fprintf(stderr, "ssir_validate: %s\n", e.what());
    return 2;
  }
}
