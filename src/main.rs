use cargo_fdo::pgo::check::pgo_check;
use cargo_fdo::pgo::instrument::{pgo_instrument, PgoInstrumentArgs};
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
    /// Perform PGO (profile-guided optimization) tasks.
    #[clap(subcommand)]
    Pgo(PgoCommand),
}

#[derive(clap::Parser, Debug)]
enum PgoCommand {
    /// Check that your environment is prepared for PGO.
    Check,
    /// Run `cargo build` with instrumentation to prepare for PGO.
    Instrument(PgoInstrumentArgs),
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();

    let Args::Fdo(args) = args;
    match args {
        Subcommand::Pgo(command) => match command {
            PgoCommand::Check => pgo_check(),
            PgoCommand::Instrument(args) => pgo_instrument(args),
        },
    }
}

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    if let Err(error) = run() {
        eprintln!("{:?}", error);
        std::process::exit(1);
    }
}
