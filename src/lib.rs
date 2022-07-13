pub mod build;
pub mod env;

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

fn run_command(program: &str, args: &[&str]) -> anyhow::Result<String> {
    let mut cmd = Command::new(program);
    for arg in args {
        cmd.arg(arg);
    }
    cmd.stdout(std::process::Stdio::piped());
    Ok(String::from_utf8(cmd.output()?.stdout)?)
}
