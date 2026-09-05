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
        } => umbam::chain_with_qc(&input, &gtf, &out_dir, threads.unwrap_or_else(num_cpus), qc),
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}
