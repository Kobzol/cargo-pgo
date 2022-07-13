use std::fmt::Write;
use std::process::{Command, Output};

pub fn build_with_flags(flags: String, cargo_args: Vec<String>) -> anyhow::Result<Output> {
    let mut rustflags = std::env::var("RUSTFLAGS").unwrap_or_default();
    write!(&mut rustflags, " {}", flags).unwrap();

    let trailing_args = filter_cargo_args(cargo_args);

    // TODO: pass --target to avoid instrumenting build scripts
    let mut command = Command::new("cargo");
    command.args(&[
        "build",
        "--release",
        "--message-format",
        "json-diagnostic-rendered-ansi",
    ]);
    for arg in trailing_args {
        command.arg(arg);
    }
    command.env("RUSTFLAGS", rustflags);
    Ok(command.output()?)
}

fn filter_cargo_args(cargo_args: Vec<String>) -> Vec<String> {
    let mut filtered_flags = vec![];

    let mut iterator = cargo_args.into_iter();
    while let Some(arg) = iterator.next() {
        match arg.as_str() {
            "--release" => {}
            "--message-format" => {
                iterator.next(); // skip flag value
            }
            _ => filtered_flags.push(arg),
        }
    }
    filtered_flags
}
