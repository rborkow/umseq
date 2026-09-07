// SPDX-License-Identifier: MIT
// Included by STAR.cpp in the private real-request build only.  The split
// implementation remains the authority for counters; this layer merely keeps
// complete, bounded per-worker read lifecycles and replays them at shutdown.
#include "split_capture_impl.hpp"
#include <cstdlib>
#include <memory>
#include <mutex>
#include <sstream>
#include <sys/wait.h>
#include <vector>

namespace capture {
using U = unsigned long long;
enum { ENDPOINT = 0, BINARY = 1, FIND_MULT = 2 };
struct Outer {
  U piece = 0, start = 0, site = 0, fragment = 0, S = 0, N = 0, dir = 0,
    reductions = 0;
  unsigned kind = 0, context = 0;
  Inner inner;
  Work direct;
};
struct BufferedRead {
  U ordinal = 0, length = 0, splits = 0;
  Bytes b0, b1;
  std::vector<Outer> outers;
};
struct State {
  U ordinal = 0;
  bool stopped = false, have = false, outer = false, inner = false,
       direct = false;
  BufferedRead current;
  Work work;
  unsigned phase = ENDPOINT, last_direction = 0;
  std::vector<BufferedRead> saved;
  U saved_records = 0, saved_outers = 0, saved_bytes = 0;
};
static std::mutex registry_mu;
static std::vector<State *> registry;
static thread_local State *local = nullptr;
static std::string directory;
static Bytes header;
static U per_worker_inners = 50000, per_worker_reads = 10000,
         per_worker_records = 100000;
static const U FILE_BYTES = 256ULL * 1024 * 1024;
static bool verified = false;
static Genome *bound_genome = nullptr;
static char *bound_g = nullptr, *bound_sa = nullptr, *bound_sai = nullptr;
static Bytes active_metadata;

// Same numeric/active-byte encoding as capture_impl.hpp; no tiny loader is
// used.
static Bytes metadata(Genome &g) {
  require(g.pGe.gType == 1 && g.pGe.transform.type == 0 &&
              g.pGe.gSAsparseD == 1 && g.genomeInsertL == 0 &&
              g.sharedMemory == NULL && g.G == g.G1 + 200,
          "static active profile");
  require(g.GstrandBit == 32 && g.nGenome > 0 && g.nSA > 0 && g.nSAi > 0 &&
              g.SA.wordLength == g.GstrandBit + 1ULL &&
              g.SAi.wordLength == g.GstrandBit + 3ULL &&
              g.SA.lengthByte == (g.nSA - 1) * g.SA.wordLength / 8 + 8 &&
              g.SAi.lengthByte == (g.nSAi - 1) * g.SAi.wordLength / 8 + 8,
          "active packed extents");
  Bytes b;
  for (U n : {g.nGenome,
              g.nSA,
              g.nSAbyte,
              U(g.SA.lengthByte),
              U(g.SA.wordLength),
              U(g.GstrandBit),
              g.GstrandMask,
              g.nSAi,
              U(g.SAi.lengthByte),
              U(g.SAi.wordLength),
              g.pGe.gSAindexNbases,
              g.pGe.gSAsparseD,
              U(0),
              U(200),
              U(200),
              U(5),
              g.SAiMarkNmaskC,
              g.SAiMarkNmask,
              g.SAiMarkAbsentMaskC,
              g.SAiMarkAbsentMask,
              g.nChrReal,
              g.sjdbN,
              g.sjGstart,
              g.sjdbOverhang,
              g.sjdbLength})
    le(b, n);
  for (U j = 0; j <= g.pGe.gSAindexNbases; ++j)
    le(b, g.genomeSAindexStart[j]);
  for (U j = 0; j <= g.nChrReal; ++j)
    le(b, g.chrStart[j]);
  for (U j = 0; j < g.nChrReal; ++j)
    le(b, g.chrLength[j]);
  for (unsigned j = 0; j < 3; ++j) {
    auto d = ssir::Sha256::hash(j == 0   ? g.G1
                                : j == 1 ? g.SA.charArray
                                         : g.SAi.charArray,
                                j == 0   ? g.nGenome + 400
                                : j == 1 ? g.SA.lengthByte
                                         : g.SAi.lengthByte);
    b.insert(b.end(), d.begin(), d.end());
  }
  return b;
}

static State &state() {
  if (local)
    return *local;
  local = new State;
  std::lock_guard<std::mutex> l(registry_mu);
  registry.push_back(local);
  return *local;
}
static std::string env(const char *k) {
  const char *v = std::getenv(k);
  require(v && *v, std::string("missing ") + k);
  return v;
}
static U estimate(const BufferedRead &r) {
  U n = 0;
  for (const auto &o : r.outers)
    if (o.kind == 2)
      ++n;
  return 176 + n * (240 + 2 * r.length);
}
static U inners(const BufferedRead &r) {
  U n = 0;
  for (const auto &o : r.outers)
    n += o.kind == 2;
  return n;
}
static U records(const BufferedRead &r) { return 1 + inners(r); }
static bool valid(const State &x, const BufferedRead &r) {
  return r.length > 0 && r.length <= 4096 &&
         x.saved.size() < per_worker_reads &&
         x.saved_outers + r.outers.size() <= 100000 &&
         x.saved_records + records(r) <= per_worker_records &&
         U(header.size()) + 120 + x.saved_bytes + estimate(r) <= FILE_BYTES &&
         x.saved_records - x.saved.size() + inners(r) <= per_worker_inners;
}
static void stop(State &x) {
  x.stopped = true;
  x.have = false;
  x.outer = x.inner = x.direct = false;
  x.current = BufferedRead{};
}

void guard(Parameters &p) {
  split_capture::guard(p);
  require(!p.peOverlap.yes && !p.var.yes, "unsupported overlap/remap profile");
}
void startup(Genome &g, int argc, char **argv) {
  directory = env("SSIR_REAL_DIRECTORY");
  ::setenv("SSIR_COUNTERS_ONLY", "1", 1);
  ::setenv("SSIR_COUNTERS_DIRECTORY", directory.c_str(), 1);
  require(!std::getenv("SSIR_DIRECTORY"),
          "SSIR_DIRECTORY forbidden for real capture");
  split_capture::startup(g);
  const char *stop_after = std::getenv("SSIR_REAL_STOP_AFTER");
  U limit = stop_after ? std::strtoull(stop_after, nullptr, 10) : 1000000;
  require(limit > 0 && limit <= 1000000 && g.P.runThreadN > 0,
          "real capture limit/thread count");
  per_worker_inners = limit / U(g.P.runThreadN);
  require(per_worker_inners > 0, "per-worker INNER budget");
  active_metadata = metadata(g);
  bound_genome = &g;
  bound_g = g.G;
  bound_sa = g.SA.charArray;
  bound_sai = g.SAi.charArray;
  exclusive(directory + "/probe.active", active_metadata);
  std::ostringstream effective;
  for (const auto *p : g.P.parArray)
    effective << p->nameString << '\t' << *p << '\n';
  const std::string resolved = effective.str();
  require(resolved.size() <= 65536, "effective parameter extent");
  exclusive(directory + "/probe.effective",
            Bytes(resolved.begin(), resolved.end()));
  Bytes args;
  for (int i = 0; i < argc; ++i) {
    const std::string arg = argv[i];
    args.insert(args.end(), arg.begin(), arg.end());
    args.push_back(0);
  }
  exclusive(directory + "/argv.bin", args);
  const std::string helper = env("SSIR_REAL_HEADER_HELPER");
  const pid_t child = fork();
  require(child >= 0, "binding helper fork");
  if (child == 0) {
    execlp("python3", "python3", "-B", helper.c_str(), directory.c_str(),
           static_cast<char *>(nullptr));
    _exit(127);
  }
  int status = 0;
  pid_t waited;
  do {
    waited = waitpid(child, &status, 0);
  } while (waited < 0 && errno == EINTR);
  require(waited == child && WIFEXITED(status) && WEXITSTATUS(status) == 0,
          "binding helper failed");
  header = read_file(directory + "/binding.header", 1048576);
  require(header.size() >= 168 && get(header, 24) == FILE_BYTES &&
              get(header, 32) == per_worker_records &&
              get(header, 40) == per_worker_reads,
          "real header caps");
  require(header.size() >= 168 + active_metadata.size() &&
              std::equal(active_metadata.begin(), active_metadata.end(),
                         header.begin() + 168),
          "binding active metadata mismatch");
}
void verify_end(Genome &g) {
  require(&g == bound_genome && g.G == bound_g && g.SA.charArray == bound_sa &&
              g.SAi.charArray == bound_sai && metadata(g) == active_metadata,
          "post-mapping active mutation");
  verified = true;
}

void read_begin(char **s, U n) {
  split_capture::read_begin(s, n);
  State &x = state();
  ++x.ordinal;
  if (x.stopped || n == 0 || n > 4096) {
    x.have = false;
    return;
  }
  x.have = true;
  x.outer = x.inner = x.direct = false;
  x.current = BufferedRead{};
  x.current.ordinal = x.ordinal;
  x.current.length = n;
  x.current.b0.assign(s[0], s[0] + n);
  x.current.b1.assign(s[1], s[1] + n);
}
void split_count(U n) {
  split_capture::split_count(n);
  State &x = state();
  if (x.have)
    x.current.splits = n;
}
void outer_begin(U p, U st, U site, U frag, U S, U N, U d) {
  split_capture::outer_begin(p, st, site, frag, S, N, d);
  State &x = state();
  if (!x.have)
    return;
  if (x.current.outers.size() >= 100000 || N == 0 || N > 4096) {
    stop(x);
    return;
  }
  x.outer = true;
  Outer o;
  o.piece = p;
  o.start = st;
  o.site = site;
  o.fragment = frag;
  o.S = S;
  o.N = N;
  o.dir = d;
  x.current.outers.push_back(o);
}
void outer_end() {
  split_capture::outer_end();
  State &x = state();
  if (x.have)
    x.outer = false;
}
void reduced() {
  split_capture::reduced();
  State &x = state();
  if (x.have && x.outer)
    ++x.current.outers.back().reductions;
}
void prefix_ready(U n) { split_capture::prefix_ready(n); }
void branch(unsigned k, bool noN, bool upper) {
  split_capture::branch(k, noN, upper);
  State &x = state();
  if (!x.have || !x.outer)
    return;
  Outer &o = x.current.outers.back();
  o.kind = k;
  o.context = (!noN ? 1 : 0) | (!upper ? 2 : 0);
  if (k == 2 && (x.saved_records - x.saved.size() + inners(x.current) >
                     per_worker_inners ||
                 U(header.size()) + 120 + x.saved_bytes + estimate(x.current) >
                     FILE_BYTES ||
                 x.saved_records + records(x.current) > per_worker_records)) {
    stop(x);
    return;
  }
  if (k == 2)
    x.inner = true;
  if (k == 1) {
    x.direct = true;
    x.work = Work{};
  }
}
void inner_begin(Genome &g, char **s, U S, U N, U lo, U hi, bool d, U L) {
  split_capture::inner_begin(g, s, S, N, lo, hi, d, L);
  State &x = state();
  if (!x.have || !x.inner)
    return;
  Outer &o = x.current.outers.back();
  o.inner = Inner{};
  o.inner.v[3] = o.piece;
  o.inner.v[4] = o.start;
  o.inner.v[5] = o.site;
  o.inner.v[6] = o.fragment;
  o.inner.v[7] = S;
  o.inner.v[8] = N;
  o.inner.v[9] = lo;
  o.inner.v[10] = hi;
  o.inner.v[11] = L;
  o.inner.dir = d ? 1 : 0;
  o.inner.context = o.context;
  o.inner.reductions = o.reductions;
  o.inner.b0 = x.current.b0;
  o.inner.b1 = x.current.b1;
  x.work = Work{};
}
void inner_end(U L, U lo, U hi, U rep) {
  split_capture::inner_end(L, lo, hi, rep);
  State &x = state();
  if (!x.have || !x.inner)
    return;
  Outer &o = x.current.outers.back();
  o.inner.v[12] = L;
  o.inner.v[13] = lo;
  o.inner.v[14] = hi;
  o.inner.v[15] = rep;
  o.inner.work = x.work;
  x.inner = false;
}
void direct_end() {
  split_capture::direct_end();
  State &x = state();
  if (x.have && x.direct) {
    x.current.outers.back().direct = x.work;
    x.direct = false;
  }
}
void read_end() {
  split_capture::read_end();
  State &x = state();
  if (!x.have)
    return;
  if (x.outer || x.inner || x.direct || !valid(x, x.current)) {
    stop(x);
    return;
  }
  x.saved_bytes += estimate(x.current);
  x.saved_records += records(x.current);
  x.saved_outers += x.current.outers.size();
  x.saved.push_back(std::move(x.current));
  x.have = false;
}
void inner_phase(unsigned p) { split_capture::inner_phase(p); }
void compare_guard(Genome &g, char **s, U S, U N, U L, U i, bool d) {
  split_capture::compare_guard(g, s, S, N, L, i, d);
}
void compare_begin(U a, bool r, bool g, U S, U N, U L) {
  split_capture::compare_begin(a, r, g, S, N, L);
  State &x = state();
  if (x.have && (x.inner || x.direct)) {
    x.last_direction = (r ? 0 : 2) + (g ? 0 : 1);
    ++x.work.calls[x.last_direction];
  }
}
void compared(U bytes) {
  split_capture::compared(bytes);
  State &x = state();
  if (x.have && (x.inner || x.direct))
    x.work.bytes[x.last_direction] += bytes;
}

static void replay(const std::vector<BufferedRead> &all, U worker) {
  if (all.empty())
    return;
  Bytes h = header;
  require(h.size() >= 56, "header");
  U count = all.size();
  for (unsigned j = 0; j < 8; ++j)
    h[48 + j] = static_cast<unsigned char>(count >> (8 * j));
  std::ostringstream n;
  n << directory << "/worker-" << worker << ".ssir";
  Writer w(n.str(), h);
  for (const BufferedRead &r : all) {
    w.begin(r.length);
    w.splits(r.splits);
    for (const Outer &o : r.outers) {
      U outer = w.outer();
      w.branch(o.kind, o.reductions, o.context);
      if (o.kind == 2) {
        Inner in = o.inner;
        in.v[0] = w.inner_id();
        in.v[1] = w.read_id();
        in.v[2] = outer;
        w.inner(in);
      } else if (o.kind == 1)
        w.direct(o.direct);
    }
    w.end();
  }
  w.finish();
}
static std::string hex_at(unsigned at) {
  static const char d[] = "0123456789abcdef";
  std::string s;
  for (unsigned j = 0; j < 32; ++j) {
    unsigned v = header[at + j];
    s += d[v >> 4];
    s += d[v & 15];
  }
  return s;
}
void finish() {
  require(verified, "missing pre-free verification");
  split_capture::finish();
  U worker = 0, total = 0, reads = 0;
  std::ostringstream per;
  per << '[';
  for (State *x : registry) {
    if (worker)
      per << ',';
    U n = 0;
    std::ostringstream a;
    a << "{\"worker\":" << worker << ",\"read_ordinals\":[";
    for (std::size_t j = 0; j < x->saved.size(); ++j) {
      if (j)
        a << ',';
      a << x->saved[j].ordinal;
      n += inners(x->saved[j]);
    }
    a << "]}\n";
    if (!x->saved.empty()) {
      const std::string side = a.str();
      std::ostringstream path;
      path << directory << "/worker-" << worker << ".attribution.json";
      exclusive(path.str(), Bytes(side.begin(), side.end()));
    }
    per << "{\"worker\":" << worker << ",\"reads\":" << x->saved.size()
        << ",\"inners\":" << n << "}";
    total += n;
    reads += x->saved.size();
    replay(x->saved, worker);
    ++worker;
  }
  per << ']';
  std::ostringstream m;
  m << "{\"schema\":\"real-inner-capture-v1\",\"selection\":\"first bounded "
       "complete arrivals per worker\",\"actual_count\":"
    << total << ",\"complete_reads\":" << reads << ",\"source_sha256\":\""
    << hex_at(56) << "\",\"index_sha256\":\"" << hex_at(88)
    << "\",\"runtime_sha256\":\"" << hex_at(120)
    << "\",\"workers\":" << per.str() << "}\n";
  const std::string json = m.str();
  exclusive(directory + "/capture-manifest.json",
            Bytes(json.begin(), json.end()));
}
} // namespace capture
