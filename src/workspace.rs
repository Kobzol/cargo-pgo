use crate::build::parse_cargo_args;
use crate::ensure_directory;
use std::path::{Path, PathBuf};

pub struct CargoContext {
    target_directory: PathBuf,
    profiles_dir: Option<PathBuf>,
}

impl CargoContext {
    pub fn get_pgo_directory(&self) -> anyhow::Result<PathBuf> {
        if let Some(profiles_dir) = &self.profiles_dir {
            Ok(profiles_dir.clone())
        } else {
            self.get_target_directory(Path::new("pgo-profiles"))
        }
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
pub fn get_cargo_ctx(
    cargo_args: &[String],
    profiles_dir: Option<PathBuf>,
) -> anyhow::Result<CargoContext> {
    let cargo_args = parse_cargo_args(cargo_args.to_vec());
    let target_directory = match cargo_args.target_dir {
        Some(dir) => dir,
        None => {
            let cmd = cargo_metadata::MetadataCommand::new();
            let metadata = cmd
                .exec()
                .map_err(|error| anyhow::anyhow!("Cannot get cargo metadata: {:?}", error))?;
            metadata.target_directory.into_std_path_buf()
        }
    };

    Ok(CargoContext {
        target_directory,
        profiles_dir,
    })
}
