use crate::{resolve_binary, run_command};
use colored::Colorize;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct PgoEnv {
    pub llvm_profdata: PathBuf,
}

pub fn find_pgo_env() -> anyhow::Result<PgoEnv> {
    // Try to resolve `llvm-profdata` from `llvm-tools-preview`
    let path = run_command("rustc", &["--print", "target-libdir"])?
        .ok()?
        .stdout;

    let mut libpath = PathBuf::from(path);
    libpath.pop();
    libpath.push("bin/llvm-profdata");

    if libpath.exists() {
        return Ok(PgoEnv {
            llvm_profdata: libpath,
        });
    }

    // Try to find `llvm-profdata` directly in PATH
    if let Ok(llvm_profdata) = resolve_binary(Path::new("llvm-profdata")) {
        log::warn!(
            "llvm-profdata was resolved from PATH. \
Make sure that its version is compatible with rustc! If not, run `{}`.",
            "rustup component add llvm-tools-preview".blue()
        );

        Ok(PgoEnv { llvm_profdata })
    } else {
        Err(anyhow::anyhow!("Could not find `llvm-profdata`"))
    }
}
