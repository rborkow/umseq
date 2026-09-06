use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "umbam")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the resident BAM chain.
    Chain {
        #[arg(long = "in", visible_alias = "input")]
        input: std::path::PathBuf,
        #[arg(long)]
        gtf: std::path::PathBuf,
        #[arg(long)]
        out_dir: std::path::PathBuf,
        #[arg(long)]
        threads: Option<usize>,
        /// Emit resident QC text outputs under the chain output directory.
        #[arg(long)]
        qc: bool,
        /// Use CUDA for duplicate marking and RSeQC duplication histograms (requires `--features cuda`).
        #[arg(long)]
        gpu: bool,
        /// Compress BGZF with nvCOMP Deflate at this algorithm (0..=5; 4 targets zlib-6
        /// ratio). Off by default: measured 7.5x slower than the CPU on GB10 at equal
        /// ratio (docs/design-phase2b.md). Requires `--gpu`.
        #[arg(long, value_parser = clap::value_parser!(i32).range(0..=5))]
        gpu_deflate_level: Option<i32>,
        /// BED12 gene model for the RSeQC-style outputs (nf-core `gtf2bed` output).
        #[arg(long)]
        bed: Option<std::path::PathBuf>,
        /// Sample name used as the RSeQC output prefix.
        #[arg(long, default_value = "chr22")]
        sample: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Chain {
            input,
            gtf,
            out_dir,
            threads,
            qc,
            gpu,
            gpu_deflate_level,
            bed,
            sample,
        } => umbam::chain_full_with_gpu_deflate(
            &input,
            &gtf,
            &out_dir,
            threads.unwrap_or_else(num_cpus),
            qc,
            bed.as_deref(),
            &sample,
            gpu,
            gpu_deflate_level.filter(|_| gpu),
        ),
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}
