use crate::utils::{get_dir_files, init_cargo_project, run_command};

use crate::utils::OutputExt;

#[test]
#[ignore]
fn test_bolt_instrument_create_bolt_profiles_dir() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["bolt", "build"])?.assert_ok();

    assert!(project.default_bolt_profile_dir().is_dir());

    Ok(())
}

#[test]
#[ignore]
fn test_bolt_instrument_run_instrumented_binary() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["bolt", "build"])?.assert_ok();

    run_command(project.bolt_instrumented_binary())?;

    assert!(!get_dir_files(&project.default_bolt_profile_dir().join("foo"))?.is_empty());

    Ok(())
}

#[test]
#[ignore]
fn test_bolt_optimize_no_profile() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["bolt", "optimize"])?.assert_ok();
    assert!(project.bolt_optimized_binary().is_file());

    Ok(())
}

#[test]
#[ignore]
fn test_bolt_optimize() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project.run(&["bolt", "build"])?.assert_ok();
    run_command(project.bolt_instrumented_binary())?;
    project.run(&["bolt", "optimize"])?.assert_ok();
    run_command(project.bolt_optimized_binary())?;

    Ok(())
}

#[test]
#[ignore]
fn test_bolt_pgo_optimize() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project.run(&["build"])?.assert_ok();
    run_command(project.main_binary())?;

    project.run(&["bolt", "build", "--with-pgo"])?.assert_ok();
    run_command(project.bolt_instrumented_binary())?;

    project
        .run(&["bolt", "optimize", "--with-pgo"])?
        .assert_ok();
    run_command(project.bolt_optimized_binary())?;

    Ok(())
}

#[test]
#[ignore]
fn test_bolt_instrument_bolt_args_empty() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project
        .run(&["bolt", "build", "--bolt-args", ""])?
        .assert_ok();

    Ok(())
}

#[test]
#[ignore]
fn test_bolt_instrument_bolt_args_multiple() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project
        .run(&[
            "bolt",
            "build",
            "--bolt-args",
            "-update-debug-sections --instrument-calls",
        ])?
        .assert_ok();

    Ok(())
}

#[test]
#[ignore]
fn test_bolt_instrument_bolt_args_invalid() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project
        .run(&["bolt", "build", "--bolt-args", "-foo"])?
        .assert_error();

    Ok(())
}

#[test]
#[ignore]
fn test_bolt_instrument_bolt_args_with_cargo_args() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project
        .run(&[
            "bolt",
            "build",
            "--bolt-args",
            "-update-debug-sections",
            "--",
            "--locked",
        ])?
        .assert_ok();

    Ok(())
}

#[test]
#[ignore]
fn test_bolt_optimize_bolt_args() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project.run(&["bolt", "build"])?.assert_ok();
    run_command(project.bolt_instrumented_binary())?;

    project
        .run(&[
            "bolt",
            "optimize",
            "--bolt-args",
            "-update-debug-sections --instrument-calls",
        ])?
        .assert_ok();

    Ok(())
}
