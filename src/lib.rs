//! This crate contains a Cargo subcommand designed for simplifying the usage of
//! feedback-directed optimizations for Rust crates.
//!
//! You can find a usage guide for `cargo-pgo` at its [repository](https://github.com/kobzol/cargo-pgo).

pub mod bolt;
pub mod build;
pub mod check;
pub mod clean;
pub(crate) mod cli;
pub mod pgo;
pub(crate) mod utils;
pub(crate) mod workspace;

use anyhow::anyhow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub use workspace::get_cargo_ctx;

pub(crate) fn resolve_binary(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(which::which(path)?)
}

#[derive(Debug)]
struct Utf8Output {
    stdout: String,
    stderr: String,
    status: ExitStatus,
}

impl Utf8Output {
    pub fn ok(self) -> anyhow::Result<Self> {
        if self.status.success() {
            Ok(self)
        } else {
            Err(anyhow::anyhow!(
                "Command ended with {}\nStderr\n{}\nStdout\n{}",
                self.status,
                self.stderr,
                self.stdout
            ))
        }
    }
}

/// Runs a command with the provided arguments and returns its stdout and stderr.
fn run_command<S: AsRef<OsStr>, Str: AsRef<OsStr>>(
    program: S,
    args: &[Str],
) -> anyhow::Result<Utf8Output> {
    let mut cmd = Command::new(program);
    for arg in args {
        cmd.arg(arg);
    }
    log::debug!("Running command {:?}", cmd);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd.output()?;
    Ok(Utf8Output {
        stdout: String::from_utf8(output.stdout)?,
        stderr: String::from_utf8(output.stderr)?,
        status: output.status,
    })
}

/// Tries to find the default target triple used for compiling on the current host computer.
pub fn get_default_target() -> anyhow::Result<String> {
    get_rustc_info("host: ")
}

pub fn get_rustc_version() -> anyhow::Result<semver::Version> {
    let version = get_rustc_info("release: ")?;
    let version = semver::Version::parse(&version)?;
    Ok(version)
}

fn get_rustc_info(field: &str) -> anyhow::Result<String> {
    // Query rustc for defaults.
    let output = run_command("rustc", &["-vV"])?;

    // Parse the field from stdout.
    let host = output
        .stdout
        .lines()
        .find(|l| l.starts_with(field))
        .map(|l| l[field.len()..].trim())
        .ok_or_else(|| anyhow!("Failed to parse field {} from rustc output.", field))?
        .to_owned();
    Ok(host)
}

/// Clears all files from the directory, and recreates it.
fn clear_directory(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    ensure_directory(path)
}

/// Make sure that directory exists.
fn ensure_directory(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}
