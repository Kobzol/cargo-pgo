use anyhow::anyhow;
use std::path::{Path, PathBuf};

use cargo_metadata::camino::Utf8PathBuf;
use cargo_metadata::{Artifact, Message};
use colored::Colorize;

use crate::bolt::env::{find_bolt_env, BoltEnv};
use crate::bolt::{bolt_pgo_rustflags, get_binary_profile_dir};
use crate::build::{cargo_command_with_flags, handle_metadata_message};
use crate::cli::cli_format_path;
use crate::pgo::CargoCommand;
use crate::workspace::CargoContext;
use crate::{clear_directory, run_command};

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct BoltInstrumentArgs {
    /// Instrument a PGO-optimized binary. To use this, you must already have PGO profiles on disk.
    /// Later also pass the same flag to `cargo pgo bolt optimize`.
    #[clap(long)]
    with_pgo: bool,
    /// Additional arguments that will be passed to `cargo build`.
    cargo_args: Vec<String>,
}

pub fn bolt_instrument(ctx: CargoContext, args: BoltInstrumentArgs) -> anyhow::Result<()> {
    let bolt_dir = ctx.get_bolt_directory()?;

    let bolt_env = find_bolt_env()?;

    log::info!("BOLT profile directory will be cleared.");
    clear_directory(&bolt_dir)?;

    log::info!(
        "BOLT profiles will be stored into {}.",
        cli_format_path(bolt_dir.display())
    );

    let flags = bolt_pgo_rustflags(&ctx, args.with_pgo)?;
    let output = cargo_command_with_flags(CargoCommand::Build, &flags, args.cargo_args)?;

    for message in Message::parse_stream(output.stdout.as_slice()) {
        let message = message?;
        match message {
            Message::CompilerArtifact(artifact) => {
                if let Some(ref executable) = artifact.executable {
                    log::info!(
                        "Binary {} built successfully. It will be now instrumented with BOLT.",
                        artifact.target.name.blue(),
                    );
                    let instrumented_path =
                        instrument_binary(&bolt_env, executable, &bolt_dir, &artifact)?;
                    log::info!(
                        "Binary {} instrumented successfully. Now run {} on your workload.",
                        artifact.target.name.blue(),
                        cli_format_path(&instrumented_path.display())
                    );
                }
            }
            Message::BuildFinished(res) => {
                if res.success {
                    log::info!(
                        "BOLT instrumentation build finished {}.",
                        "successfully".green()
                    );
                } else {
                    log::error!("BOLT instrumentation build has {}.", "failed".red());
                }
            }
            _ => handle_metadata_message(message),
        }
    }

    Ok(())
}

/// Instruments a binary using BOLT.
/// If it succeeds, returns the path to the instrumented binary.
fn instrument_binary(
    bolt_env: &BoltEnv,
    path: &Utf8PathBuf,
    profile_dir: &Path,
    artifact: &Artifact,
) -> anyhow::Result<PathBuf> {
    let basename = path
        .as_path()
        .file_stem()
        .expect("Cannot extract executable basename");

    let target_path = path
        .parent()
        .expect("Cannot get parent of compiled binary")
        .join(format!("{}-bolt-instrumented", basename));

    let profile_dir = get_binary_profile_dir(profile_dir, artifact);
    std::fs::create_dir_all(&profile_dir)?;

    let profile_path = profile_dir.join("profile");

    let output = run_command(
        &bolt_env.bolt,
        &[
            "-instrument",
            path.as_str(),
            "--instrumentation-file-append-pid",
            "--instrumentation-file",
            profile_path
                .to_str()
                .expect("Cannot get BOLT instrumentation file path"),
            "-update-debug-sections",
            "-o",
            target_path.as_str(),
        ],
    )?
    .ok()
    .map_err(|error| anyhow!("Cannot instrument binary with BOLT: {}.", error))?;

    log::debug!("BOLT instrumentation stdout\n{}\n\n", output.stdout);
    log::debug!("BOLT instrumentation stderr\n{}", output.stderr);

    Ok(target_path.into_std_path_buf())
}
