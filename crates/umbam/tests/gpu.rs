#![cfg(feature = "cuda")]

use std::{env, fs, path::PathBuf};

#[test]
#[ignore = "requires the Spark CUDA host and ~/uni-rnaseq-data/tier0"]
fn duplication_histograms_match_cpu_and_rseqc_goldens() {
    let root = PathBuf::from(env::var("HOME").expect("HOME is set")).join("uni-rnaseq-data/tier0");
    assert!(
        root.join("MANIFEST.tsv").exists(),
        "Tier 0 MANIFEST.tsv is missing"
    );
    let base = env::temp_dir().join("umbam-gpu-duphist-gate");
    let _ = fs::remove_dir_all(&base);
    let cpu = base.join("cpu");
    let gpu = base.join("gpu");
    let bam = root.join("chr22.unsorted.bam");
    let gtf = root.join("chr22.gtf");
    umbam::verify_markdup_gpu(&bam, 4).unwrap();
    umbam::chain_full(&bam, &gtf, &cpu, 4, true, None, "chr22").unwrap();
    umbam::chain_full_with_gpu(&bam, &gtf, &gpu, 4, true, None, "chr22", true).unwrap();
    assert_eq!(
        fs::read(cpu.join("markdup.metrics.txt")).unwrap(),
        fs::read(gpu.join("markdup.metrics.txt")).unwrap()
    );
    assert_eq!(
        fs::read(cpu.join("markdup.bam")).unwrap(),
        fs::read(gpu.join("markdup.bam")).unwrap()
    );
    assert_eq!(umgpu::stats::bytes_copied(), 0);
    for name in ["seq.DupRate.xls", "pos.DupRate.xls"] {
        let actual = fs::read(gpu.join("rseqc").join(name)).unwrap();
        assert_eq!(
            actual,
            fs::read(cpu.join("rseqc").join(name)).unwrap(),
            "CPU/GPU {name}"
        );
        let golden = if name.starts_with("seq") {
            "chr22.seq.DupRate.xls"
        } else {
            "chr22.pos.DupRate.xls"
        };
        assert_eq!(
            actual,
            fs::read(root.join("qc/rseqc").join(golden)).unwrap(),
            "RSeQC {name}"
        );
    }
}
