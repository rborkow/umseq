// Actual STAR types/readLoad/clipping/qualitySplit, actual
// producer/coordinator. The fixture constructor allocates only input buffers.
// The generated oneRead prefix ends at mapOneRead: alignment, SAM output and
// device work are excluded.
#define STAR_INTEGRATE 1
#include "star_integrate.cpp"
#include "star_integrate_window.cpp"
#include <cassert>
#include <sstream>

namespace {
std::string mode;
int last_status = 0;
size_t loaded = 0;
std::vector<std::string> sequences[2], records[2];
std::vector<std::streampos> boundaries[2];
std::vector<uint64_t> ordinals;
char raw_storage[3][DEF_readSeqLengthMax + 1] = {};
char num_storage[3][DEF_readSeqLengthMax + 1] = {};
char qual_storage[3][DEF_readSeqLengthMax + 1] = {};
char name_storage[3][DEF_readNameLengthMax] = {};
char *raw_ptr[3], *num_ptr[3], *qual_ptr[3], *name_ptr[3];
size_t boundary_checks = 0;
uint64_t inspected_generation = 0;
struct StockFatal {};

void inspect_peek(ReadAlignChunk &chunk) {
  using namespace star_integrate;
  // Every producer peek must leave the real streams at the last stock boundary.
  for (size_t m = 0; m < 2; ++m) {
    assert(chunk.RA->readInStream[m]->rdstate() == std::ios::goodbit);
    assert(chunk.RA->readInStream[m]->tellg() == boundaries[m][loaded]);
  }
  if (!current_window)
    return;
  if (mode == "empty-successor" && loaded >= 3 && loaded < ordinals.size())
    assert(!current_window->peek_end.has_successor &&
           "empty successor must disable further lookahead on the live tail");
  if (mode == "tail" && loaded > 0)
    assert(!next_window && "live terminal window must not duplicate lookahead");
  const auto &w = next_window ? next_window : current_window;
  if (mode == "budget" && w->frames.back().ordinal < ordinals.size()) {
    const uint64_t end = w->frames.back().ordinal;
    assert(w->peek_end.ordinal == end + 1);
    for (size_t m = 0; m < 2; ++m)
      assert(w->peek_end.stream_pos[m] == boundaries[m][end]);
    assert(w->frames.size() < MAX_WINDOW_READS);
    if (inspected_generation != w->frames.front().generation) {
      inspected_generation = w->frames.front().generation;
      std::printf("budget window: frames=%zu first=%llu last=%llu end=%llu\n",
                  w->frames.size(),
                  (unsigned long long)w->frames.front().ordinal,
                  (unsigned long long)w->frames.back().ordinal,
                  (unsigned long long)w->peek_end.ordinal);
      for (size_t i = 1; i < w->frames.size(); ++i)
        assert(w->frames[i].ordinal == w->frames[i - 1].ordinal + 1);
    }
    ++boundary_checks;
  }
}
} // namespace

ReadAlign::ReadAlign(Parameters &p, Genome &g, Transcriptome *, int)
    : mapGen(g), genOut(g), iRead(0), iReadAll(0), P(p) {
  for (size_t m = 0; m < 3; ++m) {
    raw_ptr[m] = raw_storage[m];
    num_ptr[m] = num_storage[m];
    qual_ptr[m] = qual_storage[m];
    name_ptr[m] = name_storage[m];
  }
  Read0 = raw_ptr;
  Read1 = num_ptr;
  Qual0 = qual_ptr;
  readNameMates = name_ptr;
  readNameExtra.resize(2);
  clipMates.resize(2, std::vector<ClipMate>(2));
  for (auto &mate : clipMates)
    for (auto &clip : mate) {
      clip = ClipMate();
      clip.type = -1;
    }
}
ReadAlignChunk::ReadAlignChunk(Parameters &p, Genome &g, Transcriptome *, int)
    : P(p), RA(nullptr), iChunkIn(1), iThread(0), mapGen(g) {}

// Observation at the alignment boundary, after actual generated handoff and
// stock readLoad. Exact raw names, qualities, metadata and all numeric strands.
int ReadAlign::mapOneRead() {
  using namespace star_integrate;
  assert(loaded < ordinals.size() && iReadAll == ordinals[loaded]);
  std::vector<char> expected;
  for (size_t m = 0; m < 2; ++m) {
    assert(std::string(Read0[m]) == sequences[m][loaded]);
    assert(std::string(Qual0[m]) ==
           std::string(sequences[m][loaded].size(), 'I'));
    assert(std::string(readNameMates[m]) == "@r" + std::to_string(loaded + 1));
    assert(readLength[m] == sequences[m][loaded].size());
    assert(readLengthOriginal[m] == readLength[m]);
    assert(readNameExtra[m] == "extra");
  }
  assert(readFilter == 'N' && readFilesIndex == 7 && readFileType == 2);
  for (char c : sequences[0][loaded])
    expected.push_back(c == 'N' ? 4 : std::string("ACGT").find(c));
  expected.push_back(MARK_FRAG_SPACER_BASE);
  for (auto it = sequences[1][loaded].rbegin();
       it != sequences[1][loaded].rend(); ++it)
    expected.push_back(*it == 'N' ? 4 : 3 - std::string("ACGT").find(*it));
  assert(Lread == expected.size());
  for (size_t i = 0; i < expected.size(); ++i) {
    const char complement = expected[i] < 4 ? 3 - expected[i] : expected[i];
    assert(Read1[0][i] == expected[i]);
    assert(Read1[1][i] == complement);
    assert(Read1[2][expected.size() - i - 1] == complement);
  }
  // Execute the accepted generated mapOneRead entry; alignment is excluded.
#include "generated_map_entry.inc"
  (void)starIntegrateEnabled;
  if (mode == "budget")
    assert(current_frame && current_frame->ordinal == iReadAll);
  if (mode == "overshoot" && loaded == 1)
    assert(current_frame && current_frame->ordinal == iReadAll);
  if (mode == "gap" && loaded == 1)
    assert(!current_frame);
  if (mode == "gap" && loaded == 2)
    assert(current_frame && current_frame->ordinal == iReadAll);
  ++loaded;
  return 0;
}
#include "generated_chunk.inc"
#include "generated_oneRead.inc"

// Preserve stock's fatal text and use a deterministic subprocess exit code.
void exitWithError(std::string message, std::ostream &, std::ostream &, int,
                   Parameters &) {
  std::cerr << message;
  throw StockFatal();
}
struct UsiPrefixContext {};
extern "C" int32_t usi_destroy_v2(UsiPrefixContext **ctx, UsiErrorV1 *) {
  *ctx = nullptr;
  return 0;
}
extern "C" int32_t usi_search_batch_v3(UsiPrefixContext *, uint64_t,
                                       const uint8_t *, uint64_t,
                                       const ProbeRequestV3 *, uint64_t,
                                       ProbeOutputV3 *, ProbeStats *,
                                       UsiErrorV1 *) {
  assert(false && "stream fixture never dispatches device work");
  return -1;
}

// Fails endpoint seeking on mate 0, then rollback on mate 1. Initial
// eligibility and all normal stream operations still use std::stringbuf.
class RollbackBuffer : public std::stringbuf {
public:
  int positions = 0;
  int fail_at = 0;
  explicit RollbackBuffer(const std::string &s) : std::stringbuf(s) {}
  pos_type seekpos(pos_type pos, std::ios_base::openmode which) override {
    ++positions;
    if (fail_at && positions == fail_at)
      return pos_type(off_type(-1));
    return std::stringbuf::seekpos(pos, which);
  }
};

int run_fixture(int argc, char **argv) {
  assert(argc == 2);
  mode = argv[1];
  unsetenv("STAR_INTEGRATE_WINDOW_TEST_MAX");
  Parameters p;
  p.readNends = p.readNmates = 2;
  p.outFilterBySJoutStage = 2;
  p.outQSconversionAdd = 0;
  p.maxNsplit = 1000;
  p.seedSplitMin = 2;
  p.seedMapMin = 1;
  p.seedSearchStartLmax = 0;
  p.seedSearchStartLmaxOverLread = 1;
  p.pGe.gSAsparseD = 1;
  Genome g(p, p.pGe);
  ReadAlign ra(p, g, nullptr, 0);
  ReadAlignChunk chunk(p, g, nullptr, 0);
  chunk.RA = &ra;
  size_t count = mode == "budget" ? 1800 : 6;
  if (mode == "empty-successor" || mode == "rollback")
    setenv("STAR_INTEGRATE_WINDOW_TEST_MAX", "3", 1);
  std::string input[2];
  for (size_t m = 0; m < 2; ++m) {
    boundaries[m].push_back(std::streampos(0));
    for (size_t i = 0; i < count; ++i) {
      std::string sequence;
      for (int k = 0; k < (mode == "budget" ? 50 : 1); ++k)
        sequence += std::string(4, "ACGT"[(i + m) % 4]) + "NN";
      sequences[m].push_back(sequence);
      const auto record = "@r" + std::to_string(i + 1) + " " +
                          std::to_string(i + 1) + " N 7 extra\n" + sequence +
                          "\n+\n" + std::string(sequence.size(), 'I') + "\n";
      records[m].push_back(record);
      input[m] += record;
      boundaries[m].push_back(std::streampos(input[m].size()));
    }
  }
  for (size_t i = 0; i < count; ++i)
    ordinals.push_back(i + 1);
  if (mode == "mismatch0")
    input[0] = records[0][0];
  if (mode == "mismatch1")
    input[1] = records[1][0];
  RollbackBuffer buffers[] = {RollbackBuffer(input[0]),
                              RollbackBuffer(input[1])};
  std::istream left(&buffers[0]), right(&buffers[1]);
  ra.readInStream[0] = &left;
  ra.readInStream[1] = &right;
  auto &s = star_integrate::S();
  s.enabled = true;
  if (mode == "refusal")
    s.live_requests = star_integrate::MAX_INFLIGHT_REQUESTS;
  if (mode.compare(0, 3, "eof") == 0 || mode.compare(0, 4, "fail") == 0) {
    const size_t mate = mode.back() - '0';
    const auto state = mode[0] == 'e' ? std::ios::eofbit : std::ios::failbit;
    ra.readInStream[mate]->setstate(state);
    star_integrate::prepare_window(chunk);
    assert(!star_integrate::current_window);
    assert(ra.readInStream[mate]->rdstate() == state);
    assert(ra.readInStream[1 - mate]->rdstate() == std::ios::goodbit);
    ra.readInStream[mate]->clear();
    assert(left.tellg() == 0 && right.tellg() == 0);
  } else if (mode == "rollback") {
    star_integrate::prepare_window(chunk);
    buffers[0].fail_at =
        buffers[0].positions + 2; // save succeeds, endpoint fails
    buffers[1].fail_at =
        buffers[1].positions + 2; // save succeeds, rollback fails
    star_integrate::prepare_window(chunk);
    assert(false && "rollback must abort");
  } else {
    while (loaded < count) {
      chunk.mapChunk();
      assert(last_status == 0);
      if (loaded == 1 && (mode == "overshoot" || mode == "gap")) {
        // Inject lost/stale cursor or missing prefetched frame, not synthetic
        // frames. Subsequent recovery goes through generated pre-oneRead.
        star_integrate::current_window->next_frame =
            mode == "overshoot" ? 0 : 2;
      }
    }
    // Check terminal stock read through generated order; EOF state belongs to
    // stock and must not be hidden by private lookahead.
    chunk.mapChunk();
    assert(last_status == -1 && left.eof() && right.eof());
    if (mode == "budget")
      assert(boundary_checks > 0 && s.totals.prefetch_windows >= 2);
    star_integrate::end_chunk();
    // A second chunk on the same worker starts from its own real positions.
    if (mode == "tail" || mode == "refusal") {
      left.clear();
      right.clear();
      left.seekg(0);
      right.seekg(0);
      loaded = 0;
      ra.iReadAll = 0;
      ++chunk.iChunkIn;
      chunk.mapChunk();
      assert(loaded == 1 &&
             star_integrate::current_window->frames[0].chunk == 2);
    }
  }
  star_integrate::end_chunk();
  if (mode == "refusal")
    s.live_requests = 0;
  s.stopping = true;
  s.coordinator = std::thread(star_integrate::coordinator_main);
  star_integrate::finish();
  assert(s.live_bytes == 0 && s.live_requests == 0);
  return 0;
}

int main(int argc, char **argv) {
  try {
    return run_fixture(argc, argv);
  } catch (const StockFatal &) {
    // Fatal diagnostics are observed without bypassing fixture teardown.
    auto &s = star_integrate::S();
    star_integrate::end_chunk();
    s.stopping = true;
    s.coordinator = std::thread(star_integrate::coordinator_main);
    star_integrate::finish();
    return 42;
  }
}
