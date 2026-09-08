// SPDX-License-Identifier: MIT
// PROBE structural port of replay/search.cpp; see LICENSE.star.
#pragma once
#include "seed_probe_abi.h"
#ifdef __CUDACC__
#define PROBE_FN __host__ __device__
#else
#define PROBE_FN
#endif
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
    // Preserve the SA gather, but form no sequence address for an empty
    // comparison.
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

// STAR ReadAlign_maxMappableLength2strands.cpp:17-97, sparse=1 only.
// This host/device body is also executed by the Mac oracle harness.
template <class Warp = ProbeScalar> struct ProbePrefixSearchImpl {
  const uint8_t *genome, *sa, *sai, *reads;
  ProbeU64 read_bytes;
  ProbeConfigV2 c;
  ProbeRequestV2 request;
  ProbeStats stats{};
  PROBE_FN ProbeOutputV2 reject(ProbeU64 status) {
    return {{0, 0, 0, 0, status}, 0};
  }
  PROBE_FN ProbeOutputV2 run() {
    ProbeRequest r = request.inner;
    if (r.tag == 0) {
      ProbeSearchImpl<Warp> search{genome, sa, reads, read_bytes, c.inner, r};
      const auto result = search.run();
      stats = search.stats;
      return {result, 0};
    }
    if (r.tag != 1 || r.dir > 1)
      return reject(1);
    if (c.sparse != 1 || c.seed_search_lmax != 0 || request.distance != 0)
      return reject(5);
    if (r.read_len == 0 || r.read_len > 4096 || r.length == 0 ||
        r.start >= r.read_len || r.s0 > read_bytes ||
        r.read_len > read_bytes - r.s0 || r.s1 > read_bytes ||
        r.read_len > read_bytes - r.s1 ||
        (r.dir == 1 && r.length > r.read_len - r.start) ||
        (r.dir == 0 && r.length > r.start + 1))
      return reject(2);
    if (!sai || c.index_bases == 0 || c.index_bases > 15 || c.sai_width == 0 ||
        c.sai_width > 63 || c.inner.n_sa == 0 || c.sai_offset > c.sai_bytes ||
        c.sai_bytes - c.sai_offset < 8 || c.starts[0] != 0)
      return reject(7);
    for (ProbeU64 k = 0; k < c.index_bases; ++k)
      if (c.starts[k + 1] != c.starts[k] + (ProbeU64(1) << (2 * (k + 1))))
        return reject(7);
    const ProbeU64 entries = c.starts[c.index_bases];
    if ((entries - 1) > (~ProbeU64(0) - 63) / c.sai_width ||
        (entries - 1) * c.sai_width / 8 + 8 > c.sai_bytes - c.sai_offset)
      return reject(7);
    ProbeU64 lind = probe_min(c.index_bases, r.length), ind1 = 0;
    for (ProbeU64 ii = 0; ii < lind; ++ii) {
      const auto b = reads[r.s0 + (r.dir == 1 ? r.start + ii : r.start - ii)];
      if (b > 3)
        return reject(8);
      ind1 = (ind1 << 2) + (r.dir == 1 ? b : 3 - b);
    }
    ProbeU64 isa1 = 0;
    while (lind > 0) {
      isa1 = probe_packed(sai + c.sai_offset, c.starts[lind - 1] + ind1,
                          c.sai_width);
      if ((isa1 & c.absent_mask) == 0)
        break;
      --lind;
      ind1 >>= 2;
    }
    if (!lind)
      return reject(6);
    ProbeU64 isa2 = c.inner.n_sa - 1;
    bool good = false;
    if (c.starts[lind - 1] + ind1 + 1 < c.starts[lind]) {
      const auto next = probe_packed(
          sai + c.sai_offset, c.starts[lind - 1] + ind1 + 1, c.sai_width);
      if ((next & c.absent_mask) == 0) {
        isa2 = (next & c.n_mask) - 1;
        good = true;
      }
    }
    const bool no_n = (isa1 & c.n_mask_c) == 0;
    r.low = isa1 & c.n_mask;
    r.high = isa2;
    r.prefix = good && no_n ? lind : 0;
    r.tag = 0;
    if (r.low > r.high || r.high >= c.inner.n_sa)
      return reject(7);
    if (lind < c.index_bases && no_n && good)
      return {{lind, isa1, isa2, isa2 - isa1 + 1, 0}, 1};
    ProbeSearchImpl<Warp> search{genome, sa, reads, read_bytes, c.inner, r};
    if (isa1 == isa2 && no_n && good) {
      bool ordering = false;
      const auto length = search.compare(isa1, r.length, lind, ordering);
      stats = search.stats;
      return {{length, isa1, isa1, 1, search.status}, 2};
    }
    const auto result = search.run();
    stats = search.stats;
    return {result, 3};
  }
};

using ProbePrefixSearch = ProbePrefixSearchImpl<>;
PROBE_FN inline void probe_add_stats(ProbeStats &a, const ProbeStats &b) {
  a.gathers += b.gathers;
  a.bytes += b.bytes;
  a.loops += b.loops;
  a.comparisons += b.comparisons;
  if (b.max_compare > a.max_compare)
    a.max_compare = b.max_compare;
  a.directions |= b.directions;
}
// STAR mapOneRead.cpp:62-75. No storeAligns effects run on the device.
template <class Warp = ProbeScalar> struct ProbeChainSearchImpl {
  const uint8_t *genome, *sa, *sai, *reads;
  ProbeU64 read_bytes;
  ProbeConfigV2 c;
  ProbeRequestV3 r;
  ProbeStats stats{};
  PROBE_FN ProbeOutputV3 run() {
    ProbeOutputV3 out{};
    if (r.dir > 1) {
      out.status = 1;
      return out;
    }
    if (c.sparse != 1 || c.seed_search_lmax != 0) {
      out.status = 5;
      return out;
    }
    if (!r.read_len || r.read_len > 4096 || r.piece_start > r.read_len ||
        r.piece_length > r.read_len - r.piece_start || !r.nstart ||
        r.istart >= r.nstart || r.s0 > read_bytes ||
        r.read_len > read_bytes - r.s0 || r.s1 > read_bytes ||
        r.read_len > read_bytes - r.s1 ||
        (r.istart && r.lstart > (~ProbeU64(0)) / r.istart)) {
      out.status = 2;
      return out;
    }
    const ProbeU64 initial = r.istart * r.lstart;
    ProbeU64 mapped = 0;
    // Subtractions avoid overflow of the source's sum on malformed requests.
    while (initial < r.piece_length && mapped < r.piece_length - initial &&
           r.seed_map_min < r.piece_length - initial - mapped) {
      if (out.n_steps == PROBE_CHAIN_CAPACITY) {
        out.status = 9;
        return out;
      }
      if (out.n_steps == r.max_steps) {
        out.status = 10;
        return out;
      }
      const ProbeU64 length = r.piece_length - initial - mapped;
      const ProbeU64 shift =
          r.dir ? r.piece_start + initial + mapped : r.piece_start + length - 1;
      ProbeRequestV2 q{
          {1, r.s0, r.s1, r.read_len, shift, length, 0, 0, 0, r.dir}, 0};
      ProbePrefixSearchImpl<Warp> search{genome,     sa, sai, reads,
                                         read_bytes, c,  q};
      const auto step = search.run();
      probe_add_stats(stats, search.stats);
      out.steps[out.n_steps++] = {
          shift,           step.inner.length, step.inner.count, step.inner.low,
          step.inner.high, step.branch,       step.inner.status};
      if (step.inner.status) {
        out.status = step.inner.status;
        return out;
      }
      if (step.inner.length > length) {
        out.status = 4;
        return out;
      }
      if (r.dir == 1 && r.istart == 0 && mapped == 0 &&
          shift + step.inner.length == r.piece_length)
        out.flag_dir_map_cleared = 1;
      if (!step.inner.length) {
        out.status = 11;
        return out;
      }
      mapped += step.inner.length;
    }
    return out;
  }
};
using ProbeChainSearch = ProbeChainSearchImpl<>;
