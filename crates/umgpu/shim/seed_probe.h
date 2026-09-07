// SPDX-License-Identifier: MIT
// PROBE structural port of replay/search.cpp; see LICENSE.star.
#pragma once
#include <cstddef>
#include <cstdint>
#ifdef __CUDACC__
#define PROBE_FN __host__ __device__
#else
#define PROBE_FN
#endif
using ProbeU64 = uint64_t;
struct ProbeRequest {
  ProbeU64 tag, s0, s1, read_len, start, length, prefix, low, high, dir;
};
struct ProbeOutput {
  ProbeU64 length, low, high, count, status;
};
struct ProbeStats {
  ProbeU64 gathers, bytes, loops, comparisons, max_compare, directions;
};
struct ProbeConfig {
  ProbeU64 n_genome, n_sa, strand_bit;
};
static_assert(sizeof(ProbeRequest) == 80 && sizeof(ProbeOutput) == 40 &&
                  sizeof(ProbeStats) == 48 && sizeof(ProbeConfig) == 24,
              "PROBE ABI");
PROBE_FN inline ProbeU64 probe_min(ProbeU64 a, ProbeU64 b) {
  return a < b ? a : b;
}
PROBE_FN inline ProbeU64 probe_mid(ProbeU64 a, ProbeU64 b) {
  return a / 2 + b / 2 + (a % 2 + b % 2) / 2;
}
PROBE_FN inline ProbeU64 probe_packed(const uint8_t *sa, ProbeU64 p,
                                      ProbeU64 width) {
  const ProbeU64 bit = p * width;
  const uint8_t *word = sa + bit / 8;
  ProbeU64 value = 0;
  // Defined unaligned LE load, matching PackedArray without C++ alignment UB.
  for (unsigned j = 0; j < 8; ++j)
    value |= ProbeU64(word[j]) << (8 * j);
  return (value >> (bit % 8)) & ((ProbeU64(1) << width) - 1);
}
struct ProbePoint {
  ProbeU64 position, length;
};
struct ProbeEndpoint {
  ProbePoint current, previous, older;
  PROBE_FN void move(ProbePoint next) {
    if (next.length > current.length) {
      older = previous;
      previous = current;
    }
    current = next;
  }
};
// Launch geometry shared with host protocol regression (128 threads/block).
PROBE_FN inline size_t probe_warp_blocks(size_t n) { return (n + 3) / 4; }
PROBE_FN inline size_t probe_warp_request(size_t block, unsigned thread) {
  return block * 4 + thread / 32;
}
struct ProbeScalar {
  static constexpr bool enabled = false;
};
// Every lane owns identical search state. Only the comparator uses lane-varying
// values, and it reduces them before returning to the shared search control
// flow.
template <class Warp = ProbeScalar> struct ProbeSearchImpl {
  const uint8_t *genome, *sa, *reads;
  ProbeU64 read_bytes;
  ProbeConfig c;
  ProbeRequest r;
  ProbeStats stats{};
  ProbeU64 status = 0;
  PROBE_FN ProbeU64 compare(ProbeU64 position, ProbeU64 length, ProbeU64 prefix,
                            bool &move_lower) {
    ProbeU64 encoded;
    if constexpr (Warp::enabled) {
      encoded = Warp::broadcast(
          Warp::lane() == 0 ? probe_packed(sa, position, c.strand_bit + 1) : 0);
    } else {
      encoded = probe_packed(sa, position, c.strand_bit + 1);
    }
    ++stats.gathers;
    ++stats.comparisons;
    const bool forward = (encoded >> c.strand_bit) == 0;
    const ProbeU64 address = encoded & ~(ProbeU64(1) << c.strand_bit);
    if (address >= c.n_genome || prefix > length || length > r.length) {
      status = 3;
      return 0;
    }
    stats.directions |= ProbeU64(1) << ((1 - r.dir) * 2 + !forward);
    // Preserve the SA gather, but form no sequence address for an empty comparison.
    if (prefix == length) {
      move_lower = false;
      return length;
    }
    const ProbeU64 base = forward ? address : c.n_genome - 1 - address;
    if ((forward && address + length > c.n_genome + 200) ||
        (!forward && (prefix > base || length > base + 201))) {
      status = 3;
      return 0;
    }
    const ProbeU64 offset = ((r.dir == 1) == forward) ? r.s0 : r.s1;
    if constexpr (Warp::enabled) {
      for (ProbeU64 begin = prefix; begin < length; begin += 32) {
        const ProbeU64 k = begin + Warp::lane();
        bool mismatch = false, lower = false;
        // Bounds validation above covers the entire comparison. Tail lanes must
        // not form/dereference sequence addresses; all lanes still vote twice.
        if (k < length) {
          const ProbeU64 rp = r.dir == 1 ? r.start + k : r.start - k;
          const ProbeU64 gp = forward ? 200 + base + k : 200 + base - k;
          const uint8_t q = reads[offset + rp], g = genome[gp];
          mismatch = q != g;
          lower = forward ? q > g : !(q > g || g > 3);
        }
        const unsigned mismatches = Warp::ballot(mismatch);
        const unsigned ordering = Warp::ballot(lower);
        if (mismatches) {
          const unsigned first = Warp::first(mismatches);
          const ProbeU64 matched = begin + first;
          const ProbeU64 examined = matched - prefix + 1;
          stats.bytes += examined;
          if (examined > stats.max_compare)
            stats.max_compare = examined;
          move_lower = ((ordering >> first) & 1) != 0;
          return matched; // uniform: no lane exits ahead of a collective
        }
      }
      const ProbeU64 examined = length - prefix;
      stats.bytes += examined;
      if (examined > stats.max_compare)
        stats.max_compare = examined;
      // Logical bytes match scalar exactly. Speculative in-range loads after a
      // first mismatch are real additional reads, NOT counted by this metric.
      return length;
    } else {
      ProbeU64 examined = 0;
      for (ProbeU64 k = prefix; k < length; ++k) {
        const ProbeU64 rp = r.dir == 1 ? r.start + k : r.start - k;
        const ProbeU64 gp = forward ? 200 + base + k : 200 + base - k;
        const uint8_t q = reads[offset + rp], g = genome[gp];
        ++examined;
        ++stats.bytes;
        if (q != g) {
          if (examined > stats.max_compare)
            stats.max_compare = examined;
          move_lower = forward ? q > g : !(q > g || g > 3);
          return k;
        }
      }
      if (examined > stats.max_compare)
        stats.max_compare = examined;
      return length; // exact: no ordering claim
    }
  }

  PROBE_FN ProbeU64 expand(ProbePoint best, const ProbeEndpoint &side) {
    ProbeU64 inside = side.previous.position;
    ProbePoint outside = side.older;
    if (side.current.length < best.length) {
      inside = best.position;
      outside = side.current;
    } else if (side.previous.length < side.current.length) {
      inside = side.current.position;
      outside = side.previous;
    }
    while ((inside > outside.position ? inside - outside.position
                                      : outside.position - inside) > 1) {
      ++stats.loops;
      const ProbeU64 middle = probe_mid(inside, outside.position);
      bool ignored = false;
      const ProbeU64 matched =
          compare(middle, best.length, outside.length, ignored);
      if (status)
        return 0;
      if (matched == best.length)
        inside = middle;
      else
        outside = {middle, matched};
    }
    return inside;
  }
  PROBE_FN ProbeOutput run() {
    if (r.tag != 0 || r.dir > 1)
      return {0, 0, 0, 0, 1};
    if (r.read_len == 0 || r.read_len > 4096 || r.length == 0 ||
        r.prefix > r.length || r.start >= r.read_len || r.low > r.high ||
        r.high >= c.n_sa || r.s0 > read_bytes ||
        r.read_len > read_bytes - r.s0 || r.s1 > read_bytes ||
        r.read_len > read_bytes - r.s1 ||
        (r.dir == 1 && r.length > r.read_len - r.start) ||
        (r.dir == 0 && r.length > r.start + 1))
      return {0, 0, 0, 0, 2};
    bool ordering = false;
    const ProbePoint first{r.low, compare(r.low, r.length, r.prefix, ordering)};
    if (status)
      return {0, 0, 0, 0, status};
    const ProbePoint last{r.high,
                          compare(r.high, r.length, r.prefix, ordering)};
    if (status)
      return {0, 0, 0, 0, status};
    ProbeEndpoint lower{first, first, first}, upper{last, last, last};
    ProbePoint best = first;
    ProbeU64 prefix = probe_min(first.length, last.length);
    while (upper.current.position - lower.current.position > 1) {
      ++stats.loops;
      const ProbeU64 middle =
          probe_mid(lower.current.position, upper.current.position);
      best = {middle, compare(middle, r.length, prefix, ordering)};
      if (status)
        return {0, 0, 0, 0, status};
      if (best.length == r.length)
        break;
      if (ordering)
        lower.move(best);
      else
        upper.move(best);
      prefix = probe_min(lower.current.length, upper.current.length);
    }
    if (best.length < r.length)
      best = lower.current.length > upper.current.length ? lower.current
                                                         : upper.current;
    const ProbeU64 low = expand(best, lower);
    if (status)
      return {0, 0, 0, 0, status};
    const ProbeU64 high = expand(best, upper);
    if (status)
      return {0, 0, 0, 0, status};
    if (low > high || low < r.low || high > r.high)
      return {0, 0, 0, 0, 4};
    return {best.length, low, high, high - low + 1, 0};
  }
};

using ProbeSearch = ProbeSearchImpl<>;
