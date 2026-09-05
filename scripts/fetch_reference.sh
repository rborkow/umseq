#!/usr/bin/env bash
# fetch_reference.sh — GRCh38 primary assembly + GENCODE v49 annotation + transcripts.
# Idempotent; verifies checksums; writes MANIFEST.tsv. Run on the Spark under ~/uni-rnaseq/data/reference.
set -euo pipefail
REL=${GENCODE_RELEASE:-49}
BASE="https://ftp.ebi.ac.uk/pub/databases/gencode/Gencode_human/release_${REL}"
DEST=${1:-$HOME/uni-rnaseq/data/reference}
mkdir -p "$DEST"; cd "$DEST"

files=(
  "GRCh38.primary_assembly.genome.fa.gz"
  "gencode.v${REL}.primary_assembly.annotation.gtf.gz"
  "gencode.v${REL}.transcripts.fa.gz"
)
curl -fsSL "${BASE}/MD5SUMS" -o MD5SUMS

for f in "${files[@]}"; do
  if [[ -f "$f" ]] && grep " $f\$" MD5SUMS | md5sum -c --quiet - 2>/dev/null; then
    echo "ok   $f (cached)"; continue
  fi
  echo "get  $f"
  curl -fL --retry 5 --retry-delay 10 -C - "${BASE}/${f}" -o "$f"
  grep " $f\$" MD5SUMS | md5sum -c --quiet - && echo "ok   $f"
done

# Decompressed working copies (STAR/salmon want plain FASTA/GTF)
for f in "${files[@]}"; do
  out="${f%.gz}"
  [[ -f "$out" ]] || { echo "unzip $f"; gunzip -kc "$f" > "$out.tmp" && mv "$out.tmp" "$out"; }
done

# Salmon decoy list: all genome sequence names
[[ -f decoys.txt ]] || grep '^>' GRCh38.primary_assembly.genome.fa | cut -d' ' -f1 | tr -d '>' > decoys.txt
# Gentrome for salmon: transcripts + genome
[[ -f gentrome.fa ]] || cat "gencode.v${REL}.transcripts.fa" GRCh38.primary_assembly.genome.fa > gentrome.fa

{ echo -e "file\tbytes\tsha256"; for f in *.fa *.gtf decoys.txt; do printf "%s\t%s\t%s\n" "$f" "$(stat -c %s "$f")" "$(sha256sum "$f" | cut -d' ' -f1)"; done; } > MANIFEST.tsv
echo "done: $(du -sh . | cut -f1) in $DEST"; cat MANIFEST.tsv
