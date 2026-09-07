// Production coordinator regression fixture: it includes the actual TU, not a
// model.
#define STAR_INTEGRATE 1
#include "star_integrate.cpp"
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
Parameters fixture_p;
Genome fixture_g;
char fixture_genome = 0, fixture_sa = 0, fixture_sai = 0;
void prepare_fixture_index() {
  fixture_p.pGe.gLoad = "NoSharedMemory";
  fixture_g.G = &fixture_genome;
  fixture_g.SA.charArray = &fixture_sa;
  fixture_g.SAi.charArray = &fixture_sai;
  fixture_g.nGenome = 1;
  fixture_g.nSA = 100;
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
  assert(!star_integrate::lookup(p, g, reads, 4, probe, range, nrep,
                                 maxl)); // ordinal
  star_integrate::begin_map(ra);
  assert(star_integrate::current_generation() != 0);
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  // This is the generated hook's construction: dynamic prefix fields first,
  // then the shared active-frame/chain builder fills the frozen identity.
  probe = star_integrate::build_current_inner_call(probe);
  assert(star_integrate::same_call(
      *star_integrate::current_window->jobs[1].call, probe));
  star_integrate::reverse_suppressed(77);
  assert(!star_integrate::lookup(p, g, bad_reads, 4, probe, range, nrep,
                                 maxl)); // bytes
  star_integrate::set_chain(7, 3, 1, 2, 5, 1, 0, 4, 1);
  assert(!star_integrate::lookup(p, g, reads, 4, probe, range, nrep,
                                 maxl)); // adaptive Lmapped
  star_integrate::set_chain(7, 3, 2, 3, 5, 0, 0, 4, 1);
  assert(!star_integrate::lookup(p, g, reads, 4, probe, range, nrep,
                                 maxl)); // higher start
  star_integrate::set_chain(7, 3, 1, 2, 5, 0, 0, 4, 1);
  assert(star_integrate::lookup(p, g, reads, 4, probe, range, nrep, maxl));
  assert(range[0] == 10 && range[1] == 20 && nrep == 11 && maxl == 4);
  assert(!star_integrate::lookup(p, g, reads, 4, probe, range, nrep,
                                 maxl)); // repeated key is a positional miss
  assert(!star_integrate::window_remaining()); // EOF / retirement
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
    // Two mapped frames and one exact candidate lookup per frame: no scan of
    // 40K frame jobs is permitted at consumption.
    assert(s.visits.frame_cursor == 2 && s.visits.lookup_jobs == 4 &&
           s.visits.positional_misses == 2);
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
  assert(star_integrate::lookup(fixture_p, fixture_g, reads, 4, generated,
                                range, nrep, maxl));
  assert(star_integrate::current_window->consumed == 1);
  assert(star_integrate::current_window->misses == 0);
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
    j.out.inner.length = j.call->length;
    j.out.inner.low = j.call->low;
    j.out.inner.high = j.call->high;
    j.out.inner.count = j.out.inner.high - j.out.inner.low + 1;
    j.out.inner.status = 0;
    j.out.branch = 3;
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
    assert(star_integrate::lookup(fixture_p, fixture_g, reads, 4, c, range,
                                  nrep, maxl));
    assert(range[0] == 10 + i && range[1] == 20 + i && nrep == 11 && maxl == 4);
  }
  star_integrate::close_window();
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
  star_integrate::dispatch(jobs);
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
extern "C" int32_t usi_destroy_v2(UsiPrefixContext **ctx, UsiErrorV1 *e) {
  *ctx = 0;
  std::memset(e, 0, sizeof(*e));
  return 0;
}
extern "C" int32_t usi_search_batch_v2(UsiPrefixContext *, uint64_t,
                                       const uint8_t *, uint64_t,
                                       const ProbeRequestV2 *req, uint64_t n,
                                       ProbeOutputV2 *out, ProbeStats *stats,
                                       UsiErrorV1 *e) {
  ++backend_calls;
  std::memset(e, 0, sizeof(*e));
  for (uint64_t i = 0; i < n; ++i) {
    out[i].inner.length =
        invalid_success ? req[i].inner.length + 1 : req[i].inner.length;
    out[i].inner.low = 10;
    out[i].inner.high = 20;
    out[i].inner.count = 11;
    out[i].inner.status = 0;
    out[i].branch = 3;
    stats[i].bytes = 4;
    stats[i].gathers = 1;
  }
  return 0;
}
int main(int argc, char **argv) {
  if (argc == 2 && !std::strcmp(argv[1], "strict-invalid-success"))
    strict_invalid_success();
  else if (argc == 2 && !std::strcmp(argv[1], "generated-key-hook"))
    generated_key_hook();
  else if (argc == 2 && !std::strcmp(argv[1], "positional-shuffled"))
    positional_shuffled_completion();
  else
    normal();
}
