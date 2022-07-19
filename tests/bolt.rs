use crate::utils::{get_dir_files, init_cargo_project, run_command};

mod utils;

use crate::utils::OutputExt;

#[test]
fn test_instrument_create_bolt_profiles_dir() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["bolt", "instrument"])?.assert_ok();

    assert!(project.default_bolt_profile_dir().is_dir());

    Ok(())
}

#[test]
fn test_instrument_run_instrumented_binary() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["bolt", "instrument"])?.assert_ok();

    run_command(&project.main_binary())?;

    assert!(!get_dir_files(&project.default_bolt_profile_dir().join("foo"))?.is_empty());

    Ok(())
}
