use cargo_pgo::bolt::instrument::{bolt_instrument, BoltInstrumentArgs};
use cargo_pgo::bolt::optimize::{bolt_optimize, BoltOptimizeArgs};
use cargo_pgo::build::CargoCommand;
use cargo_pgo::check::environment_info;
use cargo_pgo::clean::clean_artifacts;
use cargo_pgo::get_cargo_ctx;
use cargo_pgo::pgo::instrument::{pgo_instrument, PgoInstrumentArgs, PgoInstrumentShortcutArgs};
use cargo_pgo::pgo::optimize::{pgo_optimize, PgoOptimizeArgs};
use clap::Parser;
use env_logger::Env;

#[derive(clap::Parser, Debug)]
#[clap(author, version, about)]
#[clap(bin_name("cargo"))]
#[clap(disable_help_subcommand(true))]
enum Args {
    #[clap(subcommand)]
    #[clap(author, version, about)]
    Pgo(Subcommand),
}

#[derive(clap::Subcommand, Debug)]
#[clap(setting(clap::AppSettings::DeriveDisplayOrder))]
enum Subcommand {
    /// Display information about your environment. Can be used to check whether it is prepared for
    /// PGO and BOLT.
    Info,
    /// Execute a `cargo` command to create PGO-instrumented artifact(s).
    /// After the artifacts are executed, they will produce profiles that can be later used in the
    /// `optimize` step.
    Instrument(PgoInstrumentArgs),
    /// Execute `cargo build` to create a PGO-instrumented binary. When executed, the binary will produce
    /// profiles that can be later used in the `optimize` step.
    Build(PgoInstrumentShortcutArgs),
    /// Execute `cargo test` to produce PGO profiles from test execution, which can be later used
    /// in the `optimize` step.
    Test(PgoInstrumentShortcutArgs),
    /// Execute `cargo run` to produce PGO profiles from binary execution, which can be later used
    /// in the `optimize` step.
    Run(PgoInstrumentShortcutArgs),
    /// Execute `cargo bench` to produce PGO profiles from benchmark execution, which can be later
    /// used in the `optimize` step.
    Bench(PgoInstrumentShortcutArgs),
    /// Build an optimized version of a binary using generated PGO profiles.
    Optimize(PgoOptimizeArgs),
    /// Optimization using BOLT.
    #[clap(subcommand)]
    Bolt(BoltArgs),
    /// Clean PGO and BOLT artifacts from the disk.
    Clean,
}

#[derive(clap::Subcommand, Debug)]
enum BoltArgs {
    /// Run `cargo build` with instrumentation to prepare for BOLT optimization.
    Build(BoltInstrumentArgs),
    /// Built an optimized version of a binary using generated BOLT profiles.
    Optimize(BoltOptimizeArgs),
}

impl BoltArgs {
    fn cargo_args(&self) -> &[String] {
        match self {
            BoltArgs::Build(args) => args.cargo_args(),
            BoltArgs::Optimize(args) => args.cargo_args(),
        }
    }
}

impl Args {
    fn cargo_args(&self) -> &[String] {
        match self {
            Args::Pgo(args) => match args {
                Subcommand::Info => &[],
                Subcommand::Instrument(args) => args.cargo_args(),
                Subcommand::Build(args)
                | Subcommand::Run(args)
                | Subcommand::Test(args)
                | Subcommand::Bench(args) => args.cargo_args(),
                Subcommand::Optimize(args) => args.cargo_args(),
                Subcommand::Bolt(args) => args.cargo_args(),
                Subcommand::Clean => &[],
            },
        }
    }
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();

    let cargo_args = args.cargo_args();
    let ctx = get_cargo_ctx(cargo_args)?;

    let Args::Pgo(args) = args;
    match args {
        Subcommand::Info => environment_info(),
        Subcommand::Instrument(args) => pgo_instrument(ctx, args),
        Subcommand::Build(args) => pgo_instrument(ctx, args.into_full_args(CargoCommand::Build)),
        Subcommand::Test(args) => pgo_instrument(ctx, args.into_full_args(CargoCommand::Test)),
        Subcommand::Run(args) => pgo_instrument(ctx, args.into_full_args(CargoCommand::Run)),
        Subcommand::Bench(args) => pgo_instrument(ctx, args.into_full_args(CargoCommand::Bench)),
        Subcommand::Optimize(args) => pgo_optimize(ctx, args),
        Subcommand::Bolt(BoltArgs::Build(args)) => bolt_instrument(ctx, args),
        Subcommand::Bolt(BoltArgs::Optimize(args)) => bolt_optimize(ctx, args),
        Subcommand::Clean => clean_artifacts(ctx),
    }
}

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("cargo_pgo=info")).init();

    if let Err(error) = run() {
        eprintln!("{}", format!("{:?}", error).trim_end_matches('\n'));
        std::process::exit(1);
    }
}
