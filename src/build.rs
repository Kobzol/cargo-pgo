use crate::get_default_target;
use cargo_metadata::{Artifact, Message, MessageIter};
use std::collections::HashMap;
use std::fmt::Write as WriteFmt;
use std::io::{BufReader, Write};
use std::process::{Child, ChildStdout, Command, Stdio};

#[derive(Debug, Default)]
struct CargoArgs {
    filtered: Vec<String>,
    contains_target: bool,
}

enum ReleaseMode {
    AddRelease,
    NoRelease,
}

pub struct RunningCargo {
    child: Child,
    message_iter: MessageIter<BufReader<ChildStdout>>,
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

fn parse_cargo_args(cargo_args: Vec<String>) -> CargoArgs {
    let mut args = CargoArgs::default();

    let mut iterator = cargo_args.into_iter();
    while let Some(arg) = iterator.next() {
        match arg.as_str() {
            // Skip `--release`, we will pass it by ourselves.
            "--release" => {
                log::warn!("Do not pass `--release` manually, it will be added automatically by `cargo-pgo`");
            }
            // Skip `--message-format`, we need it to be JSON.
            "--message-format" => {
                log::warn!("Do not pass `--message-format` manually, it will be added automatically by `cargo-pgo`");
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
    use crate::build::parse_cargo_args;

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
