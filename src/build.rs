use crate::get_default_target;
use cargo_metadata::{Artifact, Message, MessageIter};
use std::collections::HashMap;
use std::fmt::Write as WriteFmt;
use std::io::{BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};

#[derive(Debug, Default)]
pub struct CargoArgs {
    pub filtered: Vec<String>,
    pub contains_target: bool,
    pub target_dir: Option<PathBuf>,
}

enum ReleaseMode {
    AddRelease,
    NoRelease,
}

pub struct RunningCargo {
    child: Child,
    message_iter: MessageIter<BufReader<ChildStdout>>,
}

#[derive(Debug, Copy, Clone, clap::ValueEnum)]
pub enum CargoCommand {
    Build,
    Test,
    Run,
    Bench,
}

impl CargoCommand {
    pub fn to_str(&self) -> &str {
        match self {
            CargoCommand::Build => "build",
            CargoCommand::Test => "test",
            CargoCommand::Run => "run",
            CargoCommand::Bench => "bench",
        }
    }
}

impl RunningCargo {
    pub fn messages(&mut self) -> &mut MessageIter<BufReader<ChildStdout>> {
        &mut self.message_iter
    }

    pub fn check_status(mut self) -> anyhow::Result<()> {
        let status = self.child.wait()?;
        if !status.success() {
            return Err(anyhow::anyhow!(
                "Cargo finished with an error ({})",
                status.code().unwrap_or(-1),
            ));
        }
        Ok(())
    }
}

/// Start a `cargo` command in release mode with the provided RUSTFLAGS and Cargo arguments.
pub fn cargo_command_with_flags(
    command: CargoCommand,
    flags: &str,
    cargo_args: Vec<String>,
) -> anyhow::Result<RunningCargo> {
    let mut rustflags = std::env::var("RUSTFLAGS").unwrap_or_default();
    write!(&mut rustflags, " {}", flags).unwrap();

    let mut env = HashMap::default();
    env.insert("RUSTFLAGS".to_string(), rustflags);

    let release_mode = match command {
        CargoCommand::Bench => ReleaseMode::NoRelease,
        _ => ReleaseMode::AddRelease,
    };

    let mut child = cargo_command(command, cargo_args, env, release_mode)?;
    let stdout = child.stdout.take().unwrap();
    Ok(RunningCargo {
        child,
        message_iter: Message::parse_stream(BufReader::new(stdout)),
    })
}

/// Spawn `cargo` command in release mode with the provided env variables and Cargo arguments.
fn cargo_command(
    cargo_cmd: CargoCommand,
    cargo_args: Vec<String>,
    env: HashMap<String, String>,
    release_mode: ReleaseMode,
) -> anyhow::Result<Child> {
    let parsed_args = parse_cargo_args(cargo_args);

    let mut command = Command::new("cargo");
    command.args(&[
        cargo_cmd.to_str(),
        "--message-format",
        "json-diagnostic-rendered-ansi",
    ]);
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::inherit());

    match release_mode {
        ReleaseMode::AddRelease => {
            command.arg("--release");
        }
        ReleaseMode::NoRelease => {}
    }

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
    for (key, value) in env {
        command.env(key, value);
    }
    log::debug!("Executing cargo command: {:?}", command);
    Ok(command.spawn()?)
}

pub fn parse_cargo_args(cargo_args: Vec<String>) -> CargoArgs {
    let mut args = CargoArgs::default();

    let mut iterator = cargo_args.into_iter();
    while let Some(arg) = iterator.next() {
        match arg.as_str() {
            // Skip `--release`, we will pass it by ourselves.
            "--release" => {
                log::warn!("Do not pass `--release` manually, it will be added automatically by `cargo-pgo`");
            }
            _ => {
                if get_key_value("--message-format", arg.as_str(), &mut iterator).is_some() {
                    // Skip `--message-format`, we need it to be JSON.
                    log::warn!("Do not pass `--message-format` manually, it will be added automatically by `cargo-pgo`");
                } else if let Some(value) = get_key_value("--target", arg.as_str(), &mut iterator) {
                    // Check if `--target` was passed
                    args.contains_target = true;
                    args.filtered.push("--target".to_string());
                    if let Some(value) = value {
                        args.filtered.push(value);
                    }
                } else if let Some(value) =
                    get_key_value("--target-dir", arg.as_str(), &mut iterator)
                {
                    // Extract `--target-dir`
                    args.target_dir = value.clone().map(PathBuf::from);
                    args.filtered.push("--target-dir".to_string());
                    if let Some(value) = value {
                        args.filtered.push(value);
                    }
                } else {
                    args.filtered.push(arg);
                }
            }
        }
    }
    args
}

/// Parses a `--key=<value>` or `--key <value>` key/value CLI argument pair.
fn get_key_value<Iter: Iterator<Item = String>>(
    key: &str,
    arg: &str,
    iter: &mut Iter,
) -> Option<Option<String>> {
    // A different argument was passed, nothing to be seen here
    if !arg.starts_with(key) {
        return None;
    }
    // --key was passed exactly, we should extract the value from the following argument
    if arg == key {
        let value = iter.next();
        return Some(value);
    }

    // --key<suffix> was passed, let's try to split it into --key=value
    if let Some((parsed_key, value)) = arg.split_once('=') {
        // if --keyfoo=value was passed, ignore it
        if parsed_key == key {
            return Some(Some(value.to_string()));
        }
    }

    None
}

pub fn handle_metadata_message(message: Message) {
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    write_metadata_message(&mut stdout, message);
    stdout.flush().unwrap();
}

fn write_metadata_message<W: Write>(mut stream: W, message: Message) {
    match message {
        Message::TextLine(line) => {
            log::debug!("TextLine {}", line);
            writeln!(stream, "{}", line).unwrap();
        }
        Message::CompilerMessage(message) => {
            log::debug!("CompilerMessage {}", message);
            write!(
                stream,
                "{}",
                message.message.rendered.unwrap_or(message.message.message)
            )
            .unwrap();
        }
        _ => {
            log::debug!("Metadata output: {:?}", message);
        }
    }
}

/// Returns a user-friendly name of an artifact kind.
pub fn get_artifact_kind(artifact: &Artifact) -> &str {
    for kind in &artifact.target.kind {
        match kind.as_str() {
            "bin" => {
                return "binary";
            }
            "bench" => {
                return "benchmark";
            }
            "example" => {
                return "example";
            }
            _ => {}
        }
    }
    "artifact"
}

#[cfg(test)]
mod tests {
    use crate::build::{get_key_value, parse_cargo_args};
    use std::path::PathBuf;

    #[test]
    fn parse_cargo_args_filter_release() {
        let args = parse_cargo_args(vec![
            "foo".to_string(),
            "--release".to_string(),
            "--bar".to_string(),
        ]);
        assert_eq!(args.filtered, vec!["foo".to_string(), "--bar".to_string()]);
    }

    #[test]
    fn parse_cargo_args_filter_message_format() {
        let args = parse_cargo_args(vec![
            "foo".to_string(),
            "--message-format".to_string(),
            "json".to_string(),
            "bar".to_string(),
        ]);
        assert_eq!(args.filtered, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn parse_cargo_args_filter_message_format_equals() {
        let args = parse_cargo_args(vec![
            "foo".to_string(),
            "--message-format=json".to_string(),
            "bar".to_string(),
        ]);
        assert_eq!(args.filtered, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn parse_cargo_args_find_target() {
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

    #[test]
    fn parse_cargo_args_find_target_equals() {
        let args = parse_cargo_args(vec!["--target=x64".to_string(), "bar".to_string()]);
        assert_eq!(
            args.filtered,
            vec!["--target".to_string(), "x64".to_string(), "bar".to_string()]
        );
        assert!(args.contains_target);
    }

    #[test]
    fn parse_cargo_args_target_dir() {
        let args = parse_cargo_args(vec![
            "--target-dir".to_string(),
            "/tmp/foo".to_string(),
            "bar".to_string(),
        ]);
        assert_eq!(
            args.filtered,
            vec![
                "--target-dir".to_string(),
                "/tmp/foo".to_string(),
                "bar".to_string()
            ]
        );
        assert_eq!(args.target_dir, Some(PathBuf::from("/tmp/foo")));
    }

    #[test]
    fn parse_cargo_args_target_dir_equals() {
        let args = parse_cargo_args(vec!["--target-dir=/tmp/foo".to_string(), "bar".to_string()]);
        assert_eq!(
            args.filtered,
            vec![
                "--target-dir".to_string(),
                "/tmp/foo".to_string(),
                "bar".to_string()
            ]
        );
        assert_eq!(args.target_dir, Some(PathBuf::from("/tmp/foo")));
    }

    #[test]
    fn get_key_value_wrong_key() {
        assert_eq!(
            get_key_value("--foo", "--bar", &mut std::iter::empty()),
            None
        );
    }

    #[test]
    fn get_key_value_exact_key_missing_value() {
        assert_eq!(
            get_key_value("--foo", "--foo", &mut std::iter::empty()),
            Some(None)
        );
    }

    #[test]
    fn get_key_value_exact_key_value() {
        assert_eq!(
            get_key_value("--foo", "--foo", &mut vec!["bar".to_string()].into_iter()),
            Some(Some("bar".to_string()))
        );
    }

    #[test]
    fn get_key_value_equals_wrong_prefix() {
        assert_eq!(
            get_key_value("--foo", "--foox=bar", &mut std::iter::empty()),
            None
        );
    }

    #[test]
    fn get_key_value_equals() {
        assert_eq!(
            get_key_value("--foo", "--foo=bar", &mut std::iter::empty()),
            Some(Some("bar".to_string()))
        );
    }
}
