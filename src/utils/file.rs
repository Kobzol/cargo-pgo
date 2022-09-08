use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Generate a user-readable hash of a file.
pub fn hash_file(path: &Path) -> anyhow::Result<String> {
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 4096];

    let mut file = File::open(path)?;

    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }

    let hash = hasher.finalize();
    Ok(hash.to_string())
}

/// Gathers all files (recursively) with the given extension
pub fn gather_files_with_extension(directory: &Path, extension: &str) -> Vec<PathBuf> {
    log::debug!(
        "Finding files with extension {} in {}.",
        extension,
        directory.display()
    );

    let mut files = vec![];
    let extension = OsStr::new(extension);

    let walker = WalkDir::new(directory).into_iter();
    for entry in walker.flatten() {
        if entry.file_type().is_file() && entry.path().extension() == Some(extension) {
            log::debug!("Found file: {:?}.", entry);
            files.push(entry.path().to_path_buf());
        }
    }

    files
}

/// Moves `src` to `dst`.
/// If the file cannot be moved, it will be copied and then deleted.
pub fn move_file(src: &Path, dest: &Path) -> std::io::Result<()> {
    match std::fs::rename(src, dest) {
        Ok(_) => {}
        Err(error) => {
            log::debug!(
                "Couldn't move file from {} to {}: {:?}",
                src.display(),
                dest.display(),
                error
            );
            std::fs::copy(src, dest)?;
            std::fs::remove_file(src)?;
        }
    }

    Ok(())
}
