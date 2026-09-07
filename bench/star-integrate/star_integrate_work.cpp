#include "star_integrate_work.hpp"

#if STAR_INTEGRATE && STAR_INTEGRATE_COUNTERS

#include <cstdlib>
#include <fstream>
#include <mutex>
#include <string>

namespace star_integrate_work {
namespace {
struct Counts {
  uint64_t inner_calls;
  uint64_t hit_calls;
  uint64_t fallback_calls;
  uint64_t oracle_calls;
  uint64_t fallback_compared_bytes;
  uint64_t fallback_gathers;
  uint64_t oracle_compared_bytes;
  uint64_t oracle_gathers;
  Counts()
      : inner_calls(0), hit_calls(0), fallback_calls(0), oracle_calls(0),
        fallback_compared_bytes(0), fallback_gathers(0),
        oracle_compared_bytes(0), oracle_gathers(0) {}
  void add(const Counts &other) {
    inner_calls += other.inner_calls;
    hit_calls += other.hit_calls;
    fallback_calls += other.fallback_calls;
    oracle_calls += other.oracle_calls;
    fallback_compared_bytes += other.fallback_compared_bytes;
    fallback_gathers += other.fallback_gathers;
    oracle_compared_bytes += other.oracle_compared_bytes;
    oracle_gathers += other.oracle_gathers;
  }
};

struct Global {
  std::mutex mutex;
  Counts counts;
};
Global &global() {
  static Global *value = new Global;
  return *value;
}

struct Local {
  Counts counts;
  ~Local() {
    std::lock_guard<std::mutex> lock(global().mutex);
    global().counts.add(counts);
  }
};
thread_local Local local;
thread_local Arm active_arm = ARM_NONE;

void merge_local() {
  std::lock_guard<std::mutex> lock(global().mutex);
  global().counts.add(local.counts);
  local.counts = Counts();
}

void count_call(Arm arm) {
  if (arm == ARM_ORACLE)
    ++local.counts.oracle_calls;
}
} // namespace

Scope::Scope(Arm arm) : previous_(active_arm) {
  active_arm = arm;
  count_call(arm);
}
Scope::~Scope() { active_arm = previous_; }

void inner_call(bool hit) {
  ++local.counts.inner_calls;
  if (hit)
    ++local.counts.hit_calls;
  else
    ++local.counts.fallback_calls;
}

void compare_begin() {
  if (active_arm == ARM_FALLBACK)
    ++local.counts.fallback_gathers;
  if (active_arm == ARM_ORACLE)
    ++local.counts.oracle_gathers;
}

void compared(uint64_t bytes) {
  if (active_arm == ARM_FALLBACK)
    local.counts.fallback_compared_bytes += bytes;
  if (active_arm == ARM_ORACLE)
    local.counts.oracle_compared_bytes += bytes;
}

void finish() {
  merge_local();
  const char *sidecar = std::getenv("STAR_INTEGRATE_SIDECAR");
  if (sidecar == 0 || *sidecar == '\0')
    return;
  Counts result;
  {
    std::lock_guard<std::mutex> lock(global().mutex);
    result = global().counts;
  }
  std::ofstream out((std::string(sidecar) + ".work.json").c_str(),
                    std::ios::out | std::ios::trunc);
  if (!out)
    return;
  out << "{\n"
      << "  \"inner_calls\": " << result.inner_calls << ",\n"
      << "  \"hit_calls\": " << result.hit_calls << ",\n"
      << "  \"fallback_calls\": " << result.fallback_calls << ",\n"
      << "  \"oracle_calls\": " << result.oracle_calls << ",\n"
      << "  \"fallback_compared_bytes\": " << result.fallback_compared_bytes
      << ",\n"
      << "  \"fallback_gathers\": " << result.fallback_gathers << ",\n"
      << "  \"oracle_compared_bytes\": " << result.oracle_compared_bytes
      << ",\n"
      << "  \"oracle_gathers\": " << result.oracle_gathers << "\n}\n";
}

} // namespace star_integrate_work
#endif
