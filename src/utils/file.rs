use std::ffi::OsStr;
use std::fs::File;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

const OWNER_MARKER: &str = ".cargo-pgo-owner";
const ROLLBACK_MARKER: &str = ".cargo-pgo-rollback";
const COMMITTED_MARKER: &str = ".cargo-pgo-committed";
static TRANSACTION_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub enum ProfileKind {
    Pgo,
    Bolt,
}

impl ProfileKind {
    fn reset_directory_prefix(self) -> &'static str {
        match self {
            ProfileKind::Pgo => ".cargo-pgo-pgo-reset",
            ProfileKind::Bolt => ".cargo-pgo-bolt-reset",
        }
    }

    fn marker_prefix(self) -> &'static str {
        match self {
            ProfileKind::Pgo => "cargo-pgo:pgo:",
            ProfileKind::Bolt => "cargo-pgo:bolt:",
        }
    }

    fn transaction_marker(self, sequence: u64) -> Vec<u8> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!(
            "{}{}:{timestamp}:{sequence}\n",
            self.marker_prefix(),
            std::process::id()
        )
        .into_bytes()
    }

    fn transaction_directory_name(self, contents: &[u8]) -> Option<String> {
        let Ok(contents) = std::str::from_utf8(contents) else {
            return None;
        };
        let identity = contents
            .strip_prefix(self.marker_prefix())
            .and_then(|contents| contents.strip_suffix('\n'))?;
        let mut fields = identity.split(':');
        let (Some(process), Some(timestamp), Some(sequence), None) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return None;
        };
        process.parse::<u32>().ok()?;
        timestamp.parse::<u128>().ok()?;
        sequence.parse::<u64>().ok()?;
        Some(format!(
            "{}-{process}-{timestamp}-{sequence}",
            self.reset_directory_prefix()
        ))
    }

    fn owns_reset_directory(self, path: &Path, marker: &[u8]) -> bool {
        self.transaction_directory_name(marker).as_deref()
            == path.file_name().and_then(OsStr::to_str)
    }
}

pub struct CustomProfileReset {
    directory: PathBuf,
    kind: ProfileKind,
    reset_directory: Option<PathBuf>,
    finished: bool,
}

impl CustomProfileReset {
    pub fn finish(mut self, result: anyhow::Result<()>) -> anyhow::Result<()> {
        match result {
            Ok(()) => {
                self.commit()?;
                Ok(())
            }
            Err(error) => match self.rollback() {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(error.context(format!(
                    "Could not restore profiles after the command failed: {rollback_error}"
                ))),
            },
        }
    }

    fn commit(&mut self) -> std::io::Result<()> {
        let Some(reset_directory) = self.reset_directory.as_ref() else {
            self.finished = true;
            return Ok(());
        };
        match std::fs::symlink_metadata(reset_directory) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
                let Some(owner) = owned_reset_marker(reset_directory, self.kind)? else {
                    return Err(unowned_reset_directory_error(reset_directory));
                };
                if read_optional_marker(&reset_directory.join(ROLLBACK_MARKER))?.as_deref()
                    != Some(owner.as_slice())
                {
                    return Err(invalid_reset_state_error(reset_directory));
                }
                let committed_marker = reset_directory.join(COMMITTED_MARKER);
                if path_entry_exists(&committed_marker)? {
                    return Err(std::io::Error::new(
                        ErrorKind::AlreadyExists,
                        format!(
                            "Cannot commit custom profile reset because {} already exists",
                            committed_marker.display()
                        ),
                    ));
                }
                // Publish the committed state before deleting backups so recovery never restores
                // artifacts from a reset that has already succeeded.
                std::fs::rename(reset_directory.join(ROLLBACK_MARKER), &committed_marker)?;
                self.finished = true;
                if let Err(error) =
                    discard_committed_reset_directory(&self.directory, reset_directory, self.kind)
                {
                    log::debug!(
                        "Could not finish removing committed profile reset directory {}: {:?}",
                        reset_directory.display(),
                        error
                    );
                }
                return Ok(());
            }
            Ok(_) => return Err(unowned_reset_directory_error(reset_directory)),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.finished = true;
        Ok(())
    }

    fn rollback(&mut self) -> std::io::Result<()> {
        if let Some(reset_directory) = self.reset_directory.as_ref() {
            let Some(owner) = owned_reset_marker(reset_directory, self.kind)? else {
                return Err(unowned_reset_directory_error(reset_directory));
            };
            recover_owned_reset_directory(&self.directory, reset_directory, self.kind, &owner)?;
        }
        self.finished = true;
        Ok(())
    }
}

impl Drop for CustomProfileReset {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.rollback();
        }
    }
}

pub fn begin_custom_profile_reset(
    directory: &Path,
    kind: ProfileKind,
) -> std::io::Result<CustomProfileReset> {
    begin_custom_profile_reset_inner(directory, kind, true)
        .map(|reset| reset.expect("creating a missing profile directory should start a reset"))
}

pub fn begin_custom_profile_cleanup(
    directory: &Path,
    kind: ProfileKind,
) -> std::io::Result<Option<CustomProfileReset>> {
    begin_custom_profile_reset_inner(directory, kind, false)
}

fn begin_custom_profile_reset_inner(
    directory: &Path,
    kind: ProfileKind,
    create_missing: bool,
) -> std::io::Result<Option<CustomProfileReset>> {
    recover_custom_profile_directory(directory, kind)?;
    if !validate_custom_profile_directory(directory)? {
        if !create_missing {
            return Ok(None);
        }
        create_directory_path_without_symlinks(directory)?;
    }

    let artifacts = custom_profile_artifacts(directory, kind)?;
    let reset_directory = (!artifacts.is_empty())
        .then(|| create_reset_directory(directory, kind))
        .transpose()?;

    let mut reset = CustomProfileReset {
        directory: directory.to_path_buf(),
        kind,
        reset_directory,
        finished: false,
    };
    for artifact in artifacts {
        let relative = artifact
            .strip_prefix(directory)
            .expect("Profile artifact should be below its profile directory");
        let destination = reset
            .reset_directory
            .as_ref()
            .expect("A reset directory should exist when profiles are reset")
            .join(relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Err(error) = std::fs::rename(&artifact, &destination) {
            let _ = reset.rollback();
            return Err(error);
        }
    }

    Ok(Some(reset))
}

pub fn recover_custom_profile_directory(
    directory: &Path,
    kind: ProfileKind,
) -> std::io::Result<()> {
    if !validate_custom_profile_directory(directory)? {
        return Ok(());
    }

    recover_owned_profile_resets(directory, kind)
}

pub fn recover_default_profile_directory(
    directory: &Path,
    kind: ProfileKind,
) -> std::io::Result<()> {
    // Resolve target-directory aliases without following a profile root that is itself a link.
    let Some(name) = directory.file_name() else {
        return Ok(());
    };
    let Some(parent) = directory.parent() else {
        return Ok(());
    };
    let directory = std::fs::canonicalize(parent)?.join(name);
    match std::fs::symlink_metadata(&directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            recover_owned_profile_resets(&directory, kind)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn recover_owned_profile_resets(directory: &Path, kind: ProfileKind) -> std::io::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let reset_directory = entry.path();
        let Some(owner) = owned_reset_marker(&reset_directory, kind)? else {
            continue;
        };
        recover_owned_reset_directory(directory, &reset_directory, kind, &owner)?;
    }
    Ok(())
}

fn recover_owned_reset_directory(
    directory: &Path,
    reset_directory: &Path,
    kind: ProfileKind,
    owner: &[u8],
) -> std::io::Result<()> {
    if !kind.owns_reset_directory(reset_directory, owner) {
        return Err(unowned_reset_directory_error(reset_directory));
    }

    let committed_marker = reset_directory.join(COMMITTED_MARKER);
    match read_optional_marker(&committed_marker)? {
        Some(contents) if contents == owner => {
            return discard_committed_reset_directory(directory, reset_directory, kind);
        }
        Some(_) => return Err(invalid_reset_state_error(reset_directory)),
        None => {}
    }
    if read_optional_marker(&reset_directory.join(ROLLBACK_MARKER))?.as_deref() != Some(owner) {
        return Err(invalid_reset_state_error(reset_directory));
    }

    let artifacts = reset_artifacts(directory, reset_directory, kind, ROLLBACK_MARKER)?;
    for artifact in artifacts {
        let relative = artifact
            .strip_prefix(reset_directory)
            .expect("Reset artifact should be below its reset directory");
        let destination = directory.join(relative);
        if let Some(parent) = destination.parent() {
            create_directory_without_symlinks(directory, parent)?;
        }
        if path_entry_exists(&destination)? {
            retain_conflicting_profile(&destination, directory, kind, owner)?;
        }
        std::fs::rename(&artifact, &destination)?;
    }
    remove_empty_reset_directory(reset_directory, ROLLBACK_MARKER)
}

fn discard_committed_reset_directory(
    directory: &Path,
    reset_directory: &Path,
    kind: ProfileKind,
) -> std::io::Result<()> {
    // Keep the ownership and commit markers until every backup is gone. If deletion is
    // interrupted, the next command can still authenticate and resume this committed reset.
    let artifacts = reset_artifacts(directory, reset_directory, kind, COMMITTED_MARKER)?;
    for artifact in artifacts {
        remove_regular_file(&artifact)?;
    }

    remove_empty_reset_directory(reset_directory, COMMITTED_MARKER)
}

fn reset_artifacts(
    directory: &Path,
    reset_directory: &Path,
    kind: ProfileKind,
    state_marker: &str,
) -> std::io::Result<Vec<PathBuf>> {
    // Validate every saved entry before callers mutate the transaction, so unexpected user
    // content cannot leave recovery partially applied.
    let mut artifacts = vec![];
    for entry in WalkDir::new(reset_directory)
        .follow_links(false)
        .min_depth(1)
    {
        let entry = map_walkdir_error(entry)?;
        if entry.file_type().is_symlink() {
            return Err(unexpected_reset_entry_error(entry.path()));
        }
        if entry.file_type().is_dir() || is_reset_state_marker(&entry, state_marker) {
            continue;
        }
        if !entry.file_type().is_file() {
            return Err(unexpected_reset_entry_error(entry.path()));
        }

        let relative = entry
            .path()
            .strip_prefix(reset_directory)
            .expect("Reset artifact should be below its reset directory");
        if !is_profile_artifact(&directory.join(relative), directory, kind) {
            return Err(unexpected_reset_entry_error(entry.path()));
        }
        artifacts.push(entry.path().to_path_buf());
    }
    Ok(artifacts)
}

fn is_reset_state_marker(entry: &walkdir::DirEntry, state_marker: &str) -> bool {
    entry.depth() == 1
        && entry.file_type().is_file()
        && matches!(
            entry.file_name().to_str(),
            Some(name) if name == OWNER_MARKER || name == state_marker
        )
}

fn remove_empty_reset_directory(reset_directory: &Path, state_marker: &str) -> std::io::Result<()> {
    // Removing only empty directories makes a concurrently added entry block cleanup instead of
    // being unlinked as part of a recursive deletion.
    for entry in WalkDir::new(reset_directory)
        .follow_links(false)
        .min_depth(1)
        .contents_first(true)
    {
        let entry = map_walkdir_error(entry)?;
        if is_reset_state_marker(&entry, state_marker) {
            continue;
        }
        if !entry.file_type().is_dir() {
            return Err(unexpected_reset_entry_error(entry.path()));
        }
        std::fs::remove_dir(entry.path())?;
    }

    remove_regular_file(&reset_directory.join(OWNER_MARKER))?;
    remove_regular_file(&reset_directory.join(state_marker))?;
    std::fs::remove_dir(reset_directory)
}

fn remove_regular_file(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(unexpected_reset_entry_error(path));
    }
    std::fs::remove_file(path)
}

fn unexpected_reset_entry_error(path: &Path) -> std::io::Error {
    std::io::Error::new(
        ErrorKind::InvalidData,
        format!(
            "Cannot recover custom profiles because {} is not an owned profile backup",
            path.display()
        ),
    )
}

fn owned_reset_marker(
    reset_directory: &Path,
    kind: ProfileKind,
) -> std::io::Result<Option<Vec<u8>>> {
    let metadata = match std::fs::symlink_metadata(reset_directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Ok(None);
    }

    let owner_path = reset_directory.join(OWNER_MARKER);
    let owner_metadata = match std::fs::symlink_metadata(&owner_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !owner_metadata.is_file() || owner_metadata.file_type().is_symlink() {
        return Ok(None);
    }
    let owner = std::fs::read(owner_path)?;
    Ok(kind
        .owns_reset_directory(reset_directory, &owner)
        .then_some(owner))
}

fn unowned_reset_directory_error(reset_directory: &Path) -> std::io::Error {
    std::io::Error::new(
        ErrorKind::AlreadyExists,
        format!(
            "Cannot recover custom profiles because {} is not an owned reset directory",
            reset_directory.display()
        ),
    )
}

fn invalid_reset_state_error(reset_directory: &Path) -> std::io::Error {
    std::io::Error::new(
        ErrorKind::AlreadyExists,
        format!(
            "Cannot recover custom profiles because {} has invalid reset state",
            reset_directory.display()
        ),
    )
}

fn retain_conflicting_profile(
    path: &Path,
    directory: &Path,
    kind: ProfileKind,
    owner: &[u8],
) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "Cannot retain a newly created profile because {} is not a regular file",
                path.display()
            ),
        ));
    }

    let identity = blake3::hash(owner).to_hex().to_string();
    for sequence in 0u64.. {
        let candidate = match kind {
            ProfileKind::Pgo if path.parent() == Some(directory) && is_merged_pgo_profile(path) => {
                directory
                    .join(format!(
                        ".cargo-pgo-retained-{}-{sequence}",
                        &identity[..16]
                    ))
                    .join("profile.profraw")
            }
            ProfileKind::Bolt
                if path.file_name() == Some(OsStr::new("merged.profdata"))
                    && path
                        .strip_prefix(directory)
                        .is_ok_and(|relative| relative.components().count() == 2) =>
            {
                let target = directory.join(format!(
                    ".cargo-pgo-retained-{}-{sequence}",
                    &identity[..16]
                ));
                target.join("merged.profdata")
            }
            _ => retained_raw_profile_candidate(path, &identity, sequence),
        };

        let mut created_parent = None;
        if candidate.parent() != path.parent() {
            let parent = candidate
                .parent()
                .expect("A retained profile candidate should have a parent");
            match std::fs::create_dir(parent) {
                Ok(()) => created_parent = Some(parent.to_path_buf()),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }

        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => {
                if let Some(parent) = created_parent {
                    let _ = std::fs::remove_dir(parent);
                }
                continue;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                let result = std::fs::rename(path, &candidate);
                if result.is_err() {
                    if let Some(parent) = created_parent {
                        let _ = std::fs::remove_dir(parent);
                    }
                }
                return result;
            }
            Err(error) => {
                if let Some(parent) = created_parent {
                    let _ = std::fs::remove_dir(parent);
                }
                return Err(error);
            }
        }
    }
    unreachable!()
}

fn retained_raw_profile_candidate(path: &Path, identity: &str, sequence: u64) -> PathBuf {
    let parent = path
        .parent()
        .expect("A retained profile should have a parent");
    let original_file_name = path
        .file_name()
        .expect("A retained profile should have a file name");
    let file_name = match original_file_name {
        name if name == OsStr::new(".profraw") => OsStr::new("profile.profraw"),
        name if name == OsStr::new(".fdata") => OsStr::new("profile.fdata"),
        name => name,
    };
    parent
        .join(format!(
            ".cargo-pgo-retained-{}-{sequence}",
            &identity[..16]
        ))
        .join(file_name)
}

fn create_reset_directory(directory: &Path, kind: ProfileKind) -> std::io::Result<PathBuf> {
    let (reset_directory, marker) = loop {
        let sequence = TRANSACTION_COUNTER.fetch_add(1, Ordering::Relaxed);
        let marker = kind.transaction_marker(sequence);
        let candidate = directory.join(
            kind.transaction_directory_name(&marker)
                .expect("A generated transaction marker should always be valid"),
        );
        match std::fs::create_dir(&candidate) {
            Ok(()) => break (candidate, marker),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };

    let initialized = (|| {
        // Ownership is published last so recovery cannot authenticate a partially initialized
        // transaction directory.
        std::fs::write(reset_directory.join(ROLLBACK_MARKER), &marker)?;
        std::fs::write(reset_directory.join(OWNER_MARKER), &marker)
    })();
    if let Err(error) = initialized {
        let _ = remove_initializing_reset_directory(&reset_directory);
        return Err(error);
    }
    Ok(reset_directory)
}

fn remove_initializing_reset_directory(reset_directory: &Path) -> std::io::Result<()> {
    for marker in [OWNER_MARKER, ROLLBACK_MARKER] {
        let path = reset_directory.join(marker);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
                std::fs::remove_file(path)?;
            }
            Ok(_) => return Err(unexpected_reset_entry_error(&path)),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    std::fs::remove_dir(reset_directory)
}

fn read_optional_marker(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            std::fs::read(path).map(Some)
        }
        Ok(_) => Err(std::io::Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "Profile reset marker is not a regular file: {}",
                path.display()
            ),
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn path_entry_exists(path: &Path) -> std::io::Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

pub fn create_directory_without_symlinks(root: &Path, directory: &Path) -> std::io::Result<()> {
    let relative = directory.strip_prefix(root).map_err(|_| {
        std::io::Error::new(
            ErrorKind::InvalidInput,
            format!(
                "Directory {} is outside profile root {}",
                directory.display(),
                root.display()
            ),
        )
    })?;

    create_directory_path_without_symlinks(root)?;

    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(std::io::Error::new(
                ErrorKind::InvalidInput,
                format!("Invalid profile directory: {}", directory.display()),
            ));
        };
        current.push(component);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Profile directory component is not a regular directory: {}",
                        current.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => std::fs::create_dir(&current)?,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub fn validate_profile_output(root: &Path, output: Option<&Path>) -> std::io::Result<()> {
    let root = lexical_absolute_path(root)?;
    let Some(output) = output.filter(|path| !path.as_os_str().is_empty()) else {
        return Ok(());
    };
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()?.join(output)
    };
    validate_profile_output_entry(&root, &output, 0)
}

fn validate_profile_output_entry(
    root: &Path,
    output: &Path,
    symlink_depth: usize,
) -> std::io::Result<()> {
    if symlink_depth >= 40 {
        return Err(too_many_profile_output_symlinks(output));
    }
    let Some(parent) = output.parent() else {
        return Ok(());
    };
    let Some(resolved_parent) = resolve_profile_output_parent(root, parent, symlink_depth)? else {
        return Ok(());
    };
    let file_name = output.file_name().ok_or_else(|| {
        std::io::Error::new(
            ErrorKind::InvalidInput,
            format!("Invalid PGO profile output path: {}", output.display()),
        )
    })?;
    let resolved_output = resolved_parent.join(file_name);

    match std::fs::symlink_metadata(&resolved_output) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            if path_is_within_profile_root(&resolved_parent, root) {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "PGO profile output cannot be a symbolic link: {}",
                        resolved_output.display()
                    ),
                ));
            }

            let target = match std::fs::read_link(&resolved_output) {
                Ok(target) => target,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
                Err(_) => return Ok(()),
            };
            let target = if target.is_absolute() {
                target
            } else {
                resolved_parent.join(target)
            };
            validate_profile_output_entry(root, &target, symlink_depth + 1)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) if path_is_within_profile_root(&resolved_parent, root) => Err(error),
        Err(_) => Ok(()),
    }
}

fn resolve_profile_output_parent(
    root: &Path,
    parent: &Path,
    symlink_depth: usize,
) -> std::io::Result<Option<PathBuf>> {
    if symlink_depth >= 40 {
        return Err(too_many_profile_output_symlinks(parent));
    }

    let mut current = PathBuf::new();
    let mut components = parent.components();
    while let Some(component) = components.next() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                current.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(component) => {
                let parent_is_in_root = path_is_within_profile_root(&current, root);
                current.push(component);
                match std::fs::symlink_metadata(&current) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        if parent_is_in_root {
                            return Err(std::io::Error::new(
                                ErrorKind::InvalidInput,
                                format!(
                                    "PGO profile output cannot traverse a symbolic link: {}",
                                    current.display()
                                ),
                            ));
                        }

                        // Links outside the selected root are resolved only to determine whether
                        // traversal enters it. Once inside, links are rejected instead of followed.
                        let target = match std::fs::read_link(&current) {
                            Ok(target) => target,
                            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                            Err(_) => return Ok(None),
                        };
                        let mut resolved = if target.is_absolute() {
                            target
                        } else {
                            current
                                .parent()
                                .expect("An absolute path component should have a parent")
                                .join(target)
                        };
                        for remaining in components {
                            resolved.push(remaining.as_os_str());
                        }
                        return resolve_profile_output_parent(root, &resolved, symlink_depth + 1);
                    }
                    Ok(metadata) if parent_is_in_root && !metadata.is_dir() => {
                        return Err(std::io::Error::new(
                            ErrorKind::InvalidInput,
                            format!(
                                "PGO profile output component is not a directory: {}",
                                current.display()
                            ),
                        ));
                    }
                    Ok(metadata) if !metadata.is_dir() => return Ok(None),
                    Ok(_) => {}
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(error) if parent_is_in_root => return Err(error),
                    Err(_) => return Ok(None),
                }
            }
        }
    }
    Ok(Some(current))
}

fn too_many_profile_output_symlinks(path: &Path) -> std::io::Error {
    std::io::Error::new(
        ErrorKind::InvalidInput,
        format!(
            "PGO profile output traverses too many symbolic links: {}",
            path.display()
        ),
    )
}

#[cfg(not(windows))]
fn path_starts_with(path: &Path, base: &Path) -> bool {
    path.starts_with(base)
}

#[cfg(windows)]
fn path_starts_with(path: &Path, base: &Path) -> bool {
    let mut path_components = path.components();
    base.components().all(|base_component| {
        path_components.next().is_some_and(|path_component| {
            path_component
                .as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(base_component.as_os_str().to_string_lossy().as_ref())
        })
    })
}

fn path_is_within_profile_root(path: &Path, root: &Path) -> bool {
    if path_starts_with(path, root) {
        return true;
    }

    let (Ok(path), Ok(root)) = (std::fs::canonicalize(path), std::fs::canonicalize(root)) else {
        return false;
    };
    path_starts_with(&path, &root)
}

fn lexical_absolute_path(path: &Path) -> std::io::Result<PathBuf> {
    let mut normalized = if path.is_absolute() {
        PathBuf::new()
    } else {
        std::env::current_dir()?
    };

    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                normalized = PathBuf::from(prefix.as_os_str());
            }
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(component) => normalized.push(component),
        }
    }
    Ok(normalized)
}

fn validate_custom_profile_directory(directory: &Path) -> std::io::Result<bool> {
    let original_exists = validate_directory_path_without_symlinks(directory, false)?;
    let normalized = lexical_absolute_path(directory)?;
    let normalized_exists = validate_directory_path_without_symlinks(&normalized, false)?;
    Ok(original_exists && normalized_exists)
}

fn create_directory_path_without_symlinks(directory: &Path) -> std::io::Result<()> {
    validate_directory_path_without_symlinks(directory, false)?;
    let normalized = lexical_absolute_path(directory)?;
    validate_directory_path_without_symlinks(&normalized, false)?;
    validate_directory_path_without_symlinks(directory, true).map(|_| ())
}

fn validate_directory_path_without_symlinks(
    directory: &Path,
    create_missing: bool,
) -> std::io::Result<bool> {
    let mut current = PathBuf::new();
    let mut exists = true;
    for component in directory.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Prefix(_)) {
            continue;
        }

        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Custom profile directory cannot traverse a symbolic link: {}",
                        current.display()
                    ),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "Custom profile directory component is not a directory: {}",
                        current.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound && create_missing => {
                std::fs::create_dir(&current)?;
            }
            Err(error) if error.kind() == ErrorKind::NotFound => exists = false,
            Err(error) => return Err(error),
        }
    }
    Ok(exists)
}

fn custom_profile_artifacts(directory: &Path, kind: ProfileKind) -> std::io::Result<Vec<PathBuf>> {
    let mut artifacts = vec![];
    for entry in WalkDir::new(directory).follow_links(false).min_depth(1) {
        let entry = map_walkdir_error(entry)?;
        if entry.file_type().is_file() && is_profile_artifact(entry.path(), directory, kind) {
            artifacts.push(entry.path().to_path_buf());
        }
    }
    Ok(artifacts)
}

fn map_walkdir_error(
    entry: Result<walkdir::DirEntry, walkdir::Error>,
) -> std::io::Result<walkdir::DirEntry> {
    entry.map_err(|error| {
        let message = error.to_string();
        error
            .into_io_error()
            .unwrap_or_else(|| std::io::Error::other(message))
    })
}

fn is_profile_artifact(path: &Path, directory: &Path, kind: ProfileKind) -> bool {
    let depth = path
        .strip_prefix(directory)
        .map(|relative| relative.components().count())
        .unwrap_or_default();

    match kind {
        ProfileKind::Pgo => {
            (path.extension() == Some(OsStr::new("profraw"))
                || path.file_name() == Some(OsStr::new(".profraw")))
                || (depth == 1 && is_merged_pgo_profile(path))
        }
        ProfileKind::Bolt => {
            (depth >= 2
                && (path.extension() == Some(OsStr::new("fdata"))
                    || path.file_name() == Some(OsStr::new(".fdata"))))
                || (depth == 2 && path.file_name() == Some(OsStr::new("merged.profdata")))
        }
    }
}

fn is_merged_pgo_profile(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    let Some(hash) = name
        .strip_prefix("merged-")
        .and_then(|name| name.strip_suffix(".profdata"))
    else {
        return false;
    };

    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

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
    let dotfile_name = format!(".{extension}");
    let extension = OsStr::new(extension);

    let walker = WalkDir::new(directory).into_iter();
    for entry in walker.flatten() {
        if entry.file_type().is_file()
            && (entry.path().extension() == Some(extension)
                || entry.file_name() == OsStr::new(&dotfile_name))
        {
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
