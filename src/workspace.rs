use cargo::core::Workspace;
use cargo::util::important_paths::find_root_manifest_for_wd;
use std::path::{Path, PathBuf};

/// Finds the root `Cargo.toml` file.
fn get_root_manifest() -> anyhow::Result<PathBuf> {
    let manifest = find_root_manifest_for_wd(&std::env::current_dir()?)
        .map_err(|error| anyhow::anyhow!("Cannot find root `Cargo.toml`: {:?}", error))?;
    Ok(manifest)
}

/// Find the Cargo workspace from the current working directory.
pub fn get_cargo_workspace(config: &cargo::Config) -> anyhow::Result<Workspace> {
    let manifest = get_root_manifest()?;
    let workspace = Workspace::new(&manifest, config)?;
    Ok(workspace)
}

pub fn get_pgo_directory(workspace: &Workspace) -> anyhow::Result<PathBuf> {
    get_workspace_directory(workspace, Path::new("pgo-profiles"))
}

pub fn get_bolt_directory(workspace: &Workspace) -> anyhow::Result<PathBuf> {
    get_workspace_directory(workspace, Path::new("bolt-profiles"))
}

fn get_workspace_directory(workspace: &Workspace, path: &Path) -> anyhow::Result<PathBuf> {
    let target_dir = workspace.target_dir();
    let pgo_dir = target_dir.join(path);
    pgo_dir.create_dir()?;
    Ok(pgo_dir.into_path_unlocked())
}
