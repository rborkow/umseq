# Storage tiers

Three tiers. Data moves **down** on explicit promotion after a run closes; it comes back
**up** only via `tier-restore`, onto local NVMe.

| Tier | Where | Spark path | Mac path | For |
|---|---|---|---|---|
| T0 hot | local NVMe | `~/uni-rnaseq/{data,runs}`, `~/uni-rnaseq-*-lab` | `~/uni-rnaseq-data/tier0`, `~/projects/uni-rnaseq` | every run, every timing |
| T1 warm | NFS `llm_work` (TrueNAS, 19 TB) | `/mnt/llm_work` | `~/llm_work` | canonical data, closed evidence, goldens, cross-host handoff |
| T2 cold | rustfs S3 (`aws --profile rustfs`, `rclone rustfs:`) | — | — | immutable archives, idle models, private eval material |

Link reality (2026-09): Spark→T1 ≈ 900 MB/s; Mac→T1 ≈ 17 MB/s on Wi-Fi. **Bulk moves originate on the Spark.**

## T1 layout — `llm_work/`
```
umseq/data/{reference,index,samples}/     canonical; <name>.MANIFEST.sha256 beside each
umseq/goldens/<run>/                      nf-core results/ + trace/report/env/config; never work/
umseq/evidence/<probe-lab dir>/           closed evidence; MANIFEST.sha256 inside; seed-lab/ for seed-lab sets
umseq/scratch/                            cross-host handoff; purged after 30 d
biocoder/{eval-staging,private-staging}/  working private material (results go to S3)
_trash/                                   tier-promote parks originals here; purged after 14 d
```

## T2 layout — buckets
```
s3://rnaseq/evidence/<name>.tar.zst + <name>.sha256     closed evidence (immutable, never overwritten)
s3://rnaseq/goldens/…                                    frozen golden sets
s3://llm-models/hf/models--<org>--<name>/                HF cache dirs not in active service
s3://llm-models/lora/<project>/<run>/                    adapters + training config
s3://artifacts/biocoder/private-results/<run>/           private holdout results — never git
s3://artifacts/biocoder/git-bundles/                     `git bundle --all` snapshots
s3://artifacts/docker/<image>-<tag>.tar.zst              saved images
```

## Tools — `scripts/storage/` (installed to `~/bin` on both hosts)

| Command | Does |
|---|---|
| `tier-promote <dir> <t1-subpath>` | manifest → rsync to T1 → verify → original to `_trash` → **symlink left at `<dir>`** |
| `tier-archive <t1-dir> <bucket/prefix>` | `tar --zstd` → S3 + `.sha256`; refuses to overwrite; appends URI to `ARCHIVED.txt` |
| `tier-restore <bucket/prefix/name> [parent]` | pull to `~/restore/` (NVMe), verify manifest |
| `tier-df` | all three tiers, exit 1 on budget breach |
| `test.sh [--s3]` | self-test against a temp dir |

Because promotion leaves a symlink, paths cited in `bench/*.md` (`~/uni-rnaseq-probe-lab/<dir>`)
keep resolving on the Spark. **Never rewrite a citation to point at T1.**

## Rules

1. **Runs write to NVMe; timings are measured on NVMe.** Never point a benchmark's input, index or
   output at `llm_work` or S3.
2. **Promote when the bench doc lands.** The commit adding `bench/*.md` that cites an evidence dir is the
   trigger to `tier-promote` it the same day.
3. **Uncited evidence lives 14 days.** Not referenced from `bench/`, `docs/` or a card → `_trash`.
   Test: `grep -rl <name> bench docs .hermes/cards`.
4. **Archive at milestones.** When a lane closes, `tier-archive` every evidence set it cites; append the
   S3 URI to the bench doc. Archives are immutable — a re-run gets a new dated name.
5. **Nextflow `work/` is scratch.** Delete once `results/` is verified. Keep `results/ trace.txt
   report.html timeline.html env.txt *.config samplesheet.csv`.
6. **Reference data: one canonical copy on T1.** Spark keeps a hot mirror. Mac keeps `tier0` + the 5M/20M
   subsets only. Rebuilt indexes get a new dated dir + manifest; old ones are archived, not overwritten.
   FASTQs are re-fetchable from ENA (`data-manifest.json` has md5s) and are never archived.
7. **HF cache holds only models in service.** Others → `s3://llm-models/hf/`, back via `rclone copy`.
8. **Private eval material** stays in `s3://artifacts/biocoder/`; not in git, not on NFS past the cycle.
9. **Docker:** `docker system prune` monthly; an image >20 GB unused 30 d is `docker save`d to S3 and removed.
10. **Worktrees are per-card.** Card closes → worktree goes. `~/.hermes/worktrees` >10 GB is a bug.
11. **No direct deletes on T1, no `rsync --delete` into a shared T1 path.** Everything goes via `_trash`.
12. **Budgets** (`tier-df` weekly): Mac ≥100 GB free, Spark ≥500 GB free, Mac `~/uni-rnaseq-data` ≤10 GB.
13. **Naming:** `<name>-YYYYMMDD`; `MANIFEST.sha256` at the root of anything promoted or archived.
14. **Workers:** a promoted dir is a symlink — follow it, don't `rsync -a` it back to NVMe. If a re-measure
    needs it local, `tier-restore` or copy explicitly to a *new* dated dir.

## Pitfalls
- The share squashes ownership to uid/gid 3000: use `rsync -rltD --no-perms --chmod=ugo=rwX`, not `-a`.
- rclone against rustfs needs `no_check_bucket = true` (the key can't `CreateBucket`).
- Mac: `export COPYFILE_DISABLE=1` before tar/rsync; `dot_clean -m ~/llm_work` clears `._*` litter.
- Mac: deleting large trees frees nothing until local Time Machine snapshots are gone:
  `tmutil listlocalsnapshots / ; tmutil deletelocalsnapshots <date>`.
- Spark: take `~/.cache/uni-rnaseq-resource.lock` (`flock`) for bulk moves — they saturate NVMe.
