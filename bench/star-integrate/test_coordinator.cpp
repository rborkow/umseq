// Production coordinator regression fixture: it includes the actual TU, not a model.
#define STAR_INTEGRATE 1
#include "star_integrate.cpp"
#include <cassert>
#include <cstdlib>
#include <cstring>
#include <thread>
struct UsiContext { int marker; };
namespace {
UsiContext fake_context = {1};
bool invalid_success = false;
uint64_t backend_calls = 0;
Parameters fixture_p;
Genome fixture_g;
char fixture_genome=0, fixture_sa=0, fixture_sai=0;
void prepare_fixture_index() {
  fixture_p.pGe.gLoad="NoSharedMemory";
  fixture_g.G=&fixture_genome; fixture_g.SA.charArray=&fixture_sa;
  fixture_g.SAi.charArray=&fixture_sai; fixture_g.nGenome=1; fixture_g.nSA=100;
  star_integrate::bind_index(fixture_g);
}
star_integrate::InnerCall call(uint64_t ordinal, uint64_t distance, bool suppressed) {
  star_integrate::InnerCall c = {};
  c.start=0; c.length=4; c.low=10; c.high=20; c.dir=suppressed?0:1; c.prefix=1;
  c.piece=suppressed?77:7; c.fragment=3; c.distance=distance;
  c.nstart=suppressed?0:2; c.lstart=suppressed?0:5; c.istart=suppressed?0:1;
  c.piece_start=0; c.piece_length=4; c.kind=star_integrate::INITIAL_KIND;
  c.read_id=ordinal; return c;
}
star_integrate::WindowRead frame(uint64_t ordinal) {
  star_integrate::WindowRead f = {}; f.ordinal=ordinal;
  f.a.assign(4, static_cast<uint8_t>(ordinal)); f.b.assign(4, static_cast<uint8_t>(ordinal+1));
  f.mate0_len=4; f.mate1_len=0; f.candidates.reserve(40000);
  f.candidates.push_back(call(ordinal, 0, true));
  for (uint64_t i=1; i!=40000; ++i) f.candidates.push_back(call(ordinal, i, false));
  return f;
}
void map_and_check(uint64_t ordinal) {
  Parameters &p=fixture_p; Genome &g=fixture_g;
  ReadAlign wrong(ordinal+999), ra(ordinal);
  char a[4], b[4], changed[4]={9,9,9,9};
  std::memset(a, static_cast<int>(ordinal), 4); std::memset(b, static_cast<int>(ordinal+1), 4);
  char *reads[]={a,b}, *bad_reads[]={changed,b}; uint64_t range[2]={}, nrep=0, maxl=0;
  star_integrate::InnerCall probe=call(ordinal, 1, false);
  star_integrate::set_chain(7,3,1,2,5,0,0,4);
  star_integrate::begin_map(wrong);
  assert(!star_integrate::lookup(p,g,reads,4,probe,range,nrep,maxl)); // ordinal
  star_integrate::begin_map(ra); assert(star_integrate::current_generation()!=0);
  probe.generation=star_integrate::current_generation(); probe.index_epoch=star_integrate::current_epoch();
  star_integrate::reverse_suppressed(77);
  assert(!star_integrate::lookup(p,g,bad_reads,4,probe,range,nrep,maxl)); // bytes
  star_integrate::InnerCall bad=probe; ++bad.distance;
  assert(!star_integrate::lookup(p,g,reads,4,bad,range,nrep,maxl)); // full key
  bad=probe; ++bad.generation;
  assert(!star_integrate::lookup(p,g,reads,4,bad,range,nrep,maxl)); // generation
  bad=probe; ++bad.index_epoch;
  assert(!star_integrate::lookup(p,g,reads,4,bad,range,nrep,maxl)); // index epoch
  star_integrate::set_chain(7,3,1,2,5,1,0,4);
  assert(!star_integrate::lookup(p,g,reads,4,probe,range,nrep,maxl)); // adaptive Lmapped
  star_integrate::set_chain(7,3,2,3,5,0,0,4);
  assert(!star_integrate::lookup(p,g,reads,4,probe,range,nrep,maxl)); // higher start
  star_integrate::set_chain(7,3,1,2,5,0,0,4);
  assert(star_integrate::lookup(p,g,reads,4,probe,range,nrep,maxl));
  assert(range[0]==10 && range[1]==20 && nrep==11 && maxl==4);
  assert(!star_integrate::lookup(p,g,reads,4,probe,range,nrep,maxl)); // repeat map
  assert(!star_integrate::window_remaining()); // EOF / retirement
}
void normal() {
  prepare_fixture_index();
  star_integrate::State &s=star_integrate::S();
  s.ctx=&fake_context; s.enabled=true; s.stopping=false; s.fault=false; s.epoch=41; s.next_generation=1;
  s.coordinator=std::thread(star_integrate::coordinator_main);
  std::thread one([] { std::vector<star_integrate::WindowRead> v; v.push_back(frame(11)); star_integrate::submit_window(std::move(v)); map_and_check(11); });
  std::thread two([] { std::vector<star_integrate::WindowRead> v; v.push_back(frame(29)); star_integrate::submit_window(std::move(v)); map_and_check(29); });
  one.join(); two.join();
  { std::lock_guard<std::mutex> lock(s.mu);
    assert(s.totals.batches==1 && s.totals.batch_sizes.size()==1 && s.totals.batch_sizes[0]==65536);
    assert(s.totals.cpu_tails==14464 && s.totals.gpu_consumed>0);
    assert(s.totals.suppressed_unused>0 && s.pending.empty() && s.pending_bytes==0);
    // Two mapped frames and one exact candidate lookup per frame: no scan of
    // 40K frame jobs is permitted at consumption.
    assert(s.visits.frame_cursor==2 && s.visits.lookup_jobs==4);
    assert(s.visits.dispatched_jobs==65536 && s.visits.frame_offsets<=2);
  }
  assert(backend_calls==1); star_integrate::finish();
}
void strict_invalid_success() {
  invalid_success=true; setenv("STAR_INTEGRATE_STRICT", "1", 1);
  star_integrate::State &s=star_integrate::S(); s.ctx=&fake_context; s.enabled=true; s.epoch=9;
  star_integrate::WindowRead f=frame(3); star_integrate::Job j(&f,&f.candidates[1]);
  std::vector<star_integrate::Job*> jobs(1,&j); star_integrate::dispatch(jobs);
  assert(false && "strict invalid backend success must abort");
}
}
// TEST-ONLY fake USI C transport.  It returns deterministic valid probe
// records; it is not a CUDA backend and supplies no GPU evidence.
extern "C" int32_t usi_init_v1(const char*,const UsiIdentityV1*,uint64_t,UsiContext **out,UsiErrorV1 *e) { *out=&fake_context; std::memset(e,0,sizeof(*e)); return 0; }
extern "C" int32_t usi_destroy_v1(UsiContext **ctx,UsiErrorV1 *e) { *ctx=0; std::memset(e,0,sizeof(*e)); return 0; }
extern "C" int32_t usi_search_batch_v1(UsiContext*,uint64_t,const uint8_t*,uint64_t,const ProbeRequest *req,uint64_t n,ProbeOutput*out,ProbeStats*stats,UsiErrorV1*e) {
  ++backend_calls; std::memset(e,0,sizeof(*e)); for(uint64_t i=0;i<n;++i) { out[i].length=req[i].length; out[i].low=req[i].low; out[i].high=invalid_success?req[i].high+1:req[i].high; out[i].count=out[i].high-out[i].low+1; out[i].status=0; stats[i].bytes=4; stats[i].gathers=1; } return 0;
}
int main(int argc,char**argv) { if(argc==2 && !std::strcmp(argv[1],"strict-invalid-success")) strict_invalid_success(); else normal(); }
