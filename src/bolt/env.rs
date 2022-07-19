use crate::resolve_binary;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct BoltEnv {
    pub bolt: PathBuf,
    pub merge_fdata: PathBuf,
}

pub(crate) fn find_llvm_bolt() -> anyhow::Result<PathBuf> {
    resolve_binary(Path::new("llvm-bolt"))
        .map_err(|error| anyhow::anyhow!("Cannot find llvm-bolt: {:?}", error))
}

pub(crate) fn find_merge_fdata() -> anyhow::Result<PathBuf> {
    resolve_binary(Path::new("merge-fdata"))
        .map_err(|error| anyhow::anyhow!("Cannot find merge-fdata: {:?}", error))
}

pub fn find_bolt_env() -> anyhow::Result<BoltEnv> {
    let bolt = find_llvm_bolt()?;
    let merge_fdata = find_merge_fdata()?;

    Ok(BoltEnv { bolt, merge_fdata })
}
