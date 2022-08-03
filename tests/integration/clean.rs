use crate::utils::{init_cargo_project, run_command};

use crate::utils::OutputExt;

#[test]
fn test_clean_pgo_dir() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["build"])?.assert_ok();

    run_command(project.main_binary())?;

    project.run(&["clean"])?.assert_ok();
    assert!(!project.default_pgo_profile_dir().exists());

    Ok(())
}

#[test]
#[ignore]
fn test_clean_bolt_dir() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["bolt", "instrument"])?.assert_ok();

    run_command(project.bolt_instrumented_binary())?;

    project.run(&["clean"])?.assert_ok();
    assert!(!project.default_bolt_profile_dir().exists());

    Ok(())
}
