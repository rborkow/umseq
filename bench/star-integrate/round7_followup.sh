#!/usr/bin/env bash
# Round 7 follow-ups on the box, one lock, sequential:
#  A. sys-time attribution for the GPU arm: perf cpu-clock, kernel+user stacks, per thread,
#     8M reads (the +43 s sys over the huge-page baseline).
#  B. stock-arm baseline check: does stock get THP? (AnonHugePages from smaps_rollup mid-run),
#     GLIBC_TUNABLES / THP policy recorded.
#  C. madvise vs fadvise split: patched binary with STAR_INTEGRATE unset, index page cache
#     warm vs cold before the run (fadvise(DONTNEED) is applied by the patched binary itself,
#     so "cold" = run it twice and take the second; "warm" = cat the index files first).
set -u
O=$HOME/uni-rnaseq-probe-lab; G=$O/integrate-gate-host22; P=$O/round7-followup
STOCK=$HOME/uni-rnaseq-seed-lab/seed-split-private-v2/baseline/STAR
INTEG=$G/private/integrated/STAR
INDEX=/home/rborkows/uni-rnaseq/data/index/star_full
exec 9>"$HOME/.cache/uni-rnaseq-resource.lock"; flock 9
rm -rf "$P"; mkdir -p "$P"
ARGV=$(python3 - "$G/stages/integrated.argv.json" <<'EOF'
import json,sys
a=json.load(open(sys.argv[1]))
a=a[a.index("--runMode"):]
i=a.index("--outFileNamePrefix"); del a[i:i+2]
if "--readMapNumber" in a:
    i=a.index("--readMapNumber"); del a[i:i+2]
print(" ".join(a))
EOF
)
{
echo "thp_enabled: $(cat /sys/kernel/mm/transparent_hugepage/enabled)"
echo "thp_defrag: $(cat /sys/kernel/mm/transparent_hugepage/defrag)"
echo "GLIBC_TUNABLES: ${GLIBC_TUNABLES:-<unset>}"
echo "glibc: $(ldd --version | head -1)"
} > "$P/env.txt"

run() { # name binary env-string readN extra
  local name=$1 bin=$2 envs=$3 n=$4; mkdir -p "$P/$name"
  env $envs /usr/bin/time -f "%e s wall %U user %S sys %M KiB maxrss" -o "$P/$name/time.txt" "$bin" $ARGV --readMapNumber $n --outFileNamePrefix "$P/$name/" >/dev/null 2>"$P/$name/err" &
  local pid=$!; local sampled=0
  while kill -0 $pid 2>/dev/null; do
    sleep 5
    if [ $sampled -lt 3 ] && grep -q "Started mapping" "$P/$name/Log.out" 2>/dev/null; then
      grep -E "^(Rss|AnonHugePages)" /proc/$pid/smaps_rollup >> "$P/$name/smaps_rollup.txt" 2>/dev/null; sampled=$((sampled+1))
    fi
  done
  wait $pid
  echo "$name: $(cat $P/$name/time.txt) | $(grep AnonHugePages $P/$name/smaps_rollup.txt 2>/dev/null | tail -1)"
}

echo "== B. stock THP check (4M) =="
run stock-thp "$STOCK" "" 4000000

echo "== C. madvise-only vs fadvise effect (bypass arm, 4M) =="
cat $INDEX/Genome $INDEX/SA $INDEX/SAindex > /dev/null   # warm page cache
run bypass-warm "$INTEG" "" 4000000
run bypass-cold "$INTEG" "" 4000000                        # previous run fadvise'd → cold
cat $INDEX/Genome $INDEX/SA $INDEX/SAindex > /dev/null
run stock-warm "$STOCK" "" 4000000

echo "== A. GPU arm sys attribution (8M, perf per-thread) =="
mkdir -p "$P/gpu-perf"; cd "$P/gpu-perf"
STAR_INTEGRATE=1 STAR_INTEGRATE_SIDECAR=$P/gpu-perf/stats.jsonl perf record -g -e cpu-clock -F 499 -o "$P/gpu-perf/perf.data" -- "$INTEG" $ARGV --readMapNumber 8000000 --outFileNamePrefix "$P/gpu-perf/" >/dev/null 2>"$P/gpu-perf/err"
perf script -i "$P/gpu-perf/perf.data" -F tid,ip,sym 2>/dev/null > "$P/gpu-perf/script.txt"
python3 - "$P/gpu-perf/script.txt" <<'EOF'
import sys,collections
KERN=("_raw_spin","smaps","el0_","do_page_fault","folio","page_counter","zap_","mas_walk","lruvec","uncharge","percpu","try_charge","memcg","vm_normal","vma_","__pi_clear","handle_mm","alloc_pages","ret_from","el0t_","do_mem_abort","do_translation","clear_page","mod_","get_page","__do_","free_","unmap_","tlb_","__flush","rmap","__pte","pte_","pmd_","filemap","__handle","copy_page","do_anonymous","do_wp","wp_page","finish_fault","__alloc","__folio","release_pages","lru_","__free","cgroup","mem_cgroup","__count","fixup_","__arm64","kmem","__kmem","mmap_","vm_area","__vma","find_vma","refcount","sys_","__sys","ksys","__se_","el0_svc","invoke_syscall","do_el0","vfs_","seq_","proc_","show_","walk_","__walk","__mmap","__vm_","schedule","__schedule","futex","__futex","do_futex","hrtimer","ktime","__arm64_sys","el1_","__pi_","arch_","cpu_","irq","__irq","gic_","__gic","nvidia","nv_","os_","uvm_","__uvm","rm_","_nv","copy_to_user","copy_from_user","__arch_copy","memset","memcpy","__memcpy","__memset","kfree","kmalloc","__kmalloc","slab","__slab","rcu","__rcu","srcu","mutex","__mutex","rwsem","down_","up_","wake_","__wake","try_to_wake","ttwu","select_task","enqueue","dequeue","put_prev","pick_next","update_","__update","psi_","account_","cgroup_","blk_","__blk","bio_","submit_bio","ext4","__ext4","jbd2","generic_","__generic","iov_","copy_iov","fault_in","__get_user","__put_user","get_user","put_user","__might","might_","lock_","__lock","unlock","spin_","raw_spin","_raw","queued_","native_","default_idle","cpuidle","__cpuidle","arch_cpu_idle","do_idle","cpu_startup","secondary_start","__secondary","start_kernel","rest_init","kernel_init","ret_to_user","work_pending","do_notify","preempt_","__preempt","finish_task","__switch_to","context_switch","__cond_resched","cond_resched","yield","__yield","nanosleep","hrtimer_nanosleep","do_nanosleep","schedule_hrtimeout","__hrtimer","ksoftirqd","run_ksoftirqd","__do_softirq","__softirq","irq_exit","handle_softirqs","tasklet","__tasklet","net_rx","__napi","napi_","process_backlog","ip_","__ip","tcp_","__tcp","sock_","__sock","sk_","__sk","skb_","__skb","dev_","__dev","netif_","__netif")
# classify each sample: kernel vs user by leaf symbol; per-thread sys share; top kernel leaf syms; top user callers of kernel time
tid=None; cur=[]; per_tid=collections.defaultdict(lambda:[0,0]); kleaf=collections.Counter(); kcaller=collections.Counter(); N=0
def is_k(f): return any(k in f for k in KERN) or f.startswith("0x") or f=="[unknown]"
def flush():
    global cur,N
    if tid is None or not cur: cur=[]; return
    N+=1; leaf=cur[0]; k=is_k(leaf)
    per_tid[tid][1 if k else 0]+=1
    if k:
        kleaf[leaf]+=1
        u=[f for f in cur if not is_k(f)]
        kcaller[(u[0] if u else "(none)")]+=1
    cur=[]
for l in open(sys.argv[1],errors="replace"):
    s=l.rstrip("\n")
    if not s.strip(): flush(); continue
    if not s.startswith("\t"): flush(); tid=s.strip(); cur=[]; continue
    parts=s.strip().split(" ",1); cur.append((parts[1] if len(parts)>1 else parts[0]).split("+0x")[0])
flush()
ks=sum(v[1] for v in per_tid.values()); us=sum(v[0] for v in per_tid.values())
print(f"samples {N}: user {us} ({100*us/N:.1f}%) kernel {ks} ({100*ks/N:.1f}%)")
print("--- kernel share by thread (top 6 by kernel samples) ---")
for t,(u,k) in sorted(per_tid.items(), key=lambda x:-x[1][1])[:6]: print(f"  tid {t}: user {u} kernel {k}  ({100*k/max(1,u+k):.0f}% kernel)")
print("--- top kernel leaf symbols ---")
for s_,n in kleaf.most_common(18): print(f"  {100*n/max(1,ks):5.1f}%  {s_[:90]}")
print("--- user-space callers of kernel time ---")
for s_,n in kcaller.most_common(14): print(f"  {100*n/max(1,ks):5.1f}%  {s_[:120]}")
EOF
echo "gpu-perf: $(grep -E 'Started mapping|Finished on' $P/gpu-perf/Log.final.out | awk -F'\t' '{print $2}' | tr '\n' ' ')"
echo FOLLOWUP-DONE
