#ifndef STAR_INTEGRATE_TEST_READ_ALIGN_H
#define STAR_INTEGRATE_TEST_READ_ALIGN_H
#include <cstdint>
class ReadAlign {
public:
  uint64_t iReadAll;
  char **Read1;
  explicit ReadAlign(uint64_t n = 0) : iReadAll(n), Read1(0) {}
};
#endif
