use crate::build::{cargo_command_with_flags, handle_metadata_message};
use crate::clear_directory;
use crate::cli::cli_format_path;
use crate::pgo::CargoCommand;
use crate::workspace::{get_cargo_workspace, get_pgo_directory};
use cargo_metadata::Message;
use colored::Colorize;

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct PgoInstrumentArgs {
    /// Additional arguments that will be passed to the executed `cargo` command.
    cargo_args: Vec<String>,
}

pub fn pgo_instrument_command(
    args: PgoInstrumentArgs,
    command: CargoCommand,
) -> anyhow::Result<()> {
    let config = cargo::Config::default()?;
    let workspace = get_cargo_workspace(&config)?;
    let pgo_dir = get_pgo_directory(&workspace)?;

    if pgo_dir.exists() {
        log::info!("Profile directory already exists, it will be cleared");
        clear_directory(&pgo_dir)?;
    }

    log::info!("PGO profiles will be stored into {}", pgo_dir.display());

    let flags = format!("-Cprofile-generate={}", pgo_dir.display());
    let output = cargo_command_with_flags(command, &flags, args.cargo_args)?;

    for message in Message::parse_stream(output.stdout.as_slice()) {
        let message = message?;
        match message {
            Message::CompilerArtifact(artifact) => {
                if let Some(executable) = artifact.executable {
                    if let CargoCommand::Build = command {
                        log::info!(
                            "PGO-instrumented binary {} built successfully. Now run {} on your workload",
                            artifact.target.name.blue(),
                            cli_format_path(&executable)
                        );
                    }
                }
            }
            Message::BuildFinished(res) => {
                if res.success {
                    log::info!(
                        "PGO instrumentation build finished {}",
                        "successfully".green()
                    );
                } else {
                    log::error!("PGO instrumentation build has {}", "failed".red());
                }
            }
            _ => handle_metadata_message(message),
        }
    }

    Ok(())
}
