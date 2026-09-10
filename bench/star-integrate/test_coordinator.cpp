// Production coordinator regression fixture: it includes the actual TU, not a
// model.
#define STAR_INTEGRATE 1
#include "star_integrate.cpp"
#include <atomic>
#include <cassert>
#include <cstdlib>
#include <cstring>
#include <thread>
struct UsiPrefixContext {
  int marker;
};
namespace {
UsiPrefixContext fake_context = {1};
bool invalid_success = false;
uint64_t backend_calls = 0;
std::atomic<bool> backend_wait(false), backend_entered(false),
    backend_release(false);
Parameters fixture_p;
Genome fixture_g;
char fixture_genome[1024] = {}, fixture_sa[1024] = {}, fixture_sai[1024] = {};
uint fixture_sai_starts[3] = {};
void prepare_fixture_index() {
  fixture_p.pGe.gLoad = "NoSharedMemory";
  fixture_g.G = fixture_genome + 200;
  fixture_g.SA.charArray = fixture_sa;
  fixture_g.SAi.charArray = fixture_sai;
  fixture_g.nGenome = 1;
  fixture_g.nSA = 10;
  fixture_g.genomeSAindexStart = fixture_sai_starts;
  fixture_g.SA.lengthByte = sizeof(fixture_sa);
  fixture_g.SAi.lengthByte = sizeof(fixture_sai);
  star_integrate::bind_index(fixture_g);
}
star_integrate::InnerCall call(uint64_t ordinal, uint64_t distance,
                               bool suppressed) {
  star_integrate::InnerCall c = {};
  c.start = 0;
  c.length = 4;
  c.low = 10;
  c.high = 20;
  c.dir = suppressed ? 0 : 1;
  c.prefix = 1;
  c.piece = suppressed ? 77 : 7;
  c.fragment = 3;
  c.distance = distance;
  c.nstart = suppressed ? 0 : 2;
  c.lstart = suppressed ? 0 : 5;
  c.istart = suppressed ? 0 : 1;
  c.piece_start = 0;
  c.piece_length = 4;
  c.kind = star_integrate::INITIAL_KIND;
  c.read_id = ordinal;
  return c;
}
star_integrate::WindowRead frame(uint64_t ordinal) {
  star_integrate::WindowRead f = {};
  f.a.assign(4, static_cast<uint8_t>(ordinal));
  f.b.assign(4, static_cast<uint8_t>(ordinal + 1));
  f.mate0_len = 4;
  f.mate1_len = 0;
  f.split_count = 1;
  f.candidates.reserve(40000);
  star_integrate::assign_frame_identity(f, ordinal, 17, 23);
  star_integrate::ChainContext suppressed;
  suppressed.piece = 77;
  f.candidates.push_back(star_integrate::build_inner_call(
      call(ordinal, 0, true), f, suppressed, f.index_epoch));
  for (uint64_t i = 1; i != 40000; ++i) {
    star_integrate::ChainContext context;
    context.piece = 7;
    context.fragment = 3;
    context.istart = 1;
    context.nstart = 2;
    context.lstart = 5;
    context.piece_start = 0;
    context.piece_length = 4;
    f.candidates.push_back(star_integrate::build_inner_call(
        call(ordinal, i, false), f, context, f.index_epoch));
  }
  return f;
}
void map_and_check(uint64_t ordinal) {
  Parameters &p = fixture_p;
  Genome &g = fixture_g;
  ReadAlign wrong(ordinal + 999), ra(ordinal);
  char a[4], b[4], changed[4] = {9, 9, 9, 9};
  std::memset(a, static_cast<int>(ordinal), 4);
  std::memset(b, static_cast<int>(ordinal + 1), 4);
  char *reads[] = {a, b}, *bad_reads[] = {changed, b};
  uint64_t range[2] = {}, nrep = 0, maxl = 0;
  star_integrate::InnerCall probe = call(ordinal, 1, false);
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  star_integrate::begin_map(wrong);
  assert(!star_integrate::lookup(p, g, reads, 4, 0, probe, range, nrep,
                                 maxl)); // ordinal
  assert(star_integrate::current_window
             ->miss_reasons[star_integrate::MISS_NO_WINDOW] == 1);
  star_integrate::begin_map(ra);
  assert(star_integrate::current_generation() != 0);
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  // This is the generated hook's construction: dynamic prefix fields first,
  // then the shared active-frame/chain builder fills the frozen identity.
  probe = star_integrate::build_current_inner_call(probe);
  assert(star_integrate::same_call(
      *star_integrate::current_window->jobs[1].call, probe));
  star_integrate::reverse_suppressed(77);
  assert(!star_integrate::lookup(p, g, bad_reads, 4, 0, probe, range, nrep,
                                 maxl)); // bytes
  star_integrate::set_chain(7, 3, 1, 2, 5, 1, 0, 4, 1);
  assert(!star_integrate::lookup(p, g, reads, 4, 0, probe, range, nrep,
                                 maxl)); // adaptive Lmapped
  star_integrate::set_chain(7, 3, 2, 3, 5, 0, 0, 4, 1);
  assert(!star_integrate::lookup(p, g, reads, 4, 0, probe, range, nrep,
                                 maxl)); // higher start
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  star_integrate::InnerCall mismatched = probe;
  ++mismatched.distance;
  assert(!star_integrate::lookup(p, g, reads, 4, 0, mismatched, range, nrep,
                                 maxl));
  assert(star_integrate::current_window
             ->miss_reasons[star_integrate::MISS_KEY_MISMATCH] == 1);
  star_integrate::current_window->cursors[star_integrate::current_index] =
      star_integrate::current_window->ranges[star_integrate::current_index]
          .first +
      1;
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  assert(star_integrate::lookup(p, g, reads, 4, 0, probe, range, nrep, maxl));
  assert(range[0] == 10 && range[1] == 20 && nrep == 11 && maxl == 4);
  star_integrate::current_window->cursors[star_integrate::current_index] =
      star_integrate::current_window->ranges[star_integrate::current_index]
          .last;
  assert(!star_integrate::lookup(p, g, reads, 4, 0, probe, range, nrep,
                                 maxl)); // repeated key is a positional miss
  assert(star_integrate::current_window
             ->miss_reasons[star_integrate::MISS_POSITIONAL_EXHAUSTED] == 1);
  assert(!star_integrate::window_remaining(ordinal + 1)); // EOF / retirement
}
void normal() {
  prepare_fixture_index();
  star_integrate::State &s = star_integrate::S();
  s.ctx = &fake_context;
  s.enabled = true;
  s.stopping = false;
  s.fault = false;
  s.epoch = 41;
  s.next_generation = 1;
  s.coordinator = std::thread(star_integrate::coordinator_main);
  std::thread one([] {
    std::vector<star_integrate::WindowRead> v;
    v.push_back(frame(11));
    star_integrate::submit_window(std::move(v));
    star_integrate::settle_for_test();
    map_and_check(11);
  });
  std::thread two([] {
    std::vector<star_integrate::WindowRead> v;
    v.push_back(frame(29));
    star_integrate::submit_window(std::move(v));
    star_integrate::settle_for_test();
    map_and_check(29);
  });
  one.join();
  two.join();
  {
    std::lock_guard<std::mutex> lock(s.mu);
    // v2 batching is asynchronous: two 40k windows may land in one or two
    // batches depending on arrival order. Assert the invariants, not the
    // v1 schedule: every batch within [submit_floor, cap], every submitted
    // job accounted for exactly once, and both frames' lookups hit.
    uint64_t dispatched = 0;
    assert(s.totals.batches >= 1 && s.totals.batches <= 2 &&
           s.totals.batch_sizes.size() == s.totals.batches);
    for (size_t i = 0; i < s.totals.batch_sizes.size(); ++i) {
      assert(s.totals.batch_sizes[i] >= 16384 &&
             s.totals.batch_sizes[i] <= 262144);
      dispatched += s.totals.batch_sizes[i];
    }
    assert(dispatched + s.totals.cpu_tails == 80000);
    assert(s.totals.gpu_consumed > 0);
    assert(s.totals.suppressed_unused > 0 && s.pending.empty() &&
           s.pending_bytes == 0);
    // Two mapped frames, one exact lookup, key mismatch, and positional
    // exhaustion per frame: no scan of 40K frame jobs is permitted.
    assert(s.visits.frame_cursor == 2 && s.visits.lookup_jobs == 4 &&
           s.visits.positional_misses == 4);
    assert(s.visits.dispatched_jobs == dispatched &&
           s.visits.frame_offsets <= 2);
    // Two 40K windows cross the notification floor at most once before the
    // coordinator drains them.  A polling coordinator would wake hundreds of
    // times during this fixture; lifecycle wakeups leave a small fixed bound.
    assert(s.totals.coordinator_wakeups <= 8);
  }
  assert(backend_calls >= 1 && backend_calls <= 2);
  star_integrate::finish();
}
void generated_key_hook() {
  prepare_fixture_index();
  star_integrate::State &s = star_integrate::S();
  s.ctx = &fake_context;
  s.enabled = true;
  s.stopping = false;
  s.fault = false;
  s.epoch = 41;
  s.next_generation = 1;
  s.coordinator = std::thread(star_integrate::coordinator_main);
  star_integrate::WindowRead f = {};
  f.a.assign(4, 3);
  f.b.assign(4, 4);
  f.mate0_len = 4;
  f.mate1_len = 0;
  f.split_count = 1;
  star_integrate::assign_frame_identity(f, 3, 17, 23);
  star_integrate::ChainContext context;
  context.piece = 7;
  context.fragment = 3;
  context.istart = 1;
  context.nstart = 2;
  context.lstart = 5;
  context.piece_start = 0;
  context.piece_length = 4;
  f.candidates.push_back(star_integrate::build_inner_call(
      call(3, 1, false), f, context, f.index_epoch));
  // Fill one transport batch so the fixture exercises a completed producer
  // candidate rather than the coordinator's deliberate CPU tail path.
  f.candidates.reserve(65536);
  for (uint64_t i = 1; i != 65536; ++i)
    f.candidates.push_back(f.candidates[0]);
  std::vector<star_integrate::WindowRead> frames;
  frames.push_back(std::move(f));
  star_integrate::submit_window(std::move(frames));
  // v2 is asynchronous: the batch completes on the coordinator thread. Wait
  // for it here so the assertion below tests the hit path, not the race.
  star_integrate::settle_for_test();
  ReadAlign ra(3);
  char a[4] = {3, 3, 3, 3}, b[4] = {4, 4, 4, 4};
  char *reads[] = {a, b};
  uint64_t range[2] = {}, nrep = 0, maxl = 0;
  star_integrate::begin_map(ra);
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  // Mirrors the generated ReadAlign_maxMappableLength2strands hook: only
  // dynamic prefix data is named here; the frozen key comes from the builder.
  star_integrate::InnerCall generated = call(3, 1, false);
  generated = star_integrate::build_current_inner_call(generated);
  assert(star_integrate::same_call(
      *star_integrate::current_window->jobs[0].call, generated));
  star_integrate::current_window->jobs[0].state.store(
      0, std::memory_order_relaxed);
  assert(!star_integrate::lookup(fixture_p, fixture_g, reads, 4, 0, generated,
                                 range, nrep, maxl));
  assert(star_integrate::current_window
             ->miss_reasons[star_integrate::MISS_NOT_READY] == 1);
  assert(star_integrate::current_window
             ->not_ready_where[star_integrate::DRAINING] == 1);
  star_integrate::current_window->jobs[0].state.store(
      star_integrate::COMPLETE | star_integrate::VALID,
      std::memory_order_relaxed);
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  generated = star_integrate::build_current_inner_call(generated);
  assert(star_integrate::lookup(fixture_p, fixture_g, reads, 4, 0, generated,
                                range, nrep, maxl));
  assert(star_integrate::current_window->consumed == 1);
  assert(star_integrate::current_window
             ->miss_reasons[star_integrate::MISS_NO_WINDOW] == 0);
  std::puts("generated key hook: consumed=1 key_misses=0");
  star_integrate::finish();
}
void positional_shuffled_completion() {
  prepare_fixture_index();
  std::shared_ptr<star_integrate::Window> w(new star_integrate::Window);
  w->frames.reserve(1000);
  for (uint64_t i = 0; i != 1000; ++i) {
    star_integrate::WindowRead f = {};
    f.a.assign(4, static_cast<uint8_t>(i));
    f.b.assign(4, static_cast<uint8_t>(i + 1));
    f.mate0_len = 4;
    f.split_count = 1;
    star_integrate::assign_frame_identity(f, i, 17, 23);
    star_integrate::ChainContext context;
    context.piece = 7;
    context.fragment = 3;
    context.istart = 1;
    context.nstart = 2;
    context.lstart = 5;
    context.piece_length = 4;
    star_integrate::InnerCall c = call(i, i, false);
    c.low = 10 + i;
    c.high = 20 + i;
    f.candidates.push_back(
        star_integrate::build_inner_call(c, f, context, f.index_epoch));
    w->frames.push_back(std::move(f));
  }
  for (size_t i = 0; i != w->frames.size(); ++i) {
    const size_t first = w->jobs.size();
    w->jobs.push_back(
        star_integrate::Job(&w->frames[i], &w->frames[i].candidates[0]));
    w->ranges.push_back(star_integrate::Range(first, first + 1));
    w->cursors.push_back(first);
  }
  // Complete slots in a permutation.  Drain is required to put each result
  // back at its dense request index; consumption must not observe this order.
  for (size_t n = 0; n != w->jobs.size(); ++n) {
    const size_t i = (n * 37) % w->jobs.size();
    star_integrate::Job &j = w->jobs[i];
    j.out.n_steps = 1;
    j.out.steps[0].shift = j.call->start;
    j.out.steps[0].max_l = j.call->length;
    j.out.steps[0].low = j.call->low;
    j.out.steps[0].high = j.call->high;
    j.out.steps[0].nrep = j.out.steps[0].high - j.out.steps[0].low + 1;
    j.out.steps[0].branch = 3;
    j.out.steps[0].status = 0;
    j.out.status = 0;
    j.state.store(star_integrate::COMPLETE | star_integrate::VALID);
  }
  star_integrate::current_window = w;
  for (uint64_t i = 0; i != 1000; ++i) {
    ReadAlign ra(i);
    char a[4], b[4];
    std::memset(a, static_cast<int>(i), sizeof(a));
    std::memset(b, static_cast<int>(i + 1), sizeof(b));
    char *reads[] = {a, b};
    uint64_t range[2] = {}, nrep = 0, maxl = 0;
    star_integrate::begin_map(ra);
    star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
    star_integrate::InnerCall c = call(i, i, false);
    c.low = 10 + i;
    c.high = 20 + i;
    c = star_integrate::build_current_inner_call(c);
    assert(star_integrate::lookup(fixture_p, fixture_g, reads, 4, 0, c, range,
                                  nrep, maxl));
    assert(range[0] == 10 + i && range[1] == 20 + i && nrep == 11 && maxl == 4);
  }
  star_integrate::close_window();
}
// The coordinator is commonly the last owner.  Once it has drained the first
// window, dropping the producer reference must return it to that producer's
// pool, with no counters or Job entries carried into the next submission.
void window_rotation() {
  prepare_fixture_index();
  star_integrate::State &s = star_integrate::S();
  s.ctx = &fake_context;
  s.enabled = true;
  s.stopping = false;
  s.fault = false;
  s.epoch = 41;
  s.next_generation = 1;
  s.coordinator = std::thread(star_integrate::coordinator_main);

  std::vector<star_integrate::WindowRead> first_frames;
  first_frames.push_back(frame(401));
  star_integrate::submit_window(std::move(first_frames));
  star_integrate::settle_for_test();
  star_integrate::Window *const first = star_integrate::current_window.get();
  star_integrate::current_window->consumed = 99;
  star_integrate::current_window->miss_reasons[star_integrate::MISS_NOT_READY] =
      7;

  std::vector<star_integrate::WindowRead> second_frames;
  second_frames.push_back(frame(402));
  star_integrate::submit_window(std::move(second_frames));
  assert(star_integrate::current_window.get() == first);
  assert(star_integrate::next_window);
  assert(star_integrate::next_window->jobs.size() == 40000);
  star_integrate::current_window->next_frame =
      star_integrate::current_window->frames.size();
  assert(star_integrate::window_remaining(401));
  assert(star_integrate::current_window->frames[0].ordinal == 402);
  assert(s.totals.prefetch_windows == 1);
  star_integrate::settle_for_test();
  map_and_check(402);
  star_integrate::close_window();
  star_integrate::finish();
  std::puts("window pool: rotation=1 not_ready=0");
}
void chunk_boundary_drops_next() {
  prepare_fixture_index();
  star_integrate::State &s = star_integrate::S();
  s.ctx = &fake_context;
  s.enabled = true;
  s.stopping = false;
  s.fault = false;
  s.epoch = 41;
  s.next_generation = 1;
  s.coordinator = std::thread(star_integrate::coordinator_main);
  std::vector<star_integrate::WindowRead> first, second;
  first.push_back(frame(501));
  second.push_back(frame(502));
  assert(star_integrate::submit_window(std::move(first)));
  assert(star_integrate::submit_window(std::move(second)));
  star_integrate::settle_for_test();
  assert(star_integrate::next_window);
  for (int spins = 0; spins != 50000; ++spins) {
    bool pending = false;
    for (size_t i = 0; i < star_integrate::next_window->jobs.size(); ++i)
      pending |= !(star_integrate::next_window->jobs[i].state.load(
                       std::memory_order_acquire) &
                   star_integrate::COMPLETE);
    if (!pending)
      break;
    std::this_thread::sleep_for(std::chrono::microseconds(100));
  }
  star_integrate::end_chunk();
  assert(!star_integrate::current_window && !star_integrate::next_window);
  assert(s.live_bytes == 0 && s.live_requests == 0);
  star_integrate::finish();
  std::puts("chunk boundary: next_dropped=1 live_bytes=0");
}
void prefetch_refusal() {
  prepare_fixture_index();
  star_integrate::State &s = star_integrate::S();
  s.ctx = &fake_context;
  s.enabled = true;
  s.stopping = false;
  s.fault = false;
  s.epoch = 41;
  s.next_generation = 1;
  s.coordinator = std::thread(star_integrate::coordinator_main);
  std::vector<star_integrate::WindowRead> first, second;
  first.push_back(frame(601));
  second.push_back(frame(602));
  assert(star_integrate::submit_window(std::move(first)));
  star_integrate::settle_for_test();
  s.live_bytes = star_integrate::MAX_INFLIGHT_BYTES;
  assert(!star_integrate::submit_window(std::move(second)));
  assert(!star_integrate::next_window && s.totals.prefetch_refused == 1);
  // Restore the real first-window charge so close_window releases it exactly.
  s.live_bytes = star_integrate::current_window->charged_bytes;
  star_integrate::close_window();
  assert(s.live_bytes == 0 && s.live_requests == 0);
  star_integrate::finish();
  std::puts("prefetch refusal: refused=1 fallback=1");
}
// B1: all seven producers have closed their worker windows before a delayed
// coordinator is allowed to stop.  Six 40k windows fit the cap and the
// seventh is the whole-window exclusion which used to survive finish().
// Normal process teardown after this test is intentional: it exercises the
// queue-slot deleter rather than masking it with _Exit.
void shutdown_drains_queued_windows(bool delayed = false) {
  prepare_fixture_index();
  star_integrate::State &s = star_integrate::S();
  s.ctx = &fake_context;
  s.enabled = true;
  s.stopping = false;
  s.fault = false;
  s.epoch = 41;
  s.next_generation = 1;
  std::atomic<int> closed(0);
  std::vector<std::thread> producers;
  for (uint64_t k = 0; k < 7; ++k)
    producers.emplace_back([&, k] {
      std::vector<star_integrate::WindowRead> v;
      v.push_back(frame(700 + k));
      assert(star_integrate::submit_window(std::move(v)));
      star_integrate::end_chunk();
      closed.fetch_add(1, std::memory_order_release);
    });
  for (size_t k = 0; k < producers.size(); ++k)
    producers[k].join();
  assert(closed.load(std::memory_order_acquire) == 7);
  backend_wait.store(delayed);
  s.stopping = !delayed; // deterministic final-fill exclusion in the CPU case
  s.coordinator = std::thread(star_integrate::coordinator_main);
  std::thread release;
  if (delayed) {
    while (!backend_entered.load())
      std::this_thread::yield();
    release = std::thread([&] {
      for (;;) {
        {
          std::lock_guard<std::mutex> lock(s.mu);
          if (s.stopping)
            break;
        }
        std::this_thread::yield();
      }
      backend_release.store(true);
    });
  }
  star_integrate::finish();
  if (release.joinable())
    release.join();
  {
    std::lock_guard<std::mutex> lock(s.mu);
    assert(s.live_bytes == 0 && s.live_requests == 0);
    for (size_t qi = 0; qi < s.queues.size(); ++qi) {
      assert(s.queues[qi]->requests.load(std::memory_order_acquire) == 0);
      std::shared_ptr<star_integrate::Window> none;
      assert(!s.queues[qi]->pop(none));
    }
    assert(s.totals.cpu_tails == (delayed ? 40000 : 280000));
  }
  std::puts(delayed ? "shutdown delayed: queued=0 live_requests=0 tails=40000"
                    : "shutdown drain: queued=0 live_requests=0 tails=280000");
}
// B2 remains fixed: refusal of the initial/current window must never release
// an admission charge it did not acquire.
void current_window_refusal() {
  prepare_fixture_index();
  star_integrate::State &s = star_integrate::S();
  s.ctx = &fake_context;
  s.enabled = true;
  s.stopping = false;
  s.fault = false;
  s.live_requests = star_integrate::MAX_INFLIGHT_REQUESTS;
  std::vector<star_integrate::WindowRead> v;
  v.push_back(frame(800));
  // Initial refusal is retained as a CPU-resolved current window so stock can
  // consume it; unlike a prefetch refusal, this API path still returns true.
  assert(star_integrate::submit_window(std::move(v)));
  assert(star_integrate::current_window);
  assert(star_integrate::current_window->charged_bytes == 0);
  assert(star_integrate::current_window->charged_requests == 0);
  star_integrate::close_window();
  assert(s.live_bytes == 0 &&
         s.live_requests == star_integrate::MAX_INFLIGHT_REQUESTS);
  s.live_requests = 0;
  star_integrate::finish();
  std::puts("current refusal: charge=0");
}

// Separate from rotation: ownership in the actual SPSC slot and a retained
// shared_ptr must both end before the exact allocation can be reacquired.
void pool_reuse() {
  using namespace star_integrate;
  prepare_fixture_index();
  State &s = S();
  s.enabled = true;
  std::vector<WindowRead> first_frames;
  first_frames.push_back(frame(901));
  assert(submit_window(std::move(first_frames)));
  auto retained = current_window;
  Window *first = retained.get();
  first->consumed = 99;
  first->consumed_stats.bytes = 123;
  first->other_unused = 8;
  first->prefetch_refused = true;
  first->peek_end.ordinal = 902;
  first->peek_end.has_successor = true;
  first->peek_end.stream_pos.push_back(std::streampos(100));
  first->miss_reasons[MISS_NOT_READY] = 7;
  first->device_stop_status[4] = 5;
  first->not_ready_where[DRAINING] = 6;
  end_chunk();
  // Explicitly retain the actual coordinator queue owner as well.
  std::shared_ptr<Window> queued;
  assert(worker_queue->pop(queued) && queued.get() == first);
  worker_queue->requests.fetch_sub(queued->jobs.size());
  std::vector<WindowRead> second_frames;
  second_frames.push_back(frame(902));
  assert(submit_window(std::move(second_frames)));
  assert(current_window.get() != first);
  end_chunk();
  std::shared_ptr<Window> second;
  assert(worker_queue->pop(second));
  worker_queue->requests.fetch_sub(second->jobs.size());
  second.reset();
  assert(s.live_requests == 40000);
  queued.reset();
  assert(s.live_requests == 40000 && retained.get() == first);
  retained.reset();
  assert(s.live_requests == 0 && s.live_bytes == 0);
  std::vector<WindowRead> third_frames;
  auto fresh = frame(903);
  fresh.candidates.resize(2);
  third_frames.push_back(std::move(fresh));
  assert(submit_window(std::move(third_frames)));
  assert(current_window.get() == first);
  assert(first->active && first->next_frame == 0 && !first->prefetch_refused);
  assert(first->peek_end.ordinal == 0 && !first->peek_end.has_successor &&
         first->peek_end.stream_pos.empty());
  assert(first->consumed == 0 && first->other_unused == 0 &&
         first->consumed_stats.bytes == 0 && first->hit_bytes == 0);
  for (auto n : first->miss_reasons)
    assert(n == 0);
  for (auto n : first->device_stop_status)
    assert(n == 0);
  for (auto n : first->not_ready_where)
    assert(n == 0);
  assert(first->frames.size() == 1 && first->frames[0].ordinal == 903);
  assert(first->jobs.size() == 2 && first->ranges.size() == 1 &&
         first->ranges[0].first == 0 && first->ranges[0].last == 2 &&
         first->cursors.size() == 1 && first->cursors[0] == 0);
  for (size_t i = 0; i < 2; ++i) {
    const auto &j = first->jobs[i];
    assert(j.frame == &first->frames[0] &&
           j.call == &first->frames[0].candidates[i]);
    assert(j.state.load() == 0 && j.phase.load() == QUEUED &&
           j.out.n_steps == 0 && j.stats.bytes == 0);
  }
  assert(first->charged_requests == 2 && s.live_requests == 2 &&
         first->charged_bytes > 0 && s.live_bytes == first->charged_bytes);
  end_chunk();
  s.stopping = true;
  s.coordinator = std::thread(coordinator_main);
  finish();
  assert(s.live_requests == 0 && s.live_bytes == 0);
  std::puts("pool reuse: retained owner blocks reuse; third reacquires exact "
            "pointer");
}

void chain_accounting() {
  using namespace star_integrate;
  prepare_fixture_index();
  State &s = S();
  // Admission-refused windows are real submit_window products. Replace only
  // the fake transport output for one job to exercise a two-step hit chain.
  s.enabled = true;
  s.live_requests = MAX_INFLIGHT_REQUESTS;
  std::vector<WindowRead> v;
  auto f = frame(3);
  f.candidates.erase(f.candidates.begin());
  f.candidates.resize(1);
  v.push_back(std::move(f));
  assert(submit_window(std::move(v)));
  auto &w = *current_window;
  auto &j = w.jobs[0];
  ReadAlign ra(3);
  char a[4] = {3, 3, 3, 3}, b[4] = {4, 4, 4, 4};
  char *reads[] = {a, b};
  uint64_t range[2] = {}, nrep = 0, maxl = 0;
  begin_map(ra);
  set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  InnerCall probe = build_current_inner_call(call(3, 1, false));
  assert(!lookup(fixture_p, fixture_g, reads, 4, 0, probe, range, nrep, maxl));
  assert(w.miss_reasons[MISS_CPU_ADMISSION] == 1 &&
         w.miss_reasons[MISS_DEVICE_STOPPED] == 0 &&
         w.device_stop_status[0] == 0);
  // An actual completed-invalid transport result remains a device stop.
  j.state.store(COMPLETE);
  w.cursors[0] = 0;
  set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  assert(!lookup(fixture_p, fixture_g, reads, 4, 0, probe, range, nrep, maxl));
  assert(w.miss_reasons[MISS_DEVICE_STOPPED] == 1 &&
         w.device_stop_status[0] == 1);
  j.out.n_steps = 2;
  for (size_t i = 0; i < 2; ++i) {
    j.out.steps[i].shift = i;
    j.out.steps[i].max_l = 1;
    j.out.steps[i].branch = 3;
  }
  j.stats.bytes = 123;
  j.stats.gathers = 17;
  j.state.store(COMPLETE | VALID);
  w.cursors[0] = 0;
  set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  assert(lookup(fixture_p, fixture_g, reads, 4, 0, probe, range, nrep, maxl));
  set_chain(7, 3, 1, 2, 5, 1, 0, 4, 1);
  assert(lookup(fixture_p, fixture_g, reads, 4, 1, probe, range, nrep, maxl));
  assert(w.consumed == 1 && w.steps_consumed == 2 && w.hit_bytes == 123 &&
         w.hit_gathers == 17 && w.consumed_stats.bytes == 123 &&
         w.consumed_stats.gathers == 17);
  // Completed, unused next-window work belongs to the next window even when
  // current has already been closed. No publication/retirement partition claim.
  next_window.reset(new Window);
  next_window->jobs.emplace_back();
  next_window->jobs[0].state.store(COMPLETE | VALID);
  next_window->jobs[0].stats.bytes = 77;
  next_window->jobs[0].stats.gathers = 9;
  end_chunk();
  assert(s.totals.other_unused == 1 && s.totals.unused_bytes == 77 &&
         s.totals.other_stats.bytes == 77 && s.totals.unused_gathers == 9);
  assert(s.totals.consumed_stats.bytes == 123 && s.totals.steps_consumed == 2);
  s.live_requests = 0;
  finish();
  std::puts(
      "chain accounting: two steps, one stats charge; CPU admission separate");
}

void strict_invalid_success() {
  invalid_success = true;
  setenv("STAR_INTEGRATE_STRICT", "1", 1);
  star_integrate::State &s = star_integrate::S();
  s.ctx = &fake_context;
  s.enabled = true;
  s.epoch = 9;
  star_integrate::WindowRead f = frame(3);
  star_integrate::Job j(&f, &f.candidates[1]);
  std::vector<star_integrate::Job *> jobs(1, &j);
  star_integrate::DispatchScratch scratch;
  star_integrate::dispatch(jobs, scratch);
  assert(false && "strict invalid backend success must abort");
}
} // namespace
// TEST-ONLY fake USI C transport.  It returns deterministic valid probe
// records; it is not a CUDA backend and supplies no GPU evidence.
extern "C" int32_t usi_init_v2(const char *, const UsiIdentityV1 *,
                               const ProbeConfigV2 *, uint64_t,
                               UsiPrefixContext **out, UsiErrorV1 *e) {
  *out = &fake_context;
  std::memset(e, 0, sizeof(*e));
  return 0;
}
extern "C" int32_t usi_init_v2_borrowed(const uint8_t *, uint64_t,
                                        const uint8_t *, uint64_t,
                                        const uint8_t *, uint64_t,
                                        const UsiIdentityV1 *,
                                        const ProbeConfigV2 *, uint64_t,
                                        UsiPrefixContext **out, UsiErrorV1 *e) {
  *out = &fake_context;
  std::memset(e, 0, sizeof(*e));
  return 0;
}
extern "C" int32_t usi_destroy_v2(UsiPrefixContext **ctx, UsiErrorV1 *e) {
  *ctx = 0;
  std::memset(e, 0, sizeof(*e));
  return 0;
}
extern "C" int32_t usi_search_batch_v3(UsiPrefixContext *, uint64_t,
                                       const uint8_t *, uint64_t,
                                       const ProbeRequestV3 *req, uint64_t n,
                                       ProbeOutputV3 *out, ProbeStats *stats,
                                       UsiErrorV1 *e) {
  ++backend_calls;
  if (backend_wait.load()) {
    backend_entered.store(true);
    while (!backend_release.load())
      std::this_thread::yield();
  }
  std::memset(e, 0, sizeof(*e));
  for (uint64_t i = 0; i < n; ++i) {
    out[i].n_steps = invalid_success ? PROBE_CHAIN_CAPACITY + 1 : 1;
    // Fixture records use deliberately tiny synthetic geometry; the generated
    // hook's current Shift is their piece_start.
    out[i].steps[0].shift = req[i].piece_start;
    out[i].steps[0].max_l = req[i].piece_length;
    out[i].steps[0].low = 10;
    out[i].steps[0].high = 20;
    out[i].steps[0].nrep = 11;
    out[i].steps[0].branch = 3;
    out[i].steps[0].status = 0;
    out[i].status = 0;
    stats[i].bytes = 4;
    stats[i].gathers = 1;
  }
  return 0;
}
void index_lifecycle() {
  setenv("STAR_INTEGRATE", "1", 1);
  prepare_fixture_index();
  assert(star_integrate::setup(fixture_p, fixture_g));

  std::vector<star_integrate::WindowRead> first_frames;
  first_frames.push_back(frame(11));
  star_integrate::submit_window(std::move(first_frames));
  star_integrate::settle_for_test();
  ReadAlign first_read(11);
  char a[4] = {11, 11, 11, 11}, b[4] = {12, 12, 12, 12};
  char *reads[] = {a, b};
  uint64_t range[2] = {}, nrep = 0, maxl = 0;
  star_integrate::begin_map(first_read);
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  star_integrate::reverse_suppressed(77);
  star_integrate::InnerCall stale =
      star_integrate::build_current_inner_call(call(11, 1, false));
  assert(star_integrate::lookup(fixture_p, fixture_g, reads, 4, 0, stale, range,
                                nrep, maxl));
  star_integrate::end_chunk();

  char second_genome[1024] = {}, second_sa[768] = {}, second_sai[1024] = {};
  fixture_g.G = second_genome + 200;
  fixture_g.SA.charArray = second_sa;
  fixture_g.SA.lengthByte = sizeof(second_sa);
  fixture_g.SAi.charArray = second_sai;
  fixture_g.SAi.lengthByte = sizeof(second_sai);
  fixture_g.nGenome = 2;
  fixture_g.nSA = 11;
  star_integrate::rearm(fixture_p, fixture_g);
  {
    std::lock_guard<std::mutex> lock(star_integrate::S().mu);
    assert(star_integrate::S().live_requests == 0);
    assert(star_integrate::S().generations.size() == 2);
  }

  std::vector<star_integrate::WindowRead> second_frames;
  second_frames.push_back(frame(29));
  star_integrate::submit_window(std::move(second_frames));
  star_integrate::settle_for_test();
  ReadAlign second_read(29);
  char c[4] = {29, 29, 29, 29}, d[4] = {30, 30, 30, 30};
  char *second_reads[] = {c, d};
  star_integrate::begin_map(second_read);
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  star_integrate::reverse_suppressed(77);
  assert(!star_integrate::lookup(fixture_p, fixture_g, second_reads, 4, 0,
                                 stale, range, nrep, maxl));
  assert(star_integrate::current_window
             ->miss_reasons[star_integrate::MISS_KEY_MISMATCH] == 1);
  star_integrate::end_chunk();
  std::vector<star_integrate::WindowRead> fresh_frames;
  fresh_frames.push_back(frame(30));
  star_integrate::submit_window(std::move(fresh_frames));
  star_integrate::settle_for_test();
  ReadAlign fresh_read(30);
  char e[4] = {30, 30, 30, 30}, f[4] = {31, 31, 31, 31};
  char *fresh_reads[] = {e, f};
  star_integrate::begin_map(fresh_read);
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  star_integrate::reverse_suppressed(77);
  star_integrate::InnerCall fresh =
      star_integrate::build_current_inner_call(call(30, 1, false));
  assert(star_integrate::lookup(fixture_p, fixture_g, fresh_reads, 4, 0, fresh,
                                range, nrep, maxl));
  star_integrate::finish();
  std::puts("index lifecycle: generations=2 stale_key_mismatch=1");
}

// Round 7 (host20): 76% of chains missed because the coordinator split a
// window across the 262,144-job batch cap — the head of the window was
// dispatched, the tail was neither dispatched nor CPU-resolved, and every
// lookup into the tail missed. Seven 40,000-job windows from one producer
// exceed the cap; every one of them must be consumable in full.
void whole_window_batching() {
  prepare_fixture_index();
  star_integrate::State &s = star_integrate::S();
  s.ctx = &fake_context;
  s.enabled = true;
  s.stopping = false;
  s.fault = false;
  s.epoch = 41;
  s.next_generation = 1;
  // Seven producers each submit one 40,000-job window before the coordinator
  // is started, so its first fill sees 280,000 queued jobs against a cap of
  // 262,144 and must leave a whole window for the second batch.
  std::atomic<int> submitted(0);
  std::atomic<bool> go(false);
  std::vector<std::thread> producers;
  for (uint64_t k = 0; k < 7; ++k)
    producers.emplace_back([&, k] {
      std::vector<star_integrate::WindowRead> v;
      v.push_back(frame(100 + k));
      star_integrate::submit_window(std::move(v));
      submitted.fetch_add(1);
      while (!go.load())
        std::this_thread::yield();
      star_integrate::settle_for_test();
      map_and_check(100 + k);
    });
  while (submitted.load() != 7)
    std::this_thread::yield();
  s.coordinator = std::thread(star_integrate::coordinator_main);
  go.store(true);
  for (size_t k = 0; k < producers.size(); ++k)
    producers[k].join();
  {
    std::lock_guard<std::mutex> lock(s.mu);
    assert(s.totals.batches >= 2);
    uint64_t dispatched = 0;
    for (size_t i = 0; i < s.totals.batch_sizes.size(); ++i) {
      assert(s.totals.batch_sizes[i] % 40000 == 0); // never a partial window
      dispatched += s.totals.batch_sizes[i];
    }
    assert(dispatched == 280000 && s.totals.cpu_tails == 0);
    assert(s.totals.gpu_consumed == 7);
  }
  star_integrate::finish();
  std::printf("whole window batching: batches=%llu consumed=%llu\n",
              (unsigned long long)s.totals.batches,
              (unsigned long long)s.totals.gpu_consumed);
}
int main(int argc, char **argv) {
  if (argc == 2 && !std::strcmp(argv[1], "whole-window-batching"))
    whole_window_batching();
  else if (argc == 2 && !std::strcmp(argv[1], "strict-invalid-success"))
    strict_invalid_success();
  else if (argc == 2 && !std::strcmp(argv[1], "generated-key-hook"))
    generated_key_hook();
  else if (argc == 2 && !std::strcmp(argv[1], "positional-shuffled"))
    positional_shuffled_completion();
  else if (argc == 2 && !std::strcmp(argv[1], "window-pool"))
    window_rotation();
  else if (argc == 2 && !std::strcmp(argv[1], "chunk-boundary"))
    chunk_boundary_drops_next();
  else if (argc == 2 && !std::strcmp(argv[1], "prefetch-refusal"))
    prefetch_refusal();
  else if (argc == 2 && !std::strcmp(argv[1], "shutdown-drain"))
    shutdown_drains_queued_windows();
  else if (argc == 2 && !std::strcmp(argv[1], "shutdown-delayed"))
    shutdown_drains_queued_windows(true);
  else if (argc == 2 && !std::strcmp(argv[1], "pool-reuse"))
    pool_reuse();
  else if (argc == 2 && !std::strcmp(argv[1], "chain-accounting"))
    chain_accounting();
  else if (argc == 2 && !std::strcmp(argv[1], "current-refusal"))
    current_window_refusal();
  else if (argc == 2 && !std::strcmp(argv[1], "index-lifecycle"))
    index_lifecycle();
  else
    normal();
}
