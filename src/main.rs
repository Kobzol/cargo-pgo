use cargo_pgo::bolt::instrument::{bolt_instrument, BoltInstrumentArgs};
use cargo_pgo::bolt::optimize::{bolt_optimize, BoltOptimizeArgs};
use cargo_pgo::check::environment_info;
use cargo_pgo::clean::clean_artifacts;
use cargo_pgo::pgo::instrument::{pgo_instrument_command, PgoInstrumentArgs};
use cargo_pgo::pgo::optimize::{pgo_optimize, PgoOptimizeArgs};
use cargo_pgo::pgo::CargoCommand;
use clap::Parser;
use env_logger::Env;

#[derive(clap::Parser, Debug)]
#[clap(author, version, about)]
#[clap(bin_name("cargo"))]
enum Args {
    #[clap(subcommand)]
    Pgo(Subcommand),
}

#[derive(clap::Parser, Debug)]
enum Subcommand {
    /// Display information about your environment. Can be used to check whether it is prepared for
    /// PGO and BOLT.
    Info,
    /// Execute `cargo build` to create a PGO-instrumented binary. When executed, the binary will produce
    /// profiles that can be later used in the `optimize` step.
    Build(PgoInstrumentArgs),
    /// Execute `cargo test` to produce PGO profiles from test execution, which can be later used
    /// in the `optimize` step.
    Test(PgoInstrumentArgs),
    /// Execute `cargo run` to produce PGO profiles from binary execution, which can be later used
    /// in the `optimize` step.
    Run(PgoInstrumentArgs),
    /// Build an optimized version of a binary using generated PGO profiles.
    Optimize(PgoOptimizeArgs),
    /// Clean PGO and BOLT artifacts from the disk.
    Clean,
    /// Optimization using BOLT.
    #[clap(subcommand)]
    Bolt(BoltArgs),
}

#[derive(clap::Parser, Debug)]
enum BoltArgs {
    /// Run `cargo build` with instrumentation to prepare for BOLT optimization.
    Build(BoltInstrumentArgs),
    /// Built na optimized version of a binary using generated BOLT profiles.
    Optimize(BoltOptimizeArgs),
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();

    let Args::Pgo(args) = args;
    match args {
        Subcommand::Info => environment_info(),
        Subcommand::Build(args) => pgo_instrument_command(args, CargoCommand::Build),
        Subcommand::Test(args) => pgo_instrument_command(args, CargoCommand::Test),
        Subcommand::Run(args) => pgo_instrument_command(args, CargoCommand::Run),
        Subcommand::Optimize(args) => pgo_optimize(args),
        Subcommand::Bolt(BoltArgs::Build(args)) => bolt_instrument(args),
        Subcommand::Bolt(BoltArgs::Optimize(args)) => bolt_optimize(args),
        Subcommand::Clean => clean_artifacts(),
    }
}

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("cargo_pgo=info")).init();

    if let Err(error) = run() {
        eprintln!("{}", format!("{:?}", error).trim_end_matches('\n'));
        std::process::exit(1);
    }
}
