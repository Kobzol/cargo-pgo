use std::path::PathBuf;

use crate::build::{
    cargo_command_with_rustflags, get_artifact_kind, handle_metadata_message, CargoCommand,
};
use crate::clear_directory;
use crate::cli::cli_format_path;
use crate::workspace::CargoContext;
use cargo_metadata::Message;
use colored::Colorize;

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct PgoInstrumentArgs {
    /// Cargo command that will be used for PGO-instrumented compilation.
    #[clap(value_enum, default_value = "build")]
    command: CargoCommand,

    /// Do not remove profiles that were gathered during previous runs.
    #[clap(long, action)]
    keep_profiles: bool,

    /// Override the PGO profile path.
    #[clap(long)]
    profiles_dir: Option<PathBuf>,

    /// Additional arguments that will be passed to the executed `cargo` command.
    cargo_args: Vec<String>,
}

impl PgoInstrumentArgs {
    pub fn cargo_args(&self) -> &[String] {
        &self.cargo_args
    }

    pub fn profiles_dir(&self) -> &Option<PathBuf> {
        &self.profiles_dir
    }
}

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct PgoInstrumentShortcutArgs {
    /// Do not remove profiles that were gathered during previous runs.
    #[clap(long, action)]
    keep_profiles: bool,

    /// Override the PGO profile path.
    #[clap(long)]
    profiles_dir: Option<PathBuf>,

    /// Additional arguments that will be passed to the executed `cargo` command.
    cargo_args: Vec<String>,
}

impl PgoInstrumentShortcutArgs {
    pub fn cargo_args(&self) -> &[String] {
        &self.cargo_args
    }

    pub fn profiles_dir(&self) -> &Option<PathBuf> {
        &self.profiles_dir
    }
}

impl PgoInstrumentShortcutArgs {
    pub fn into_full_args(self, command: CargoCommand) -> PgoInstrumentArgs {
        let PgoInstrumentShortcutArgs {
            keep_profiles,
            profiles_dir,
            cargo_args,
        } = self;

        PgoInstrumentArgs {
            command,
            keep_profiles,
            profiles_dir,
            cargo_args,
        }
    }
}

pub fn pgo_instrument(ctx: CargoContext, args: PgoInstrumentArgs) -> anyhow::Result<()> {
    let pgo_dir = ctx.get_pgo_directory()?;

    if !args.keep_profiles {
        log::info!("PGO profile directory will be cleared.");
        clear_directory(&pgo_dir)?;
    }

    log::info!(
        "PGO profiles will be stored into {}.",
        cli_format_path(pgo_dir.display())
    );

    let flags = vec![format!("-Cprofile-generate={}", pgo_dir.display())];
    let mut cargo = cargo_command_with_rustflags(args.command, flags, args.cargo_args)?;

    for message in cargo.messages() {
        let message = message?;
        match message {
            Message::CompilerArtifact(artifact) => {
                if let Some(ref executable) = artifact.executable {
                    if let CargoCommand::Build = args.command {
                        log::info!(
                            "PGO-instrumented {} {} built successfully.",
                            get_artifact_kind(&artifact).yellow(),
                            artifact.target.name.blue()
                        );
                        log::info!(
                            "Now run {} on your workload.\nIf your program creates multiple processes \
or you will execute it multiple times in parallel, consider running it \
with the following environment variable to have more precise profiles:\n{}",
                            cli_format_path(executable),
                            format!(
                                "LLVM_PROFILE_FILE={}/{}_%m_%p.profraw",
                                pgo_dir.display(),
                                artifact.target.name
                            )
                            .blue()
                        );
                    }
                }
            }
            Message::BuildFinished(res) => {
                if res.success {
                    log::info!(
                        "PGO instrumentation build finished {}.",
                        "successfully".green()
                    );
                } else {
                    log::error!("PGO instrumentation build has {}.", "failed".red());
                }
            }
            _ => handle_metadata_message(message),
        }
    }

    cargo.check_status()?;

    Ok(())
}
