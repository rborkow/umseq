//! Compatibility gates for deterministic resident QC outputs.

use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

fn fixture() -> PathBuf {
    PathBuf::from(env::var("HOME").expect("HOME is set")).join("uni-rnaseq-data/tier0")
}

fn output() -> &'static Path {
    static OUTPUT: OnceLock<PathBuf> = OnceLock::new();
    OUTPUT
        .get_or_init(|| {
            let root = fixture();
            assert!(
                root.join("MANIFEST.tsv").exists(),
                "Tier 0 MANIFEST.tsv is missing"
            );
            let out = env::temp_dir().join("umbam-tier0-qc-test");
            let _ = fs::remove_dir_all(&out);
            umbam::chain_with_qc(
                &root.join("chr22.unsorted.bam"),
                &root.join("chr22.gtf"),
                &out,
                4,
                true,
            )
            .expect("umbam QC chain must complete");
            out
        })
        .as_path()
}

fn assert_bytes(actual: &Path, expected: &Path, what: &str) {
    let actual = fs::read(actual).unwrap();
    let expected = fs::read(expected).unwrap();
    if actual != expected {
        let at = actual
            .iter()
            .zip(&expected)
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| actual.len().min(expected.len()));
        panic!(
            "{what}: {} actual vs {} expected bytes; first difference at byte {at}",
            actual.len(),
            expected.len()
        );
    }
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/qc"]
fn rseqc_bam_stat_gate() {
    let root = fixture();
    assert_bytes(
        &output().join("rseqc/bam_stat.txt"),
        &root.join("qc/rseqc/bam_stat.txt"),
        "RSeQC bam_stat",
    );
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/qc"]
fn rseqc_sequence_duplication_gate() {
    let root = fixture();
    assert_bytes(
        &output().join("rseqc/seq.DupRate.xls"),
        &root.join("qc/rseqc/chr22.seq.DupRate.xls"),
        "RSeQC sequence duplication",
    );
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/qc"]
fn rseqc_position_duplication_gate() {
    let root = fixture();
    assert_bytes(
        &output().join("rseqc/pos.DupRate.xls"),
        &root.join("qc/rseqc/chr22.pos.DupRate.xls"),
        "RSeQC position duplication",
    );
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/qc"]
fn rseqc_read_distribution_gate() {
    let root = fixture();
    assert_bytes(
        &output().join("rseqc/read_distribution.txt"),
        &root.join("qc/rseqc/read_distribution.txt"),
        "RSeQC read distribution",
    );
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/qc"]
fn rseqc_junction_annotation_log_gate() {
    let root = fixture();
    assert_bytes(
        &output().join("rseqc/chr22.junction_annotation.log"),
        &root.join("qc/rseqc/chr22.junction_annotation.log"),
        "RSeQC junction annotation log",
    );
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/qc"]
fn rseqc_infer_experiment_gate() {
    let root = fixture();
    assert_bytes(
        &output().join("rseqc/infer_experiment.txt"),
        &root.join("qc/rseqc/infer_experiment.txt"),
        "RSeQC infer experiment",
    );
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/qc"]
fn rseqc_junction_saturation_100_percent_gate() {
    let actual =
        fs::read_to_string(output().join("rseqc/chr22.junctionSaturation_plot.r")).unwrap();
    let expected =
        fs::read_to_string(fixture().join("qc/rseqc/chr22.junctionSaturation_plot.r")).unwrap();
    let last = |text: &str, name: &str| {
        text.lines()
            .find(|line| line.starts_with(&format!("{name}=c(")))
            .and_then(|line| line.strip_suffix(')'))
            .and_then(|line| line.split(',').next_back())
            .unwrap()
            .to_owned()
    };
    for name in ["y", "z", "w"] {
        assert_eq!(
            last(&actual, name),
            last(&expected, name),
            "{name} 100% total"
        );
    }
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/qc"]
fn rseqc_inner_distance_gate() {
    let root = fixture();
    assert_bytes(
        &output().join("rseqc/chr22.inner_distance_freq.txt"),
        &root.join("qc/rseqc/chr22.inner_distance_freq.txt"),
        "RSeQC mRNA inner distance",
    );
}
