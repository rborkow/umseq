// PROBE host protocol emulator: 32 real host threads, NOT GPU execution.
#pragma once
#include "seed_probe.h"
#include <array>
#include <chrono>
#include <condition_variable>
#include <cstdlib>
#include <cstring>
#include <mutex>
#include <thread>
#include <vector>
struct ProbeHostWarp {
  static constexpr bool enabled = true;
  inline static thread_local unsigned id = 0;
  inline static std::mutex mutex;
  inline static std::condition_variable cv;
  inline static unsigned arrivals = 0, generation = 0;
  inline static std::array<ProbeU64, 32> values{};
  static unsigned lane() { return id; }
  static void barrier() {
    std::unique_lock<std::mutex> lock(mutex);
    const unsigned before = generation;
    if (++arrivals == 32) {
      arrivals = 0;
      ++generation;
      cv.notify_all();
    } else if (!cv.wait_for(lock, std::chrono::seconds(10),
                            [before] { return generation != before; })) {
      std::abort(); // a missing lane is a protocol failure, not an infinite
                    // hang
    }
  }
  static ProbeU64 broadcast(ProbeU64 value) {
    values[id] = value;
    barrier();
    const auto result = values[0];
    barrier();
    return result;
  }
  static unsigned ballot(bool predicate) {
    values[id] = predicate;
    barrier();
    unsigned result = 0;
    for (unsigned j = 0; j < 32; ++j)
      result |= unsigned(values[j]) << j;
    barrier();
    return result;
  }
  static unsigned first(unsigned mask) { return __builtin_ctz(mask); }
};
inline void probe_host_warp_batch(const uint8_t *g, const uint8_t *sa,
                                  const uint8_t *reads, size_t read_bytes,
                                  ProbeConfig c,
                                  const std::vector<ProbeRequest> &requests,
                                  std::vector<ProbeOutput> &out,
                                  std::vector<ProbeStats> &stats) {
  std::vector<std::thread> lanes;
  // All lanes independently check every output/stat against scalar, so an
  // accidentally lane-varying search state is detected even off the leader.
  for (unsigned lane = 0; lane < 32; ++lane) {
    lanes.emplace_back([&, lane] {
      ProbeHostWarp::id = lane;
      for (size_t i = 0; i < requests.size(); ++i) {
        ProbeSearchImpl<ProbeHostWarp> search{g,          sa, reads,
                                              read_bytes, c,  requests[i]};
        const auto result = search.run();
        ProbeSearch scalar{g, sa, reads, read_bytes, c, requests[i]};
        const auto expected = scalar.run();
        if (std::memcmp(&result, &expected, sizeof(result)) != 0 ||
            std::memcmp(&search.stats, &scalar.stats, sizeof(ProbeStats)) != 0)
          std::abort();
        if (lane == 0) {
          out[i] = result;
          stats[i] = search.stats;
        }
        ProbeHostWarp::barrier();
      }
    });
  }
  for (auto &lane : lanes)
    lane.join();
}
