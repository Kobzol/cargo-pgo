use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::anyhow;
use cargo_metadata::camino::Utf8PathBuf;
use cargo_metadata::Message;
use colored::Colorize;

use crate::bolt::bolt_rustflags;
use crate::bolt::env::{find_bolt_env, BoltEnv};
use crate::build::{cargo_command_with_flags, handle_metadata_message};
use crate::cli::cli_format_path;
use crate::pgo::CargoCommand;
use crate::run_command;
use crate::workspace::{get_bolt_directory, get_cargo_workspace};

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct BoltOptimizeArgs {
    cargo_args: Vec<String>,
}

pub fn bolt_optimize(args: BoltOptimizeArgs) -> anyhow::Result<()> {
    let config = cargo::Config::default()?;
    let workspace = get_cargo_workspace(&config)?;
    let bolt_dir = get_bolt_directory(&workspace)?;
    let bolt_env = find_bolt_env()?;

    // TODO: parametrize path to profile file
    // TODO: do not join all profiles together, split them by binary
    let target_file = bolt_dir.join("merged.profdata");
    merge_profiles(&bolt_env, &bolt_dir, &target_file)?;

    let output = cargo_command_with_flags(CargoCommand::Build, bolt_rustflags(), args.cargo_args)?;

    for message in Message::parse_stream(output.stdout.as_slice()) {
        let message = message?;
        match message {
            Message::CompilerArtifact(artifact) => {
                if let Some(ref executable) = artifact.executable {
                    log::info!(
                        "Binary {} built successfully. It will be now optimized with BOLT.",
                        artifact.target.name.blue()
                    );
                    let optimized_path = optimize_binary(&bolt_env, &executable, &target_file)?;
                    log::info!(
                        "Binary {} successfully optimized with BOLT. You can find it at {}.",
                        artifact.target.name.blue(),
                        cli_format_path(&optimized_path.display())
                    );
                }
            }
            Message::BuildFinished(res) => {
                if res.success {
                    log::info!("BOLT optimized build finished {}", "successfully".green());
                } else {
                    log::error!("BOLT optimized build has {}", "failed".red());
                }
            }
            _ => handle_metadata_message(message),
        }
    }

    Ok(())
}

/// Optimizes `binary` with BOLT and returns a path to the optimized binary.
fn optimize_binary(
    bolt_env: &BoltEnv,
    binary: &Utf8PathBuf,
    profile: &PathBuf,
) -> anyhow::Result<PathBuf> {
    log::debug!(
        "Optimizing {} with BOLT profile {}",
        binary.as_str(),
        profile.display()
    );

    let basename = binary
        .as_path()
        .file_stem()
        .expect("Cannot extract executable basename");

    let target_path = binary
        .parent()
        .expect("Cannot get parent of compiled binary")
        .join(format!("{}-bolt-optimized", basename));

    let output = run_command(
        &bolt_env.bolt,
        &[
            binary.as_str(),
            "-data",
            profile.to_str().expect("Could not convert profile path"),
            "-o",
            target_path.as_str(),
            "-reorder-blocks=cache+",
            "-reorder-functions=hfsort",
            "-split-functions",
            "-split-all-cold",
            "-update-debug-sections",
            "-dyno-stats",
        ],
    )?
    .ok()
    .map_err(|error| anyhow!("Cannot optimize binary with BOLT: {}", error))?;

    log::debug!("BOLT stdout\n{}\n\n", output.stdout);
    log::debug!("BOLT stderr\n{}", output.stderr);

    Ok(target_path.into_std_path_buf())
}

fn merge_profiles(
    bolt_env: &BoltEnv,
    bolt_dir: &Path,
    target_profile: &Path,
) -> anyhow::Result<()> {
    let mut command = Command::new(&bolt_env.merge_fdata);
    command.args(&[
        "-o",
        &target_profile.display().to_string(),
        &bolt_dir.display().to_string(),
    ]);
    let output = command.output()?;
    if output.status.success() {
        log::info!("Merged BOLT profile(s) to {}", target_profile.display());
        Ok(())
    } else {
        return Err(anyhow!(
            "Failed to merge BOLT profile(s): {}",
            String::from_utf8_lossy(&output.stderr).red()
        ));
    }
}
