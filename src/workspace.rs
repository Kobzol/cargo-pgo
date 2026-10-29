use crate::build::parse_cargo_args;
use crate::utils::file::{
    CustomProfileReset, ProfileKind, begin_custom_profile_cleanup, begin_custom_profile_reset,
    recover_custom_profile_directory, recover_default_profile_directory,
};
use crate::{clear_directory, ensure_directory};
use std::path::{Path, PathBuf};

pub struct CargoContext {
    target_directory: PathBuf,
    pgo_profiles_dir: Option<PathBuf>,
    bolt_profiles_dir: Option<PathBuf>,
}

impl CargoContext {
    pub fn get_pgo_directory(&self) -> anyhow::Result<PathBuf> {
        if let Some(profiles_dir) = &self.pgo_profiles_dir {
            recover_custom_profile_directory(profiles_dir, ProfileKind::Pgo)?;
            Ok(profiles_dir.clone())
        } else {
            let directory = self.get_target_directory(Path::new("pgo-profiles"))?;
            recover_default_profile_directory(&directory, ProfileKind::Pgo)?;
            Ok(directory)
        }
    }

    pub fn get_bolt_directory(&self) -> anyhow::Result<PathBuf> {
        if let Some(profiles_dir) = &self.bolt_profiles_dir {
            recover_custom_profile_directory(profiles_dir, ProfileKind::Bolt)?;
            Ok(profiles_dir.clone())
        } else {
            let directory = self.get_target_directory(Path::new("bolt-profiles"))?;
            recover_default_profile_directory(&directory, ProfileKind::Bolt)?;
            Ok(directory)
        }
    }

    pub(crate) fn reset_pgo_directory(
        &self,
        directory: &Path,
    ) -> std::io::Result<Option<CustomProfileReset>> {
        self.reset_profile_directory(directory, self.pgo_profiles_dir.is_some(), ProfileKind::Pgo)
    }

    pub(crate) fn reset_bolt_directory(
        &self,
        directory: &Path,
    ) -> std::io::Result<Option<CustomProfileReset>> {
        self.reset_profile_directory(
            directory,
            self.bolt_profiles_dir.is_some(),
            ProfileKind::Bolt,
        )
    }

    pub(crate) fn clean_pgo_directory(
        &self,
        directory: &Path,
    ) -> std::io::Result<Option<CustomProfileReset>> {
        self.clean_profile_directory(directory, self.pgo_profiles_dir.is_some(), ProfileKind::Pgo)
    }

    pub(crate) fn clean_bolt_directory(
        &self,
        directory: &Path,
    ) -> std::io::Result<Option<CustomProfileReset>> {
        self.clean_profile_directory(
            directory,
            self.bolt_profiles_dir.is_some(),
            ProfileKind::Bolt,
        )
    }

    fn reset_profile_directory(
        &self,
        directory: &Path,
        custom: bool,
        kind: ProfileKind,
    ) -> std::io::Result<Option<CustomProfileReset>> {
        if custom {
            begin_custom_profile_reset(directory, kind).map(Some)
        } else {
            clear_directory(directory)?;
            Ok(None)
        }
    }

    fn clean_profile_directory(
        &self,
        directory: &Path,
        custom: bool,
        kind: ProfileKind,
    ) -> std::io::Result<Option<CustomProfileReset>> {
        if custom {
            begin_custom_profile_cleanup(directory, kind)
        } else {
            std::fs::remove_dir_all(directory)?;
            Ok(None)
        }
    }

    pub(crate) fn pgo_profiles_are_custom(&self) -> bool {
        self.pgo_profiles_dir.is_some()
    }

    pub(crate) fn bolt_profiles_are_custom(&self) -> bool {
        self.bolt_profiles_dir.is_some()
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
    pgo_profiles_dir: Option<PathBuf>,
    bolt_profiles_dir: Option<PathBuf>,
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
        pgo_profiles_dir,
        bolt_profiles_dir,
    })
}
