mod plan;
mod profiles;

use crate::build::{CargoCommand, cargo_command_with_rustflags, handle_metadata_message};
use crate::get_cargo_ctx;
use anyhow::{Context, bail, ensure};
use cargo_metadata::{Message, TargetKind};
use plan::PreparedWorkload;
use profiles::AttemptProfiles;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(clap::Parser, Debug)]
pub struct PgoTrainArgs {
    /// JSON workload plan to replay.
    #[arg(long)]
    plan: PathBuf,
    /// Write per-attempt results and invocation totals as JSON.
    #[arg(long)]
    report: PathBuf,
    /// Override the PGO profile directory.
    #[arg(long)]
    profiles_dir: Option<PathBuf>,
    /// Additional arguments for the instrumented Cargo build.
    #[arg(last(true))]
    cargo_args: Vec<String>,
}

#[derive(Default, Serialize)]
struct Report {
    attempts: Vec<Attempt>,
    succeeded: u64,
    failed: u64,
    profiles: u64,
}

#[derive(Serialize)]
struct Attempt {
    workload: usize,
    repetition: u64,
    success: bool,
    profiles: u64,
    error: Option<String>,
}

impl Report {
    fn record(&mut self, workload: usize, repetition: u64, result: anyhow::Result<u64>) {
        let (success, profiles, error) = match result {
            Ok(count) => {
                self.succeeded += 1;
                self.profiles += count;
                (true, count, None)
            }
            Err(error) => {
                self.failed += 1;
                log::error!("Workload {workload}, repetition {repetition}: {error:#}");
                (false, 0, Some(format!("{error:#}")))
            }
        };
        self.attempts.push(Attempt {
            workload,
            repetition,
            success,
            profiles,
            error,
        });
    }

    fn write(&self, path: &Path) -> anyhow::Result<()> {
        let mut file = File::create(path)
            .with_context(|| format!("Cannot create report {}", path.display()))?;
        serde_json::to_writer_pretty(&mut file, self)
            .context("Cannot serialize training report")?;
        file.write_all(b"\n")?;
        file.flush().context("Cannot flush training report")
    }
}

pub fn pgo_train(args: PgoTrainArgs) -> anyhow::Result<()> {
    let mut report = Report::default();
    let result =
        plan::read_plan(&args.plan).and_then(|workloads| train(&args, workloads, &mut report));
    if let Err(error) = report.write(&args.report) {
        if let Err(training_error) = result {
            log::error!("Training also failed: {training_error:#}");
        }
        return Err(error);
    }
    result
}

fn train(
    args: &PgoTrainArgs,
    workloads: Vec<PreparedWorkload>,
    report: &mut Report,
) -> anyhow::Result<()> {
    let ctx = get_cargo_ctx(&args.cargo_args, args.profiles_dir.clone(), None)?;
    let profiles = std::path::absolute(ctx.get_pgo_directory()?)?;
    let executable = build(&args.cargo_args, &profiles)?;
    for (index, workload) in workloads.iter().enumerate() {
        for repetition in 0..workload.repeat {
            let result = execute(&executable, workload, &profiles);
            report.record(index, repetition, result);
        }
    }
    ensure!(
        report.failed == 0,
        "{} training attempts failed",
        report.failed
    );
    Ok(())
}

fn build(cargo_args: &[String], profiles: &Path) -> anyhow::Result<PathBuf> {
    let flags = vec![format!("-Cprofile-generate={}", profiles.display())];
    let mut cargo = cargo_command_with_rustflags(CargoCommand::Build, flags, cargo_args.to_vec())?;
    let mut executables = BTreeMap::new();
    let mut build_failed = false;
    for message in cargo.messages() {
        match message? {
            Message::CompilerArtifact(artifact) => {
                if !artifact.profile.test
                    && artifact
                        .target
                        .kind
                        .iter()
                        .any(|kind| matches!(kind, TargetKind::Bin | TargetKind::Example))
                {
                    if let Some(executable) = artifact.executable {
                        let target = (
                            artifact.package_id.repr,
                            artifact.target.name,
                            artifact.target.kind.contains(&TargetKind::Example),
                        );
                        executables.insert(target, executable.into_std_path_buf());
                    }
                }
            }
            Message::BuildFinished(finished) => build_failed |= !finished.success,
            message => handle_metadata_message(message),
        }
    }
    cargo.check_status()?;
    ensure!(!build_failed, "Instrumented build failed");
    ensure!(
        executables.len() == 1,
        "Training needs exactly one executable; Cargo selected {}",
        executables.len()
    );
    let executable = executables.into_values().next().unwrap();
    which::which(&executable)
        .with_context(|| format!("Cannot use training executable {}", executable.display()))
}

fn execute(
    executable: &Path,
    workload: &PreparedWorkload,
    destination: &Path,
) -> anyhow::Result<u64> {
    let profiles = AttemptProfiles::new(destination)?;
    let stdin = match &workload.stdin {
        Some(path) => Stdio::from(File::open(path).context("Cannot reopen workload stdin")?),
        None => Stdio::null(),
    };
    let mut command = Command::new(executable);
    command
        .args(&workload.args)
        .current_dir(&workload.cwd)
        .stdin(stdin)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    for (key, value) in &workload.env {
        match value {
            Some(value) => {
                command.env(key, value);
            }
            None => {
                command.env_remove(key);
            }
        }
    }
    command.env("LLVM_PROFILE_FILE", profiles.pattern());
    let status = command
        .status()
        .context("Cannot launch training workload")?;
    if !status.success() {
        bail!("Training workload exited with {status}");
    }
    profiles.publish()
}
