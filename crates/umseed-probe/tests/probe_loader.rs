//! Small authored PROBE layout/CLI fixtures only; never a real-index performance run.
use std::fs;
use umseed_probe::{
    index::probe_load,
    requests::{probe_generate, probe_read},
};
#[test]
fn probe_loader_and_deterministic_requests() {
    use std::io::Write;
    let root = std::env::temp_dir().join(format!("umseed-probe-loader-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    let params = "### GstrandBit 32\nversionGenome 2.7.4a\ngenomeSAindexNbases 1\ngenomeSAsparseD 1\nsjdbOverhang 0\ngenomeFileSizes 32 25\n";
    fs::write(root.join("genomeParameters.txt"), params).unwrap();
    fs::write(root.join("Genome"), [0u8; 32]).unwrap();
    fs::write(root.join("SA"), [0u8; 25]).unwrap();
    let mut sai = Vec::new();
    for n in [1u64, 0, 4] {
        sai.extend(n.to_le_bytes());
    }
    sai.extend([0u8; 21]);
    fs::write(root.join("SAindex"), sai).unwrap();
    for (name, content) in [
        ("chrStart.txt", "0\n32\n"),
        ("chrLength.txt", "32\n"),
        ("chrName.txt", "probe-fixture\n"),
        ("chrNameLength.txt", "probe-fixture\t32\n"),
    ] {
        fs::write(root.join(name), content).unwrap();
    }
    let index = probe_load(&root, true).unwrap();
    assert_eq!(index.config.n_sa, 6);
    assert_eq!(index.sa.len(), 28);
    assert_eq!(index.genome.as_slice()[..200], [5; 200]);
    assert_eq!(index.genome.page_report().huge_bytes, 0);
    let fastq = root.join("probe-fixture.fastq.gz");
    let mut gzip = flate2::write::GzEncoder::new(
        fs::File::create(&fastq).unwrap(),
        flate2::Compression::fast(),
    );
    for i in 0..512 {
        writeln!(gzip,"@PROBE-{i}\nACGTACGTACGTACGTACGTACGTACGTACGTACGTACGT\n+\nIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIIII").unwrap();
    }
    gzip.finish().unwrap();
    let a = root.join("probe-a.bin");
    let b = root.join("probe-b.bin");
    probe_generate(&fastq, &a, &root, 64, 188140).unwrap();
    probe_generate(&fastq, &b, &root, 64, 188140).unwrap();
    assert_eq!(fs::read(&a).unwrap(), fs::read(&b).unwrap());
    let req = probe_read(&a, true).unwrap();
    assert_eq!(req.count, 64);
    let r = req.requests.as_pod_slice::<umgpu::ProbeRequest>();
    assert_eq!((r[0].dir, r[0].length, r[0].high), (1, 40, 5));
    assert_eq!((r[1].dir, r[1].start, r[1].length), (0, 39, 40));
    // Annotation-generated SJ tail stays resident; Basic is not a mapping command.
    fs::write(
        root.join("genomeParameters.txt"),
        params
            .replace("32 25", "37 25")
            .replace("sjdbOverhang 0", "sjdbOverhang 2")
            + "sjdbInsertSave Basic\n",
    )
    .unwrap();
    let mut sj_genome = vec![0u8; 32];
    sj_genome.extend([1, 2, 3, 0, 5]);
    fs::write(root.join("Genome"), sj_genome).unwrap();
    fs::write(root.join("sjdbInfo.txt"), "1 2\n3 10 1 0 0 1\n").unwrap();
    let sj_index = probe_load(&root, true).unwrap();
    assert_eq!(&sj_index.genome.as_slice()[232..237], &[1, 2, 3, 0, 5]);
    fs::write(
        root.join("genomeParameters.txt"),
        params.replace("genomeSAsparseD 1", "genomeSAsparseD 2"),
    )
    .unwrap();
    let err = probe_load(&root, true).err().unwrap().to_string();
    assert!(err.contains("sparse SA"), "{err}");
    fs::remove_dir_all(root).unwrap();
}
