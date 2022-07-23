use crate::utils::{get_dir_files, init_cargo_project, run_command};

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

    run_command(project.bolt_instrumented_binary())?;

    assert!(!get_dir_files(&project.default_bolt_profile_dir().join("foo"))?.is_empty());

    Ok(())
}

#[test]
fn test_bolt_optimize_no_profile() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["bolt", "optimize"])?.assert_error();

    Ok(())
}

#[test]
fn test_bolt_optimize() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project.run(&["bolt", "instrument"])?.assert_ok();
    run_command(&project.bolt_instrumented_binary())?;
    project.run(&["bolt", "optimize"])?.assert_ok();
    run_command(&project.bolt_optimized_binary())?;

    Ok(())
}
