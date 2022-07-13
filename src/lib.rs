pub(crate) mod cli;
pub mod pgo;
pub(crate) mod workspace;

use anyhow::anyhow;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn resolve_binary(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(which::which(path)?)
}

#[derive(Debug)]
pub struct BoltEnv {
    pub bolt: PathBuf,
    pub merge_fdata: PathBuf,
}

pub fn find_bolt_env() -> anyhow::Result<BoltEnv> {
    let bolt = resolve_binary(Path::new("llvm-bolt"))
        .map_err(|error| anyhow::anyhow!("Cannot find llvm-bolt: {error:?}"))?;
    let merge_fdata = resolve_binary(Path::new("merge-fdata"))
        .map_err(|error| anyhow::anyhow!("Cannot find merge-fdata: {error:?}"))?;

    Ok(BoltEnv { bolt, merge_fdata })
}

/// Runs a command with the provided arguments and returns its stdout.
fn run_command(program: &str, args: &[&str]) -> anyhow::Result<String> {
    let mut cmd = Command::new(program);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdout(std::process::Stdio::piped());
    Ok(String::from_utf8(cmd.output()?.stdout)?)
}

/// Tries to find the default target triple used for compiling on the current host computer.
pub fn get_default_target() -> anyhow::Result<String> {
    const HOST_FIELD: &str = "host: ";

    // Query rustc for defaults.
    let output = run_command("rustc", &["-vV"])?;

    // Parse the default target from stdout.
    let host = output
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
