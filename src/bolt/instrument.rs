use anyhow::anyhow;
use std::path::{Path, PathBuf};

use cargo_metadata::camino::Utf8PathBuf;
use cargo_metadata::{Artifact, Message};
use colored::Colorize;

use crate::bolt::cli::{BoltArgs, add_bolt_args};
use crate::bolt::env::{BoltEnv, find_bolt_env};
use crate::bolt::{bolt_pgo_rustflags, get_binary_profile_dir};
use crate::build::{
    CargoCommand, cargo_command_with_rustflags, get_artifact_kind, handle_metadata_message,
};
use crate::clear_directory;
use crate::cli::cli_format_path;
use crate::run_command;
use crate::utils::file::create_directory_without_symlinks;
use crate::utils::str::capitalize;
use crate::workspace::CargoContext;

#[derive(clap::Parser, Debug)]
pub struct BoltInstrumentArgs {
    /// Instrument a PGO-optimized binary. To use this, you must already have PGO profiles on disk.
    /// Later also pass the same flag to `cargo pgo bolt optimize`.
    #[clap(long)]
    with_pgo: bool,

    /// Do not remove profiles that were gathered during previous runs.
    #[clap(long, action)]
    keep_profiles: bool,

    /// Override the BOLT profile path.
    #[clap(long)]
    profiles_dir: Option<PathBuf>,

    #[clap(flatten)]
    bolt_args: BoltArgs,
    /// Additional arguments that will be passed to `cargo build`.
    #[arg(last(true))]
    cargo_args: Vec<String>,
}

impl BoltInstrumentArgs {
    pub fn cargo_args(&self) -> &[String] {
        &self.cargo_args
    }

    pub fn profiles_dir(&self) -> &Option<PathBuf> {
        &self.profiles_dir
    }
}

pub fn bolt_instrument(ctx: CargoContext, args: BoltInstrumentArgs) -> anyhow::Result<()> {
    let bolt_dir = ctx.get_bolt_directory()?;
    let bolt_env = find_bolt_env()?;
    let custom_profiles = args.profiles_dir.is_some();
    let keep_profiles = args.keep_profiles;
    let custom_reset = if !keep_profiles && custom_profiles {
        log::info!("BOLT profile directory will be cleared.");
        ctx.reset_bolt_directory(&bolt_dir)?
    } else {
        None
    };
    if !keep_profiles && !custom_profiles {
        log::info!("BOLT profile directory will be cleared.");
        clear_directory(&bolt_dir)?;
    }
    let build_result = (|| {
        let bolt_args = parse_bolt_args(&args.bolt_args)?;

        log::info!(
            "BOLT profiles will be stored into {}.",
            cli_format_path(bolt_dir.display())
        );

        let flags = bolt_pgo_rustflags(&ctx, args.with_pgo)?;
        let mut cargo = cargo_command_with_rustflags(CargoCommand::Build, flags, args.cargo_args)?;
        let mut artifacts = vec![];

        for message in cargo.messages() {
            let message = message?;
            match message {
                Message::CompilerArtifact(artifact) => {
                    if let Some(executable) = artifact.executable.clone() {
                        log::info!(
                            "{} {} built successfully. It will be now instrumented with BOLT.",
                            capitalize(get_artifact_kind(&artifact)).yellow(),
                            artifact.target.name.blue(),
                        );
                        artifacts.push((artifact, executable));
                    }
                }
                Message::BuildFinished(res) if !res.success => {
                    log::error!("BOLT instrumentation build has {}.", "failed".red());
                }
                Message::BuildFinished(_) => {}
                _ => handle_metadata_message(message),
            }
        }
        cargo.check_status()?;
        Ok((bolt_args, artifacts))
    })();
    let (bolt_args, artifacts) = match build_result {
        Ok(result) => result,
        Err(error) => {
            return match custom_reset {
                Some(reset) => reset.finish(Err(error)),
                None => Err(error),
            };
        }
    };

    let result = (|| {
        for (artifact, executable) in artifacts {
            let instrumented_path = instrument_binary(
                &bolt_env,
                bolt_args.as_deref(),
                &executable,
                &bolt_dir,
                &artifact,
                custom_profiles,
            )?;
            log::info!(
                "{} {} instrumented successfully. Now run {} on your workload.",
                capitalize(get_artifact_kind(&artifact)).yellow(),
                artifact.target.name.blue(),
                cli_format_path(instrumented_path.display())
            );
        }

        log::info!(
            "BOLT instrumentation build finished {}.",
            "successfully".green()
        );
        Ok(())
    })();

    match custom_reset {
        Some(reset) => reset.finish(result),
        None => result,
    }
}

/// Instruments a binary using BOLT.
/// If it succeeds, returns the path to the instrumented binary.
fn instrument_binary(
    bolt_env: &BoltEnv,
    bolt_args: Option<&[String]>,
    path: &Utf8PathBuf,
    profile_root: &Path,
    artifact: &Artifact,
    custom_profiles: bool,
) -> anyhow::Result<PathBuf> {
    let basename = path
        .as_path()
        .file_stem()
        .expect("Cannot extract executable basename");

    let target_path = path
        .parent()
        .expect("Cannot get parent of compiled binary")
        .join(format!("{basename}-bolt-instrumented"));

    let profile_dir = get_binary_profile_dir(profile_root, artifact);
    let profile_path = profile_dir.join("profile");

    let mut args = vec![
        "-instrument".to_string(),
        path.to_string(),
        "--instrumentation-file-append-pid".to_string(),
        "--instrumentation-file".to_string(),
        profile_path
            .to_str()
            .expect("Cannot get BOLT instrumentation file path")
            .to_string(),
        "-o".to_string(),
        target_path.as_str().to_string(),
    ];

    match bolt_args {
        Some(bolt_args) => args.extend_from_slice(bolt_args),
        None => args.push("-update-debug-sections".to_string()),
    }

    create_bolt_profile_directory(profile_root, &profile_dir, custom_profiles)?;
    let output = run_command(&bolt_env.bolt, &args)?
        .ok()
        .map_err(|error| anyhow!("Cannot instrument binary with BOLT: {}.", error))?;

    log::debug!("BOLT instrumentation stdout\n{}\n\n", output.stdout);
    log::debug!("BOLT instrumentation stderr\n{}", output.stderr);

    Ok(target_path.into_std_path_buf())
}

fn create_bolt_profile_directory(
    profile_root: &Path,
    profile_dir: &Path,
    custom_profiles: bool,
) -> std::io::Result<()> {
    if custom_profiles {
        return create_directory_without_symlinks(profile_root, profile_dir);
    }

    match std::fs::symlink_metadata(profile_dir) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "BOLT profile path is not a regular directory: {}",
                    profile_dir.display()
                ),
            ))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(profile_dir)
        }
        Err(error) => Err(error),
    }
}

fn parse_bolt_args(args: &BoltArgs) -> anyhow::Result<Option<Vec<String>>> {
    let Some(args) = &args.bolt_args else {
        return Ok(None);
    };
    let mut parsed = Vec::new();
    add_bolt_args(&mut parsed, args)?;
    Ok(Some(parsed))
}
