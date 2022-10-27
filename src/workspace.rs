use crate::ensure_directory;
use std::path::{Path, PathBuf};

pub struct CargoContext {
    target_directory: PathBuf,
}

impl CargoContext {
    pub fn get_pgo_directory(&self) -> anyhow::Result<PathBuf> {
        self.get_target_directory(Path::new("pgo-profiles"))
    }

    pub fn get_bolt_directory(&self) -> anyhow::Result<PathBuf> {
        self.get_target_directory(Path::new("bolt-profiles"))
    }

    fn get_target_directory(&self, path: &Path) -> anyhow::Result<PathBuf> {
        let directory = self.target_directory.join(path);
        ensure_directory(&directory)?;
        Ok(directory)
    }
}

/// Finds Cargo metadata from the current directory.
pub fn get_cargo_ctx() -> anyhow::Result<CargoContext> {
    let cmd = cargo_metadata::MetadataCommand::new();
    let metadata = cmd
        .exec()
        .map_err(|error| anyhow::anyhow!("Cannot get cargo metadata: {:?}", error))?;
    Ok(CargoContext {
        target_directory: metadata.target_directory.into_std_path_buf(),
    })
}
