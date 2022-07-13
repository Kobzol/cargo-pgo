use crate::{resolve_binary, run_command};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct PgoEnv {
    pub llvm_profdata: PathBuf,
}

pub fn find_pgo_env() -> anyhow::Result<PgoEnv> {
    // Try to find `llvm-profdata` directly in PATH
    if let Ok(llvm_profdata) = resolve_binary(Path::new("llvm-profdata")) {
        return Ok(PgoEnv { llvm_profdata });
    }

    // Try to resolve `llvm-profdata` from `llvm-tools-preview`
    let mut libpath = PathBuf::from(run_command("rustc", &["--print", "target-libdir"])?);
    libpath.pop();
    libpath.push("bin/llvm-profdata");

    if libpath.exists() {
        Ok(PgoEnv {
            llvm_profdata: libpath,
        })
    } else {
        Err(anyhow::anyhow!("Could not find `llvm-profdata`"))
    }
}
