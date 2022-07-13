use anyhow::anyhow;
use cargo::core::compiler::CompileMode;
use cargo::core::Verbosity;
use cargo::ops::{CompileFilter, CompileOptions, FilterRule, LibRule};
use cargo::util::interning::InternedString;
use cargo_fdo::build::{get_cargo_workspace, get_pgo_directory};
use cargo_fdo::env::pgo::{find_pgo_env, PgoEnv};
use clap::Parser;
use colored::Colorize;
use env_logger::Env;
use std::fmt::Write;

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
    Check,
    Instrument(PgoInstrumentArgs),
}

#[derive(clap::Parser, Debug)]
struct PgoInstrumentArgs {
    /// Select specific binary that should be instrumented.
    #[clap(long)]
    bin: Option<String>,
}

fn llvm_profdata_install_hint() -> String {
    format!(
        "Try installing `llvm-profdata` using `{}` or build LLVM manually and \
add its `bin` directory to PATH.",
        "rustup component add llvm-tools-preview".blue()
    )
}

/// Check that binaries required for performing PGO can be found.
fn pgo_check() -> anyhow::Result<()> {
    match find_pgo_env() {
        Ok(env) => {
            println!(
                "{}: {} at {}",
                "[llvm-profdata]".bold(),
                "found".green(),
                env.llvm_profdata.display()
            );
            Ok(())
        }
        Err(_) => {
            println!(
                "{}: {}",
                "[llvm-profdata]".bold(),
                "could not be found".red()
            );
            Err(anyhow!("{}", llvm_profdata_install_hint()))
        }
    }
}

fn pgo_instrument(args: PgoInstrumentArgs) -> anyhow::Result<()> {
    let config = cargo::Config::default()?;
    let workspace = get_cargo_workspace(&config)?;
    let pgo_dir = get_pgo_directory(&workspace)?;

    let mut options = CompileOptions::new(&config, CompileMode::Build)?;
    options.target_rustc_args = Some(vec![format!("-Cprofile-generate={}", pgo_dir.display())]);
    options.build_config.requested_profile = InternedString::from("release");
    options.build_config.force_rebuild = true;
    config.shell().set_verbosity(Verbosity::Normal);

    if let Some(binary) = args.bin {
        options.filter = CompileFilter::Only {
            all_targets: false,
            lib: LibRule::Default,
            bins: FilterRule::Just(vec![binary]),
            examples: FilterRule::none(),
            tests: FilterRule::none(),
            benches: FilterRule::none(),
        }
    }

    log::info!("PGO profiles will be stored into {}", pgo_dir.display());

    let compilation = cargo::ops::compile(&workspace, &options)?;
    for binary in compilation.binaries {
        log::info!(
            "PGO-instrumented binary `{}` built successfully. Now run `{}` on your workload.",
            binary.unit.target.name(),
            binary.path.display()
        );
    }

    Ok(())
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
