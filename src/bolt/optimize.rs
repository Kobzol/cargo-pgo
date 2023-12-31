use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::anyhow;
use cargo_metadata::camino::Utf8PathBuf;
use cargo_metadata::Message;
use colored::Colorize;

use crate::bolt::cli::{add_bolt_args, BoltArgs};
use crate::bolt::env::{find_bolt_env, BoltEnv};
use crate::bolt::{bolt_pgo_rustflags, get_binary_profile_dir};
use crate::build::{
    cargo_command_with_flags, get_artifact_kind, handle_metadata_message, CargoCommand,
};
use crate::cli::cli_format_path;
use crate::run_command;
use crate::utils::file::gather_files_with_extension;
use crate::utils::str::capitalize;
use crate::workspace::CargoContext;

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct BoltOptimizeArgs {
    /// Optimize a PGO-optimized binary. To use this, you must already have PGO profiles on disk.
    /// Use this flag only if you have also used it for `cargo pgo bolt build`.
    #[clap(long)]
    with_pgo: bool,
    #[clap(flatten)]
    bolt_args: BoltArgs,
    /// Additional arguments that will be passed to `cargo build`.
    cargo_args: Vec<String>,
}

impl BoltOptimizeArgs {
    pub fn cargo_args(&self) -> &[String] {
        &self.cargo_args
    }
}

pub fn bolt_optimize(ctx: CargoContext, args: BoltOptimizeArgs) -> anyhow::Result<()> {
    let bolt_dir = ctx.get_bolt_directory()?;
    let bolt_env = find_bolt_env()?;

    let flags = bolt_pgo_rustflags(&ctx, args.with_pgo)?;
    let mut cargo = cargo_command_with_flags(CargoCommand::Build, &flags, args.cargo_args)?;

    for message in cargo.messages() {
        let message = message?;
        match message {
            Message::CompilerArtifact(artifact) => {
                if let Some(ref executable) = artifact.executable {
                    log::info!(
                        "{} {} built successfully. It will be now optimized with BOLT.",
                        capitalize(get_artifact_kind(&artifact)).yellow(),
                        artifact.target.name.blue()
                    );

                    let profile_dir = get_binary_profile_dir(&bolt_dir, &artifact);
                    let optimized_path = match merge_profiles(&bolt_env, &profile_dir)? {
                        Some(profile_file) => optimize_binary(
                            &bolt_env,
                            &args.bolt_args,
                            executable,
                            Some(&profile_file),
                        )?,
                        None => {
                            log::warn!(
                                "No profiles found for target {}. \
The optimization will probably not be very effective.",
                                artifact.target.name.blue()
                            );
                            optimize_binary(&bolt_env, &args.bolt_args, executable, None)?
                        }
                    };
                    log::info!(
                        "{} {} successfully optimized with BOLT. You can find it at {}.",
                        capitalize(get_artifact_kind(&artifact)).yellow(),
                        artifact.target.name.blue(),
                        cli_format_path(&optimized_path.display())
                    );
                }
            }
            Message::BuildFinished(res) => {
                if res.success {
                    log::info!("BOLT optimized build finished {}.", "successfully".green());
                } else {
                    log::error!("BOLT optimized build has {}.", "failed".red());
                }
            }
            _ => handle_metadata_message(message),
        }
    }

    cargo.check_status()?;

    Ok(())
}

/// Optimizes `binary` with BOLT and returns a path to the optimized binary.
fn optimize_binary(
    bolt_env: &BoltEnv,
    bolt_args: &BoltArgs,
    binary: &Utf8PathBuf,
    profile: Option<&Path>,
) -> anyhow::Result<PathBuf> {
    match profile {
        Some(profile) => {
            log::debug!(
                "Optimizing {} with BOLT profile {}.",
                binary.as_str(),
                profile.display()
            );
        }
        None => {
            log::debug!("Optimizing {} without a BOLT profile.", binary.as_str());
        }
    }

    let basename = binary
        .as_path()
        .file_stem()
        .expect("Cannot extract executable basename");

    let target_path = binary
        .parent()
        .expect("Cannot get parent of compiled binary")
        .join(format!("{}-bolt-optimized", basename));

    let mut args = vec![binary.to_string()];

    if let Some(profile) = profile {
        args.extend([
            "-data".to_string(),
            profile
                .to_str()
                .expect("Could not convert profile path")
                .to_string(),
        ]);
    }

    args.extend(["-o".to_string(), target_path.to_string()]);

    match bolt_args.bolt_args {
        Some(ref bolt_args) => add_bolt_args(&mut args, bolt_args)?,
        None => {
            args.extend(
                [
                    "-reorder-blocks=ext-tsp",
                    "-reorder-functions=hfsort",
                    "-split-functions=2",
                    "-split-all-cold",
                    "-jump-tables=move",
                    "-use-gnu-stack",
                    "-split-eh",
                    "-lite=1",
                    "-icf=1",
                    "-relocs",
                    "-update-debug-sections",
                    "-dyno-stats",
                ]
                .map(|s| s.to_string()),
            );
        }
    }

    let output = run_command(&bolt_env.bolt, &args)?
        .ok()
        .map_err(|error| anyhow!("Cannot optimize binary with BOLT: {}.", error))?;

    log::debug!("BOLT optimization stdout\n{}\n\n", output.stdout);
    log::debug!("BOLT optimization stderr\n{}", output.stderr);

    Ok(target_path.into_std_path_buf())
}

/// Merges BOLT profiles from `profile_dir` and returns a path to the merged profile file,
/// if it was successfully generated.
fn merge_profiles(bolt_env: &BoltEnv, profile_dir: &Path) -> anyhow::Result<Option<PathBuf>> {
    let mut command = Command::new(&bolt_env.merge_fdata);

    let profile_files = gather_files_with_extension(profile_dir, "fdata");
    if profile_files.is_empty() {
        return Ok(None);
    }

    for file in profile_files {
        command.arg(file);
    }

    let target_profile = profile_dir.join("merged.profdata");
    let output_file = File::create(&target_profile)?;
    let output_stdio = Stdio::from(output_file);
    command.stdout(output_stdio);

    let output = command.output()?;
    if output.status.success() {
        log::info!(
            "Merged BOLT profile(s) to {}.",
            cli_format_path(target_profile.display())
        );
        Ok(Some(target_profile))
    } else {
        Err(anyhow!(
            "Failed to merge BOLT profile(s): {}.",
            String::from_utf8_lossy(&output.stderr).red()
        ))
    }
}
