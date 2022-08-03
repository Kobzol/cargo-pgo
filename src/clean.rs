use crate::workspace::{get_bolt_directory, get_cargo_workspace, get_pgo_directory};

pub fn clean_artifacts() -> anyhow::Result<()> {
    let config = cargo::Config::default()?;
    let workspace = get_cargo_workspace(&config)?;

    let pgo_dir = get_pgo_directory(&workspace)?;
    let res = std::fs::remove_dir_all(pgo_dir);

    let bolt_dir = get_bolt_directory(&workspace)?;
    std::fs::remove_dir_all(bolt_dir).and(res)?;

    Ok(())
}
