#!/usr/bin/env python3
"""Run the integrated STAR production gate with nf-core's real argv template.

The JSON template is an argv array and must contain exactly one each of
{STAR}, {R1}, {R2}, and {OUT}; optional {GENOME_DIR} and {GTF} take their
matching CLI paths. {OUT} is the output-prefix directory.  Do not
put shell quoting in it: it is executed as an argv array.
"""
import argparse
import json
import os
import shutil
import subprocess
from pathlib import Path

from measurement_common import (clean_env, executable_identity, fresh_dir,
                                identity, load_base_argv, run_stage, verify_lock)


REQUIRED_OUTPUTS = ("Aligned.out.bam", "Aligned.toTranscriptome.out.bam",
                    "SJ.out.tab", "Log.final.out", "_STARpass1/SJ.out.tab")
TIMING_LOG_FIELDS = frozenset(("Started job on", "Started mapping on", "Finished on",
                               "Mapping speed, Million of reads per hour"))
PLACEHOLDERS = ("{STAR}", "{R1}", "{R2}", "{OUT}", "{GENOME_DIR}", "{GTF}")
REQUIRED_PLACEHOLDERS = PLACEHOLDERS[:4]


def load_template(path):
    template = load_base_argv(path)
    for placeholder in REQUIRED_PLACEHOLDERS:
        if sum(token.count(placeholder) for token in template) != 1:
            raise ValueError("argv template must contain exactly one " + placeholder)
    if any(("{" in token or "}" in token) and token not in PLACEHOLDERS for token in template):
        raise ValueError("argv template contains an unknown placeholder")
    if template.count("--runThreadN") != 1 or template[template.index("--runThreadN") + 1] != "16":
        raise ValueError("production argv must contain exactly --runThreadN 16")
    for option in ("--readFilesIn", "--outFileNamePrefix", "--twopassMode", "--quantMode"):
        if template.count(option) != 1:
            raise ValueError("production argv must contain exactly one " + option)
    required = (("--twopassMode", "Basic"), ("--quantMode", "TranscriptomeSAM"),
                ("--outSAMtype", "BAM", "Unsorted"), ("--readFilesCommand", "zcat"),
                ("--runRNGseed", "0"), ("--outFilterMultimapNmax", "20"),
                ("--alignSJDBoverhangMin", "1"), ("--outSAMstrandField", "intronMotif"),
                ("--quantTranscriptomeSAMoutput", "BanSingleEnd"))
    for values in required:
        position = template.index(values[0])
        if tuple(template[position:position + len(values)]) != values:
            raise ValueError("production argv differs at " + values[0])
    if "--outSAMorder" in template:
        raise ValueError("production argv must retain STAR's default unsorted output order")
    return template


def render_argv(template, executable, mate1, mate2, output, genome_dir=None, sjdb_gtf=None):
    values = {"{STAR}": str(executable), "{R1}": str(mate1), "{R2}": str(mate2),
              "{OUT}": str(output) + "/", "{GENOME_DIR}": str(genome_dir), "{GTF}": str(sjdb_gtf)}
    for placeholder, value in (("{GENOME_DIR}", genome_dir), ("{GTF}", sjdb_gtf)):
        if placeholder in template and value is None:
            raise ValueError(placeholder + " requires its matching CLI path")
    return [next((value for key, value in values.items() if token == key), token)
            for token in template]


def production_environment(sidecar):
    return clean_env({"STAR_INTEGRATE": "1", "STAR_INTEGRATE_STRICT": "1",
                      "STAR_INTEGRATE_SIDECAR": sidecar, "STAR_INTEGRATE_THP": "1"})


def sidecar_stats(path):
    rows = [json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()]
    required_zero = ("batch_faults", "rejected", "shift_mismatch", "flag_mismatch", "step_count_mismatch")
    if len(rows) != 1 or not isinstance(rows[0], dict) or "gpu_consumed" not in rows[0]:
        raise RuntimeError("expected one complete final sidecar row")
    stats = rows[0]
    if (not isinstance(stats["gpu_consumed"], (int, float)) or stats["gpu_consumed"] <= 0 or
            any(not isinstance(stats.get(name), (int, float)) or stats[name] != 0 for name in required_zero)):
        raise RuntimeError("production sidecar accounting rejected: " + json.dumps(stats, sort_keys=True))
    return stats


def require_outputs(root):
    missing = [name for name in REQUIRED_OUTPUTS if not (Path(root) / name).is_file()]
    if missing:
        raise RuntimeError("successful STAR run missing output(s): " + ", ".join(missing))


def normalized_log(path):
    kept = []
    for line in Path(path).read_text().splitlines(keepends=True):
        field = line.split("|", 1)[0].strip()
        if field not in TIMING_LOG_FIELDS:
            kept.append(line)
    return "".join(kept)


def write_normalized_logs(stock, integrated, root):
    root = Path(root)
    root.mkdir(exist_ok=True)
    for label, source in (("stock", stock), ("integrated", integrated)):
        (root / (label + ".Log.final.non-timing.out")).write_text(normalized_log(Path(source) / "Log.final.out"))


def command_ok(command, label, cwd=None, env=None, stdout=None):
    completed = subprocess.run(command, cwd=cwd, env=env, stdout=stdout, stderr=subprocess.PIPE)
    if completed.returncode:
        raise RuntimeError(label + " failed (exit " + str(completed.returncode) + ")")


def compare_cmp(stock, integrated, root):
    root = Path(root)
    write_normalized_logs(stock, integrated, root / "normalized")
    pairs = [(name, Path(stock) / name, Path(integrated) / name) for name in REQUIRED_OUTPUTS if name != "Log.final.out"]
    pairs.append(("Log.final.non-timing.out", root / "normalized/stock.Log.final.non-timing.out",
                  root / "normalized/integrated.Log.final.non-timing.out"))
    for name, left, right in pairs:
        command_ok(["cmp", "-s", str(left), str(right)], "cmp " + name)


def normalize_bam(samtools, bam, destination):
    raw = destination.with_suffix(".sam")
    with raw.open("wb") as stream:
        command_ok([str(samtools), "view", str(bam)], "samtools view " + str(bam), stdout=stream)
    env = dict(os.environ, LC_ALL="C")
    with destination.open("wb") as stream:
        # Full-record sort, not by read name: a multimapper's several records come out in
        # thread-chunk-dependent relative order, so a stable name sort still differs
        # between two stock runs (measured on the transcriptome BAM, stock-twice 2026-09-08).
        command_ok(["sort", "-S", "4G", "--parallel", "8", str(raw)], "sort records " + str(bam), env=env, stdout=stream)


def compare_namesorted_sam(stock, integrated, root, samtools):
    if shutil.which(str(samtools)) is None and not Path(samtools).is_file():
        raise ValueError("samtools is required for namesorted-sam comparison")
    root = Path(root) / "normalized"
    root.mkdir(exist_ok=True)
    print("NORMALIZATION: BAM headers removed with 'samtools view'; remaining SAM records sorted as whole lines (LC_ALL=C sort). Record multiset must be identical; emitted order is not compared.")
    for name in ("Aligned.out.bam", "Aligned.toTranscriptome.out.bam"):
        left, right = root / ("stock." + name + ".namesorted.sam"), root / ("integrated." + name + ".namesorted.sam")
        normalize_bam(samtools, Path(stock) / name, left)
        normalize_bam(samtools, Path(integrated) / name, right)
        command_ok(["cmp", "-s", str(left), str(right)], "namesorted-sam " + name)
    # The non-BAM artifacts retain their byte contract, apart from documented Log timing fields.
    write_normalized_logs(stock, integrated, root)
    for name in ("SJ.out.tab", "_STARpass1/SJ.out.tab"):
        command_ok(["cmp", "-s", str(Path(stock) / name), str(Path(integrated) / name)], "cmp " + name)
    command_ok(["cmp", "-s", str(root / "stock.Log.final.non-timing.out"),
                str(root / "integrated.Log.final.non-timing.out")], "cmp Log.final.non-timing.out")


def compare_outputs(mode, stock, integrated, root, samtools):
    if mode == "cmp":
        compare_cmp(stock, integrated, root)
    elif mode == "namesorted-sam":
        compare_namesorted_sam(stock, integrated, root, samtools)
    else:
        raise ValueError("unknown comparison mode: " + mode)


def check_common(args):
    for path in (args.stock, args.mate1, args.mate2, args.argv_template):
        if not Path(path).is_file():
            raise ValueError("required file missing: " + str(path))
    if args.timeout_s <= 0:
        raise ValueError("--timeout-s must be positive")
    template = load_template(args.argv_template)
    for placeholder, path, predicate in (("{GENOME_DIR}", args.genome_dir, Path.is_dir),
                                         ("{GTF}", args.sjdb_gtf, Path.is_file)):
        if placeholder in template and (path is None or not predicate(Path(path))):
            raise ValueError(placeholder + " requires an existing matching CLI path")
    return template


def record_identity(output, args):
    (output / "identity.json").write_text(json.dumps({
        "stock": executable_identity(args.stock), "integrated": executable_identity(args.integrated),
        "mate1": identity(args.mate1, args.hash_inputs), "mate2": identity(args.mate2, args.hash_inputs),
        "argv_template": str(args.argv_template), "compare": args.compare,
    }, indent=2) + "\n")


def main(args):
    template = check_common(args)
    if not Path(args.integrated).is_file():
        raise ValueError("required file missing: " + str(args.integrated))
    output = fresh_dir(args.output)
    lock = verify_lock(args.lock)
    try:
        record_identity(output, args)
        stock_dir, integrated_dir = output / "stock", output / "integrated"
        stock_dir.mkdir(); integrated_dir.mkdir()
        stock = render_argv(template, args.stock, args.mate1, args.mate2, stock_dir, args.genome_dir, args.sjdb_gtf)
        integrated = render_argv(template, args.integrated, args.mate1, args.mate2, integrated_dir, args.genome_dir, args.sjdb_gtf)
        sidecar = integrated_dir / "integrate-stats.jsonl"
        run_stage(output, "stock", stock, clean_env(), args.timeout_s)
        run_stage(output, "integrated", integrated, production_environment(sidecar), args.timeout_s)
        require_outputs(stock_dir); require_outputs(integrated_dir)
        compare_outputs(args.compare, stock_dir, integrated_dir, output, args.samtools)
        stats = sidecar_stats(sidecar)
        (output / "gate-summary.json").write_text(json.dumps({"status": "PASS_PRODUCTION_GATE",
            "compare": args.compare, "stats": stats}, indent=2) + "\n")
    finally:
        if lock is not None:
            lock.close()


def stock_twice(args):
    template = check_common(args)
    output = fresh_dir(args.output)
    lock = verify_lock(args.lock)
    try:
        (output / "identity.json").write_text(json.dumps({"stock": executable_identity(args.stock),
            "mate1": identity(args.mate1, args.hash_inputs), "mate2": identity(args.mate2, args.hash_inputs),
            "argv_template": str(args.argv_template)}, indent=2) + "\n")
        first, second = output / "stock-1", output / "stock-2"
        first.mkdir(); second.mkdir()
        run_stage(output, "stock-1", render_argv(template, args.stock, args.mate1, args.mate2, first, args.genome_dir, args.sjdb_gtf), clean_env(), args.timeout_s)
        run_stage(output, "stock-2", render_argv(template, args.stock, args.mate1, args.mate2, second, args.genome_dir, args.sjdb_gtf), clean_env(), args.timeout_s)
        require_outputs(first); require_outputs(second)
        compare_cmp(first, second, output)
        (output / "gate-summary.json").write_text(json.dumps({"status": "PASS_STOCK_TWICE_CMP"}, indent=2) + "\n")
    finally:
        if lock is not None:
            lock.close()


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--stock", type=Path, required=True); p.add_argument("--integrated", type=Path)
    p.add_argument("--argv-template", type=Path, required=True); p.add_argument("--mate1", type=Path, required=True); p.add_argument("--mate2", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True); p.add_argument("--timeout-s", type=int, required=True)
    p.add_argument("--compare", choices=("cmp", "namesorted-sam"), default="cmp"); p.add_argument("--samtools", default="samtools")
    p.add_argument("--genome-dir", type=Path); p.add_argument("--sjdb-gtf", type=Path)
    p.add_argument("--stock-twice", action="store_true"); p.add_argument("--hash-inputs", action="store_true")
    p.add_argument("--lock", type=Path, default=Path.home() / ".cache/uni-rnaseq-resource.lock")
    parsed = p.parse_args()
    if parsed.stock_twice:
        stock_twice(parsed)
    else:
        if parsed.integrated is None:
            p.error("--integrated is required unless --stock-twice is used")
        main(parsed)
