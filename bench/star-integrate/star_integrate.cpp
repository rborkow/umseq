// SPDX-License-Identifier: MIT
// Private STAR_INTEGRATE coordinator. STAR mapping threads never call USI.
#include "star_integrate.hpp"
#include "Genome.h"
#include "Parameters.h"
#include "ReadAlign.h"
#include "prefix_config.hpp"
#include "usi.h"
#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <fstream>
#include <map>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>
namespace star_integrate {
namespace {
// A backend call drains synchronously, but its input is owned by this thread.
// While it drains, producers append whole windows to their SPSC rings.  The
// next fill buffer is therefore already populated when the call returns.
// `target = 65536` remains the documented v1 transport floor; v2 instead
// admits at submit_floor after a prior drain, as its latency contract requires.
constexpr uint64_t target = 65536, submit_floor = 16384, cap = 262144,
                   read_cap = 4096;
// Longest a partially filled batch (>= submit_floor) waits for more producers
// before launching anyway.  Keeps the GPU fed at chunk boundaries and lets a
// single early window complete without a second producer.
constexpr uint64_t FILL_MAX_US = 2000;
static_assert(submit_floor < target && target < cap,
              "v2 admission bounds must preserve the v1 transport floor");
constexpr size_t queue_slots = 64;
enum JobState : uint8_t { COMPLETE = 1, VALID = 2, CONSUMED = 4, RETIRED = 8 };
struct Job {
  WindowRead *frame;
  InnerCall *call;
  ProbeOutputV3 out;
  ProbeStats stats;
  std::atomic<uint8_t> state;
  Job(WindowRead *f = nullptr, InnerCall *c = nullptr)
      : frame(f), call(c), out(), stats(), state(0) {}
  Job(Job &&o)
      : frame(o.frame), call(o.call), out(o.out), stats(o.stats),
        state(o.state.load(std::memory_order_relaxed)) {}
  Job &operator=(Job &&o) {
    frame = o.frame;
    call = o.call;
    out = o.out;
    stats = o.stats;
    state.store(o.state.load(std::memory_order_relaxed),
                std::memory_order_relaxed);
    return *this;
  }
  Job(const Job &) = delete;
  Job &operator=(const Job &) = delete;
};
static_assert(sizeof(Job) + sizeof(InnerCall) <= CANDIDATE_BUDGET_BYTES,
              "candidate budget covers InnerCall+Job");
struct Range {
  size_t first, last;
  Range(size_t a = 0, size_t b = 0) : first(a), last(b) {}
};
struct Window {
  std::vector<WindowRead> frames;
  std::vector<Job> jobs;
  std::vector<Range> ranges;
  // STAR visits admitted initial starts in producer order.  A frame-local
  // cursor therefore selects its next candidate without a hash/search.
  std::vector<size_t> cursors;
  size_t next_frame;
  uint64_t charged_bytes, charged_requests;
  uint64_t consumed, steps_consumed, misses, prefix_only, unique, searched,
      suppressed_unused, other_unused, rejected, cpu_fallback, read1_fallback,
      hit_bytes, hit_gathers, unused_bytes, unused_gathers;
  ProbeStats consumed_stats, suppressed_stats, other_stats;
  Window()
      : next_frame(0), charged_bytes(0), charged_requests(0), consumed(0),
        steps_consumed(0), misses(0), prefix_only(0), unique(0), searched(0),
        suppressed_unused(0), other_unused(0), rejected(0), cpu_fallback(0),
        read1_fallback(0), hit_bytes(0), hit_gathers(0), unused_bytes(0),
        unused_gathers(0), consumed_stats(), suppressed_stats(), other_stats() {
  }
};
// One producer (the mapping thread which owns a window) and one consumer
// (coordinator).  A full ring is a CPU-only admission result, never a wait.
struct SpscQueue {
  std::array<std::shared_ptr<Window>, queue_slots> slots;
  std::atomic<size_t> head, tail;
  // Producer-side admission counter: one notification at each floor crossing.
  std::atomic<uint64_t> requests;
  SpscQueue() : slots(), head(0), tail(0), requests(0) {}
  bool push(const std::shared_ptr<Window> &w) {
    const size_t t = tail.load(std::memory_order_relaxed);
    const size_t next = (t + 1) % queue_slots;
    if (next == head.load(std::memory_order_acquire))
      return false;
    slots[t] = w;
    tail.store(next, std::memory_order_release);
    return true;
  }
  bool pop(std::shared_ptr<Window> &w) {
    const size_t h = head.load(std::memory_order_relaxed);
    if (h == tail.load(std::memory_order_acquire))
      return false;
    w = std::move(slots[h]);
    head.store((h + 1) % queue_slots, std::memory_order_release);
    return true;
  }
};
struct Visits {
  uint64_t frame_cursor, frame_offsets, dispatched_jobs, lookup_jobs,
      positional_misses;
  Visits()
      : frame_cursor(0), frame_offsets(0), dispatched_jobs(0), lookup_jobs(0),
        positional_misses(0) {}
};
struct Totals {
  uint64_t batches, submitted, chains_submitted, chains_consumed,
      steps_consumed, chain_overflow, chain_max_steps, chain_no_progress,
      chain_rejected_other, shift_mismatch, flag_mismatch, step_count_mismatch,
      gpu_consumed, cpu_tails, misses, prefix_only, unique, searched,
      suppressed_unused, other_unused, rejected, faults, cpu_fallback,
      read1_fallback, coordinator_wakeups, hit_bytes, hit_gathers, unused_bytes,
      unused_gathers;
  std::vector<uint64_t> batch_sizes, fill_wait_us;
  uint64_t chain_length_hist[64];
  ProbeStats submitted_stats, consumed_stats, suppressed_stats, other_stats,
      rejected_stats;
  Totals()
      : batches(0), submitted(0), chains_submitted(0), chains_consumed(0),
        steps_consumed(0), chain_overflow(0), chain_max_steps(0),
        chain_no_progress(0), chain_rejected_other(0), shift_mismatch(0),
        flag_mismatch(0), step_count_mismatch(0), gpu_consumed(0), cpu_tails(0),
        misses(0), prefix_only(0), unique(0), searched(0), suppressed_unused(0),
        other_unused(0), rejected(0), faults(0), cpu_fallback(0),
        read1_fallback(0), coordinator_wakeups(0), hit_bytes(0), hit_gathers(0),
        unused_bytes(0), unused_gathers(0), submitted_stats(), consumed_stats(),
        suppressed_stats(), other_stats(), rejected_stats() {
    for (size_t i = 0; i < 64; ++i)
      chain_length_hist[i] = 0;
  }
};
struct State {
  std::mutex mu;
  std::condition_variable cv;
  UsiPrefixContext *ctx;
  std::thread coordinator;
  // `mu` guards setup, queue registration, lifecycle accounting and totals;
  // it is deliberately absent from the frame publication fast path.
  std::deque<Job *> pending; // retained only for old diagnostic fixtures.
  std::vector<std::unique_ptr<SpscQueue>> queues;
  uint64_t pending_bytes, epoch;
  std::atomic<uint64_t> next_generation;
  uint64_t live_bytes, live_requests;
  const Genome *index_object;
  const void *index_g, *index_sa, *index_sai;
  uint64_t index_nsa, index_ngenome;
  double setup_wall_s;
  bool tried, enabled, stopping, fault;
  Totals totals;
  Visits visits;
  State()
      : ctx(nullptr), pending_bytes(0), epoch(1), next_generation(1),
        live_bytes(0), live_requests(0), index_object(nullptr),
        index_g(nullptr), index_sa(nullptr), index_sai(nullptr), index_nsa(0),
        index_ngenome(0), setup_wall_s(0), tried(false), enabled(false),
        stopping(false), fault(false) {}
};
thread_local std::shared_ptr<Window> current_window;
thread_local WindowRead *current_frame = nullptr;
thread_local size_t current_index = 0;
thread_local ChainContext chain;
thread_local Job *chain_job = nullptr;
thread_local uint64_t chain_cursor = 0;
thread_local bool chain_rejected = false;
thread_local bool chain_stock_clear = false;
State &S();
// Per-chain outer-call count (every maxMappableLength2strands call of one
// (read, piece, dir, istart) chain, including prefix-only/unique steps). A
// chain starts at lmapped == 0; its length is flushed at the next chain start
// or at begin_map. Thread-local, merged into totals at finish. Evidence for the
// V3 per-chain output capacity; no effect on mapping.
constexpr size_t CHAIN_HIST_BINS = 64;
thread_local uint64_t chain_steps = 0;
thread_local uint64_t chain_hist[CHAIN_HIST_BINS] = {};
void flush_chain() {
  if (chain_steps) {
    ++chain_hist[chain_steps < CHAIN_HIST_BINS ? chain_steps
                                               : CHAIN_HIST_BINS - 1];
    chain_steps = 0;
  }
}
void finish_active_chain() {
  if (!chain_job || chain_rejected)
    return;
  const ProbeOutputV3 &o = chain_job->out;
  if (chain_cursor != o.n_steps) {
    ++S().totals.step_count_mismatch;
    if (strict())
      fail_strict("V3 chain step count mismatch");
  }
  const bool device_clear = o.flag_dir_map_cleared != 0;
  if (chain_stock_clear != device_clear) {
    ++S().totals.flag_mismatch;
    if (strict())
      fail_strict("V3 flagDirMap cross-check mismatch");
  }
}
thread_local SpscQueue *worker_queue = nullptr;
State &S() {
  static State s;
  return s;
}
void bind_index(const Genome &g) {
  State &s = S();
  s.index_object = &g;
  s.index_g = g.G;
  s.index_sa = g.SA.charArray;
  s.index_sai = g.SAi.charArray;
  s.index_nsa = g.nSA;
  s.index_ngenome = g.nGenome;
}
bool same_index(const Genome &g) {
  const State &s = S();
  return s.index_object == &g && s.index_g == g.G &&
         s.index_sa == g.SA.charArray && s.index_sai == g.SAi.charArray &&
         s.index_nsa == g.nSA && s.index_ngenome == g.nGenome;
}
void add_stats(ProbeStats &a, const ProbeStats &b) {
  a.gathers += b.gathers;
  a.bytes += b.bytes;
  a.loops += b.loops;
  a.comparisons += b.comparisons;
  a.max_compare = std::max(a.max_compare, b.max_compare);
  a.directions |= b.directions;
}
bool same_call(const InnerCall &a, const InnerCall &b) {
  return a.start == b.start && a.length == b.length && a.low == b.low &&
         a.high == b.high && a.dir == b.dir && a.prefix == b.prefix &&
         a.piece == b.piece && a.fragment == b.fragment &&
         a.distance == b.distance && a.nstart == b.nstart &&
         a.lstart == b.lstart && a.istart == b.istart &&
         a.generation == b.generation && a.piece_start == b.piece_start &&
         a.piece_length == b.piece_length && a.kind == b.kind &&
         a.read_id == b.read_id && a.index_epoch == b.index_epoch &&
         a.worker == b.worker && a.chunk == b.chunk &&
         a.mate_context == b.mate_context && a.split_context == b.split_context;
}
void write_stats(std::ostream &f, const ProbeStats &s) {
  f << "{\"gathers\":" << s.gathers << ",\"bytes\":" << s.bytes
    << ",\"loops\":" << s.loops << ",\"comparisons\":" << s.comparisons
    << ",\"max_compare\":" << s.max_compare
    << ",\"directions\":" << s.directions << "}";
}
bool env1(const char *n) {
  const char *v = getenv(n);
  return v && !strcmp(v, "1");
}
[[noreturn]] void strict_fail(const char *s) {
  fprintf(stderr, "STAR_INTEGRATE strict failure: %s\n", s);
  abort();
}
// Identity v1 sample layout is frozen: first/last MiB and 64 evenly-spaced
// 64 KiB slices, retaining overlap.  This FNV-1a digest is only an in-process
// consistency check between two resident copies, not a security boundary.
uint64_t sampled_hash_parts(const uint8_t *head, uint64_t head_length,
                            const uint8_t *bytes, uint64_t length,
                            uint8_t *out) {
  const uint64_t mib = 1024 * 1024, sample = 65536;
  const uint64_t total = head_length + length;
  uint64_t h = 1469598103934665603ULL;
  const auto feed = [&](uint64_t offset, uint64_t n) {
    for (uint64_t i = 0; i < n; ++i) {
      const uint64_t at = offset + i;
      const uint8_t value =
          at < head_length ? head[at] : bytes[at - head_length];
      h = (h ^ value) * 1099511628211ULL;
    }
  };
  const uint64_t edge = std::min(mib, total);
  feed(0, edge);
  feed(total - edge, edge);
  const uint64_t max_offset = total > sample ? total - sample : 0;
  for (uint64_t i = 0; i < 64; ++i) {
    const uint64_t offset = max_offset * i / 63;
    feed(offset, std::min(sample, total - offset));
  }
  memset(out, 0, 32);
  for (unsigned i = 0; i < 8; ++i)
    out[i] = static_cast<uint8_t>(h >> (i * 8));
  return h;
}
bool admitted(const Parameters &p, const Genome &g) {
  return p.runThreadN > 0 && p.runThreadN <= 20 &&
         p.pGe.gLoad == "NoSharedMemory" && !p.twoPass.yes &&
         !p.sjdbInsert.yes && !p.wasp.yes && !p.peOverlap.yes &&
         p.outFilterBySJoutStage != 2 && p.pGe.transform.type == 0 &&
         p.seedSearchLmax == 0 && g.GstrandBit == 32 && p.pGe.gSAsparseD == 1 &&
         g.G && g.SA.charArray && g.SAi.charArray;
}
void resolve_cpu(Job &j) {
  uint8_t state = j.state.load(std::memory_order_relaxed);
  while (!(state & COMPLETE) && !j.state.compare_exchange_weak(
                                    state, COMPLETE, std::memory_order_release,
                                    std::memory_order_relaxed))
    ;
}
bool valid_result(const Job &j) {
  const ProbeOutputV3 &o = j.out;
  return o.status == 0 && o.n_steps <= PROBE_CHAIN_CAPACITY;
}
void retire(Job &j, bool suppressed) {
  const uint8_t before = j.state.fetch_or(RETIRED, std::memory_order_acq_rel);
  if (before & (RETIRED | CONSUMED))
    return;
  if (!current_window || !(before & COMPLETE) || !(before & VALID))
    return;
  current_window->unused_bytes += j.stats.bytes;
  current_window->unused_gathers += j.stats.gathers;
  if (suppressed) {
    ++current_window->suppressed_unused;
    add_stats(current_window->suppressed_stats, j.stats);
  } else {
    ++current_window->other_unused;
    add_stats(current_window->other_stats, j.stats);
  }
}
void dispatch(std::vector<Job *> jobs) {
#if !STAR_INTEGRATE
  for (size_t i = 0; i < jobs.size(); ++i)
    resolve_cpu(*jobs[i]);
#else
  State &s = S();
  std::vector<uint8_t> reads;
  std::unordered_map<WindowRead *, std::pair<uint64_t, uint64_t>> offsets;
  std::vector<ProbeRequestV3> req(jobs.size());
  std::vector<ProbeOutputV3> out(jobs.size());
  std::vector<ProbeStats> stats(jobs.size());
  for (size_t i = 0; i < jobs.size(); ++i) {
    Job &j = *jobs[i];
    std::unordered_map<WindowRead *, std::pair<uint64_t, uint64_t>>::iterator
        at = offsets.find(j.frame);
    if (at == offsets.end()) {
      ++s.visits.frame_offsets;
      uint64_t a = reads.size();
      reads.insert(reads.end(), j.frame->a.begin(), j.frame->a.end());
      uint64_t b = reads.size();
      reads.insert(reads.end(), j.frame->b.begin(), j.frame->b.end());
      at = offsets.insert(std::make_pair(j.frame, std::make_pair(a, b))).first;
    }
    const InnerCall &c = *j.call;
    ++s.visits.dispatched_jobs;
    ProbeRequestV3 q = {};
    q.s0 = at->second.first;
    q.s1 = at->second.second;
    q.read_len = j.frame->a.size();
    q.piece_start = c.piece_start;
    q.piece_length = c.piece_length;
    q.istart = c.istart;
    q.nstart = c.nstart;
    q.lstart = c.lstart;
    q.dir = c.dir;
    q.seed_map_min = c.prefix;
    q.max_steps = PROBE_CHAIN_CAPACITY;
    req[i] = q;
  }
  UsiErrorV1 e = {};
  int32_t rc = usi_search_batch_v3(s.ctx, s.epoch, reads.data(), reads.size(),
                                   req.data(), jobs.size(), out.data(),
                                   stats.data(), &e);
  std::lock_guard<std::mutex> lock(s.mu);
  ++s.totals.batches;
  s.totals.submitted += jobs.size();
  s.totals.chains_submitted += jobs.size();
  s.totals.batch_sizes.push_back(jobs.size());
  if (rc) {
    ++s.totals.faults;
    s.fault = true;
    s.enabled = false;
    if (strict())
      strict_fail(e.message[0] ? e.message : "backend batch failure");
    for (size_t i = 0; i < jobs.size(); ++i)
      resolve_cpu(*jobs[i]);
  } else
    for (size_t i = 0; i < jobs.size(); ++i) {
      Job &j = *jobs[i];
      j.out = out[i];
      j.stats = stats[i];
      add_stats(s.totals.submitted_stats, j.stats);
      const bool valid = valid_result(j);
      // RETIRED may have been set by the mapping thread while this synchronous
      // backend call was draining.  Publication must preserve it: that frame
      // remains owned by `owners` until this return, but can no longer be hit.
      j.state.fetch_or(COMPLETE | (valid ? VALID : 0),
                       std::memory_order_release);
      if (!valid) {
        add_stats(s.totals.rejected_stats, j.stats);
        ++s.totals.rejected;
        if (j.out.status == 9)
          ++s.totals.chain_overflow;
        else if (j.out.status == 10)
          ++s.totals.chain_max_steps;
        else if (j.out.status == 11)
          ++s.totals.chain_no_progress;
        else
          ++s.totals.chain_rejected_other;
        // Rejected chains deliberately fall back as a whole; strict compares
        // the stock body at each step rather than treating transport rejection
        // as a fatal result.
      }
    }
#endif
}
void coordinator_main() {
  State &s = S();
  std::vector<Job *> fill;
  std::vector<std::shared_ptr<Window>> owners;
  std::chrono::steady_clock::time_point fill_started;
  bool filling = false, previous_drained = false;
  for (;;) {
    bool stopping;
    bool enabled;
    {
      std::lock_guard<std::mutex> lock(s.mu);
      stopping = s.stopping;
      enabled = s.enabled && !s.fault;
      // Queue registry changes only at a producer's first publication.
      for (size_t qi = 0; qi < s.queues.size() && fill.size() < cap; ++qi) {
        std::shared_ptr<Window> w;
        while (fill.size() < cap && s.queues[qi]->pop(w)) {
          s.queues[qi]->requests.fetch_sub(w->jobs.size(),
                                           std::memory_order_acq_rel);
          if (!filling) {
            fill_started = std::chrono::steady_clock::now();
            filling = true;
          }
          owners.push_back(w); // jobs/frame bytes remain live through drain.
          for (size_t ji = 0; ji < w->jobs.size() && fill.size() < cap; ++ji)
            fill.push_back(&w->jobs[ji]);
        }
      }
    }
    // At startup we fill toward the efficient 256k target.  Once a backend
    // call has drained, a 16k floor is sufficient to launch again; no timer
    // converts ordinary producer skew into a CPU tail.  A bounded fill age
    // (FILL_MAX_US) prevents starvation when producers stop short of the
    // target (a single window at startup, the last frames of a chunk).
    const uint64_t fill_age_us =
        filling ? static_cast<uint64_t>(
                      std::chrono::duration_cast<std::chrono::microseconds>(
                          std::chrono::steady_clock::now() - fill_started)
                          .count())
                : 0;
    if (fill.size() >= cap ||
        (filling && !stopping && fill.size() >= submit_floor &&
         (previous_drained || fill_age_us >= FILL_MAX_US))) {
      const uint64_t wait_us = fill_age_us;
      if (enabled)
        dispatch(std::move(fill));
      else {
        for (size_t i = 0; i < fill.size(); ++i)
          resolve_cpu(*fill[i]);
        std::lock_guard<std::mutex> lock(s.mu);
        s.totals.cpu_tails += fill.size();
      }
      {
        std::lock_guard<std::mutex> lock(s.mu);
        s.totals.fill_wait_us.push_back(wait_us);
      }
      fill.clear();
      owners.clear(); // only after dispatch (and its synchronous drain).
      filling = false;
      previous_drained = true;
      continue;
    }
    if (stopping) {
      for (size_t i = 0; i < fill.size(); ++i)
        resolve_cpu(*fill[i]);
      if (!fill.empty()) {
        std::lock_guard<std::mutex> lock(s.mu);
        s.totals.cpu_tails += fill.size();
      }
      break;
    }
    std::unique_lock<std::mutex> lock(s.mu);
    // A partially filled batch may need a bounded age-out.  With no fill,
    // producers wake us only when a queue crosses submit_floor
    // (or shutdown/window retirement changes lifecycle state).
    if (filling)
      s.cv.wait_for(lock, std::chrono::microseconds(FILL_MAX_US));
    else
      s.cv.wait(lock);
    ++s.totals.coordinator_wakeups;
  }
  s.cv.notify_all();
}
void merge_window(const Window &w) {
  State &s = S();
  std::lock_guard<std::mutex> lock(s.mu);
  s.totals.gpu_consumed += w.consumed;
  s.totals.chains_consumed += w.consumed;
  s.totals.steps_consumed += w.steps_consumed;
  s.totals.misses += w.misses;
  s.totals.prefix_only += w.prefix_only;
  s.totals.unique += w.unique;
  s.totals.searched += w.searched;
  s.totals.suppressed_unused += w.suppressed_unused;
  s.totals.other_unused += w.other_unused;
  s.totals.rejected += w.rejected;
  s.totals.cpu_fallback += w.cpu_fallback;
  s.totals.read1_fallback += w.read1_fallback;
  s.totals.hit_bytes += w.hit_bytes;
  s.totals.hit_gathers += w.hit_gathers;
  s.totals.unused_bytes += w.unused_bytes;
  s.totals.unused_gathers += w.unused_gathers;
  add_stats(s.totals.consumed_stats, w.consumed_stats);
  add_stats(s.totals.suppressed_stats, w.suppressed_stats);
  add_stats(s.totals.other_stats, w.other_stats);
  s.live_bytes -= w.charged_bytes;
  s.live_requests -= w.charged_requests;
  s.cv.notify_one();
}
void close_window() {
  if (!current_window)
    return;
  finish_active_chain();
  for (size_t i = 0; i < current_window->jobs.size(); ++i)
    retire(current_window->jobs[i], false);
  merge_window(*current_window);
  current_window.reset();
  current_frame = nullptr;
  current_index = 0;
  chain = ChainContext();
  chain_job = nullptr;
  chain_cursor = 0;
  chain_rejected = false;
  chain_stock_clear = false;
}
void sidecar(const State &s) {
  const char *p = getenv("STAR_INTEGRATE_SIDECAR");
  if (!p)
    return;
  std::ofstream f(p, std::ios::app);
  const Totals &t = s.totals;
  f << "{\"mode\":\"" << (s.enabled ? "enabled" : "cpu-bypass")
    << "\",\"batches\":" << t.batches << ",\"submitted\":" << t.submitted
    << ",\"gpu_consumed\":" << t.gpu_consumed
    << ",\"chains_submitted\":" << t.chains_submitted
    << ",\"chains_consumed\":" << t.chains_consumed
    << ",\"steps_consumed\":" << t.steps_consumed
    << ",\"chain_overflow\":" << t.chain_overflow
    << ",\"chain_max_steps\":" << t.chain_max_steps
    << ",\"chain_no_progress\":" << t.chain_no_progress
    << ",\"chain_rejected_other\":" << t.chain_rejected_other
    << ",\"shift_mismatch\":" << t.shift_mismatch
    << ",\"flag_mismatch\":" << t.flag_mismatch
    << ",\"step_count_mismatch\":" << t.step_count_mismatch
    << ",\"cpu_tails\":" << t.cpu_tails << ",\"key_misses\":" << t.misses
    << ",\"prefix_only\":" << t.prefix_only << ",\"unique\":" << t.unique
    << ",\"searched\":" << t.searched
    << ",\"suppressed_unused\":" << t.suppressed_unused
    << ",\"other_unused\":" << t.other_unused
    << ",\"cpu_fallback\":" << t.cpu_fallback
    << ",\"read1_fallback\":" << t.read1_fallback
    << ",\"coordinator_wakeups\":" << t.coordinator_wakeups
    << ",\"compared_bytes_hit\":" << t.hit_bytes
    << ",\"gathers_hit\":" << t.hit_gathers
    << ",\"compared_bytes_unused\":" << t.unused_bytes
    << ",\"gathers_unused\":" << t.unused_gathers
    << ",\"rejected\":" << t.rejected << ",\"batch_faults\":" << t.faults
    << ",\"gpu_batch_sizes\":[";
  for (size_t i = 0; i < t.batch_sizes.size(); ++i)
    f << (i ? "," : "") << t.batch_sizes[i];
  f << "],\"gpu_batch_size_histogram\":{";
  std::map<uint64_t, uint64_t> histogram;
  for (size_t i = 0; i < t.batch_sizes.size(); ++i)
    ++histogram[t.batch_sizes[i]];
  for (std::map<uint64_t, uint64_t>::const_iterator it = histogram.begin();
       it != histogram.end(); ++it)
    f << (it == histogram.begin() ? "" : ",") << "\"" << it->first
      << "\":" << it->second;
  f << "},\"chain_length_histogram\":{";
  bool first_bin = true;
  for (size_t i = 1; i < 64; ++i)
    if (t.chain_length_hist[i]) {
      f << (first_bin ? "" : ",") << "\""
        << (i == 63 ? "63+" : std::to_string(i))
        << "\":" << t.chain_length_hist[i];
      first_bin = false;
    }
  f << "},\"submitted_stats\":";
  write_stats(f, t.submitted_stats);
  f << ",\"consumed_stats\":";
  write_stats(f, t.consumed_stats);
  f << ",\"suppressed_unused_stats\":";
  write_stats(f, t.suppressed_stats);
  f << ",\"other_unused_stats\":";
  write_stats(f, t.other_stats);
  f << ",\"rejected_stats\":";
  write_stats(f, t.rejected_stats);
  f << ",\"live_bytes_at_finish\":" << s.live_bytes
    << ",\"live_requests_at_finish\":" << s.live_requests
    << ",\"setup_wall_s\":" << s.setup_wall_s
    << ",\"gpu_inflight_depth\":2,\"gpu_fill_wait_us\":[";
  for (size_t i = 0; i < t.fill_wait_us.size(); ++i)
    f << (i ? "," : "") << t.fill_wait_us[i];
  f << "],\"gpu_fill_wait_us_histogram\":{";
  std::map<uint64_t, uint64_t> fill_histogram;
  for (size_t i = 0; i < t.fill_wait_us.size(); ++i)
    ++fill_histogram[t.fill_wait_us[i]];
  for (std::map<uint64_t, uint64_t>::const_iterator it = fill_histogram.begin();
       it != fill_histogram.end(); ++it)
    f << (it == fill_histogram.begin() ? "" : ",") << "\"" << it->first
      << "\":" << it->second;
  f << "}";
  f << ",\"suppression_opportunities_reference\":13435368,\"directional_"
       "opportunities_reference\":159989084}\n";
}
} // namespace
bool fast_enabled = false;
bool strict() { return env1("STAR_INTEGRATE_STRICT"); }
[[noreturn]] void fail_strict(const char *message) { strict_fail(message); }
bool strict_read1(char **read1, uint64_t read_len) {
  return current_frame && read1 && read1[0] && read1[1] &&
         current_frame->a.size() == read_len &&
         current_frame->b.size() == read_len &&
         !memcmp(current_frame->a.data(), read1[0], read_len) &&
         !memcmp(current_frame->b.data(), read1[1], read_len);
}
uint64_t current_generation() {
  return current_frame ? current_frame->generation : 0;
}
uint64_t current_epoch() { return S().epoch; }
void assign_frame_identity(WindowRead &frame, uint64_t read_id, uint64_t worker,
                           uint64_t chunk) {
  State &s = S();
  frame.ordinal = read_id;
  frame.worker = worker;
  frame.chunk = chunk;
  frame.generation = s.next_generation.fetch_add(1, std::memory_order_relaxed);
  frame.index_epoch = s.epoch;
}
InnerCall build_inner_call(InnerCall call, const WindowRead &frame,
                           const ChainContext &context, uint64_t epoch) {
  call.piece = context.piece;
  call.fragment = context.fragment;
  call.nstart = context.nstart;
  call.lstart = context.lstart;
  call.istart = context.istart;
  call.generation = frame.generation;
  call.piece_start = context.piece_start;
  call.piece_length = context.piece_length;
  call.kind = INITIAL_KIND;
  call.read_id = frame.ordinal;
  call.index_epoch = epoch;
  call.worker = frame.worker;
  call.chunk = frame.chunk;
  // Both mate lengths, then Nsplit plus its split-loop index, are explicit.
  call.mate_context = (frame.mate0_len << 32) | frame.mate1_len;
  call.split_context = (frame.split_count << 32) | context.piece;
  return call;
}
InnerCall build_current_inner_call(InnerCall call) {
  return current_frame
             ? build_inner_call(call, *current_frame, chain, S().epoch)
             : call;
}
void note_cpu_fallback() {
  if (current_window && current_frame)
    ++current_window->cpu_fallback;
}
bool setup(const Parameters &p, const Genome &g) {
#if !STAR_INTEGRATE
  (void)p;
  (void)g;
  return false;
#else
  State &s = S();
  const std::chrono::steady_clock::time_point start =
      std::chrono::steady_clock::now();
  struct SetupWall {
    State &state;
    std::chrono::steady_clock::time_point start;
    ~SetupWall() {
      state.setup_wall_s = std::chrono::duration<double>(
                               std::chrono::steady_clock::now() - start)
                               .count();
    }
  } wall = {s, start};
  std::lock_guard<std::mutex> lock(s.mu);
  if (!admitted(p, g))
    return false;
  if (s.tried)
    return s.enabled && same_index(g);
  s.tried = true;
  if (!env1("STAR_INTEGRATE"))
    return false;
  UsiIdentityV1 id = {};
  uint64_t gb = g.nGenome, sab = g.nSAbyte;
  const uint64_t sai_header_bytes =
      sizeof(uint) * static_cast<uint64_t>(p.pGe.gSAindexNbases + 2);
  std::vector<uint8_t> sai_header(static_cast<size_t>(sai_header_bytes));
  memcpy(sai_header.data(), &p.pGe.gSAindexNbases, sizeof(uint));
  memcpy(sai_header.data() + sizeof(uint), g.genomeSAindexStart,
         sai_header_bytes - sizeof(uint));
  sampled_hash_parts(nullptr, 0, (const uint8_t *)g.G, gb, id.sha256);
  sampled_hash_parts(nullptr, 0, (const uint8_t *)g.SA.charArray, sab,
                     id.sha256 + 32);
  sampled_hash_parts(sai_header.data(), sai_header_bytes,
                     (const uint8_t *)g.SAi.charArray, g.SAi.lengthByte,
                     id.sha256 + 64);
  id.genome_file_bytes = gb;
  id.sa_file_bytes = sab;
  id.sai_file_bytes = sai_header_bytes + g.SAi.lengthByte;
  id.n_sa = g.nSA;
  id.strand_bit = g.GstrandBit;
  id.sparse = p.pGe.gSAsparseD;
  UsiErrorV1 e = {};
  ProbeConfigV2 config =
      probe_config_v2(g, p.seedSearchLmax, id.sai_file_bytes);
  if (const char *dump = getenv("STAR_INTEGRATE_CONFIG_DUMP"))
    write_probe_config_v2(dump, config);
  if (usi_init_v2(p.pGe.gDir.c_str(), &id, &config, s.epoch, &s.ctx, &e) ||
      !s.ctx) {
    s.ctx = nullptr;
    return false;
  }
  s.enabled = true;
  fast_enabled = true;
  bind_index(g);
  s.coordinator = std::thread(coordinator_main);
  return true;
#endif
}
bool enabled() { return S().enabled; }
bool window_remaining() {
  if (!current_window)
    return false;
  if (current_window->next_frame < current_window->frames.size()) {
    current_frame = nullptr;
    return true;
  }
  close_window();
  return false;
}
void submit_window(std::vector<WindowRead> &&frames) {
#if STAR_INTEGRATE
  if (frames.empty() || frames.size() > MAX_WINDOW_READS)
    return;
  uint64_t bytes = frames.capacity() * (sizeof(WindowRead) + 32), nj = 0;
  for (size_t i = 0; i < frames.size(); ++i) {
    bytes += frames[i].a.capacity() + frames[i].b.capacity() +
             frames[i].candidates.capacity() * CANDIDATE_BUDGET_BYTES;
    nj += frames[i].candidates.size();
  }
  if (bytes > MAX_WINDOW_BYTES || nj > MAX_WINDOW_CANDIDATES)
    return;
  close_window();
  std::shared_ptr<Window> w(new Window);
  w->frames = std::move(frames);
  State &s = S();
  w->charged_requests = nj;
  w->charged_bytes = bytes;
  w->jobs.reserve((size_t)nj);
  w->ranges.reserve(w->frames.size());
  w->cursors.reserve(w->frames.size());
  for (size_t fi = 0; fi < w->frames.size(); ++fi) {
    WindowRead &fr = w->frames[fi];
    size_t first = w->jobs.size();
    for (size_t ci = 0; ci < fr.candidates.size(); ++ci) {
      InnerCall &c = fr.candidates[ci];
      w->jobs.push_back(Job(&fr, &c));
    }
    w->ranges.push_back(Range(first, w->jobs.size()));
    w->cursors.push_back(first);
  }
  // Register once per mapping thread.  The publish below is a release store
  // into that thread's SPSC ring; it never takes s.mu or waits for a GPU fence.
  if (!worker_queue) {
    std::lock_guard<std::mutex> lock(s.mu);
    s.queues.emplace_back(new SpscQueue);
    worker_queue = s.queues.back().get();
  }
  bool publish = false, notify = false;
  {
    std::lock_guard<std::mutex> lock(s.mu);
    if (s.enabled && !s.stopping && !s.fault &&
        s.live_requests + nj <= MAX_INFLIGHT_REQUESTS &&
        s.live_bytes + bytes <= MAX_INFLIGHT_BYTES) {
      s.live_requests += nj;
      s.live_bytes += bytes;
      const uint64_t before =
          worker_queue->requests.fetch_add(nj, std::memory_order_acq_rel);
      publish = worker_queue->push(w);
      if (!publish) {
        worker_queue->requests.fetch_sub(nj, std::memory_order_acq_rel);
        s.live_requests -= nj;
        s.live_bytes -= bytes;
      } else {
        notify = before < submit_floor && before + nj >= submit_floor;
      }
    }
  }
  if (!publish) {
    // Bounded admission is an immediate CPU fallback, never producer wait.
    for (size_t i = 0; i < w->jobs.size(); ++i)
      resolve_cpu(w->jobs[i]);
  } else if (notify)
    s.cv.notify_one();
  current_window = w;
  current_frame = nullptr;
  current_index = 0;
#else
  (void)frames;
#endif
}
void begin_map(ReadAlign &ra) {
  finish_active_chain();
  flush_chain();
  current_frame = nullptr;
  chain = ChainContext();
  chain_job = nullptr;
  chain_cursor = 0;
  chain_rejected = false;
  chain_stock_clear = false;
  if (!current_window ||
      current_window->next_frame >= current_window->frames.size())
    return;
  WindowRead &f = current_window->frames[current_window->next_frame];
  if (f.ordinal != ra.iReadAll)
    return;
  current_index = current_window->next_frame++;
  ++S().visits.frame_cursor;
  current_frame = &f;
}
bool handoff_read1(ReadAlign &ra) {
#if !STAR_INTEGRATE
  (void)ra;
  return false;
#else
  if (!current_window ||
      current_window->next_frame >= current_window->frames.size())
    return false;
  WindowRead &f = current_window->frames[current_window->next_frame];
  const uint64_t length =
      f.mate1_len ? f.mate0_len + f.mate1_len + 1 : f.mate0_len;
  if (f.ordinal != ra.iReadAll || !length || f.a.size() != length ||
      f.b.size() != length) {
    ++current_window->read1_fallback;
    return false;
  }
  memcpy(ra.Read1[0], f.a.data(), static_cast<size_t>(length));
  memcpy(ra.Read1[1], f.b.data(), static_cast<size_t>(length));
  for (uint64_t i = 0; i < length; ++i)
    ra.Read1[2][length - i - 1] = static_cast<char>(f.b[i]);
  return true;
#endif
}
void set_chain(uint64_t piece, uint64_t fragment, uint64_t istart,
               uint64_t nstart, uint64_t lstart, uint64_t lmapped,
               uint64_t piece_start, uint64_t piece_length,
               uint64_t split_count) {
  chain.piece = piece;
  chain.fragment = fragment;
  chain.istart = istart;
  chain.nstart = nstart;
  chain.lstart = lstart;
  chain.lmapped = lmapped;
  if (!lmapped) {
    finish_active_chain();
    flush_chain();
    chain_job = nullptr;
    chain_cursor = 0;
    chain_rejected = false;
    chain_stock_clear = false;
  }
  ++chain_steps;
  chain.piece_start = piece_start;
  chain.piece_length = piece_length;
  chain.split_count = split_count;
}
void note_flag_clear() {
  if (chain_job && !chain_rejected)
    chain_stock_clear = true;
}
void reverse_suppressed(uint64_t piece) {
  if (!current_window || !current_frame)
    return;
  Range r = current_window->ranges[current_index];
  for (size_t i = r.first; i < r.last; ++i) {
    Job &j = current_window->jobs[i];
    if (j.call->piece == piece && j.call->dir == 0 && j.call->istart == 0)
      retire(j, true);
  }
}
void end_chunk() {
  close_window();
  flush_chain();
  State &s = S();
  std::lock_guard<std::mutex> lock(s.mu);
  for (size_t i = 0; i < CHAIN_HIST_BINS; ++i) {
    s.totals.chain_length_hist[i] += chain_hist[i];
    chain_hist[i] = 0;
  }
}
bool lookup(const Parameters &p, const Genome &g, char **r, uint64_t len,
            uint64_t shift, const InnerCall &in, uint64_t out[2],
            uint64_t &nrep, uint64_t &maxL) {
#if !STAR_INTEGRATE
  (void)p;
  (void)g;
  (void)r;
  (void)len;
  (void)shift;
  (void)in;
  (void)out;
  (void)nrep;
  (void)maxL;
  return false;
#else
  if (!current_window || !current_frame || !r || !r[0] || !r[1] || !len ||
      len > read_cap || chain.piece == ~uint64_t(0) || chain.istart >= 2 ||
      !same_index(g) || !admitted(p, g))
    return false;
  const uint64_t mate_length =
      current_frame->mate1_len
          ? current_frame->mate0_len + current_frame->mate1_len + 1
          : current_frame->mate0_len;
  if (mate_length != len || current_frame->a.size() != len ||
      current_frame->b.size() != len ||
      memcmp(current_frame->a.data(), r[0], len) ||
      memcmp(current_frame->b.data(), r[1], len)) {
    ++current_window->misses;
    return false;
  }
  Range range = current_window->ranges[current_index];
  size_t &cursor = current_window->cursors[current_index];
  // Suppressed reverse candidates are never visited by STAR.  Retire them
  // before selecting the next positional initial-start candidate.
  Job *jp = chain_job;
  if (!chain.lmapped) {
    while (cursor < range.last &&
           (current_window->jobs[cursor].state.load(std::memory_order_acquire) &
            RETIRED))
      ++cursor;
    if (cursor == range.last) {
      ++current_window->misses;
      ++S().visits.positional_misses;
      return false;
    }
    jp = &current_window->jobs[cursor++];
    chain_job = jp;
    chain_cursor = 0;
  }
  if (!jp) {
    ++current_window->misses;
    return false;
  }
  Job &j = *jp;
  ++S().visits.lookup_jobs;
  const InnerCall &c = *j.call;
  const bool initial = chain.lmapped == 0;
  const bool quick_match =
      c.dir == in.dir && c.piece == chain.piece && c.istart == chain.istart &&
      (!initial || (c.start == in.start && c.length == in.length));
  const bool immutable_match =
      c.generation == in.generation && c.index_epoch == in.index_epoch &&
      c.read_id == in.read_id && c.piece == in.piece &&
      c.fragment == in.fragment && c.istart == in.istart &&
      c.nstart == in.nstart && c.lstart == in.lstart &&
      c.piece_start == in.piece_start && c.piece_length == in.piece_length &&
      c.kind == in.kind && c.worker == in.worker && c.chunk == in.chunk &&
      c.mate_context == in.mate_context && c.split_context == in.split_context;
  if (!quick_match || c.generation != current_frame->generation ||
      c.index_epoch != S().epoch || current_frame->index_epoch != S().epoch ||
      in.generation != c.generation || in.index_epoch != c.index_epoch ||
      c.read_id != current_frame->ordinal || in.read_id != c.read_id ||
      c.piece != chain.piece || c.fragment != chain.fragment ||
      c.istart != chain.istart || c.nstart != chain.nstart ||
      c.lstart != chain.lstart || c.piece_start != chain.piece_start ||
      c.piece_length != chain.piece_length || c.kind != INITIAL_KIND ||
      in.kind != c.kind || (initial && c.distance != in.distance) ||
      in.fragment != c.fragment || c.worker != current_frame->worker ||
      c.chunk != current_frame->chunk || c.worker != in.worker ||
      c.chunk != in.chunk || c.mate_context != in.mate_context ||
      c.split_context != in.split_context ||
      (initial ? !same_call(c, in) : !immutable_match)) {
    ++current_window->misses;
    ++S().visits.positional_misses;
    return false;
  }
  uint8_t state = j.state.load(std::memory_order_acquire);
  if ((state & RETIRED) || !(state & COMPLETE) || !(state & VALID)) {
    ++current_window->misses;
    chain_rejected = true;
    return false;
  }
  if (chain_cursor >= j.out.n_steps || j.out.steps[chain_cursor].status != 0) {
    ++current_window->misses;
    chain_rejected = true;
    return false;
  }
  const ProbeStepV3 &step = j.out.steps[chain_cursor];
  if (step.shift != shift) {
    ++S().totals.shift_mismatch;
    if (strict())
      strict_fail("V3 step shift mismatch");
    chain_rejected = true;
    return false;
  }
  out[0] = step.low;
  out[1] = step.high;
  nrep = step.nrep;
  maxL = step.max_l;
  ++chain_cursor;
  if (chain_cursor == 1) {
    while (!(state & (CONSUMED | RETIRED)) &&
           !j.state.compare_exchange_weak(state, state | CONSUMED,
                                          std::memory_order_acq_rel,
                                          std::memory_order_acquire))
      ;
    if (state & (CONSUMED | RETIRED)) {
      ++current_window->misses;
      return false;
    }
    ++current_window->consumed;
  }
  ++current_window->steps_consumed;
  if (step.branch == 1)
    ++current_window->prefix_only;
  else if (step.branch == 2)
    ++current_window->unique;
  else if (step.branch == 3)
    ++current_window->searched;
  current_window->hit_bytes += j.stats.bytes;
  current_window->hit_gathers += j.stats.gathers;
  add_stats(current_window->consumed_stats, j.stats);
  return true;
#endif
}
void finish() {
#if STAR_INTEGRATE
  end_chunk();
  State &s = S();
  {
    std::lock_guard<std::mutex> lock(s.mu);
    if (s.coordinator.joinable()) {
      s.stopping = true;
      s.cv.notify_all();
    }
  }
  if (s.coordinator.joinable())
    s.coordinator.join();
  std::lock_guard<std::mutex> lock(s.mu);
  sidecar(s);
  if (s.ctx) {
    UsiErrorV1 e = {};
    usi_destroy_v2(&s.ctx, &e);
  }
  s.enabled = false;
  fast_enabled = false;
#endif
}
void settle_for_test() {
#if STAR_INTEGRATE
  // Poll the caller's current window until every job has left the pending
  // state.  Bounded so a coordinator bug fails the test instead of hanging it.
  if (!current_window)
    return;
  for (int spins = 0; spins < 50000; ++spins) {
    bool pending = false;
    for (size_t i = 0; i < current_window->jobs.size() && !pending; ++i)
      pending =
          !(current_window->jobs[i].state.load(std::memory_order_acquire) &
            COMPLETE);
    if (!pending)
      return;
    std::this_thread::sleep_for(std::chrono::microseconds(100));
  }
  fprintf(stderr, "settle_for_test: window did not complete\n");
  abort();
#endif
}
} // namespace star_integrate
