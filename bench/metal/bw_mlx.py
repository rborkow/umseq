"""bw_mlx.py — M4 Pro unified-memory probe via MLX (Metal).
Measures GPU streaming-read bandwidth and random-gather throughput across
working-set sizes, plus CPU-side reference. No copies exist on this platform:
the same buffer is visible to CPU and GPU.
Run: /Users/rborkows/projects/ft/venv/bin/python bw_mlx.py
"""
import time, sys
import numpy as np
import mlx.core as mx

def timeit(fn, reps=5):
    fn(); mx.synchronize()          # warm
    best = 1e30
    for _ in range(reps):
        t = time.perf_counter(); fn(); mx.synchronize(); best = min(best, time.perf_counter() - t)
    return best

print(f"MLX {mx.__version__}  device={mx.default_device()}")
info = mx.device_info()
print({k: info[k] for k in ("architecture", "memory_size", "max_recommended_working_set_size") if k in info})
print()

# --- Streaming read: sum of a large uint32 buffer ---
print(f"{'streaming read (GPU sum)':<36}{'MiB':>8}{'GB/s':>10}{'ms':>10}")
for mb in (256, 1024, 4096):
    a = mx.ones((mb << 18,), dtype=mx.uint32); mx.eval(a)   # 4 bytes each
    s = timeit(lambda: mx.eval(mx.sum(a, stream=mx.gpu)))
    print(f"{'':<36}{mb:>8}{(mb<<20)/s/1e9:>10.1f}{s*1e3:>10.2f}")
    a_np = np.ones(mb << 18, dtype=np.uint32)
    t = time.perf_counter(); a_np.sum(); s_cpu = time.perf_counter() - t
    print(f"{'  (numpy single-thread CPU sum)':<36}{mb:>8}{(mb<<20)/s_cpu/1e9:>10.1f}{s_cpu*1e3:>10.2f}")
    del a, a_np
print()

# --- Random gather: 64M independent lookups into a working set ---
N_LOOK = 1 << 26
print(f"{'random gather (GPU take)':<36}{'WS MiB':>8}{'Mlk/s':>10}{'ms':>10}")
for mb in (16, 64, 256, 1024, 4096, 7168):
    n = mb << 18
    table = mx.random.randint(0, n, (n,), dtype=mx.uint32); mx.eval(table)
    idx = mx.random.randint(0, n, (N_LOOK,), dtype=mx.uint32); mx.eval(idx)
    s = timeit(lambda: mx.eval(mx.take(table, idx, stream=mx.gpu)))
    print(f"{'':<36}{mb:>8}{N_LOOK/s/1e6:>10.0f}{s*1e3:>10.2f}")
    # CPU reference via numpy fancy indexing on same sizes (single-thread)
    tn = np.random.randint(0, n, n, dtype=np.uint32); ix = np.random.randint(0, n, N_LOOK, dtype=np.uint32)
    t = time.perf_counter(); tn[ix]; s_cpu = time.perf_counter() - t
    print(f"{'  (numpy CPU fancy index)':<36}{mb:>8}{N_LOOK/s_cpu/1e6:>10.0f}{s_cpu*1e3:>10.2f}")
    del table, idx, tn, ix
print()

# --- Dependent pointer chase on GPU (latency-bound, like SA binary search) ---
# 64K independent chains x 64 dependent hops via repeated take.
print(f"{'dependent chase (64K chains x 64 hops)':<36}{'WS MiB':>8}{'Mlk/s':>10}{'ns/hop':>10}")
for mb in (16, 256, 1024, 4096):
    n = mb << 18
    perm = np.random.permutation(n).astype(np.uint32)
    nxt = np.empty(n, dtype=np.uint32); nxt[perm] = np.roll(perm, -1)   # single cycle
    table = mx.array(nxt); mx.eval(table)
    p0 = mx.random.randint(0, n, (65536,), dtype=mx.uint32); mx.eval(p0)
    def chain():
        p = p0
        for _ in range(64): p = mx.take(table, p, stream=mx.gpu)
        mx.eval(p)
    s = timeit(chain, reps=3)
    print(f"{'':<36}{mb:>8}{65536*64/s/1e6:>10.0f}{s/64*1e9:>10.0f}")
    del table, nxt, perm
