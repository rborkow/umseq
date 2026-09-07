// Bounded, non-consuming prefix lookahead for the private STAR integration.
// It deliberately shares STAR's readLoad path rather than parsing FASTQ again.
#include "Genome.h"
#include "Parameters.h"
#include "ReadAlign.h"
#include "ReadAlignChunk.h"
#include "SequenceFuns.h"
#include "readLoad.h"
#include "star_integrate.hpp"
#include <algorithm>
#include <array>
#include <cstdio>
#include <cstdlib>
#include <ios>
#include <utility>
#include <vector>

namespace star_integrate {
namespace {
constexpr uint32_t kWindowMax = static_cast<uint32_t>(MAX_WINDOW_READS);
uint32_t window_limit() {
  const char *s = std::getenv("STAR_INTEGRATE_WINDOW_TEST_MAX");
  if (!s)
    return kWindowMax;
  char *end = nullptr;
  const unsigned long n = std::strtoul(s, &end, 10);
  return end && !*end && n && n <= kWindowMax ? static_cast<uint32_t>(n)
                                              : kWindowMax;
}
bool supported_clip(const std::vector<ClipMate> &v) {
  for (const auto &m : v)
    if (m.cr4 || m.type >= 10)
      return false;
  return true;
}
struct SavedStream {
  std::streampos pos = std::streampos(-1);
  std::ios::iostate state = std::ios::goodbit;
};

// A peek is permitted only when it can be rewound before the first readLoad.
// This is intentionally stricter than treating a non-seekable stream as an
// empty window: stock STAR must remain the sole consumer in that case.
bool save_seekable(std::istream &stream, SavedStream &saved) {
  saved.state = stream.rdstate();
  if (saved.state != std::ios::goodbit)
    return false;
  saved.pos = stream.tellg();
  if (saved.pos == std::streampos(-1)) {
    stream.clear(saved.state);
    return false;
  }
  stream.clear();
  stream.seekg(saved.pos);
  if (stream.fail()) {
    stream.clear(saved.state);
    return false;
  }
  stream.clear(saved.state);
  return true;
}

bool restore_streams(ReadAlign &ra, uint32_t ends,
                     const std::array<SavedStream, MAX_N_MATES> &saved) {
  for (uint32_t mate = 0; mate < ends; ++mate) {
    std::istream &stream = *ra.readInStream[mate];
    stream.clear();
    stream.seekg(saved[mate].pos);
    if (stream.fail())
      return false;
    stream.clear(saved[mate].state);
  }
  return true;
}

[[noreturn]] void fatal_restore() {
  std::fputs("STAR_INTEGRATE fatal: input lookahead rewind failed; "
             "refusing to continue with advanced private input\n",
             stderr);
  std::abort();
}

// Exact prefix portion of ReadAlign::maxMappableLength2strands for admitted
// gSAsparseD==1. False leaves short/direct and unsafe paths to stock CPU.
bool append_prefix_call(const Parameters &p, Genome &map_gen, const char *read,
                        uint piece_start_in, uint piece_length_in, uint i_dir,
                        uint piece, uint fragment, uint nstart, uint lstart,
                        uint istart, std::vector<InnerCall> &out) {
  if (!read || !piece_length_in || p.pGe.gSAsparseD != 1 ||
      !p.pGe.gSAindexNbases)
    return false;
  const bool dir_r = i_dir == 0;
  const uint i_dist = 0, piece_length = piece_length_in;
  const uint lmax = std::min(p.pGe.gSAindexNbases, piece_length);
  if (!lmax)
    return false;
  uint piece_start = piece_start_in, ind1 = 0;
  if (dir_r)
    for (uint ii = 0; ii < lmax; ++ii) {
      ind1 <<= 2LLU;
      ind1 += static_cast<uint>(read[piece_start + ii]);
    }
  else
    for (uint ii = 0; ii < lmax; ++ii) {
      ind1 <<= 2LLU;
      ind1 += 3 - static_cast<uint>(read[piece_start - ii]);
    }
  uint lind = lmax, isa1 = 0;
  while (lind > 0) {
    isa1 = map_gen.SAi[map_gen.genomeSAindexStart[lind - 1] + ind1];
    if ((isa1 & map_gen.SAiMarkAbsentMaskC) == 0)
      break;
    --lind;
    ind1 >>= 2;
  }
  // Stock assumes this cannot happen at a mapping call and would index Lind-1.
  if (!lind)
    return false;
  uint isa2 = 0;
  bool isa2_good = true;
  if (map_gen.genomeSAindexStart[lind - 1] + ind1 + 1 <
      map_gen.genomeSAindexStart[lind]) {
    isa2 = map_gen.SAi[map_gen.genomeSAindexStart[lind - 1] + ind1 + 1];
    if ((isa2 & map_gen.SAiMarkAbsentMaskC) == 0)
      isa2 = (isa2 & map_gen.SAiMarkNmask) - 1;
    else {
      isa2 = map_gen.nSA - 1;
      isa2_good = false;
    }
  } else {
    isa2 = map_gen.nSA - 1;
    isa2_good = false;
  }
  const bool isa1_no_n = (isa1 & map_gen.SAiMarkNmaskC) == 0;
  if ((lind < p.pGe.gSAindexNbases && isa1_no_n && isa2_good) ||
      (isa1 == isa2 && isa1_no_n && isa2_good))
    return false;
  const uint prefix = isa2_good && isa1_no_n ? lind : 0;
  InnerCall call = {};
  call.start = piece_start;
  call.length = piece_length;
  call.low = isa1 & map_gen.SAiMarkNmask;
  call.high = isa2;
  call.dir = dir_r;
  call.prefix = prefix;
  call.piece = piece;
  call.fragment = fragment;
  call.distance = i_dist;
  call.nstart = nstart;
  call.lstart = lstart;
  call.istart = istart;
  out.push_back(call);
  return true;
}
} // namespace

void prepare_window(ReadAlignChunk &chunk) {
#if !STAR_INTEGRATE
  (void)chunk;
  return;
#else
  if (window_remaining() || !setup(chunk.P, chunk.mapGen))
    return;
  ReadAlign &ra = *chunk.RA;
  const uint32_t ends = chunk.P.readNends;
  if (!ends || ends > MAX_N_MATES || ra.clipMates.size() < ends)
    return;
  for (uint32_t mate = 0; mate < ends; ++mate)
    if (!ra.readInStream[mate] || !supported_clip(ra.clipMates[mate]))
      return;

  std::vector<WindowRead> frames;
  frames.reserve(static_cast<size_t>(
      std::min<uint64_t>(window_limit(), MAX_WINDOW_READS)));
  uint64_t frame_bytes = frames.capacity() * (sizeof(WindowRead) + 32);
  uint64_t frame_candidates = 0;
  {
    std::array<SavedStream, MAX_N_MATES> saved{};
    for (uint32_t mate = 0; mate < ends; ++mate)
      if (!save_seekable(*ra.readInStream[mate], saved[mate]))
        return;
    for (uint32_t mate = 0; mate < ends; ++mate)
      ra.readInStream[mate]->clear();
    for (uint32_t rec = 0; rec < window_limit(); ++rec) {
      std::array<std::array<char, DEF_readSeqLengthMax + 1>, MAX_N_MATES> raw{},
          num{}, qual{};
      std::array<std::array<char, DEF_readNameLengthMax>, MAX_N_MATES> name{};
      std::array<uint, MAX_N_MATES> len{}, original{};
      std::array<std::vector<ClipMate>, MAX_N_MATES> clips;
      std::array<std::string, MAX_N_MATES> extra;
      uint ordinal = 0;
      uint32_t file_index = 0;
      char filter = 0;
      int status0 = -1;
      bool consistent = true;
      for (uint32_t mate = 0; mate < ends; ++mate) {
        clips[mate] = ra.clipMates[mate];
        const int status =
            readLoad(*ra.readInStream[mate], chunk.P, len[mate], original[mate],
                     name[mate].data(), raw[mate].data(), num[mate].data(),
                     qual[mate].data(), clips[mate], ordinal, file_index,
                     filter, extra[mate]);
        if (!mate)
          status0 = status;
        else if (status != status0)
          consistent = false;
      }
      if (!consistent || status0 == -1)
        break;
      uint length = len[0];
      if (chunk.P.readNmates == 2) {
        length = len[0] + len[1] + 1;
        if (length > DEF_readSeqLengthMax)
          break;
        num[0][len[0]] = MARK_FRAG_SPACER_BASE;
        complementSeqNumbers(num[1].data(), num[0].data() + len[0] + 1, len[1]);
        for (uint ii = 0; ii < len[1] / 2; ++ii)
          std::swap(num[0][length - ii - 1], num[0][ii + len[0] + 1]);
      }
      std::vector<uint> split(3 * chunk.P.maxNsplit);
      std::array<uint *, 3> split_r = {split.data(),
                                       split.data() + chunk.P.maxNsplit,
                                       split.data() + 2 * chunk.P.maxNsplit};
      const uint nsplit = qualitySplit(num[0].data(), length, chunk.P.maxNsplit,
                                       chunk.P.seedSplitMin, split_r.data());
      const uint seed_start_max =
          std::min(chunk.P.seedSearchStartLmax,
                   static_cast<uint>(chunk.P.seedSearchStartLmaxOverLread *
                                     (length - 1)));
      // This is a conservative admission charge: every eligible initial call
      // owns CANDIDATE_BUDGET_BYTES in the coordinator, even when its sparse
      // prefix later declines to submit.  It bounds the producer before any
      // frame-byte or candidate-vector allocation.
      uint64_t candidate_bound = 0;
      for (uint ip = 0; ip < nsplit; ++ip) {
        const uint nstart =
            chunk.P.seedSearchStartLmax > 0 && seed_start_max < split_r[1][ip]
                ? split_r[1][ip] / seed_start_max + 1
                : 1;
        const uint lstart = split_r[1][ip] / nstart;
        for (uint istart = 0; istart < std::min<uint>(nstart, 2); ++istart)
          if (istart * lstart + chunk.P.seedMapMin < split_r[1][ip])
            candidate_bound += 2;
      }
      const uint64_t read_charge = 2ULL * length;
      const bool fits =
          frames.size() < MAX_WINDOW_READS &&
          candidate_bound <= MAX_WINDOW_CANDIDATES - frame_candidates &&
          read_charge <= MAX_WINDOW_BYTES - frame_bytes &&
          candidate_bound <= (MAX_WINDOW_BYTES - frame_bytes - read_charge) /
                                 CANDIDATE_BUDGET_BYTES;
      if (!fits)
        break;

      WindowRead frame = {};
      frame.mate0_len = len[0];
      frame.mate1_len = ends > 1 ? len[1] : 0;
      frame.split_count = nsplit;
      // Establish complete identity before this frame appends any candidate.
      assign_frame_identity(frame, ordinal, chunk.iThread, chunk.iChunkIn);
      frame.a.assign(reinterpret_cast<const uint8_t *>(num[0].data()),
                     reinterpret_cast<const uint8_t *>(num[0].data()) + length);
      std::array<char, DEF_readSeqLengthMax + 1> complement{};
      complementSeqNumbers(num[0].data(), complement.data(), length);
      frame.b.assign(reinterpret_cast<const uint8_t *>(complement.data()),
                     reinterpret_cast<const uint8_t *>(complement.data()) +
                         length);
      frame.candidates.reserve(static_cast<size_t>(candidate_bound));
      for (uint ip = 0; ip < nsplit; ++ip) {
        const uint nstart =
            chunk.P.seedSearchStartLmax > 0 && seed_start_max < split_r[1][ip]
                ? split_r[1][ip] / seed_start_max + 1
                : 1;
        const uint lstart = split_r[1][ip] / nstart;
        // Initial stock seed calls only: Lmapped continuations/suppression stay
        // CPU.
        for (uint idir = 0; idir < 2; ++idir)
          for (uint istart = 0; istart < std::min<uint>(nstart, 2); ++istart) {
            if (istart * lstart + chunk.P.seedMapMin >= split_r[1][ip])
              continue;
            const uint shift = idir == 0 ? split_r[0][ip] + istart * lstart
                                         : split_r[0][ip] + split_r[1][ip] -
                                               istart * lstart - 1;
            if (append_prefix_call(chunk.P, chunk.mapGen, num[0].data(), shift,
                                   split_r[1][ip] - istart * lstart, idir, ip,
                                   split_r[2][ip], nstart, lstart, istart,
                                   frame.candidates)) {
              ChainContext context;
              context.piece = ip;
              context.fragment = split_r[2][ip];
              context.istart = istart;
              context.nstart = nstart;
              context.lstart = lstart;
              context.piece_start = split_r[0][ip];
              context.piece_length = split_r[1][ip];
              context.split_count = nsplit;
              frame.candidates.back() = build_inner_call(
                  frame.candidates.back(), frame, context, frame.index_epoch);
            }
          }
      }
      frame_bytes += read_charge + candidate_bound * CANDIDATE_BUDGET_BYTES;
      frame_candidates += candidate_bound;
      frames.push_back(std::move(frame));
    }
    if (!restore_streams(ra, ends, saved))
      fatal_restore();
  } // restoration is verified before stock consumes any published frame
  submit_window(std::move(frames));
#endif
}
} // namespace star_integrate
