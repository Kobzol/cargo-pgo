use std::ffi::OsStr;
use std::path::Path;
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

use crate::build::{cargo_command_with_flags, handle_metadata_message};
use crate::cli::cli_format_path;
use crate::pgo::env::{find_pgo_env, PgoEnv};
use crate::pgo::{llvm_profdata_install_hint, CargoCommand};
use crate::utils::str::pluralize;
use crate::workspace::CargoContext;

#[derive(clap::Parser, Debug)]
#[clap(trailing_var_arg(true))]
pub struct PgoOptimizeArgs {
    cargo_args: Vec<String>,
}

/// Merges PGO profiles and creates RUSTFLAGS that use them.
pub fn prepare_pgo_optimization_flags(pgo_env: &PgoEnv, pgo_dir: &Path) -> anyhow::Result<String> {
    print_pgo_profile_stats(pgo_dir)?;

    let target_file = pgo_dir.join("merged.profdata");
    merge_profiles(pgo_env, pgo_dir, &target_file)?;

    Ok(format!(
        "-Cprofile-use={} -Cllvm-args=-pgo-warn-missing-function",
        target_file.display()
    ))
}

pub fn pgo_optimize(ctx: CargoContext, args: PgoOptimizeArgs) -> anyhow::Result<()> {
    let pgo_dir = ctx.get_pgo_directory()?;
    let pgo_env = get_pgo_env()?;

    let flags = prepare_pgo_optimization_flags(&pgo_env, &pgo_dir)?;

    let output = cargo_command_with_flags(CargoCommand::Build, &flags, args.cargo_args)?;
    log::debug!("Cargo stderr\n {}", String::from_utf8_lossy(&output.stderr));

    let mut counter = MissingProfileCounter::default();
    for message in Message::parse_stream(output.stdout.as_slice()) {
        let message = message?;
        match message {
            Message::CompilerArtifact(artifact) => {
                if artifact.executable.is_some() {
                    log::info!(
                        "PGO-optimized binary {} built successfully.",
                        artifact.target.name.blue()
                    );
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
            log::debug!("Found profile file {}.", entry.path().display());
            stats.total_size += metadata.len();
            stats.file_count += 1;
        }
    }

    Ok(stats)
}

fn print_pgo_profile_stats(pgo_dir: &Path) -> anyhow::Result<()> {
    log::debug!("Locating PGO profile files at {}.", pgo_dir.display());

    let stats = gather_pgo_profile_stats(pgo_dir)?;
    if stats.file_count == 0 {
        return Err(anyhow!(
            "No profile files were found at {}. Did you execute your instrumented program?",
            cli_format_path(pgo_dir.display())
        ));
    }

    log::info!(
        "Found {} PGO profile {} with total size {} at {}.",
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

pub fn get_pgo_env() -> anyhow::Result<PgoEnv> {
    let pgo_env =
        find_pgo_env().map_err(|error| anyhow!("{}\n{}", error, llvm_profdata_install_hint()))?;
    log::debug!(
        "Found `llvm-profdata` at {}.",
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
        log::info!(
            "Merged PGO profile(s) to {}.",
            cli_format_path(target_profile.display())
        );
        Ok(())
    } else {
        return Err(anyhow!(
            "Failed to merge PGO profile(s): {}.",
            String::from_utf8_lossy(&output.stderr).red()
        ));
    }
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
