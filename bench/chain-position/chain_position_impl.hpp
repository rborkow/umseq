// SPDX-License-Identifier: MIT
// Included by the private STAR.cpp only.  Fixed-size worker-local counters: no
// request map and no mutex on the comparator/byte path.
#include <algorithm>
#include <array>
#include <cstdlib>
#include <memory>
#include <mutex>
#include <sstream>
#include <vector>

namespace chain_position {
using U = unsigned long long;
enum {
  ENDPOINT = 0,
  BINARY = 1,
  FIND_MULT = 2,
  LIMIT = 4096,
  OVER = LIMIT + 2
};
struct Cell {
  U requests = 0, gathers = 0, bytes = 0;
};
struct Totals {
  std::array<Cell, 2 * 2 * OVER>
      joint{}; // [initial][iDir][istart], last istart is overflow
  std::array<Cell, OVER> offsets{}; // full directional split-edge offset
  std::array<Cell, 3>
      work_partition{}; // initial, grid-only, tail; before bucketing
  std::array<U, 2 * OVER> ip_actual{}, ip_opportunity{}, ip_suppressed{};
  U inner = 0, gathers = 0, bytes = 0, initial = 0, grid_only = 0,
    grid_union = 0, adaptive_tail = 0;
  U opportunities = 0, suppressed = 0, tuple_checks = 0;
};
struct State {
  Totals t;
  bool read = false, outer = false, inner = false;
  unsigned phase = 0, partition = 0;
  U offset = 0;
  U ip = 0, istart = 0, idir = 0, b = 0, f = 0, lstart = 0, lmapped = 0,
    shift = 0, n = 0;
};
static std::mutex registry_mu;
static std::vector<State *> registry;
static thread_local State *local = nullptr;
static std::string directory;
static State &state() {
  if (local)
    return *local;
  local = new State();
  std::lock_guard<std::mutex> lock(registry_mu);
  registry.push_back(local);
  return *local;
}
static void add(U &a, U b) {
  capture::require(~U(0) - a >= b, "chain-position counter overflow");
  a += b;
}
static unsigned capped(U n) { return n <= LIMIT ? unsigned(n) : OVER - 1; }
static unsigned ji(bool initial, U idir, U istart) {
  return (initial ? 2 * OVER : 0) + (idir ? OVER : 0) + capped(istart);
}
static unsigned ii(U ip, U idir) { return (idir ? OVER : 0) + capped(ip); }
static void cell_add(Cell &a, const Cell &b) {
  add(a.requests, b.requests);
  add(a.gathers, b.gathers);
  add(a.bytes, b.bytes);
}
void guard(Parameters &p) {
  capture::require(
      p.seedSearchLmax == 0 && p.pGe.gSAsparseD == 1,
      "chain-position supports only default seedSearchLmax=0 and sparseD=1; "
      "fixed-length call has no initialized Lmapped attribution");
}
void startup() {
  const char *out = std::getenv("SSIR_COUNTERS_DIRECTORY");
  capture::require(
      out && *out && !std::getenv("SSIR_DIRECTORY"),
      "chain-position requires counters directory and forbids SSIR");
  directory = out;
}
void read_begin() {
  State &x = state();
  capture::require(!x.read && !x.outer && !x.inner, "chain read context");
  x.read = true;
}
void read_end() {
  State &x = state();
  capture::require(x.read && !x.outer && !x.inner, "chain read end");
  x.read = false;
}
void chain_opportunity(U ip, U istart, U idir, U, U, U) {
  State &x = state();
  capture::require(x.read && idir < 2, "chain opportunity");
  add(x.t.opportunities, 1);
  add(x.t.ip_opportunity[ii(ip, idir)], 1);
}
void reverse_suppressed(U ip, U istart, U idir, U, U, U) {
  State &x = state();
  capture::require(x.read && idir == 1 && istart == 0,
                   "reverse suppression source condition");
  add(x.t.suppressed, 1);
  add(x.t.ip_suppressed[ii(ip, idir)], 1);
}
void outer_begin(U ip, U istart, U idir, U b, U f, U lstart, U lmapped, U shift,
                 U n) {
  State &x = state();
  capture::require(x.read && !x.outer && !x.inner && idir < 2 && n > 0,
                   "chain outer");
  x.outer = true;
  x.ip = ip;
  x.istart = istart;
  x.idir = idir;
  x.b = b;
  x.f = f;
  x.lstart = lstart;
  x.lmapped = lmapped;
  x.shift = shift;
  x.n = n;
  // Preserve the source formulas as a checked attribution, not a replacement.
  const U edge = idir == 0 ? b + istart * lstart : b + f - istart * lstart - 1;
  const U expected_shift = idir == 0 ? edge + lmapped : edge - lmapped;
  capture::require(expected_shift == shift &&
                       n == f - istart * lstart - lmapped,
                   "chain source tuple");
}
void outer_end() {
  State &x = state();
  capture::require(x.outer && !x.inner, "chain outer end");
  x.outer = false;
}
void inner_begin(U pieceStart, U pieceLength, U i1, U i2, bool dirR, U Lin) {
  State &x = state();
  capture::require(x.outer && !x.inner && pieceStart == x.shift &&
                       pieceLength == x.n && dirR == (x.idir == 0) &&
                       Lin <= pieceLength && i1 <= i2,
                   "chain complete inner tuple");
  x.inner = true;
  x.phase = ENDPOINT;
  const bool initial = x.lmapped == 0;
  x.offset = x.istart * x.lstart + x.lmapped;
  const bool grid = (x.offset % 20) == 0;
  x.partition = initial ? 0 : (grid ? 1 : 2);
  add(x.t.work_partition[x.partition].requests, 1);
  add(x.t.inner, 1);
  add(x.t.tuple_checks, 1);
  add(x.t.joint[ji(initial, x.idir, x.istart)].requests, 1);
  add(x.t.ip_actual[ii(x.ip, x.idir)], 1);
  if (initial)
    add(x.t.initial, 1);
  if (grid && !initial)
    add(x.t.grid_only, 1);
  if (initial || grid)
    add(x.t.grid_union, 1);
  if (!initial && !grid)
    add(x.t.adaptive_tail, 1);
  add(x.t.offsets[capped(x.offset)].requests, 1);
}
void inner_end() {
  State &x = state();
  capture::require(x.inner, "chain inner end");
  x.inner = false;
}
void phase(unsigned p) {
  State &x = state();
  if (x.inner) {
    capture::require(p <= FIND_MULT, "chain phase");
    x.phase = p;
  }
}
void compare_begin() {
  State &x = state();
  if (!x.inner)
    return;
  add(x.t.gathers, 1);
  add(x.t.joint[ji(x.lmapped == 0, x.idir, x.istart)].gathers, 1);
  add(x.t.offsets[capped(x.offset)].gathers, 1);
  add(x.t.work_partition[x.partition].gathers, 1);
}
void compared(U n) {
  State &x = state();
  if (!x.inner)
    return;
  add(x.t.bytes, n);
  add(x.t.joint[ji(x.lmapped == 0, x.idir, x.istart)].bytes, n);
  add(x.t.offsets[capped(x.offset)].bytes, n);
  add(x.t.work_partition[x.partition].bytes, n);
}
static void array(std::ostringstream &s, const std::array<U, 2 * OVER> &a) {
  s << '[';
  for (unsigned i = 0; i < a.size(); ++i) {
    if (i)
      s << ',';
    s << a[i];
  }
  s << ']';
}
static void rows(std::ostringstream &s, const Totals &t) {
  s << '[';
  bool first = true;
  for (unsigned initial = 0; initial < 2; ++initial)
    for (unsigned dir = 0; dir < 2; ++dir)
      for (unsigned start = 0; start < OVER; ++start) {
        const Cell &c =
            t.joint[(initial ? 2 * OVER : 0) + (dir ? OVER : 0) + start];
        if (!c.requests && !c.gathers && !c.bytes)
          continue;
        if (!first)
          s << ',';
        first = false;
        s << "{\"initial\":" << initial
          << ",\"istart\":" << (start == OVER - 1 ? -1 : int(start))
          << ",\"iDir\":" << dir << ",\"inner_requests\":" << c.requests
          << ",\"gathers\":" << c.gathers << ",\"compared_bytes\":" << c.bytes
          << '}';
      }
  s << ']';
}
static void offset_rows(std::ostringstream &s, const Totals &t) {
  s << '[';
  bool first = true;
  for (unsigned o = 0; o < OVER; ++o) {
    const Cell &c = t.offsets[o];
    if (!c.requests && !c.gathers && !c.bytes)
      continue;
    if (!first)
      s << ',';
    first = false;
    s << "{\"offset\":" << (o == OVER - 1 ? -1 : int(o))
      << ",\"inner_requests\":" << c.requests << ",\"gathers\":" << c.gathers
      << ",\"compared_bytes\":" << c.bytes << '}';
  }
  s << ']';
}
void finish() {
  Totals a;
  for (State *x : registry) {
    const Totals &q = x->t;
    for (unsigned i = 0; i < a.joint.size(); ++i)
      cell_add(a.joint[i], q.joint[i]);
    for (unsigned i = 0; i < a.offsets.size(); ++i)
      cell_add(a.offsets[i], q.offsets[i]);
    for (unsigned i = 0; i < a.work_partition.size(); ++i)
      cell_add(a.work_partition[i], q.work_partition[i]);
    for (unsigned i = 0; i < a.ip_actual.size(); ++i) {
      add(a.ip_actual[i], q.ip_actual[i]);
      add(a.ip_opportunity[i], q.ip_opportunity[i]);
      add(a.ip_suppressed[i], q.ip_suppressed[i]);
    }
    for (U Totals::*m :
         {&Totals::inner, &Totals::gathers, &Totals::bytes, &Totals::initial,
          &Totals::grid_only, &Totals::grid_union, &Totals::adaptive_tail,
          &Totals::opportunities, &Totals::suppressed, &Totals::tuple_checks})
      add(a.*m, q.*m);
  }
  U summed = 0;
  for (const Cell &c : a.joint)
    add(summed, c.gathers);
  capture::require(summed == a.gathers && a.tuple_checks == a.inner,
                   "chain gathers/tuple reconciliation");
  std::ostringstream s;
  s << "{\"status\":\"COUNTERS_ONLY_NO_SSIR\",\"schema\":\"chain-position-v1\","
       "\"profile\":{\"seedSearchLmax\":0,\"sparseD\":1},\"actual\":{\"inner_"
       "requests\":"
    << a.inner << ",\"complete_tuple_checks\":" << a.tuple_checks
    << ",\"gathers\":" << a.gathers << ",\"compared_bytes\":" << a.bytes
    << "},\"coverage\":{\"initial\":{\"inner_requests\":" << a.initial
    << "},\"grid_only_added\":{\"inner_requests\":" << a.grid_only
    << "},\"union_initial_grid20\":{\"inner_requests\":" << a.grid_union
    << "},\"adaptive_tail\":{\"inner_requests\":" << a.adaptive_tail
    << "}},\"joint_gathers_bytes_by_initial_istart_iDir\":";
  rows(s, a);
  s << ",\"offsets\":";
  offset_rows(s, a);
  s << ",\"work_partition\":[";
  for (unsigned i = 0; i < a.work_partition.size(); ++i) {
    if (i)
      s << ',';
    const Cell &c = a.work_partition[i];
    s << "{\"inner_requests\":" << c.requests << ",\"gathers\":" << c.gathers
      << ",\"compared_bytes\":" << c.bytes << '}';
  }
  s << ']';
  s << ",\"reverse_suppression\":{\"chain_opportunities\":" << a.opportunities
    << ",\"suppressed_reverse_chains\":" << a.suppressed
    << ",\"actual_by_ip_iDir\":";
  array(s, a.ip_actual);
  s << ",\"opportunity_by_ip_iDir\":";
  array(s, a.ip_opportunity);
  s << ",\"suppressed_by_ip_iDir\":";
  array(s, a.ip_suppressed);
  s << "}}\n";
  const std::string out = s.str();
  capture::exclusive(directory + "/chain-position-counters.json",
                     capture::Bytes(out.begin(), out.end()));
}
} // namespace chain_position
