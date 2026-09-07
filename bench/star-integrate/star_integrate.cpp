// SPDX-License-Identifier: MIT
// Private STAR_INTEGRATE coordinator. STAR mapping threads never call USI.
#include "star_integrate.hpp"
#include "Genome.h"
#include "Parameters.h"
#include "ReadAlign.h"
#include "sha256.hpp"
#include "usi.h"
#include <algorithm>
#include <array>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <deque>
#include <fstream>
#include <memory>
#include <mutex>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>
namespace star_integrate {
namespace {
constexpr uint64_t target = 65536, cap = 262144, read_cap = 4096;
struct Job {
  WindowRead *frame;
  InnerCall *call;
  ProbeOutput out;
  ProbeStats stats;
  bool complete, valid, consumed, retired;
  Job(WindowRead *f = nullptr, InnerCall *c = nullptr)
      : frame(f), call(c), out(), stats(), complete(false), valid(false),
        consumed(false), retired(false) {}
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
  // Hash selects a candidate bucket; `same_call` below establishes every
  // frozen-key field before a result can be consumed.
  std::unordered_multimap<uint64_t, size_t> lookup_index;
  size_t next_frame;
  uint64_t charged_bytes, charged_requests;
  uint64_t consumed, misses, suppressed_unused, other_unused, rejected,
      cpu_fallback, hit_bytes, hit_gathers, unused_bytes, unused_gathers;
  ProbeStats consumed_stats, suppressed_stats, other_stats;
  Window()
      : next_frame(0), charged_bytes(0), charged_requests(0), consumed(0),
        misses(0), suppressed_unused(0), other_unused(0), rejected(0),
        cpu_fallback(0), hit_bytes(0), hit_gathers(0), unused_bytes(0),
        unused_gathers(0), consumed_stats(), suppressed_stats(), other_stats() {
  }
};
struct Visits {
  uint64_t frame_cursor, frame_offsets, dispatched_jobs, lookup_buckets,
      lookup_jobs;
  Visits()
      : frame_cursor(0), frame_offsets(0), dispatched_jobs(0),
        lookup_buckets(0), lookup_jobs(0) {}
};
struct Totals {
  uint64_t batches, submitted, gpu_consumed, cpu_tails, misses,
      suppressed_unused, other_unused, rejected, faults, cpu_fallback,
      hit_bytes, hit_gathers, unused_bytes, unused_gathers;
  std::vector<uint64_t> batch_sizes;
  ProbeStats submitted_stats, consumed_stats, suppressed_stats, other_stats,
      rejected_stats;
  Totals()
      : batches(0), submitted(0), gpu_consumed(0), cpu_tails(0), misses(0),
        suppressed_unused(0), other_unused(0), rejected(0), faults(0),
        cpu_fallback(0), hit_bytes(0), hit_gathers(0), unused_bytes(0),
        unused_gathers(0), submitted_stats(), consumed_stats(),
        suppressed_stats(), other_stats(), rejected_stats() {}
};
struct State {
  std::mutex mu;
  std::condition_variable cv;
  UsiContext *ctx;
  std::thread coordinator;
  std::deque<Job *> pending;
  uint64_t pending_bytes, epoch, next_generation;
  uint64_t live_bytes, live_requests;
  const Genome *index_object;
  const void *index_g, *index_sa, *index_sai;
  uint64_t index_nsa, index_ngenome;
  bool tried, enabled, stopping, fault;
  Totals totals;
  Visits visits;
  State()
      : ctx(nullptr), pending_bytes(0), epoch(1), next_generation(1),
        live_bytes(0), live_requests(0), index_object(nullptr),
        index_g(nullptr), index_sa(nullptr), index_sai(nullptr), index_nsa(0),
        index_ngenome(0), tried(false), enabled(false), stopping(false),
        fault(false) {}
};
thread_local std::shared_ptr<Window> current_window;
thread_local WindowRead *current_frame = nullptr;
thread_local size_t current_index = 0;
thread_local ChainContext chain;
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
uint64_t mix(uint64_t h, uint64_t x) { return (h ^ x) * 1099511628211ULL; }
uint64_t call_hash(const InnerCall &c) {
  const uint64_t v[] = {
      c.start,        c.length,       c.low,          c.high,
      c.dir,          c.prefix,       c.piece,        c.fragment,
      c.distance,     c.nstart,       c.lstart,       c.istart,
      c.generation,   c.piece_start,  c.piece_length, c.kind,
      c.read_id,      c.index_epoch,  c.worker,       c.chunk,
      c.mate_context, c.split_context};
  uint64_t h = 1469598103934665603ULL;
  for (size_t i = 0; i < sizeof(v) / sizeof(*v); ++i)
    h = mix(h, v[i]);
  return h;
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
bool file_matches(const std::string &p, const uint8_t *r, uint64_t bytes,
                  uint8_t *out, uint64_t &file_bytes, uint64_t skip = 0) {
  std::ifstream f(p.c_str(), std::ios::binary);
  if (!f)
    return false;
  ssir::Sha256 h;
  std::array<uint8_t, 65536> b;
  file_bytes = 0;
  uint64_t compared = 0;
  while (f) {
    f.read((char *)b.data(), b.size());
    std::streamsize z = f.gcount();
    if (z <= 0)
      continue;
    h.update(b.data(), (size_t)z);
    uint64_t old = file_bytes;
    file_bytes += (uint64_t)z;
    uint64_t begin = old < skip ? skip - old : 0;
    if (begin < (uint64_t)z) {
      uint64_t take = (uint64_t)z - begin;
      if (compared + take > bytes ||
          memcmp(b.data() + begin, r + compared, (size_t)take))
        return false;
      compared += take;
    }
  }
  if (!f.eof() || compared != bytes)
    return false;
  std::array<uint8_t, 32> d = h.final();
  memcpy(out, d.data(), 32);
  return true;
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
  if (!j.complete) {
    j.complete = true;
    j.valid = false;
  }
}
bool valid_result(const Job &j) {
  const InnerCall &c = *j.call;
  const ProbeOutput &o = j.out;
  return o.status == 0 && o.low >= c.low && o.low <= o.high &&
         o.high <= c.high && o.length >= c.prefix && o.length <= c.length &&
         o.count == o.high - o.low + 1;
}
void retire(Job &j, bool suppressed) {
  if (j.retired || j.consumed)
    return;
  j.retired = true;
  if (!current_window || !j.complete || !j.valid)
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
  std::vector<ProbeRequest> req(jobs.size());
  std::vector<ProbeOutput> out(jobs.size());
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
    req[i] = {0,
              at->second.first,
              at->second.second,
              (uint64_t)j.frame->a.size(),
              c.start,
              c.length,
              c.prefix,
              c.low,
              c.high,
              c.dir};
  }
  UsiErrorV1 e = {};
  int32_t rc = usi_search_batch_v1(s.ctx, s.epoch, reads.data(), reads.size(),
                                   req.data(), jobs.size(), out.data(),
                                   stats.data(), &e);
  std::lock_guard<std::mutex> lock(s.mu);
  ++s.totals.batches;
  s.totals.submitted += jobs.size();
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
      j.complete = true;
      j.valid = valid_result(j);
      if (!j.valid) {
        add_stats(s.totals.rejected_stats, j.stats);
        ++s.totals.rejected;
        if (strict())
          strict_fail("backend returned invalid successful result");
      }
    }
#endif
}
void coordinator_main() {
  State &s = S();
  std::unique_lock<std::mutex> lock(s.mu);
  while (!s.stopping) {
    if (s.pending.empty()) {
      s.cv.wait(lock, [&] { return s.stopping || !s.pending.empty(); });
      continue;
    }
    if (s.pending.size() < target)
      s.cv.wait_for(lock, std::chrono::milliseconds(2),
                    [&] { return s.stopping || s.pending.size() >= target; });
    if (s.pending.empty())
      continue;
    uint64_t n = std::min<uint64_t>(s.pending.size(),
                                    s.pending.size() >= cap ? cap : target);
    std::vector<Job *> batch;
    batch.reserve((size_t)n);
    for (uint64_t i = 0; i < n; ++i) {
      batch.push_back(s.pending.front());
      s.pending.pop_front();
      s.pending_bytes -= CANDIDATE_BUDGET_BYTES;
    }
    if (n < target || !s.enabled || s.fault) {
      for (size_t i = 0; i < batch.size(); ++i)
        resolve_cpu(*batch[i]);
      s.totals.cpu_tails += n;
      s.cv.notify_all();
      continue;
    }
    lock.unlock();
    dispatch(std::move(batch));
    lock.lock();
    s.cv.notify_all();
  }
  while (!s.pending.empty()) {
    resolve_cpu(*s.pending.front());
    s.pending.pop_front();
    s.pending_bytes -= CANDIDATE_BUDGET_BYTES;
    ++s.totals.cpu_tails;
  }
  s.cv.notify_all();
}
void merge_window(const Window &w) {
  State &s = S();
  std::lock_guard<std::mutex> lock(s.mu);
  s.totals.gpu_consumed += w.consumed;
  s.totals.misses += w.misses;
  s.totals.suppressed_unused += w.suppressed_unused;
  s.totals.other_unused += w.other_unused;
  s.totals.rejected += w.rejected;
  s.totals.cpu_fallback += w.cpu_fallback;
  s.totals.hit_bytes += w.hit_bytes;
  s.totals.hit_gathers += w.hit_gathers;
  s.totals.unused_bytes += w.unused_bytes;
  s.totals.unused_gathers += w.unused_gathers;
  add_stats(s.totals.consumed_stats, w.consumed_stats);
  add_stats(s.totals.suppressed_stats, w.suppressed_stats);
  add_stats(s.totals.other_stats, w.other_stats);
  s.live_bytes -= w.charged_bytes;
  s.live_requests -= w.charged_requests;
  s.cv.notify_all();
}
void close_window() {
  if (!current_window)
    return;
  for (size_t i = 0; i < current_window->jobs.size(); ++i)
    retire(current_window->jobs[i], false);
  merge_window(*current_window);
  current_window.reset();
  current_frame = nullptr;
  current_index = 0;
  chain = ChainContext();
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
    << ",\"cpu_tails\":" << t.cpu_tails << ",\"key_misses\":" << t.misses
    << ",\"suppressed_unused\":" << t.suppressed_unused
    << ",\"other_unused\":" << t.other_unused
    << ",\"cpu_fallback\":" << t.cpu_fallback
    << ",\"compared_bytes_hit\":" << t.hit_bytes
    << ",\"gathers_hit\":" << t.hit_gathers
    << ",\"compared_bytes_unused\":" << t.unused_bytes
    << ",\"gathers_unused\":" << t.unused_gathers
    << ",\"rejected\":" << t.rejected << ",\"batch_faults\":" << t.faults
    << ",\"gpu_batch_sizes\":[";
  for (size_t i = 0; i < t.batch_sizes.size(); ++i)
    f << (i ? "," : "") << t.batch_sizes[i];
  f << "],\"submitted_stats\":";
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
    << ",\"live_requests_at_finish\":" << s.live_requests;
  f << ",\"suppression_opportunities_reference\":13435368,\"directional_"
       "opportunities_reference\":159989084}\n";
}
} // namespace
bool strict() { return env1("STAR_INTEGRATE_STRICT"); }
uint64_t current_generation() {
  return current_frame ? current_frame->generation : 0;
}
uint64_t current_epoch() { return S().epoch; }
void assign_frame_identity(WindowRead &frame, uint64_t read_id, uint64_t worker,
                           uint64_t chunk) {
  State &s = S();
  std::lock_guard<std::mutex> lock(s.mu);
  frame.ordinal = read_id;
  frame.worker = worker;
  frame.chunk = chunk;
  frame.generation = s.next_generation++;
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
  std::lock_guard<std::mutex> lock(s.mu);
  if (!admitted(p, g))
    return false;
  if (s.tried)
    return s.enabled && same_index(g);
  s.tried = true;
  if (!env1("STAR_INTEGRATE"))
    return false;
  UsiIdentityV1 id = {};
  uint64_t gb = 0, sab = 0, sib = 0;
  if (!file_matches(p.pGe.gDir + "/Genome", (const uint8_t *)g.G, g.nGenome,
                    id.sha256, gb) ||
      !file_matches(p.pGe.gDir + "/SA", (const uint8_t *)g.SA.charArray,
                    g.nSAbyte, id.sha256 + 32, sab) ||
      !file_matches(p.pGe.gDir + "/SAindex", (const uint8_t *)g.SAi.charArray,
                    g.SAi.lengthByte, id.sha256 + 64, sib,
                    sizeof(uint) * (uint64_t)(p.pGe.gSAindexNbases + 2)))
    return false;
  id.genome_file_bytes = gb;
  id.sa_file_bytes = sab;
  id.sai_file_bytes = sib;
  id.n_sa = g.nSA;
  id.strand_bit = g.GstrandBit;
  id.sparse = p.pGe.gSAsparseD;
  UsiErrorV1 e = {};
  if (usi_init_v1(p.pGe.gDir.c_str(), &id, s.epoch, &s.ctx, &e) || !s.ctx) {
    s.ctx = nullptr;
    return false;
  }
  s.enabled = true;
  bind_index(g);
  s.coordinator = std::thread(coordinator_main);
  return true;
#endif
}
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
  std::unique_lock<std::mutex> lock(s.mu);
  while (!s.stopping && s.enabled &&
         (s.live_requests + nj > MAX_INFLIGHT_REQUESTS ||
          s.live_bytes + bytes > MAX_INFLIGHT_BYTES))
    s.cv.wait_for(lock, std::chrono::milliseconds(2));
  if (s.live_requests + nj > MAX_INFLIGHT_REQUESTS ||
      s.live_bytes + bytes > MAX_INFLIGHT_BYTES)
    return;
  s.live_requests += nj;
  s.live_bytes += bytes;
  w->charged_requests = nj;
  w->charged_bytes = bytes;
  w->jobs.reserve((size_t)nj);
  w->ranges.reserve(w->frames.size());
  for (size_t fi = 0; fi < w->frames.size(); ++fi) {
    WindowRead &fr = w->frames[fi];
    size_t first = w->jobs.size();
    for (size_t ci = 0; ci < fr.candidates.size(); ++ci) {
      InnerCall &c = fr.candidates[ci];
      w->jobs.push_back(Job(&fr, &c));
      w->lookup_index.emplace(call_hash(c), w->jobs.size() - 1);
      if (s.enabled && !s.stopping) {
        // Backpressure occurs before the queue grows: this producer retains
        // window ownership while the coordinator drains prior work.
        while (!s.stopping && s.enabled &&
               (s.pending.size() >= MAX_PENDING_REQUESTS ||
                s.pending_bytes + CANDIDATE_BUDGET_BYTES > MAX_PENDING_BYTES))
          s.cv.wait(lock);
        if (s.stopping || !s.enabled) {
          resolve_cpu(w->jobs.back());
          continue;
        }
        s.pending.push_back(&w->jobs.back());
        s.pending_bytes += CANDIDATE_BUDGET_BYTES;
      } else
        resolve_cpu(w->jobs.back());
    }
    w->ranges.push_back(Range(first, w->jobs.size()));
  }
  s.cv.notify_one();
  lock.unlock();
  lock.lock();
  s.cv.wait(lock, [&] {
    for (size_t i = 0; i < w->jobs.size(); ++i)
      if (!w->jobs[i].complete)
        return false;
    return true;
  });
  lock.unlock();
  current_window = w;
  current_frame = nullptr;
  current_index = 0;
#else
  (void)frames;
#endif
}
void begin_map(ReadAlign &ra) {
  current_frame = nullptr;
  chain = ChainContext();
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
  chain.piece_start = piece_start;
  chain.piece_length = piece_length;
  chain.split_count = split_count;
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
void end_chunk() { close_window(); }
bool lookup(const Parameters &p, const Genome &g, char **r, uint64_t len,
            const InnerCall &in, uint64_t out[2], uint64_t &nrep,
            uint64_t &maxL) {
#if !STAR_INTEGRATE
  (void)p;
  (void)g;
  (void)r;
  (void)len;
  (void)in;
  (void)out;
  (void)nrep;
  (void)maxL;
  return false;
#else
  if (!current_window || !current_frame || !r || !r[0] || !r[1] || !len ||
      len > read_cap || chain.piece == ~uint64_t(0) || chain.lmapped ||
      chain.istart >= 2 || !same_index(g) || !admitted(p, g))
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
  std::pair<std::unordered_multimap<uint64_t, size_t>::iterator,
            std::unordered_multimap<uint64_t, size_t>::iterator>
      bucket = current_window->lookup_index.equal_range(call_hash(in));
  ++S().visits.lookup_buckets;
  for (std::unordered_multimap<uint64_t, size_t>::iterator it = bucket.first;
       it != bucket.second; ++it) {
    ++S().visits.lookup_jobs;
    size_t i = it->second;
    Job &j = current_window->jobs[i];
    const InnerCall &c = *j.call;
    if (j.consumed || j.retired || !j.complete || !j.valid)
      continue;
    if (c.generation != current_frame->generation ||
        c.index_epoch != S().epoch || current_frame->index_epoch != S().epoch ||
        in.generation != c.generation || in.index_epoch != c.index_epoch ||
        c.read_id != current_frame->ordinal || in.read_id != c.read_id ||
        c.piece != chain.piece || c.fragment != chain.fragment ||
        c.istart != chain.istart || c.nstart != chain.nstart ||
        c.lstart != chain.lstart || c.piece_start != chain.piece_start ||
        c.piece_length != chain.piece_length || c.kind != INITIAL_KIND ||
        in.kind != c.kind || c.start != in.start || c.length != in.length ||
        c.low != in.low || c.high != in.high || c.dir != in.dir ||
        c.prefix != in.prefix || c.distance != in.distance ||
        in.fragment != c.fragment || c.worker != current_frame->worker ||
        c.chunk != current_frame->chunk || c.worker != in.worker ||
        c.chunk != in.chunk || c.mate_context != in.mate_context ||
        c.split_context != in.split_context || !same_call(c, in))
      continue;
    out[0] = j.out.low;
    out[1] = j.out.high;
    nrep = j.out.count;
    maxL = j.out.length;
    j.consumed = true;
    ++current_window->consumed;
    current_window->hit_bytes += j.stats.bytes;
    current_window->hit_gathers += j.stats.gathers;
    add_stats(current_window->consumed_stats, j.stats);
    return true;
  }
  ++current_window->misses;
  return false;
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
    usi_destroy_v1(&s.ctx, &e);
  }
  s.enabled = false;
#endif
}
} // namespace star_integrate
