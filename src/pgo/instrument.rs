use crate::build::{cargo_command_with_flags, handle_metadata_message};
use crate::clear_directory;
use crate::cli::cli_format_path;
use crate::pgo::CargoCommand;
use crate::workspace::CargoContext;
use cargo_metadata::Message;
use colored::Colorize;

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct PgoInstrumentArgs {
    /// Additional arguments that will be passed to the executed `cargo` command.
    cargo_args: Vec<String>,
}

pub fn pgo_instrument_command(
    ctx: CargoContext,
    args: PgoInstrumentArgs,
    command: CargoCommand,
) -> anyhow::Result<()> {
    let pgo_dir = ctx.get_pgo_directory()?;

    log::info!("PGO profile directory will be cleared.");
    clear_directory(&pgo_dir)?;

    log::info!(
        "PGO profiles will be stored into {}.",
        cli_format_path(pgo_dir.display())
    );

    let flags = format!("-Cprofile-generate={}", pgo_dir.display());
    let output = cargo_command_with_flags(command, &flags, args.cargo_args)?;
    log::debug!("Cargo stderr\n {}", String::from_utf8_lossy(&output.stderr));

    for message in Message::parse_stream(output.stdout.as_slice()) {
        let message = message?;
        match message {
            Message::CompilerArtifact(artifact) => {
                if let Some(executable) = artifact.executable {
                    if let CargoCommand::Build = command {
                        log::info!(
                            "PGO-instrumented binary {} built successfully.",
                            artifact.target.name.blue()
                        );
                        log::info!(
                            "Now run {} on your workload.\nFor more precise profiles, run \
it with the following environment variable: {}.",
                            cli_format_path(&executable),
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

    Ok(())
}
