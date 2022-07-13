use crate::get_default_target;
use std::fmt::Write;
use std::process::{Command, Output};

#[derive(Debug, Default)]
struct CargoArgs {
    filtered: Vec<String>,
    contains_target: bool,
}

/// Run `cargo build --release` with the provided RUSTFLAGS and Cargo arguments.
pub fn build_with_flags(flags: String, cargo_args: Vec<String>) -> anyhow::Result<Output> {
    let mut rustflags = std::env::var("RUSTFLAGS").unwrap_or_default();
    write!(&mut rustflags, " {}", flags).unwrap();

    let parsed_args = parse_cargo_args(cargo_args);

    let mut command = Command::new("cargo");
    command.args(&[
        "build",
        "--release",
        "--message-format",
        "json-diagnostic-rendered-ansi",
    ]);

    // --target is passed to avoid instrumenting build scripts
    // See https://doc.rust-lang.org/rustc/profile-guided-optimization.html#a-complete-cargo-workflow
    if !parsed_args.contains_target {
        let default_target = get_default_target().map_err(|error| {
            anyhow::anyhow!(
                "Unable to find default target triple for your platform: {:?}",
                error
            )
        })?;
        command.args(&["--target", &default_target]);
    }

    for arg in parsed_args.filtered {
        command.arg(arg);
    }
    command.env("RUSTFLAGS", rustflags);
    Ok(command.output()?)
}

fn parse_cargo_args(cargo_args: Vec<String>) -> CargoArgs {
    let mut args = CargoArgs::default();

    let mut iterator = cargo_args.into_iter();
    while let Some(arg) = iterator.next() {
        match arg.as_str() {
            // Skip `--release`, we will pass it by ourselves.
            "--release" => {
                log::warn!("Do not pass `--release` manually, it will be added automatically by `cargo-fdo`");
            }
            // Skip `--message-format`, we need it to be JSON.
            "--message-format" => {
                iterator.next(); // skip flag value
            }
            "--target" => {
                args.contains_target = true;
                args.filtered.push(arg);
            }
            _ => args.filtered.push(arg),
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use crate::pgo::build::parse_cargo_args;

    #[test]
    fn test_parse_cargo_args_filter_release() {
        let args = parse_cargo_args(vec![
            "foo".to_string(),
            "--release".to_string(),
            "--bar".to_string(),
        ]);
        assert_eq!(args.filtered, vec!["foo".to_string(), "--bar".to_string()]);
    }

    #[test]
    fn test_parse_cargo_args_filter_message_format() {
        let args = parse_cargo_args(vec![
            "foo".to_string(),
            "--message-format".to_string(),
            "json".to_string(),
            "bar".to_string(),
        ]);
        assert_eq!(args.filtered, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn test_parse_cargo_args_find_target() {
        let args = parse_cargo_args(vec![
            "--target".to_string(),
            "x64".to_string(),
            "bar".to_string(),
        ]);
        assert_eq!(
            args.filtered,
            vec!["--target".to_string(), "x64".to_string(), "bar".to_string()]
        );
        assert!(args.contains_target);
    }
}
