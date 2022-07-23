use crate::utils::{get_dir_files, init_cargo_project, run_command};

use crate::utils::OutputExt;

#[test]
fn test_instrument_create_pgo_profiles_dir() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["instrument"])?.assert_ok();

    assert!(project.default_pgo_profile_dir().is_dir());

    Ok(())
}

#[test]
fn test_instrument_run_instrumented_binary() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["instrument"])?.assert_ok();

    run_command(&project.main_binary())?;

    assert!(!get_dir_files(&project.default_pgo_profile_dir())?.is_empty());

    Ok(())
}

#[test]
fn test_optimize_no_profile() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["optimize"])?.assert_error();

    Ok(())
}

#[test]
fn test_optimize() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project.run(&["instrument"])?.assert_ok();
    run_command(&project.main_binary())?;
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}
