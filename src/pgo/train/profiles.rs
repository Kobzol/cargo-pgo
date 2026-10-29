use anyhow::{Context, ensure};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub(super) struct AttemptProfiles {
    directory: TempDir,
    destination: PathBuf,
}

impl AttemptProfiles {
    pub fn new(destination: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(destination)?;
        let destination = std::fs::canonicalize(destination)?;
        let parent = destination
            .parent()
            .context("Profile directory has no parent")?;
        Ok(Self {
            directory: tempfile::tempdir_in(parent)
                .context("Cannot create attempt profile directory")?,
            destination,
        })
    }

    pub fn pattern(&self) -> PathBuf {
        self.directory.path().join("profile-%m-%p.profraw")
    }

    pub fn publish(self) -> anyhow::Result<u64> {
        let files = collect(self.directory.path())?;
        ensure!(
            !files.is_empty(),
            "Workload produced no nonempty raw profiles"
        );
        let output = tempfile::Builder::new()
            .prefix("training-")
            .tempdir_in(self.directory.path().parent().unwrap())
            .context("Cannot create training contribution")?;
        let result = (|| -> anyhow::Result<()> {
            for (index, source) in files.iter().enumerate() {
                let target = output.path().join(format!("{index}.profraw"));
                std::fs::copy(source, target).context("Cannot publish training profile")?;
            }
            let target = self.destination.join(output.path().file_name().unwrap());
            std::fs::rename(output.path(), target)
                .context("Cannot publish training contribution")?;
            Ok(())
        })();
        if let Err(error) = result {
            if let Err(cleanup) = output.close() {
                return Err(error.context(format!("Cannot clean failed contribution: {cleanup}")));
            }
            return Err(error);
        }
        // The complete contribution is visible only after the directory rename.
        let _ = output.keep();
        Ok(files.len() as u64)
    }
}

fn collect(directory: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(directory).follow_links(false) {
        let entry = entry.context("Cannot inspect attempt profiles")?;
        let path = entry.path();
        if entry.file_type().is_file()
            && (path.extension() == Some(OsStr::new("profraw"))
                || path.file_name() == Some(OsStr::new(".profraw")))
            && entry.metadata()?.len() > 0
        {
            files.push(path.to_path_buf());
        }
    }
    Ok(files)
}
