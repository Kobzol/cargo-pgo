use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::anyhow;
use cargo_metadata::diagnostic::DiagnosticLevel;
use cargo_metadata::{CompilerMessage, Message};
use colored::Colorize;
use humansize::file_size_opts::BINARY;
use humansize::FileSize;
use once_cell::sync::OnceCell;
use regex::Regex;
use rustc_demangle::{demangle, Demangle};

use crate::build::{
    cargo_command_with_rustflags, get_artifact_kind, handle_metadata_message, CargoCommand,
};
use crate::cli::cli_format_path;
use crate::pgo::env::{find_pgo_env, PgoEnv};
use crate::pgo::llvm_profdata_install_hint;
use crate::utils::file::{gather_files_with_extension, hash_file, move_file};
use crate::utils::str::pluralize;
use crate::workspace::CargoContext;

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct PgoOptimizeArgs {
    /// Cargo command that will be used for PGO-optimized compilation.
    #[clap(value_enum, default_value = "build")]
    command: CargoCommand,
    cargo_args: Vec<String>,
}

impl PgoOptimizeArgs {
    pub fn cargo_args(&self) -> &[String] {
        &self.cargo_args
    }
}

/// Merges PGO profiles and creates RUSTFLAGS that use them.
pub fn prepare_pgo_optimization_flags(
    pgo_env: &PgoEnv,
    pgo_dir: &Path,
) -> anyhow::Result<Vec<String>> {
    let stats = gather_pgo_profile_stats(pgo_dir)?;

    print_pgo_profile_stats(&stats, pgo_dir)?;

    let target_file = merge_profiles(pgo_env, &stats, pgo_dir)?;

    Ok(vec![
        format!("-Cprofile-use={}", target_file.display()),
        "-Cllvm-args=-pgo-warn-missing-function".to_string(),
    ])
}

pub fn pgo_optimize(ctx: CargoContext, args: PgoOptimizeArgs) -> anyhow::Result<()> {
    let pgo_dir = ctx.get_pgo_directory()?;
    let pgo_env = get_pgo_env()?;

    let flags = prepare_pgo_optimization_flags(&pgo_env, &pgo_dir)?;

    let mut cargo = cargo_command_with_rustflags(args.command, flags, args.cargo_args)?;

    let mut counter = MissingProfileCounter::default();
    for message in cargo.messages() {
        let message = message?;
        match message {
            Message::CompilerArtifact(artifact) => {
                if let CargoCommand::Build = args.command {
                    if artifact.executable.is_some() {
                        log::info!(
                            "PGO-optimized {} {} built successfully.",
                            get_artifact_kind(&artifact).yellow(),
                            artifact.target.name.blue()
                        );
                    }
                }
            }
            Message::BuildFinished(res) => {
                if res.success {
                    println!("{}", "PGO optimized build successfully finished.".green());
                } else {
                    println!("{}", "PGO optimized build has failed.".red());
                }
            }
            Message::CompilerMessage(msg) => {
                if let Some(profile) = get_pgo_missing_profile(&msg) {
                    log::debug!(
                        "Missing profile data: {}/{}.",
                        profile.module,
                        profile.function
                    );
                    counter.handle_missing_profile(profile);
                } else {
                    handle_metadata_message(Message::CompilerMessage(msg));
                }
            }
            _ => handle_metadata_message(message),
        }
    }

    cargo.check_status()?;

    if counter.counter > 0 {
        log::warn!(
            "PGO profile data was not found for {} {}.",
            counter.counter,
            pluralize("function", counter.counter)
        );
    }

    Ok(())
}

#[derive(Debug, Default)]
struct ProfileStats {
    profile_files: Vec<PathBuf>,
    total_size: u64,
}

impl ProfileStats {
    fn file_count(&self) -> usize {
        self.profile_files.len()
    }
}

/// Check if the directory with profiles is non-empty and prints basic profile statistics.
fn gather_pgo_profile_stats(pgo_dir: &Path) -> anyhow::Result<ProfileStats> {
    let mut stats = ProfileStats::default();

    for file in gather_files_with_extension(pgo_dir, "profraw") {
        log::debug!("Found profile file {}.", file.display());
        stats.total_size += std::fs::metadata(&file)?.len();
        stats.profile_files.push(file);
    }

    Ok(stats)
}

fn print_pgo_profile_stats(stats: &ProfileStats, pgo_dir: &Path) -> anyhow::Result<()> {
    if stats.file_count() == 0 {
        return Err(anyhow!(
            "No profile files were found at {}. Did you execute your instrumented program?",
            cli_format_path(pgo_dir.display())
        ));
    }

    log::info!(
        "Found {} PGO profile {} with total size {} at {}.",
        stats.file_count(),
        if stats.file_count() > 1 {
            "files"
        } else {
            "file"
        },
        stats.total_size.file_size(BINARY).unwrap().yellow(),
        cli_format_path(pgo_dir.display())
    );
    Ok(())
}

pub fn get_pgo_env() -> anyhow::Result<PgoEnv> {
    let pgo_env =
        find_pgo_env().map_err(|error| anyhow!("{}\n{}", error, llvm_profdata_install_hint()))?;
    log::debug!(
        "Found `llvm-profdata` at {}.",
        pgo_env.llvm_profdata.display()
    );
    Ok(pgo_env)
}

/// Merges PGO profiles from the given `pgo_dir` directory and returns a path to the merged profile.
///
/// This function takes care of calculating the hash of the profile and naming the profile with
/// its given hash, so that if the contents of the profile change, the names of the profile will
/// also change. This is done to properly invalidate the `rustc` compilation session
/// (https://github.com/rust-lang/rust/issues/100397).
fn merge_profiles(
    pgo_env: &PgoEnv,
    stats: &ProfileStats,
    pgo_dir: &Path,
) -> anyhow::Result<PathBuf> {
    let tempdir = tempfile::tempdir()?;
    let profile_tmp_path = tempdir.path().join("merged.profile");

    // Merge profiles
    let mut command = Command::new(&pgo_env.llvm_profdata);
    command.args(["merge", "-o", &profile_tmp_path.display().to_string()]);
    for file in &stats.profile_files {
        command.arg(file);
    }

    let output = command.output()?;
    if !output.status.success() {
        return Err(anyhow!(
            "Failed to merge PGO profile(s): {}.",
            String::from_utf8_lossy(&output.stderr).red()
        ));
    }

    // Calculate hash
    let hash = hash_file(&profile_tmp_path)
        .map_err(|error| anyhow::anyhow!("Cannot hash merged profile file: {:?}", error))?;

    let profile_name = format!("merged-{}.profdata", hash);
    let target_profile = pgo_dir.join(profile_name);

    // Move the merged profile to PGO profile directory
    move_file(&profile_tmp_path, &target_profile)?;

    log::info!(
        "Merged PGO profile(s) to {}.",
        cli_format_path(target_profile.display())
    );
    Ok(target_profile)
}

struct PgoMissingProfile<'msg> {
    module: &'msg str,
    function: Demangle<'msg>,
}

#[derive(Debug, Default)]
struct MissingProfileCounter {
    counter: usize,
}

impl MissingProfileCounter {
    fn handle_missing_profile(&mut self, _profile: PgoMissingProfile) {
        self.counter += 1;
    }
}

fn get_pgo_missing_profile(message: &CompilerMessage) -> Option<PgoMissingProfile> {
    static REGEX: OnceCell<Regex> = OnceCell::new();

    let regex = REGEX.get_or_init(|| {
        Regex::new("^(?P<module>.*): no profile data available for function (?P<function>.*?) .*$")
            .unwrap()
    });

    if message.message.level != DiagnosticLevel::Warning {
        return None;
    }

    regex.captures(&message.message.message).map(|regex_match| {
        let module = regex_match.name("module").unwrap().as_str();
        let function = demangle(regex_match.name("function").unwrap().as_str());
        PgoMissingProfile { module, function }
    })
}
