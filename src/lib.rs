pub mod bolt;
mod build;
pub mod check;
pub(crate) mod cli;
pub mod pgo;
pub(crate) mod workspace;

use anyhow::anyhow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub fn resolve_binary(path: &Path) -> anyhow::Result<PathBuf> {
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
                "Command ended with exit code {}\n{}",
                self.status,
                self.stderr
            ))
        }
    }
}

/// Runs a command with the provided arguments and returns its stdout and stderr.
fn run_command<S: AsRef<OsStr>>(program: S, args: &[&str]) -> anyhow::Result<Utf8Output> {
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
    const HOST_FIELD: &str = "host: ";

    // Query rustc for defaults.
    let output = run_command("rustc", &["-vV"])?;

    // Parse the default target from stdout.
    let host = output
        .stdout
        .lines()
        .find(|l| l.starts_with(HOST_FIELD))
        .map(|l| &l[HOST_FIELD.len()..])
        .ok_or_else(|| anyhow!("Failed to parse target from rustc output."))?
        .to_owned();
    Ok(host)
}

/// Clears all files from the directory, if it exists.
fn clear_directory(path: &Path) -> std::io::Result<()> {
    std::fs::remove_dir_all(path)?;
    std::fs::create_dir_all(path)
}
