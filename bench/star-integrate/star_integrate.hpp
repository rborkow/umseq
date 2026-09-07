// Private STAR_INTEGRATE hook.  It is compiled only into the private STAR copy.
// The coordinator owns only immutable numeric read bytes and inner-call
// records.
#ifndef STAR_INTEGRATE_HPP
#define STAR_INTEGRATE_HPP
#include <stdint.h>
#include <vector>
class Parameters;
class Genome;
class ReadAlign;
class ReadAlignChunk;
namespace star_integrate {
struct InnerCall {
  uint64_t start, length, low, high, dir, prefix, piece, fragment, distance,
      nstart, lstart, istart, generation;
  uint64_t piece_start, piece_length, kind, read_id, index_epoch, worker,
      chunk, mate_context, split_context;
};
// Internal lookahead/consumer boundary (not the frozen external C ABI).
// Read identity and bytes are owned once per frame, never per candidate.
struct WindowRead {
  uint64_t ordinal;
  std::vector<uint8_t> a, b;
  std::vector<InnerCall> candidates;
  uint64_t mate0_len, mate1_len, generation, index_epoch, worker, chunk;
};
constexpr uint64_t INITIAL_KIND = 1;
constexpr uint64_t MAX_WINDOW_READS = 32768;
constexpr uint64_t MAX_WINDOW_CANDIDATES = 262144;
constexpr uint64_t MAX_WINDOW_BYTES = 128ULL * 1024 * 1024;
constexpr uint64_t CANDIDATE_BUDGET_BYTES = 512;
constexpr uint64_t MAX_INFLIGHT_REQUESTS = 8ULL * 1024 * 1024;
constexpr uint64_t MAX_INFLIGHT_BYTES = 4ULL * 1024 * 1024 * 1024;
constexpr uint64_t MAX_PENDING_REQUESTS = MAX_WINDOW_CANDIDATES;
constexpr uint64_t MAX_PENDING_BYTES = MAX_WINDOW_BYTES;
bool window_remaining();
void submit_window(std::vector<WindowRead> &&);
void set_chain(uint64_t piece, uint64_t fragment, uint64_t istart,
               uint64_t nstart, uint64_t lstart, uint64_t lmapped,
               uint64_t piece_start, uint64_t piece_length);
void reverse_suppressed(uint64_t piece);
void end_chunk();
bool setup(const Parameters &, const Genome &);
bool lookup(const Parameters &, const Genome &, char **read1, uint64_t read_len,
            const InnerCall &, uint64_t out_range[2], uint64_t &nrep,
            uint64_t &maxL);
bool strict();
uint64_t current_generation();
uint64_t current_epoch();
// Chunk-local peek. This is intentionally not a submission API: lookup only
// consumes completed entries prepared before stock oneRead reaches them.
void prepare_window(ReadAlignChunk &);
void begin_map(ReadAlign &);
void finish();
} // namespace star_integrate
#endif
