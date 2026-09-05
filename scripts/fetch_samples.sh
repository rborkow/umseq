#!/usr/bin/env bash
# fetch_samples.sh — GEUVADIS FASTQs from ENA per data-manifest.json, with md5 verification,
# then Tier 1 subsamples (5M, 20M pairs) via seqtk with fixed seed. Runs on the Spark.
set -euo pipefail
ROOT=${ROOT:-$HOME/uni-rnaseq}
MAN=${MAN:-$ROOT/data-manifest.json}
DEST=$ROOT/data/samples
mkdir -p "$DEST"; cd "$DEST"
export PATH=$HOME/.local/bin:$PATH
M=$HOME/.local/bin/micromamba
SEQTK="$M run -n rnaseq seqtk"

fetch_run() { # run ftp md5
  local run=$1 ftps=$2 md5s=$3
  IFS=';' read -ra F <<< "$ftps"; IFS=';' read -ra S <<< "$md5s"
  mkdir -p "$run"
  for i in "${!F[@]}"; do
    local url="https://${F[$i]}" f="$run/$(basename "${F[$i]}")"
    if [[ -f "$f" ]] && [[ "$(md5sum "$f" | cut -d' ' -f1)" == "${S[$i]}" ]]; then echo "ok   $f"; continue; fi
    echo "get  $f"
    $M run -n rnaseq aria2c -q -x8 -s8 --file-allocation=none -c -d "$run" -o "$(basename "$f")" "$url"
    [[ "$(md5sum "$f" | cut -d' ' -f1)" == "${S[$i]}" ]] || { echo "MD5 MISMATCH $f"; exit 1; }
    echo "ok   $f"
  done
}

python3 - "$MAN" <<'EOF' > runs.tsv
import json,sys
m=json.load(open(sys.argv[1]))
for tier,rs in (("tier1",[m["tier1"]]),("tier2a",m["tier2a"])):
    for r in rs: print("\t".join([tier,r["run"],r["sample"],r["sex"],r["ftp"],r["md5"]]))
EOF

while IFS=$'\t' read -r tier run sample sex ftp md5; do
  echo "== $tier $run ($sample, $sex)"; fetch_run "$run" "$ftp" "$md5"
done < runs.tsv

# Tier 1 subsamples
T1=$(awk -F'\t' '$1=="tier1"{print $2}' runs.tsv)
for n in 5000000 20000000; do
  tag="${T1}_$((n/1000000))M"; mkdir -p "$tag"
  for r in 1 2; do
    out="$tag/${tag}_${r}.fastq.gz"
    [[ -f "$out" ]] || { echo "subsample $out"; $SEQTK sample -s42 "$T1/${T1}_${r}.fastq.gz" $n | gzip -1 > "$out.tmp" && mv "$out.tmp" "$out"; }
  done
done
{ echo -e "path\tbytes\tmd5"; for f in */*.fastq.gz; do printf "%s\t%s\t%s\n" "$f" "$(stat -c %s "$f")" "$(md5sum "$f" | cut -d' ' -f1)"; done; } > MANIFEST.tsv
echo "done: $(du -sh . | cut -f1)"; cat MANIFEST.tsv
