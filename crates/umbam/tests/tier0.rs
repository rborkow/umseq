//! End-to-end compatibility gates for the locally installed Tier 0 fixture.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
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
            let out = env::temp_dir().join("umbam-tier0-test");
            let _ = fs::remove_dir_all(&out);
            umbam::chain(
                &root.join("chr22.unsorted.bam"),
                &root.join("chr22.gtf"),
                &out,
                4,
            )
            .expect("umbam chain must complete");
            out
        })
        .as_path()
}

fn sam(path: &Path) -> Vec<String> {
    let result = Command::new("samtools")
        .args(["view", path.to_str().expect("UTF-8 path")])
        .output()
        .expect("samtools must be installed for Tier 0 tests");
    assert!(result.status.success(), "samtools view failed");
    String::from_utf8(result.stdout)
        .expect("SAM is UTF-8")
        .lines()
        .map(ToOwned::to_owned)
        .collect()
}

/// Canonical order for comparing two coordinate-sorted SAM streams whose tie-breaking at
/// equal (ref, pos) is legitimately implementation-defined: sort by the full record text so
/// two multisets of records compare equal iff they contain the same records.
fn stable_sort(lines: &mut [String]) {
    lines.sort_unstable();
}

/// Asserts two record lists are equal without dumping megabytes of SAM on failure.
fn assert_records_equal(actual: &[String], expected: &[String], what: &str) {
    if actual == expected {
        return;
    }
    let first_diff = actual
        .iter()
        .zip(expected.iter())
        .position(|(a, e)| a != e)
        .unwrap_or_else(|| actual.len().min(expected.len()));
    let show = |v: &[String]| {
        v.get(first_diff)
            .map(|s| s.as_str())
            .unwrap_or("<end>")
            .to_owned()
    };
    panic!(
        "{what}: {} actual vs {} expected records; first difference at index {first_diff}:\n  actual:   {}\n  expected: {}",
        actual.len(),
        expected.len(),
        show(actual),
        show(expected)
    );
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/MANIFEST.tsv"]
fn sorted_bam_gate() {
    let mut actual = sam(&output().join("sorted.bam"));
    let mut expected = fs::read_to_string(fixture().join("chr22.sorted.sam"))
        .unwrap()
        .lines()
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    stable_sort(&mut actual);
    stable_sort(&mut expected);
    assert_records_equal(&actual, &expected, "sorted.bam vs chr22.sorted.sam");
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/MANIFEST.tsv"]
fn flagstat_and_idxstats_gate() {
    let root = fixture();
    assert_eq!(
        fs::read(output().join("flagstat.txt")).unwrap(),
        fs::read(root.join("chr22.flagstat.txt")).unwrap(),
        "flagstat differs"
    );
    assert_eq!(
        fs::read(output().join("idxstats.txt")).unwrap(),
        fs::read(root.join("chr22.idxstats.txt")).unwrap(),
        "idxstats differs"
    );
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/MANIFEST.tsv"]
fn markdup_and_metrics_gate() {
    // Keyed by (qname, flag-without-0x400, ref, pos, cigar): identifies an alignment
    // independently of the duplicate bit, so we can compare the bit itself.
    fn key_and_dup(line: &str) -> (String, bool) {
        let f: Vec<&str> = line.split('\t').collect();
        let flag: u16 = f[1].parse().unwrap();
        let key = format!("{}\t{}\t{}\t{}\t{}", f[0], flag & !0x400, f[2], f[3], f[5]);
        (key, flag & 0x400 != 0)
    }
    // Multiset: identical secondary alignments can share a key, so compare sorted lists.
    let mut actual: Vec<(String, bool)> = sam(&output().join("markdup.bam"))
        .iter()
        .map(|l| key_and_dup(l))
        .collect();
    let expected_text = fs::read_to_string(fixture().join("chr22.markdup.sam")).unwrap();
    let mut expected: Vec<(String, bool)> = expected_text.lines().map(key_and_dup).collect();
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual.len(), expected.len(), "record count differs");
    let mut differing = 0usize;
    let mut example = None;
    for ((ak, ad), (ek, ed)) in actual.iter().zip(&expected) {
        assert_eq!(ak, ek, "record identity differs (not a dup-flag issue)");
        if ad != ed {
            differing += 1;
            example.get_or_insert_with(|| format!("{ek} (expected dup={ed})"));
        }
    }
    assert_eq!(
        differing,
        0,
        "{differing} records have a differing 0x400 flag; e.g. {}",
        example.unwrap_or_default()
    );

    // Picard metrics: compare the named numeric columns of the first data row to 6 dp.
    fn metrics_row(text: &str) -> std::collections::HashMap<String, String> {
        let mut lines = text
            .lines()
            .skip_while(|l| !l.starts_with("## METRICS CLASS"));
        lines.next();
        let header: Vec<&str> = lines.next().unwrap().split('\t').collect();
        let row: Vec<&str> = lines.next().unwrap().split('\t').collect();
        header
            .iter()
            .zip(row)
            .map(|(h, v)| (h.to_string(), v.to_string()))
            .collect()
    }
    let a = metrics_row(&fs::read_to_string(output().join("markdup.metrics.txt")).unwrap());
    let e = metrics_row(&fs::read_to_string(fixture().join("chr22.markdup.metrics.txt")).unwrap());
    for col in [
        "UNPAIRED_READS_EXAMINED",
        "READ_PAIRS_EXAMINED",
        "SECONDARY_OR_SUPPLEMENTARY_RDS",
        "UNMAPPED_READS",
        "UNPAIRED_READ_DUPLICATES",
        "READ_PAIR_DUPLICATES",
        "PERCENT_DUPLICATION",
    ] {
        let av: f64 = a
            .get(col)
            .unwrap_or(&"NaN".into())
            .parse()
            .unwrap_or(f64::NAN);
        let ev: f64 = e[col].parse().unwrap();
        assert!(
            (av - ev).abs() < 5e-7,
            "metric {col}: actual {av} vs expected {ev}"
        );
    }
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/MANIFEST.tsv"]
fn featurecounts_gate() {
    // Compare Geneid -> count, ignoring the "# Program:" comment and the BAM-path header column.
    fn counts(text: &str) -> Vec<(String, String)> {
        text.lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with("Geneid"))
            .map(|l| {
                let f: Vec<&str> = l.split('\t').collect();
                (f[0].to_string(), f[6].to_string())
            })
            .collect()
    }
    let actual = counts(&fs::read_to_string(output().join("featureCounts.txt")).unwrap());
    let expected = counts(&fs::read_to_string(fixture().join("chr22.featureCounts.txt")).unwrap());
    assert_eq!(actual.len(), expected.len(), "gene row count differs");
    let mut differing = 0usize;
    let mut example = None;
    for ((ag, ac), (eg, ec)) in actual.iter().zip(&expected) {
        assert_eq!(ag, eg, "gene order differs");
        if ac != ec {
            differing += 1;
            example.get_or_insert_with(|| format!("{ag}: actual {ac} vs expected {ec}"));
        }
    }
    assert_eq!(
        differing,
        0,
        "{differing} genes differ in count; e.g. {}",
        example.unwrap_or_default()
    );
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/MANIFEST.tsv"]
fn genomecov_gate() {
    let actual: Vec<String> = fs::read_to_string(output().join("genomecov.bg"))
        .unwrap()
        .lines()
        .map(ToOwned::to_owned)
        .collect();
    let expected: Vec<String> = fs::read_to_string(fixture().join("chr22.genomecov.bg"))
        .unwrap()
        .lines()
        .map(ToOwned::to_owned)
        .collect();
    assert_records_equal(&actual, &expected, "genomecov.bg vs chr22.genomecov.bg");
}

#[test]
#[ignore = "requires ~/uni-rnaseq-data/tier0/MANIFEST.tsv"]
fn timing_gate() {
    let timing = fs::read_to_string(output().join("timing.tsv")).unwrap();
    assert!(
        timing
            .lines()
            .any(|line| line.starts_with("peak_rss_bytes\t") && line != "peak_rss_bytes\t0"),
        "timing.tsv must contain a real peak RSS"
    );
}
