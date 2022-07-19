use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use anyhow::anyhow;
use cargo_metadata::Message;
use colored::Colorize;
use humansize::file_size_opts::BINARY;
use humansize::FileSize;

use crate::build::{build_with_flags, handle_metadata_message};
use crate::cli::cli_format_path;
use crate::pgo::env::{find_pgo_env, PgoEnv};
use crate::pgo::llvm_profdata_install_hint;
use crate::workspace::{get_cargo_workspace, get_pgo_directory};

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct PgoOptimizeArgs {
    cargo_args: Vec<String>,
}

pub fn pgo_optimize(args: PgoOptimizeArgs) -> anyhow::Result<()> {
    let config = cargo::Config::default()?;
    let workspace = get_cargo_workspace(&config)?;
    let pgo_dir = get_pgo_directory(&workspace)?;
    let pgo_env = get_pgo_env()?;

    print_pgo_profile_stats(&pgo_dir)?;

    // TODO: parametrize path to profile file
    let target_file = pgo_dir.join("merged.profdata");
    merge_profiles(&pgo_env, &pgo_dir, &target_file)?;

    let flags = format!(
        "-Cprofile-use={} -Cllvm-args=-pgo-warn-missing-function",
        target_file.display()
    );

    let output = build_with_flags(&flags, args.cargo_args)?;

    for message in Message::parse_stream(output.stdout.as_slice()) {
        let message = message?;
        match message {
            Message::CompilerArtifact(artifact) => {
                if artifact.target.kind.into_iter().any(|s| s == "bin") {
                    log::info!(
                        "PGO-optimized binary {} built successfully.",
                        artifact.target.name.blue()
                    );
                }
            }
            Message::BuildFinished(res) => {
                if res.success {
                    println!("{}", "PGO optimized build successfully finished".green());
                } else {
                    println!("{}", "PGO optimized build has failed".red());
                }
            }
            _ => handle_metadata_message(message),
        }
    }

    Ok(())
}

#[derive(Debug, Default)]
struct ProfileStats {
    file_count: u64,
    total_size: u64,
}

/// Check if the directory with profiles is non-empty and prints basic profile statistics.
fn gather_pgo_profile_stats(pgo_dir: &Path) -> anyhow::Result<ProfileStats> {
    let mut stats = ProfileStats::default();

    for entry in pgo_dir.read_dir()? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if metadata.is_file() && entry.path().extension() == Some(OsStr::new("profraw")) {
            log::debug!("Found profile file {}", entry.path().display());
            stats.total_size += metadata.len();
            stats.file_count += 1;
        }
    }

    Ok(stats)
}

fn print_pgo_profile_stats(pgo_dir: &Path) -> anyhow::Result<()> {
    log::debug!("Locating PGO profile files at {}", pgo_dir.display());

    let stats = gather_pgo_profile_stats(pgo_dir)?;
    if stats.file_count == 0 {
        return Err(anyhow!(
            "No profile files were found at {}. Did you execute your instrumented program?",
            cli_format_path(pgo_dir.display())
        ));
    }

    log::info!(
        "Found {} PGO profile {} with total size {} at {}",
        stats.file_count,
        if stats.file_count > 1 {
            "files"
        } else {
            "file"
        },
        stats.total_size.file_size(BINARY).unwrap().yellow(),
        cli_format_path(pgo_dir.display())
    );
    Ok(())
}

fn get_pgo_env() -> anyhow::Result<PgoEnv> {
    let pgo_env =
        find_pgo_env().map_err(|error| anyhow!("{}\n{}", error, llvm_profdata_install_hint()))?;
    log::debug!(
        "Found `llvm-profdata` at {}",
        pgo_env.llvm_profdata.display()
    );
    Ok(pgo_env)
}

fn merge_profiles(pgo_env: &PgoEnv, pgo_dir: &Path, target_profile: &Path) -> anyhow::Result<()> {
    let mut command = Command::new(&pgo_env.llvm_profdata);
    command.args(&[
        "merge",
        "-o",
        &target_profile.display().to_string(),
        &pgo_dir.display().to_string(),
    ]);
    let output = command.output()?;
    if output.status.success() {
        log::info!("Merged PGO profile(s) to {}", target_profile.display());
        Ok(())
    } else {
        return Err(anyhow!(
            "Failed to merge PGO profile(s): {}",
            String::from_utf8_lossy(&output.stderr).red()
        ));
    }
}
