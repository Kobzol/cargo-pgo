use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct BoltEnv {
    pub bolt: PathBuf,
    pub merge_fdata: PathBuf,
}

pub fn resolve_binary(path: &Path) -> anyhow::Result<PathBuf> {
    Ok(which::which(path)?)
}

pub fn find_bolt_env() -> anyhow::Result<BoltEnv> {
    let bolt = resolve_binary(Path::new("llvm-bolt"))
        .map_err(|error| anyhow::anyhow!("Cannot find llvm-bolt: {error:?}"))?;
    let merge_fdata = resolve_binary(Path::new("merge-fdata"))
        .map_err(|error| anyhow::anyhow!("Cannot find merge-fdata: {error:?}"))?;

    Ok(BoltEnv { bolt, merge_fdata })
}
