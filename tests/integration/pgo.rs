use crate::utils::{get_dir_files, init_cargo_project, run_command};
use cargo_pgo::get_default_target;

use crate::utils::OutputExt;

#[test]
fn test_instrument_create_pgo_profiles_dir() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["build"])?.assert_ok();

    assert!(project.default_pgo_profile_dir().is_dir());

    Ok(())
}

#[test]
fn test_instrument_run_instrumented_binary() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["build"])?.assert_ok();

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
fn test_build_optimize() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project.run(&["build"])?.assert_ok();
    run_command(&project.main_binary())?;
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}

#[test]
fn test_test_optimize() -> anyhow::Result<()> {
    let mut project = init_cargo_project()?;
    project.file(
        "src/lib.rs",
        r#"
pub fn foo(data: &[u32]) -> u32 {
    let mut sum = 0;
    for item in data {
        if *item > 5 {
            sum += *item;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use crate::foo;

    #[test]
    fn test_foo() {
        let data = &[0, 1, 2, 3, 4, 5, 6, 4, 3, 2, 1, 0];
        assert_eq!(foo(data), 6);
    }
}
"#,
    );

    project.run(&["test"])?.assert_ok();
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}

#[test]
fn test_run_optimize() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["run"])?.assert_ok();
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}

#[test]
fn test_bench_optimize() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["bench"])?.assert_ok();
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}

#[test]
fn test_instrument_optimize() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project.run(&["instrument"])?.assert_ok();
    run_command(&project.main_binary())?;
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}

#[test]
fn test_instrument_build() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    project.run(&["instrument", "build"])?.assert_ok();
    run_command(&project.main_binary())?;
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}

#[test]
fn test_instrument_test() -> anyhow::Result<()> {
    let mut project = init_cargo_project()?;
    project.file(
        "src/lib.rs",
        r#"
pub fn foo(data: &[u32]) -> u32 {
    let mut sum = 0;
    for item in data {
        if *item > 5 {
            sum += *item;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use crate::foo;

    #[test]
    fn test_foo() {
        let data = &[0, 1, 2, 3, 4, 5, 6, 4, 3, 2, 1, 0];
        assert_eq!(foo(data), 6);
    }
}
"#,
    );

    project.run(&["instrument", "test"])?.assert_ok();
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}

#[test]
fn test_instrument_run() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["instrument", "run"])?.assert_ok();
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}

#[test]
fn test_instrument_bench() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    project.run(&["instrument", "bench"])?.assert_ok();
    project.run(&["optimize"])?.assert_ok();
    run_command(&project.main_binary())?;

    Ok(())
}

#[test]
fn test_run_handle_input() -> anyhow::Result<()> {
    let mut project = init_cargo_project()?;
    project.file(
        "src/main.rs",
        r#"
use std::io::BufRead;

fn main() {
    let stdin = std::io::stdin();
    let line = stdin.lock().lines().next().unwrap().unwrap();
    println!("REPEAT: {}", line);
}
"#,
    );

    let output = project.run_with_input(&["run"], b"FOOBAR")?.assert_ok();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("REPEAT: FOOBAR"));

    project.run(&["optimize"])?.assert_ok();

    Ok(())
}

#[test]
fn test_respect_target_dir() -> anyhow::Result<()> {
    let project = init_cargo_project()?;
    let target_dir = project.path("custom-target-dir");
    let profile_dir = target_dir.join("pgo-profiles");

    project
        .run(&["build", "--", "--target-dir", target_dir.to_str().unwrap()])?
        .assert_ok();
    assert!(!project.default_pgo_profile_dir().is_dir());
    assert!(profile_dir.is_dir());

    run_command(
        target_dir
            .join(get_default_target()?)
            .join("release")
            .join("foo"),
    )?;

    assert!(!get_dir_files(&profile_dir)?.is_empty());

    Ok(())
}

#[test]
fn test_respect_profile() -> anyhow::Result<()> {
    if !version_check::is_min_version("1.57.0").unwrap_or(false) {
        println!("Skipping test_respect_profile because of too old rustc");
        return Ok(());
    }

    let project = init_cargo_project()?;

    project
        .run(&["build", "--", "--profile", "dev"])?
        .assert_ok();

    Ok(())
}

#[test]
fn test_respect_user_arg() -> anyhow::Result<()> {
    let mut project = init_cargo_project()?;
    project.file(
        "src/main.rs",
        r#"
fn main() {
    assert_eq!(std::env::args().skip(1).next().unwrap(), "--release".to_string());
}
"#,
    );

    project
        .run(&["run", "--", "-v", "--", "--release"])?
        .assert_ok();

    Ok(())
}

#[test]
fn test_respect_existing_rustflags() -> anyhow::Result<()> {
    let project = init_cargo_project()?;

    let output = project
        .cmd(&["build", "--", "-v"])
        .env("RUSTFLAGS", "-Ctarget-cpu=native")
        .run()?;
    assert!(output.stderr().contains("-Ctarget-cpu=native"));
    assert!(output.stderr().contains("-Cprofile-generate"));
    output.assert_ok();

    Ok(())
}

/// This only works for Rust 1.63+.
#[test]
fn test_respect_existing_rustflags_from_config() -> anyhow::Result<()> {
    if !version_check::is_min_version("1.63.0").unwrap_or(false) {
        return Ok(());
    }

    let mut project = init_cargo_project()?;
    project.file(
        ".cargo/config.toml",
        r#"
[build]
rustflags = ["-Ctarget-cpu=native"]
"#,
    );

    let output = project.cmd(&["build", "--", "-v"]).run()?;
    println!("{}", output.stderr());
    assert!(output.stderr().contains("-Ctarget-cpu=native"));
    assert!(output.stderr().contains("-Cprofile-generate"));
    output.assert_ok();

    Ok(())
}
