use cargo_fdo::check::environment_check;
use cargo_fdo::pgo::instrument::{pgo_instrument, PgoInstrumentArgs};
use cargo_fdo::pgo::optimize::{pgo_optimize, PgoOptimizeArgs};
use clap::Parser;
use env_logger::Env;

#[derive(clap::Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(bin_name("cargo"))]
enum Args {
    #[clap(subcommand)]
    Fdo(Subcommand),
}

#[derive(clap::Parser, Debug)]
enum Subcommand {
    /// Check that your environment is prepared for PGO and BOLT.
    Check,
    /// Run `cargo build` with instrumentation to prepare for PGO.
    Instrument(PgoInstrumentArgs),
    /// Build an optimized version of a binary using generated PGO profiles.
    Optimize(PgoOptimizeArgs),
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();

    let Args::Fdo(args) = args;
    match args {
        Subcommand::Check => environment_check(),
        Subcommand::Instrument(args) => pgo_instrument(args),
        Subcommand::Optimize(args) => pgo_optimize(args),
    }
}

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("cargo_fdo=info")).init();

    if let Err(error) = run() {
        eprintln!("{}", format!("{:?}", error).trim_end_matches('\n'));
        std::process::exit(1);
    }
}
