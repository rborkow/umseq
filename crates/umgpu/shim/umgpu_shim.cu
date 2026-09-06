#include "umgpu_shim.h"
#include <cuda_runtime.h>
#include <cub/cub.cuh>
#ifdef UMGPU_NVCOMP
#include <nvcomp/deflate.h>
#endif
#include <thrust/iterator/counting_iterator.h>
#include <thrust/iterator/transform_iterator.h>

static int result(cudaError_t e) { return static_cast<int>(e); }
extern "C" int umgpu_init(int* p) {
  int device = 0; cudaError_t e = cudaGetDevice(&device); if (e != cudaSuccess) return result(e);
  cudaDeviceProp d; e = cudaGetDeviceProperties(&d, device); if (e != cudaSuccess) return result(e);
  int a = 0; e = cudaDeviceGetAttribute(&a, cudaDevAttrPageableMemoryAccess, device); if (e != cudaSuccess) return result(e); p[0] = a;
  e = cudaDeviceGetAttribute(&a, cudaDevAttrPageableMemoryAccessUsesHostPageTables, device); if (e != cudaSuccess) return result(e); p[1] = a;
  e = cudaDeviceGetAttribute(&a, cudaDevAttrDirectManagedMemAccessFromHost, device); if (e != cudaSuccess) return result(e); p[2] = a;
  e = cudaDeviceGetAttribute(&a, cudaDevAttrHostRegisterSupported, device); if (e != cudaSuccess) return result(e); p[3] = a;
  e = cudaDeviceGetAttribute(&a, cudaDevAttrConcurrentManagedAccess, device); if (e != cudaSuccess) return result(e); p[4] = a;
  p[5] = d.major; p[6] = d.minor; p[7] = d.multiProcessorCount; return result(cudaSuccess);
}
extern "C" int umgpu_stream_create(void** s) { return result(cudaStreamCreate((cudaStream_t*)s)); }
extern "C" int umgpu_stream_destroy(void* s) { return result(cudaStreamDestroy((cudaStream_t)s)); }
extern "C" int umgpu_event_create(void** e) { return result(cudaEventCreateWithFlags((cudaEvent_t*)e, cudaEventDisableTiming)); }
extern "C" int umgpu_event_destroy(void* e) { return result(cudaEventDestroy((cudaEvent_t)e)); }
extern "C" int umgpu_event_record(void* e, void* s) { return result(cudaEventRecord((cudaEvent_t)e, (cudaStream_t)s)); }
extern "C" int umgpu_event_query(void* e) { cudaError_t x = cudaEventQuery((cudaEvent_t)e); return x == cudaSuccess ? 0 : (x == cudaErrorNotReady ? 1 : -result(x)); }
extern "C" int umgpu_event_sync(void* e) { return result(cudaEventSynchronize((cudaEvent_t)e)); }
extern "C" int umgpu_host_register(void* p, size_t n) { return result(cudaHostRegister(p, n, cudaHostRegisterDefault)); }
extern "C" int umgpu_host_unregister(void* p) { return result(cudaHostUnregister(p)); }
extern "C" int umgpu_radix_sort_pairs_u64_u32_temp_size(size_t n, size_t* b) { return result(cub::DeviceRadixSort::SortPairs(nullptr, *b, (const uint64_t*)nullptr, (uint64_t*)nullptr, (const uint32_t*)nullptr, (uint32_t*)nullptr, n)); }
extern "C" int umgpu_radix_sort_pairs_u64_u32(void* t, size_t b, const uint64_t* ki, uint64_t* ko, const uint32_t* vi, uint32_t* vo, size_t n, int begin, int end, void* s) { return result(cub::DeviceRadixSort::SortPairs(t, b, ki, ko, vi, vo, n, begin, end, (cudaStream_t)s)); }
extern "C" int umgpu_rle_u64_temp_size(size_t n, size_t* b) { return result(cub::DeviceRunLengthEncode::Encode(nullptr, *b, (const uint64_t*)nullptr, (uint64_t*)nullptr, (uint32_t*)nullptr, (uint32_t*)nullptr, n)); }
extern "C" int umgpu_rle_u64(void* t, size_t b, const uint64_t* k, uint64_t* u, uint32_t* c, uint32_t* r, size_t n, void* s) { return result(cub::DeviceRunLengthEncode::Encode(t, b, k, u, c, r, n, (cudaStream_t)s)); }
extern "C" int umgpu_exclusive_scan_u32_temp_size(size_t n, size_t* b) { return result(cub::DeviceScan::ExclusiveSum(nullptr, *b, (const uint32_t*)nullptr, (uint32_t*)nullptr, n)); }
extern "C" int umgpu_exclusive_scan_u32(void* t, size_t b, const uint32_t* i, uint32_t* o, size_t n, void* s) { return result(cub::DeviceScan::ExclusiveSum(t, b, i, o, n, (cudaStream_t)s)); }
__global__ static void inc(const uint64_t* in, uint64_t* out, size_t n) { size_t i = blockIdx.x * (size_t)blockDim.x + threadIdx.x; if (i < n) out[i] = in[i] + 1; }
extern "C" int umgpu_inc_u64(const uint64_t* in, uint64_t* out, size_t n, void* s) { inc<<<(n + 255) / 256, 256, 0, (cudaStream_t)s>>>(in, out, n); return result(cudaGetLastError()); }
// Deliberately use byte offsets rather than a C++ RecordHeader: Rust owns the ABI.
__device__ static uint32_t rd32(const uint8_t* p) { return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24); }
__device__ static uint64_t rd64(const uint8_t* p) { return (uint64_t)rd32(p) | ((uint64_t)rd32(p + 4) << 32); }
__device__ static uint64_t fnv(uint64_t h, uint8_t b) { return (h ^ b) * 0x100000001b3ULL; }
__device__ static uint64_t fnv_i32(uint64_t h, int32_t x) { const uint8_t* p = (const uint8_t*)&x; for (int i = 0; i != 4; ++i) h = fnv(h, p[i]); return h; }
__global__ static void dup_keys(const uint8_t* headers, const uint8_t* arena, size_t arena_len, size_t n, int mode, uint64_t* keys, uint32_t* vals) {
  size_t i = blockIdx.x * (size_t)blockDim.x + threadIdx.x; if (i >= n) return;
  const uint8_t* h = headers + i * 48; uint16_t flag = (uint16_t)h[8] | ((uint16_t)h[9] << 8); uint8_t mapq = h[10];
  bool keep = mode == 0 ? ((flag & 0x204) == 0 && mapq >= 30) : ((flag & 0x904) == 0 && mapq >= 30);
  vals[i] = (uint32_t)i; if (!keep) { keys[i] = ~0ULL; return; }
  uint64_t offset = rd64(h + 32); uint32_t l = rd32(h + 40);
  if (offset > arena_len || (uint64_t)l > arena_len - offset) { keys[i] = ~0ULL; return; }
  const uint8_t* b = arena + offset;
  // Malformed records are excluded here; the CPU reduction will reject them too.
  if (l < 32) { keys[i] = ~0ULL; return; }
  uint64_t out = 0xcbf29ce484222325ULL;
  uint8_t name = b[8]; uint16_t ncigar = (uint16_t)b[12] | ((uint16_t)b[13] << 8);
  if (mode) {
    int32_t seq_len = (int32_t)rd32(b + 16); size_t at = 32 + (size_t)name + (size_t)ncigar * 4;
    if (seq_len < 0 || at + ((size_t)seq_len + 1) / 2 > l) { keys[i] = ~0ULL; return; }
    out = fnv((out ^ (uint64_t)seq_len), 0); // replaced below to mirror Rust's initial multiply
    out = (0xcbf29ce484222325ULL ^ (uint64_t)seq_len) * 0x100000001b3ULL;
    size_t bytes = ((size_t)seq_len + 1) / 2; for (size_t j = 0; j < bytes; ++j) { uint8_t x = b[at+j]; if (j+1 == bytes && (seq_len & 1)) x &= 0xf0; out = fnv(out, x); }
  } else {
    int32_t pos = (int32_t)rd32(h + 4), tid = (int32_t)rd32(h); out = fnv_i32(fnv_i32(out, tid), pos); int32_t ref = pos;
    size_t at = 32 + (size_t)name; if (at + (size_t)ncigar * 4 > l) { keys[i] = ~0ULL; return; }
    for (uint16_t j = 0; j < ncigar; ++j) { uint32_t c = rd32(b + at + (size_t)j * 4); int32_t len = (int32_t)(c >> 4); uint32_t op = c & 15;
      if (op == 0) { out = fnv_i32(fnv_i32(out, ref), ref + len); ref += len; } else if (op == 2 || op == 3 || op == 4) ref += len;
    }
  }
  keys[i] = out;
}
extern "C" int umgpu_dup_keys(const void* h, const uint8_t* a, size_t alen, size_t n, int mode, uint64_t* k, uint32_t* v, void* s) { dup_keys<<<(n + 255) / 256, 256, 0, (cudaStream_t)s>>>((const uint8_t*)h, a, alen, n, mode, k, v); return result(cudaGetLastError()); }
extern "C" const char* umgpu_error_string(int c) { return cudaGetErrorString((cudaError_t)c); }
#ifdef UMGPU_NVCOMP
extern "C" int umgpu_deflate_alignments(int algorithm, size_t* input, size_t* output, size_t* temp) {
  nvcompBatchedDeflateCompressOpts_t opts = {}; opts.algorithm = (nvcompDeflateAlgorithm_t)algorithm;
  nvcompAlignmentRequirements_t requirements = {};
  nvcompStatus_t status = nvcompBatchedDeflateCompressGetRequiredAlignments(opts, &requirements);
  if (status == nvcompSuccess) { *input = requirements.input; *output = requirements.output; *temp = requirements.temp; }
  return (int)status;
}
extern "C" int umgpu_deflate_temp_size(size_t n, size_t max_chunk, int algorithm, size_t* bytes) {
  nvcompBatchedDeflateCompressOpts_t opts = {}; opts.algorithm = (nvcompDeflateAlgorithm_t)algorithm;
  return (int)nvcompBatchedDeflateCompressGetTempSizeAsync(n, max_chunk, opts, bytes, n * max_chunk);
}
extern "C" int umgpu_deflate_max_output(size_t max_chunk, int algorithm, size_t* bytes) {
  nvcompBatchedDeflateCompressOpts_t opts = {}; opts.algorithm = (nvcompDeflateAlgorithm_t)algorithm;
  return (int)nvcompBatchedDeflateCompressGetMaxOutputChunkSize(max_chunk, opts, bytes);
}
extern "C" int umgpu_deflate_batch(const void* const* in, const size_t* in_bytes, size_t max_chunk,
    size_t n, void* temp, size_t temp_bytes, void* const* out, size_t* out_bytes,
    int algorithm, int* statuses, void* stream) {
  nvcompBatchedDeflateCompressOpts_t opts = {}; opts.algorithm = (nvcompDeflateAlgorithm_t)algorithm;
  return (int)nvcompBatchedDeflateCompressAsync(in, in_bytes, max_chunk, n, temp, temp_bytes,
    out, out_bytes, opts, (nvcompStatus_t*)statuses, (cudaStream_t)stream);
}
extern "C" const char* umgpu_nvcomp_error_string(int code) {
  return nvcompGetStatusString((nvcompStatus_t)code);
}
#endif

// Resident duplicate marking. All bulk storage is supplied by umem, never cudaMalloc.
#include <algorithm>
#include <cstring>
#include <limits>

namespace md {
constexpr uint32_t used = 1u << 16;
struct Work {
  uint64_t *end, *score, *key, *key_out, *ends, *ends_out;
  uint32_t *rank, *flag, *mate, *dup, *ids, *ids_out, *select, *collision;
  Work() = default;
  Work(void* p, size_t n) {
    end = (uint64_t*)p; score = end+n; key = score+n; key_out = key+n;
    ends = key_out+n; ends_out = ends+2*n;
    rank = (uint32_t*)(ends_out+2*n); flag = rank+n; mate = flag+n;
    dup = mate+n; ids = dup+n; ids_out = ids+n; select = ids_out+n;
    collision = select+n;
  }
}; // 96 bytes per record; pair ends have capacity 2*n (including a == b).
__host__ __device__ static uint64_t pack(uint32_t tid, int32_t pos, bool reverse) {
  return ((uint64_t)tid << 33) | ((uint64_t)((uint32_t)pos ^ 0x80000000u) << 1) | reverse;
}
__global__ void derive(const uint8_t* h, const uint8_t* arena, size_t alen,
                       const uint32_t* order, size_t n, Work w, uint32_t* error) {
  size_t i = blockIdx.x*(size_t)blockDim.x+threadIdx.x; if(i>=n) return;
  if(order[i]>=n) { atomicExch(error, 1); return; }
  if(atomicCAS(w.rank+order[i], UINT32_MAX, (uint32_t)i)!=UINT32_MAX) atomicExch(error,1);
  const uint8_t* r=h+48*i; uint32_t f=r[8]|((uint32_t)r[9]<<8);
  w.flag[i]=f; w.mate[i]=(uint32_t)n; w.dup[i]=0; w.collision[i]=0;
  w.select[i]=(f&0x904)==0; w.end[i]=0; w.score[i]=0;
  if(f&0x904) return;
  uint64_t off=rd64(r+32); size_t len=rd32(r+40);
  if(off>alen || len>alen-off || len<32) { atomicExch(error,1); return; }
  const uint8_t* b=arena+off; uint32_t nl=b[8], nc=b[12]|((uint32_t)b[13]<<8);
  int32_t sl=(int32_t)rd32(b+16); size_t ce=32+nl+4*(size_t)nc;
  if(sl<0 || !nl || ce+((size_t)sl+1)/2+(size_t)sl>len || b[31+nl]!=0) { atomicExch(error,1); return; }
  int64_t leading=0, trailing=0, ref=0; bool prefix=true;
  for(uint32_t j=0;j<nc;++j) {
    uint32_t c=rd32(b+32+nl+4*j), op=c&15, l=c>>4;
    if(op>9) { atomicExch(error,1); return; }
    bool clip=op==4||op==5;
    if(prefix&&clip) leading+=l; else prefix=false;
    trailing=clip?trailing+l:0;
    if(op==0||op==2||op==3||op==7||op==8) ref+=l;
  }
  int64_t pos=(int32_t)rd32(r+4); bool reverse=(f&16)!=0;
  pos=reverse?pos+ref-1+trailing:pos-leading;
  int32_t tid=(int32_t)rd32(r);
  // Explicit failure rather than silently folding out-of-domain references/positions.
  if(tid<0||tid>65535||pos<INT32_MIN||pos>INT32_MAX) { atomicExch(error,2); return; }
  w.end[i]=pack((uint32_t)tid,(int32_t)pos,reverse);
  const uint8_t* q=b+ce+((size_t)sl+1)/2; uint64_t score=0;
  for(int32_t j=0;j<sl;++j) if(q[j]>=15) score+=q[j];
  w.score[i]=score;
}
__global__ void name_keys(const uint8_t* h, Work w, size_t n) {
  size_t i=blockIdx.x*(size_t)blockDim.x+threadIdx.x;
  if(i<n) w.key[i]=rd64(h+48*(size_t)w.ids[i]+24);
}
__device__ bool same_name(const uint8_t* h,const uint8_t* arena,uint32_t a,uint32_t b) {
  const uint8_t* x=arena+rd64(h+48*(size_t)a+32);
  const uint8_t* y=arena+rd64(h+48*(size_t)b+32);
  if(x[8]!=y[8]) return false;
  for(uint32_t j=32;j<31u+x[8];++j) if(x[j]!=y[j]) return false;
  return true;
}
__global__ void group(const uint8_t* h,const uint8_t* arena,Work w,size_t m,size_t n) {
  size_t i=blockIdx.x*(size_t)blockDim.x+threadIdx.x;
  if(i>=m||(i&&w.key[i]==w.key[i-1])) return;
  size_t stop=i+1; while(stop<m&&w.key[stop]==w.key[i]) ++stop;
  uint32_t a=(uint32_t)n,b=(uint32_t)n;
  for(size_t j=i;j<stop;++j) {
    uint32_t x=w.ids[j], f=w.flag[x];
    if(!same_name(h,arena,w.ids[i],x)) { w.collision[i]=(uint32_t)(stop-i); return; }
    if(a==n&&(f&64)) a=x;
    if(b==n&&(f&128)) b=x;
  }
  if(a<n&&b<n&&(w.flag[a]&9)==1&&(w.flag[b]&9)==1) {
    w.mate[a]=b; w.flag[a]|=used; if(b!=a) w.flag[b]|=used;
  }
}
__global__ void selection(Work w,size_t n,bool pairs) {
  size_t i=blockIdx.x*(size_t)blockDim.x+threadIdx.x;
  if(i<n) w.select[i]=pairs?w.mate[i]<n:!(w.flag[i]&(0x904|used));
}
// Stable least-significant-field passes: min_rank, !score, max_end, min_end.
// Selection is ascending a, with one b per a, so (a,b) is already sorted.
// Singles: index, rank, !score, end (selection already supplies ascending index).
__global__ void tuple_keys(Work w,size_t m,bool pairs,int field) {
  size_t i=blockIdx.x*(size_t)blockDim.x+threadIdx.x; if(i>=m) return;
  uint32_t a=w.ids[i],b=pairs?w.mate[a]:a; uint64_t x=w.end[a],y=w.end[b];
  uint64_t k=0;
  if(field==0) k=b;
  if(field==1) k=a;
  if(field==2) k=pairs?(w.rank[a]<w.rank[b]?w.rank[a]:w.rank[b]):w.rank[a];
  if(field==3) k=~(w.score[a]+(pairs?w.score[b]:0));
  if(field==4) k=x>y?x:y;
  if(field==5) k=x<y?x:y;
  w.key[i]=k;
}
__global__ void pair_marks(Work w,size_t m) {
  size_t i=blockIdx.x*(size_t)blockDim.x+threadIdx.x; if(i>=m) return;
  uint32_t a=w.ids[i],b=w.mate[a]; uint64_t x=w.end[a],y=w.end[b];
  w.ends[2*i]=x; w.ends[2*i+1]=y;
  if(i) {
    uint32_t pa=w.ids[i-1],pb=w.mate[pa]; uint64_t px=w.end[pa],py=w.end[pb];
    if((x<y?x:y)==(px<py?px:py)&&(x>y?x:y)==(px>py?px:py)) {
      w.dup[a]=1; if(b!=a) w.dup[b]=1;
    }
  }
}
__global__ void single_marks(Work w,size_t m,size_t ne) {
  size_t i=blockIdx.x*(size_t)blockDim.x+threadIdx.x; if(i>=m) return;
  uint32_t a=w.ids[i]; uint64_t end=w.end[a]; size_t lo=0,hi=ne;
  while(lo<hi) { size_t mid=lo+(hi-lo)/2; if(w.ends[mid]<end) lo=mid+1; else hi=mid; }
  if((lo<ne&&w.ends[lo]==end)||(i&&end==w.end[w.ids[i-1]])) w.dup[a]=1;
}
struct Metric {
  Work w; size_t n; int field;
  __host__ __device__ uint64_t operator()(uint32_t i) const {
    uint32_t f=w.flag[i];
    switch(field) {
      case 0:return !(f&(0x904|used));
      case 1:return w.mate[i]<n;
      case 2:return (f&0x900)!=0;
      case 3:return !(f&0x900)&&((f&4)!=0);
      case 4:return !(f&(0x904|used))&&w.dup[i];
      default:return w.mate[i]<n&&w.dup[i];
    }
  }
};
// CCCL 3.x (CUDA 13) dropped cub::{Counting,Transform}InputIterator in favour of thrust's.
using Count=thrust::counting_iterator<uint32_t>;
using MetricInput=thrust::transform_iterator<Metric,Count,uint64_t>;
// Error paths drain the stream before the Rust caller can release a lease.
struct Drain { cudaStream_t s; ~Drain() { cudaStreamSynchronize(s); } };
struct Events {
  cudaEvent_t e[6]{};
  ~Events() { for(auto x:e) if(x) cudaEventDestroy(x); }
};
}
extern "C" int umgpu_markdup_temp_size(size_t n,size_t* out) {
  if(n>INT32_MAX/2) return result(cudaErrorInvalidValue);
  size_t b=0, mx=1; cudaError_t e;
#define MD_SIZE(call) do { b=0; e=(call); if(e!=cudaSuccess) return result(e); mx=std::max(mx,b); } while(0)
  MD_SIZE(cub::DeviceRadixSort::SortPairs(nullptr,b,(uint64_t*)nullptr,(uint64_t*)nullptr,(uint32_t*)nullptr,(uint32_t*)nullptr,n));
  MD_SIZE(cub::DeviceRadixSort::SortKeys(nullptr,b,(uint64_t*)nullptr,(uint64_t*)nullptr,2*n));
  MD_SIZE(cub::DeviceSelect::Flagged(nullptr,b,md::Count(0),(uint32_t*)nullptr,(uint32_t*)nullptr,(uint32_t*)nullptr,n));
  MD_SIZE(cub::DeviceSelect::Unique(nullptr,b,(uint64_t*)nullptr,(uint64_t*)nullptr,(uint32_t*)nullptr,2*n));
  MD_SIZE(cub::DeviceReduce::Sum(nullptr,b,md::MetricInput(md::Count(0),md::Metric{md::Work{},n,0}),(uint64_t*)nullptr,n));
#undef MD_SIZE
  *out=mx; return 0;
}
// control: u64 metrics[6], u32 count, u32 error, f32 milliseconds[5].
extern "C" int umgpu_markdup(const void* headers,const uint8_t* arena,size_t alen,
 const uint32_t* order,size_t n,void* work,void* temp,size_t temp_bytes,
 void* control,void* stream) {
  auto s=(cudaStream_t)stream; md::Drain drain{s}; md::Events events;
  auto h=(const uint8_t*)headers; md::Work w(work,n);
  auto metrics=(uint64_t*)control; auto count=(uint32_t*)(metrics+6); auto error=count+1;
  auto times=(float*)(error+1); size_t b;
#define MD(call) do { cudaError_t status=(call); if(status!=cudaSuccess) return result(status); } while(0)
#define LAUNCH(kernel,number,...) do { if(number) { kernel<<<((number)+255)/256,256,0,s>>>(__VA_ARGS__); MD(cudaGetLastError()); } } while(0)
#define CUB(call) do { b=temp_bytes; MD(call); } while(0)
  for(auto &e:events.e) MD(cudaEventCreate(&e));
  MD(cudaEventRecord(events.e[0],s));
  MD(cudaMemsetAsync(control,0,80,s));
  MD(cudaMemsetAsync(w.rank,0xff,n*sizeof(uint32_t),s));
  LAUNCH(md::derive,n,h,arena,alen,order,n,w,error);
  MD(cudaEventRecord(events.e[1],s));
  MD(cudaStreamSynchronize(s));
  if(*error) return result(cudaErrorInvalidValue);
  CUB(cub::DeviceSelect::Flagged(temp,b,md::Count(0),w.select,w.ids,count,n,s));
  MD(cudaStreamSynchronize(s)); size_t examined=*count;
  LAUNCH(md::name_keys,examined,h,w,examined);
  if(examined) {
    CUB(cub::DeviceRadixSort::SortPairs(temp,b,w.key,w.key_out,w.ids,w.ids_out,examined,0,64,s));
    std::swap(w.key,w.key_out); std::swap(w.ids,w.ids_out);
  }
  LAUNCH(md::group,examined,h,arena,w,examined,n);
  if(examined) {
    CUB(cub::DeviceSelect::Flagged(temp,b,md::Count(0),w.collision,w.ids_out,count,examined,s));
  }
  MD(cudaStreamSynchronize(s));
  size_t collisions=examined?*count:0;
  // Only collided runs are sorted on the host. Names and indices stay in leased UM.
  // Device grouping wrote no state for these runs; mirror the oracle's first-bit rule.
  auto name=[&](uint32_t i) {
    uint64_t off; std::memcpy(&off,h+48*(size_t)i+32,8); return arena+off;
  };
  auto compare=[&](uint32_t a,uint32_t z) {
    auto x=name(a),y=name(z); size_t nx=x[8]-1,ny=y[8]-1;
    int c=std::memcmp(x+32,y+32,std::min(nx,ny));
    return c?c<0:nx!=ny?nx<ny:a<z;
  };
  auto equal=[&](uint32_t a,uint32_t z) {
    auto x=name(a),y=name(z); return x[8]==y[8]&&!std::memcmp(x+32,y+32,x[8]-1);
  };
  for(size_t c=0;c<collisions;++c) {
    size_t i=w.ids_out[c];
    size_t stop=i+w.collision[i]; std::sort(w.ids+i,w.ids+stop,compare);
    for(size_t j=i;j<stop;) {
      size_t k=j+1; while(k<stop&&equal(w.ids[j],w.ids[k])) ++k;
      uint32_t a=(uint32_t)n,z=(uint32_t)n;
      for(size_t t=j;t<k;++t) { uint32_t x=w.ids[t],f=w.flag[x]; if(a==n&&(f&64)) a=x; if(z==n&&(f&128)) z=x; }
      if(a<n&&z<n&&(w.flag[a]&9)==1&&(w.flag[z]&9)==1) { w.mate[a]=z; w.flag[a]|=md::used; w.flag[z]|=md::used; }
      j=k;
    }
  }
  MD(cudaEventRecord(events.e[2],s));
  LAUNCH(md::selection,n,w,n,true);
  CUB(cub::DeviceSelect::Flagged(temp,b,md::Count(0),w.select,w.ids,count,n,s));
  MD(cudaStreamSynchronize(s)); size_t pairs=*count;
  for(int field=2;field<6&&pairs;++field) {
    LAUNCH(md::tuple_keys,pairs,w,pairs,true,field);
    CUB(cub::DeviceRadixSort::SortPairs(temp,b,w.key,w.key_out,w.ids,w.ids_out,pairs,0,field<3?32:field<4?64:49,s));
    std::swap(w.key,w.key_out); std::swap(w.ids,w.ids_out);
  }
  LAUNCH(md::pair_marks,pairs,w,pairs);
  size_t ne=0;
  if(pairs) {
    CUB(cub::DeviceRadixSort::SortKeys(temp,b,w.ends,w.ends_out,2*pairs,0,49,s));
    CUB(cub::DeviceSelect::Unique(temp,b,w.ends_out,w.ends,count,2*pairs,s));
    MD(cudaStreamSynchronize(s)); ne=*count;
  }
  MD(cudaEventRecord(events.e[3],s));
  LAUNCH(md::selection,n,w,n,false);
  CUB(cub::DeviceSelect::Flagged(temp,b,md::Count(0),w.select,w.ids,count,n,s));
  MD(cudaStreamSynchronize(s)); size_t singles=*count;
  for(int field=2;field<6&&singles;++field) {
    if(field==4) continue;
    LAUNCH(md::tuple_keys,singles,w,singles,false,field);
    CUB(cub::DeviceRadixSort::SortPairs(temp,b,w.key,w.key_out,w.ids,w.ids_out,singles,0,field==2?32:field==3?64:49,s));
    std::swap(w.key,w.key_out); std::swap(w.ids,w.ids_out);
  }
  LAUNCH(md::single_marks,singles,w,singles,ne);
  MD(cudaEventRecord(events.e[4],s));
  for(int field=0;field<6;++field) {
    CUB(cub::DeviceReduce::Sum(temp,b,md::MetricInput(md::Count(0),md::Metric{w,n,field}),metrics+field,n,s));
  }
  // Output always occupies the fixed select[] slot, irrespective of radix ping-pong parity.
  CUB(cub::DeviceSelect::Flagged(temp,b,md::Count(0),w.dup,w.select,count,n,s));
  MD(cudaEventRecord(events.e[5],s));
  MD(cudaStreamSynchronize(s));
  for(int i=0;i<5;++i) MD(cudaEventElapsedTime(times+i,events.e[i],events.e[i+1]));
  auto sizes=(uint64_t*)((uint8_t*)control+80);
  sizes[0]=examined; sizes[1]=pairs; sizes[2]=singles; sizes[3]=ne;
#undef CUB
#undef LAUNCH
#undef MD
  return 0;
}
