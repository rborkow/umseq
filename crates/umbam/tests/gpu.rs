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

#[cfg(feature = "nvcomp")]
#[test]
#[ignore = "requires the Spark CUDA host, nvCOMP, samtools, and ~/uni-rnaseq-data/tier0"]
fn nvcomp_bgzf_matches_cpu_records_index_and_raw_blocks() {
    use flate2::read::DeflateDecoder;
    use std::{io::Read, process::Command};

    let root = PathBuf::from(env::var("HOME").expect("HOME is set")).join("uni-rnaseq-data/tier0");
    let base = env::temp_dir().join("umbam-nvcomp-gate");
    let _ = fs::remove_dir_all(&base);
    let cpu = base.join("cpu");
    let gpu = base.join("gpu");
    let bam = root.join("chr22.unsorted.bam");
    let gtf = root.join("chr22.gtf");
    umbam::chain_full(&bam, &gtf, &cpu, 4, false, None, "chr22").unwrap();
    umbam::chain_full_with_gpu_deflate(&bam, &gtf, &gpu, 4, false, None, "chr22", true, Some(4))
        .unwrap();
    for name in ["sorted.bam", "markdup.bam"] {
        let cpu_bam = cpu.join(name);
        let gpu_bam = gpu.join(name);
        assert_eq!(
            samtools_view(&cpu_bam, None),
            samtools_view(&gpu_bam, None),
            "records: {name}"
        );
        assert_eq!(
            samtools_view(&cpu_bam, Some("chr22:1-1000000000")),
            samtools_view(&gpu_bam, Some("chr22:1-1000000000")),
            "BAI region: {name}"
        );
        assert_eq!(
            inflate_bgzf_blocks(&cpu_bam),
            inflate_bgzf_blocks(&gpu_bam),
            "raw chunks: {name}"
        );
    }

    fn samtools_view(path: &std::path::Path, region: Option<&str>) -> Vec<u8> {
        let mut command = Command::new("samtools");
        command.arg("view").arg(path);
        if let Some(region) = region {
            command.arg(region);
        }
        let output = command.output().expect("run samtools view");
        assert!(
            output.status.success(),
            "samtools view failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
    fn inflate_bgzf_blocks(path: &std::path::Path) -> Vec<Vec<u8>> {
        let bytes = fs::read(path).unwrap();
        let mut at = 0;
        let mut blocks = Vec::new();
        while at < bytes.len() {
            assert!(at + 18 <= bytes.len() && bytes[at..at + 4] == [0x1f, 0x8b, 8, 4]);
            let size = u16::from_le_bytes([bytes[at + 16], bytes[at + 17]]) as usize + 1;
            let mut raw = Vec::new();
            DeflateDecoder::new(&bytes[at + 18..at + size - 8])
                .read_to_end(&mut raw)
                .unwrap();
            if !raw.is_empty() {
                blocks.push(raw);
            }
            at += size;
        }
        blocks
    }
}
