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
enum JobState : uint8_t {
  COMPLETE = 1,
  VALID = 2,
  CONSUMED = 4,
  RETIRED = 8,
  CPU_RESOLVED = 16,
  CPU_ADMISSION = 32
};
enum JobPhase : uint8_t { QUEUED, FILLING, DRAINING };
enum MissReason : size_t {
  MISS_READ_BYTES,
  MISS_POSITIONAL_EXHAUSTED,
  MISS_CHAIN_REJECTED_RESIDUE,
  MISS_NO_JOB,
  MISS_KEY_MISMATCH,
  MISS_NOT_READY,
  MISS_DEVICE_STOPPED,
  MISS_CPU_ADMISSION,
  MISS_CPU_RESOLVED,
  MISS_SHIFT,
  MISS_CAS_LOST,
  MISS_NO_WINDOW,
  MISS_REASON_COUNT
};
constexpr size_t KEY_MISS_REASON_COUNT = MISS_NO_WINDOW;
struct Job {
  WindowRead *frame;
  InnerCall *call;
  ProbeOutputV3 out;
  ProbeStats stats;
  std::atomic<uint8_t> state;
  std::atomic<uint8_t> phase;
  Job(WindowRead *f = nullptr, InnerCall *c = nullptr)
      : frame(f), call(c), out(), stats(), state(0), phase(QUEUED) {}
  Job(Job &&o)
      : frame(o.frame), call(o.call), out(o.out), stats(o.stats),
        state(o.state.load(std::memory_order_relaxed)),
        phase(o.phase.load(std::memory_order_relaxed)) {}
  Job &operator=(Job &&o) {
    frame = o.frame;
    call = o.call;
    out = o.out;
    stats = o.stats;
    state.store(o.state.load(std::memory_order_relaxed),
                std::memory_order_relaxed);
    phase.store(o.phase.load(std::memory_order_relaxed),
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
// Live-object census for the memory investigation (round 7: RSS grew ~2.7 GB
// per million reads with no retained windows visible in the code). Every
// Window is counted at construction and destruction; STAR_INTEGRATE_MEMLOG=1
// prints the census and VmRSS from the coordinator every 16 batches.
std::atomic<int64_t> live_windows(0);
std::atomic<int64_t> windows_created(0);
struct Window {
  std::vector<WindowRead> frames;
  std::vector<Job> jobs;
  std::vector<Range> ranges;
  // STAR visits admitted initial starts in producer order.  A frame-local
  // cursor therefore selects its next candidate without a hash/search.
  std::vector<size_t> cursors;
  size_t next_frame;
  WindowEnd peek_end;
  uint64_t charged_bytes, charged_requests;
  uint64_t consumed, steps_consumed, prefix_only, unique, searched,
      suppressed_unused, other_unused, rejected, cpu_fallback, read1_fallback,
      hit_bytes, hit_gathers, unused_bytes, unused_gathers;
  uint64_t miss_reasons[MISS_REASON_COUNT], device_stop_status[256],
      not_ready_where[3];
  ProbeStats consumed_stats, suppressed_stats, other_stats;
  bool active, prefetch_refused;
  Window() : active(true) {
    live_windows.fetch_add(1, std::memory_order_relaxed);
    windows_created.fetch_add(1, std::memory_order_relaxed);
    reset();
  }
  // The admission charge is released here, by the last owner, not at
  // close_window(): a producer closes (exhausts) a window while the
  // coordinator may still hold it in the ring or in a batch. Round 7 census:
  // 146 live windows / 18.8M queued requests against a 0.5 GB charge, RSS +2.7
  // GB per million reads, because the charge was released at close.
  ~Window();
  // Reuse retains the large Job/range/cursor allocations.  Frame element
  // buffers are deliberately not retained in 1a: vector::clear destroys the
  // WindowRead elements; 1b moves those elements through prepare_window.
  void reset() {
    frames.clear();
    jobs.clear();
    ranges.clear();
    cursors.clear();
    next_frame = 0;
    peek_end = WindowEnd();
    prefetch_refused = false;
    charged_bytes = charged_requests = 0;
    consumed = steps_consumed = prefix_only = unique = searched = 0;
    suppressed_unused = other_unused = rejected = cpu_fallback = 0;
    read1_fallback = hit_bytes = hit_gathers = unused_bytes = unused_gathers =
        0;
    std::memset(miss_reasons, 0, sizeof(miss_reasons));
    std::memset(device_stop_status, 0, sizeof(device_stop_status));
    std::memset(not_ready_where, 0, sizeof(not_ready_where));
    consumed_stats = ProbeStats();
    suppressed_stats = ProbeStats();
    other_stats = ProbeStats();
  }
  void activate() {
    active = true;
    live_windows.fetch_add(1, std::memory_order_relaxed);
  }
  void release();
};
// One producer (the mapping thread which owns a window) and one consumer
// (coordinator).  A full ring is a CPU-only admission result, never a wait.
struct SpscQueue {
  std::array<std::shared_ptr<Window>, queue_slots> slots;
  std::atomic<size_t> head, tail;
  // Producer-side admission counter: one notification at each floor crossing.
  std::atomic<uint64_t> requests;
  std::mutex pool_mu;
  std::vector<Window *> free_windows;
  SpscQueue()
      : slots(), head(0), tail(0), requests(0), pool_mu(), free_windows() {}
  ~SpscQueue() {
    for (size_t i = 0; i < free_windows.size(); ++i)
      delete free_windows[i];
  }
  Window *acquire_window() {
    std::lock_guard<std::mutex> lock(pool_mu);
    if (free_windows.empty())
      return new Window;
    Window *w = free_windows.back();
    free_windows.pop_back();
    w->reset();
    w->activate();
    return w;
  }
  void recycle_window(Window *w) {
    std::lock_guard<std::mutex> lock(pool_mu);
    constexpr size_t pool_limit = 8;
    if (free_windows.size() < pool_limit) {
      free_windows.push_back(w);
      return;
    }
    delete w;
  }
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
  // Single consumer: the head slot may be inspected before it is taken. Pops
  // only if the whole window fits in `room` jobs, so a window is never split
  // across batches (a split's tail was silently dropped: never dispatched,
  // never resolved, every lookup a miss — 76% of chains in round 7).
  bool pop_if_fits(std::shared_ptr<Window> &w, size_t room) {
    const size_t h = head.load(std::memory_order_relaxed);
    if (h == tail.load(std::memory_order_acquire))
      return false;
    if (slots[h]->jobs.size() > room)
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
      gpu_consumed, cpu_tails, prefix_only, unique, searched, suppressed_unused,
      other_unused, rejected, faults, cpu_fallback, read1_fallback,
      prefetch_windows, prefetch_refused, coordinator_wakeups, hit_bytes,
      hit_gathers, unused_bytes, unused_gathers;
  uint64_t miss_reasons[MISS_REASON_COUNT], device_stop_status[256],
      not_ready_where[3];
  std::vector<uint64_t> batch_sizes, fill_wait_us;
  uint64_t chain_length_hist[64];
  ProbeStats submitted_stats, consumed_stats, suppressed_stats, other_stats,
      rejected_stats;
  Totals()
      : batches(0), submitted(0), chains_submitted(0), chains_consumed(0),
        steps_consumed(0), chain_overflow(0), chain_max_steps(0),
        chain_no_progress(0), chain_rejected_other(0), shift_mismatch(0),
        flag_mismatch(0), step_count_mismatch(0), gpu_consumed(0), cpu_tails(0),
        prefix_only(0), unique(0), searched(0), suppressed_unused(0),
        other_unused(0), rejected(0), faults(0), cpu_fallback(0),
        read1_fallback(0), prefetch_windows(0), prefetch_refused(0),
        coordinator_wakeups(0), hit_bytes(0), hit_gathers(0), unused_bytes(0),
        unused_gathers(0), miss_reasons(), device_stop_status(),
        not_ready_where(), submitted_stats(), consumed_stats(),
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
  uint64_t index_g_extent, index_sa_length, index_sai_length, index_nsa,
      index_ngenome;
  uint64_t index_anon_huge_bytes;
  double setup_wall_s;
  struct Generation {
    uint64_t submitted, consumed;
    Generation(uint64_t submitted_in, uint64_t consumed_in)
        : submitted(submitted_in), consumed(consumed_in) {}
  };
  std::vector<Generation> generations;
  bool tried, enabled, stopping, fault;
  Totals totals;
  Visits visits;
  State()
      : ctx(nullptr), pending_bytes(0), epoch(1), next_generation(1),
        live_bytes(0), live_requests(0), index_object(nullptr),
        index_g(nullptr), index_sa(nullptr), index_sai(nullptr),
        index_g_extent(0), index_sa_length(0), index_sai_length(0),
        index_nsa(0), index_ngenome(0), index_anon_huge_bytes(0),
        setup_wall_s(0), tried(false), enabled(false), stopping(false),
        fault(false) {}
};
void memlog(const State &s, uint64_t batches) {
  static const bool on = std::getenv("STAR_INTEGRATE_MEMLOG") != nullptr;
  if (!on || batches % 16)
    return;
  long rss_kb = 0;
  if (FILE *f = std::fopen("/proc/self/status", "r")) {
    char line[256];
    while (std::fgets(line, sizeof line, f))
      if (!std::strncmp(line, "VmRSS:", 6))
        rss_kb = std::atol(line + 6);
    std::fclose(f);
  }
  uint64_t queued = 0;
  for (size_t qi = 0; qi < s.queues.size(); ++qi)
    queued += s.queues[qi]->requests.load(std::memory_order_relaxed);
  std::fprintf(stderr,
               "MEMLOG batch=%llu rss_gb=%.1f live_windows=%lld created=%lld "
               "live_bytes_gb=%.2f live_requests=%llu queued_requests=%llu\n",
               (unsigned long long)batches, rss_kb / 1048576.0,
               (long long)live_windows.load(std::memory_order_relaxed),
               (long long)windows_created.load(std::memory_order_relaxed),
               s.live_bytes / 1073741824.0, (unsigned long long)s.live_requests,
               (unsigned long long)queued);
}
thread_local std::shared_ptr<Window> current_window;
thread_local std::shared_ptr<Window> next_window;
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
  // Only a chain STAR actually consumed from the device is comparable: a key
  // miss (or any early return before the first step) leaves chain_job bound
  // but chain_cursor == 0, and stock then ran the whole chain on the CPU.
  if (!chain_job || chain_rejected || chain_cursor == 0)
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
void note_miss(MissReason reason) {
  if (current_window) {
    ++current_window->miss_reasons[reason];
    return;
  }
  State &s = S();
  std::lock_guard<std::mutex> lock(s.mu);
  ++s.totals.miss_reasons[reason];
}
void note_not_ready(const Job &j) {
  const uint8_t phase = j.phase.load(std::memory_order_relaxed);
  const size_t bucket = phase <= DRAINING ? phase : QUEUED;
  ++current_window->not_ready_where[bucket];
}
void note_device_stopped(const Job &j) {
  ++current_window->device_stop_status[j.out.status];
  if (chain_cursor < j.out.n_steps)
    ++current_window->device_stop_status[j.out.steps[chain_cursor].status];
}
Window::~Window() { release(); }
void Window::release() {
  if (!active)
    return;
  active = false;
  live_windows.fetch_sub(1, std::memory_order_relaxed);
  if (!charged_requests && !charged_bytes)
    return;
  State &s = S();
  std::lock_guard<std::mutex> lock(s.mu);
  s.live_bytes -= charged_bytes;
  s.live_requests -= charged_requests;
  charged_requests = charged_bytes = 0;
  s.cv.notify_one();
}
void bind_index(const Genome &g) {
  State &s = S();
  s.index_object = &g;
  s.index_g = g.G;
  s.index_g_extent = g.nGenome + 400;
  s.index_sa = g.SA.charArray;
  s.index_sa_length = g.SA.lengthByte;
  s.index_sai = g.SAi.charArray;
  s.index_sai_length = g.SAi.lengthByte;
  s.index_nsa = g.nSA;
  s.index_ngenome = g.nGenome;
}
bool same_index(const Genome &g) {
  const State &s = S();
  return s.index_object == &g && s.index_g == g.G &&
         s.index_sa == g.SA.charArray && s.index_sa_length == g.SA.lengthByte &&
         s.index_sai == g.SAi.charArray && s.index_nsa == g.nSA &&
         s.index_ngenome == g.nGenome;
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
uint64_t anon_huge_overlapping(const void *ptr, uint64_t length) {
#if defined(__linux__)
  if (!ptr || !length)
    return 0;
  const uint64_t lo = reinterpret_cast<uintptr_t>(ptr), hi = lo + length;
  if (hi < lo)
    return 0;
  FILE *smaps = fopen("/proc/self/smaps", "r");
  if (!smaps)
    return 0;
  char line[512];
  bool overlaps_mapping = false;
  uint64_t total = 0;
  while (fgets(line, sizeof(line), smaps)) {
    unsigned long long start = 0, end = 0;
    if (sscanf(line, "%llx-%llx", &start, &end) == 2) {
      overlaps_mapping = start < hi && lo < end;
    } else if (overlaps_mapping && !strncmp(line, "AnonHugePages:", 14)) {
      unsigned long long kib = 0;
      if (sscanf(line + 14, "%llu", &kib) == 1)
        total += kib * 1024;
    }
  }
  fclose(smaps);
  return total;
#else
  (void)ptr;
  (void)length;
  return 0;
#endif
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
         p.pGe.gLoad == "NoSharedMemory" && !p.wasp.yes && !p.peOverlap.yes &&
         p.outFilterBySJoutStage != 2 && p.pGe.transform.type == 0 &&
         p.seedSearchLmax == 0 && g.GstrandBit == 32 && p.pGe.gSAsparseD == 1 &&
         g.G && g.SA.charArray && g.SAi.charArray;
}
void resolve_cpu(Job &j, bool admission = false) {
  uint8_t state = j.state.load(std::memory_order_relaxed);
  while (!(state & COMPLETE) &&
         !j.state.compare_exchange_weak(
             state,
             state | COMPLETE | CPU_RESOLVED | (admission ? CPU_ADMISSION : 0),
             std::memory_order_release, std::memory_order_relaxed))
    ;
}
bool valid_result(const Job &j) {
  const ProbeOutputV3 &o = j.out;
  return o.status == 0 && o.n_steps <= PROBE_CHAIN_CAPACITY;
}
void retire(Window &window, Job &j, bool suppressed) {
  const uint8_t before = j.state.fetch_or(RETIRED, std::memory_order_acq_rel);
  if (before & (RETIRED | CONSUMED))
    return;
  if (!(before & COMPLETE) || !(before & VALID))
    return;
  window.unused_bytes += j.stats.bytes;
  window.unused_gathers += j.stats.gathers;
  if (suppressed) {
    ++window.suppressed_unused;
    add_stats(window.suppressed_stats, j.stats);
  } else {
    ++window.other_unused;
    add_stats(window.other_stats, j.stats);
  }
}
// Coordinator-thread scratch, reused across batches. Fresh per-batch vectors
// (75 MB of 472-byte outputs alone) were malloc'd, page-faulted and munmap'd
// every batch: 14% of the coordinator thread in round 7.
struct DispatchScratch {
  std::vector<uint8_t> reads;
  std::unordered_map<WindowRead *, std::pair<uint64_t, uint64_t>> offsets;
  std::vector<ProbeRequestV3> req;
  std::vector<ProbeOutputV3> out;
  std::vector<ProbeStats> stats;
};
void dispatch(const std::vector<Job *> &jobs, DispatchScratch &d) {
#if !STAR_INTEGRATE
  (void)d;
  for (size_t i = 0; i < jobs.size(); ++i)
    resolve_cpu(*jobs[i]);
#else
  State &s = S();
  for (size_t i = 0; i < jobs.size(); ++i)
    jobs[i]->phase.store(DRAINING, std::memory_order_relaxed);
  std::vector<uint8_t> &reads = d.reads;
  std::unordered_map<WindowRead *, std::pair<uint64_t, uint64_t>> &offsets =
      d.offsets;
  std::vector<ProbeRequestV3> &req = d.req;
  std::vector<ProbeOutputV3> &out = d.out;
  std::vector<ProbeStats> &stats = d.stats;
  reads.clear();
  offsets.clear();
  req.resize(jobs.size());
  out.resize(jobs.size());
  stats.resize(jobs.size());
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
  memlog(s, s.totals.batches);
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
  DispatchScratch scratch;
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
        while (fill.size() < cap &&
               s.queues[qi]->pop_if_fits(w, cap - fill.size())) {
          s.queues[qi]->requests.fetch_sub(w->jobs.size(),
                                           std::memory_order_acq_rel);
          if (!filling) {
            fill_started = std::chrono::steady_clock::now();
            filling = true;
          }
          owners.push_back(w); // jobs/frame bytes remain live through drain.
          for (size_t ji = 0; ji < w->jobs.size(); ++ji) {
            w->jobs[ji].phase.store(FILLING, std::memory_order_relaxed);
            fill.push_back(&w->jobs[ji]);
          }
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
        dispatch(fill, scratch);
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
      uint64_t tails = fill.size();
      fill.clear();
      owners.clear();
      // Whole-window batching can leave a head which does not fit the final
      // fill.  Shutdown owns the obligation to resolve and *remove* every
      // such slot before SpscQueue (and its pool mutex) can be destroyed.
      // Do not hold State::mu while dropping these final owners: Window's
      // release path takes that mutex for its admission charge.
      std::vector<std::shared_ptr<Window>> abandoned;
      {
        std::lock_guard<std::mutex> lock(s.mu);
        for (size_t qi = 0; qi < s.queues.size(); ++qi) {
          std::shared_ptr<Window> w;
          while (s.queues[qi]->pop(w)) {
            s.queues[qi]->requests.fetch_sub(w->jobs.size(),
                                             std::memory_order_acq_rel);
            tails += w->jobs.size();
            abandoned.push_back(std::move(w));
          }
        }
      }
      for (size_t wi = 0; wi < abandoned.size(); ++wi)
        for (size_t ji = 0; ji < abandoned[wi]->jobs.size(); ++ji)
          resolve_cpu(abandoned[wi]->jobs[ji]);
      abandoned.clear();
      if (tails) {
        std::lock_guard<std::mutex> lock(s.mu);
        s.totals.cpu_tails += tails;
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
  for (size_t i = 0; i < MISS_REASON_COUNT; ++i)
    s.totals.miss_reasons[i] += w.miss_reasons[i];
  for (size_t i = 0; i < 256; ++i)
    s.totals.device_stop_status[i] += w.device_stop_status[i];
  for (size_t i = 0; i < 3; ++i)
    s.totals.not_ready_where[i] += w.not_ready_where[i];
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
}
void close_one_window(std::shared_ptr<Window> &window) {
  if (!window)
    return;
  finish_active_chain();
  for (size_t i = 0; i < window->jobs.size(); ++i)
    retire(*window, window->jobs[i], false);
  merge_window(*window);
  window.reset();
}
void close_window() {
  close_one_window(current_window);
  current_frame = nullptr;
  current_index = 0;
  chain = ChainContext();
  chain_job = nullptr;
  chain_cursor = 0;
  chain_rejected = false;
  chain_stock_clear = false;
}
void close_next_window() { close_one_window(next_window); }
void sidecar(const State &s) {
  const char *p = getenv("STAR_INTEGRATE_SIDECAR");
  if (!p)
    return;
  std::ofstream f(p, std::ios::app);
  const Totals &t = s.totals;
  uint64_t key_misses = 0;
  for (size_t i = 0; i < KEY_MISS_REASON_COUNT; ++i)
    key_misses += t.miss_reasons[i];
  f << "{\"mode\":\"" << (s.enabled ? "enabled" : "cpu-bypass")
    << "\",\"batches\":" << t.batches << ",\"submitted\":" << t.submitted
    << ",\"gpu_consumed\":" << t.gpu_consumed
    << ",\"index_generations\":" << s.generations.size()
    << ",\"generations\":[";
  for (size_t i = 0; i < s.generations.size(); ++i) {
    const State::Generation &generation = s.generations[i];
    const uint64_t submitted_end = i + 1 < s.generations.size()
                                       ? s.generations[i + 1].submitted
                                       : t.submitted;
    const uint64_t consumed_end = i + 1 < s.generations.size()
                                      ? s.generations[i + 1].consumed
                                      : t.gpu_consumed;
    f << (i ? "," : "")
      << "{\"submitted\":" << submitted_end - generation.submitted
      << ",\"consumed\":" << consumed_end - generation.consumed << "}";
  }
  f << "]"
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
    << ",\"cpu_tails\":" << t.cpu_tails << ",\"key_misses\":" << key_misses
    << ",\"no_window\":" << t.miss_reasons[MISS_NO_WINDOW]
    << ",\"miss_reasons\":{\"read_bytes\":" << t.miss_reasons[MISS_READ_BYTES]
    << ",\"positional_exhausted\":" << t.miss_reasons[MISS_POSITIONAL_EXHAUSTED]
    << ",\"chain_rejected_residue\":"
    << t.miss_reasons[MISS_CHAIN_REJECTED_RESIDUE]
    << ",\"no_job\":" << t.miss_reasons[MISS_NO_JOB]
    << ",\"key_mismatch\":" << t.miss_reasons[MISS_KEY_MISMATCH]
    << ",\"not_ready\":" << t.miss_reasons[MISS_NOT_READY]
    << ",\"device_stopped\":" << t.miss_reasons[MISS_DEVICE_STOPPED]
    << ",\"cpu_admission\":" << t.miss_reasons[MISS_CPU_ADMISSION]
    << ",\"cpu_resolved\":" << t.miss_reasons[MISS_CPU_RESOLVED]
    << ",\"shift\":" << t.miss_reasons[MISS_SHIFT]
    << ",\"cas_lost\":" << t.miss_reasons[MISS_CAS_LOST]
    << ",\"no_window\":" << t.miss_reasons[MISS_NO_WINDOW] << "}"
    << ",\"not_ready_where\":{\"queued\":" << t.not_ready_where[QUEUED]
    << ",\"filling\":" << t.not_ready_where[FILLING]
    << ",\"draining\":" << t.not_ready_where[DRAINING] << "}"
    << ",\"device_stop_status\":{";
  bool first_stop_status = true;
  for (size_t i = 0; i < 256; ++i)
    if (t.device_stop_status[i]) {
      f << (first_stop_status ? "" : ",") << "\"" << i
        << "\":" << t.device_stop_status[i];
      first_stop_status = false;
    }
  f << "}"
    << ",\"prefix_only\":" << t.prefix_only << ",\"unique\":" << t.unique
    << ",\"searched\":" << t.searched
    << ",\"suppressed_unused\":" << t.suppressed_unused
    << ",\"other_unused\":" << t.other_unused
    << ",\"cpu_fallback\":" << t.cpu_fallback
    << ",\"read1_fallback\":" << t.read1_fallback
    << ",\"prefetch_windows\":" << t.prefetch_windows
    << ",\"prefetch_refused\":" << t.prefetch_refused
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
    << ",\"index_mode\":\"borrowed\",\"index_anon_huge_bytes\":"
    << s.index_anon_huge_bytes << ",\"setup_wall_s\":" << s.setup_wall_s
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
  if (s.enabled)
    return s.enabled && same_index(g);
  if (s.tried)
    return false;
  s.tried = true;
  if (!env1("STAR_INTEGRATE"))
    return false;
  // A frame key includes this epoch.  Advance it for each replacement so a
  // retained generation-1 key cannot match generation 2 by coincidence.
  if (!s.generations.empty())
    ++s.epoch;
  UsiIdentityV1 id = {};
  const uint64_t g_extent = g.nGenome + 400;
  const uint64_t sa_extent = ((g.nSA - 1) * (g.GstrandBit + 1)) / 8 + 8;
  if (g.SA.lengthByte < sa_extent)
    return false;
  uint64_t gb = g.nGenome, sab = sa_extent;
  sampled_hash_parts(nullptr, 0, (const uint8_t *)g.G, gb, id.sha256);
  sampled_hash_parts(nullptr, 0, (const uint8_t *)g.SA.charArray, sab,
                     id.sha256 + 32);
  sampled_hash_parts(nullptr, 0, (const uint8_t *)g.SAi.charArray,
                     g.SAi.lengthByte, id.sha256 + 64);
  id.genome_file_bytes = gb;
  id.sa_file_bytes = sab;
  id.sai_file_bytes = g.SAi.lengthByte;
  id.n_sa = g.nSA;
  id.strand_bit = g.GstrandBit;
  id.sparse = p.pGe.gSAsparseD;
  UsiErrorV1 e = {};
  ProbeConfigV2 config = probe_config_v2(g, p.seedSearchLmax, g.SAi.lengthByte);
  config.sai_offset = 0;
  if (const char *dump = getenv("STAR_INTEGRATE_CONFIG_DUMP"))
    write_probe_config_v2(dump, config);
  if (usi_init_v2_borrowed((const uint8_t *)g.G - 200, g_extent,
                           (const uint8_t *)g.SA.charArray, sa_extent,
                           (const uint8_t *)g.SAi.charArray, g.SAi.lengthByte,
                           &id, &config, s.epoch, &s.ctx, &e) ||
      !s.ctx) {
    s.ctx = nullptr;
    return false;
  }
  s.enabled = true;
  fast_enabled = true;
  bind_index(g);
  s.generations.push_back(
      State::Generation(s.totals.submitted, s.totals.gpu_consumed));
  s.coordinator = std::thread(coordinator_main);
  return true;
#endif
}
bool enabled() { return S().enabled; }
bool window_remaining(uint64_t ordinal) {
  if (!current_window)
    return false;
  // The hook supplies the ordinal before its following oneRead.  If stock has
  // advanced beyond a prefetched frame, retire that stale handoff cursor now;
  // retaining it would make every later exact lookup miss until end_chunk.
  while (current_window->next_frame < current_window->frames.size() &&
         current_window->frames[current_window->next_frame].ordinal <= ordinal)
    ++current_window->next_frame;
  if (current_window->next_frame < current_window->frames.size()) {
    current_frame = nullptr;
    return true;
  }
  close_window();
  if (next_window) {
    current_window = std::move(next_window);
    current_frame = nullptr;
    current_index = 0;
    // The generated hook calls this before oneRead, so `ordinal` is the
    // previous stock ordinal.  Drop frames at or before that previous ordinal;
    // a forward gap remains available for its exact later handoff rather than
    // being handed to the wrong read or stranding the cursor forever.
    while (current_window->next_frame < current_window->frames.size() &&
           current_window->frames[current_window->next_frame].ordinal <=
               ordinal)
      ++current_window->next_frame;
    if (current_window->next_frame < current_window->frames.size()) {
      std::lock_guard<std::mutex> lock(S().mu);
      ++S().totals.prefetch_windows;
      return true;
    }
    close_window();
  }
  return false;
}
bool lookahead_start(WindowEnd &end) {
  const std::shared_ptr<Window> &window =
      next_window ? next_window : current_window;
  if (!window || !window->peek_end.has_successor ||
      window->peek_end.stream_pos.empty())
    return false;
  end = window->peek_end;
  return true;
}
bool next_window_pending() {
  return next_window || (current_window && current_window->prefetch_refused);
}
void mark_lookahead_exhausted() {
  const auto &window = next_window ? next_window : current_window;
  if (window) {
    window->peek_end.has_successor = false;
    window->peek_end.stream_pos.clear();
  }
}
bool submit_window(std::vector<WindowRead> &&frames, WindowEnd &&peek_end) {
#if STAR_INTEGRATE
  if (frames.empty() || frames.size() > MAX_WINDOW_READS)
    return false;
  uint64_t bytes = frames.capacity() * (sizeof(WindowRead) + 32), nj = 0;
  for (size_t i = 0; i < frames.size(); ++i) {
    bytes += frames[i].a.capacity() + frames[i].b.capacity() +
             frames[i].candidates.capacity() * CANDIDATE_BUDGET_BYTES;
    nj += frames[i].candidates.size();
  }
  if (bytes > MAX_WINDOW_BYTES || nj > MAX_WINDOW_CANDIDATES)
    return false;
  State &s = S();
  // Register before acquiring so the per-worker pool is available while the
  // large jobs vector is rebuilt.  Its custom deleter can run on the
  // coordinator thread, hence SpscQueue's mutex-protected free list.
  if (!worker_queue) {
    std::lock_guard<std::mutex> lock(s.mu);
    s.queues.emplace_back(new SpscQueue);
    worker_queue = s.queues.back().get();
  }
  const bool prefetch = static_cast<bool>(current_window);
  SpscQueue *const owner = worker_queue;
  std::shared_ptr<Window> w(owner->acquire_window(), [owner](Window *p) {
    p->release();
    owner->recycle_window(p);
  });
  w->frames = std::move(frames);
  w->peek_end = std::move(peek_end);
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
  // The publish below is a release store into this thread's SPSC ring; it
  // never waits for a GPU fence.
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
      resolve_cpu(w->jobs[i], true);
    // This window never acquired an admission charge; do not let its custom
    // deleter release somebody else's live-byte accounting.
    w->charged_bytes = w->charged_requests = 0;
    if (prefetch) {
      std::lock_guard<std::mutex> lock(s.mu);
      ++s.totals.prefetch_refused;
      current_window->prefetch_refused = true;
      return false;
    }
  } else if (notify)
    s.cv.notify_one();
  if (prefetch)
    next_window = w;
  else {
    current_window = w;
    current_frame = nullptr;
    current_index = 0;
  }
  return true;
#else
  (void)frames;
  (void)peek_end;
  return false;
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
      retire(*current_window, j, true);
  }
}
void end_chunk() {
  close_window();
  close_next_window();
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
      !same_index(g) || !admitted(p, g)) {
    note_miss(MISS_NO_WINDOW);
    return false;
  }
  const uint64_t mate_length =
      current_frame->mate1_len
          ? current_frame->mate0_len + current_frame->mate1_len + 1
          : current_frame->mate0_len;
  if (mate_length != len || current_frame->a.size() != len ||
      current_frame->b.size() != len ||
      memcmp(current_frame->a.data(), r[0], len) ||
      memcmp(current_frame->b.data(), r[1], len)) {
    note_miss(MISS_READ_BYTES);
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
      note_miss(MISS_POSITIONAL_EXHAUSTED);
      ++S().visits.positional_misses;
      return false;
    }
    jp = &current_window->jobs[cursor++];
    chain_job = jp;
    chain_cursor = 0;
  } else if (chain_rejected) {
    // A chain is all-device or all-CPU: once any step has gone to stock, the
    // device's remaining steps are indexed against a chain STAR is no longer
    // walking (its Lmapped advanced by the CPU result). Never resume.
    note_miss(MISS_CHAIN_REJECTED_RESIDUE);
    return false;
  }
  if (!jp) {
    note_miss(MISS_NO_JOB);
    chain_rejected = true;
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
    note_miss(MISS_KEY_MISMATCH);
    ++S().visits.positional_misses;
    // Stock runs this and every later step of the chain on the CPU; nothing
    // further from this device job is comparable.
    chain_rejected = true;
    return false;
  }
  uint8_t state = j.state.load(std::memory_order_acquire);
  if ((state & RETIRED) || !(state & COMPLETE) || !(state & VALID)) {
    // CPU publication is not a device stop. Admission is distinguished from
    // other CPU resolution (tail, shutdown, disabled/faulted dispatch). These
    // counters describe lookup fallback events, not a partition of jobs.
    if (!(state & RETIRED) && (state & COMPLETE) && !(state & VALID)) {
      if (state & CPU_RESOLVED) {
        note_miss(state & CPU_ADMISSION ? MISS_CPU_ADMISSION
                                        : MISS_CPU_RESOLVED);
      } else {
        note_miss(MISS_DEVICE_STOPPED);
        note_device_stopped(j);
      }
    } else {
      note_miss(MISS_NOT_READY);
      note_not_ready(j);
    }
    chain_rejected = true;
    return false;
  }
  if (chain_cursor >= j.out.n_steps || j.out.steps[chain_cursor].status != 0) {
    note_miss(MISS_DEVICE_STOPPED);
    note_device_stopped(j);
    chain_rejected = true;
    return false;
  }
  const ProbeStepV3 &step = j.out.steps[chain_cursor];
  if (step.shift != shift) {
    ++S().totals.shift_mismatch;
    note_miss(MISS_SHIFT);
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
      note_miss(MISS_CAS_LOST);
      chain_rejected = true;
      return false;
    }
    ++current_window->consumed;
    // ProbeStats describe the whole device chain, not one STAR continuation.
    current_window->hit_bytes += j.stats.bytes;
    current_window->hit_gathers += j.stats.gathers;
    add_stats(current_window->consumed_stats, j.stats);
  }
  ++current_window->steps_consumed;
  if (step.branch == 1)
    ++current_window->prefix_only;
  else if (step.branch == 2)
    ++current_window->unique;
  else if (step.branch == 3)
    ++current_window->searched;
  return true;
#endif
}
void finish_generation(bool emit_sidecar) {
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
  // The final sidecar is emitted before the borrowed context is destroyed.
  // Measure the installed generation's actual packed extents, not the prior
  // setup's derived SA extent: sjdbBuildIndex replaces SA between passes.
  s.index_anon_huge_bytes =
      (s.index_g ? anon_huge_overlapping(
                       static_cast<const uint8_t *>(s.index_g) - 200,
                       s.index_g_extent)
                 : 0) +
      anon_huge_overlapping(s.index_sa, s.index_sa_length) +
      anon_huge_overlapping(s.index_sai, s.index_sai_length);
  if (emit_sidecar)
    sidecar(s);
  if (s.ctx) {
    UsiErrorV1 e = {};
    usi_destroy_v2(&s.ctx, &e);
  }
  s.enabled = false;
  fast_enabled = false;
  s.tried = false;
  s.stopping = false;
#endif
}
void finish() { finish_generation(true); }
void rearm(const Parameters &p, const Genome &g) {
#if STAR_INTEGRATE
  // STAR.cpp:149 performs mapping-time GTF insertion before any chunks exist.
  // twoPassRunPass1.cpp:73 joins pass-1 mapThreadsSpawn before its :92
  // insertion.  Thus neither explicit re-arm races a mapping worker.
  if (!same_index(g)) {
    finish_generation(false);
    setup(p, g);
  }
#else
  (void)p;
  (void)g;
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
