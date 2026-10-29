use anyhow::{Context, bail, ensure};
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    workloads: Vec<Workload>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Workload {
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, Option<String>>,
    #[serde(default, deserialize_with = "present_path")]
    cwd: Option<PathBuf>,
    #[serde(default, deserialize_with = "present_path")]
    stdin: Option<PathBuf>,
    #[serde(default = "one")]
    repeat: u64,
}

fn one() -> u64 {
    1
}

fn present_path<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<PathBuf>, D::Error> {
    PathBuf::deserialize(input).map(Some)
}

pub(super) struct PreparedWorkload {
    pub args: Vec<String>,
    pub env: BTreeMap<String, Option<String>>,
    pub cwd: PathBuf,
    pub stdin: Option<PathBuf>,
    pub repeat: u64,
}

pub(super) fn read_plan(path: &Path) -> anyhow::Result<Vec<PreparedWorkload>> {
    let path = std::fs::canonicalize(path)
        .with_context(|| format!("Cannot locate workload plan {}", path.display()))?;
    let source = std::fs::read(&path)?;
    let value: Value = serde_json::from_slice(&source).context("Invalid workload plan")?;
    ensure!(value.is_object(), "The workload plan must be an object");
    if let Some(Value::Array(workloads)) = value.get("workloads") {
        ensure!(
            workloads.iter().all(Value::is_object),
            "Workloads must be objects"
        );
    }
    let plan: Plan = serde_json::from_value(value).context("Invalid workload plan")?;
    ensure!(!plan.workloads.is_empty(), "The workload plan is empty");
    let directory = path.parent().context("The plan has no parent directory")?;
    plan.workloads
        .into_iter()
        .enumerate()
        .map(|(index, workload)| {
            prepare(workload, directory)
                .with_context(|| format!("Invalid workload at index {index}"))
        })
        .collect()
}

fn prepare(workload: Workload, directory: &Path) -> anyhow::Result<PreparedWorkload> {
    ensure!(workload.repeat > 0, "repeat must be positive");
    for argument in &workload.args {
        ensure!(!argument.contains('\0'), "Arguments cannot contain NUL");
    }
    for (key, value) in &workload.env {
        ensure!(
            !key.is_empty() && !key.contains(['=', '\0']),
            "Invalid environment variable name"
        );
        let reserved = if cfg!(windows) {
            key.eq_ignore_ascii_case("LLVM_PROFILE_FILE")
        } else {
            key == "LLVM_PROFILE_FILE"
        };
        ensure!(!reserved, "LLVM_PROFILE_FILE is managed by training");
        if let Some(value) = value {
            ensure!(
                !value.contains('\0'),
                "Environment values cannot contain NUL"
            );
        }
    }
    let cwd = directory.join(workload.cwd.unwrap_or_else(|| PathBuf::from(".")));
    let cwd = std::fs::canonicalize(&cwd)
        .with_context(|| format!("Cannot locate working directory {}", cwd.display()))?;
    ensure!(cwd.is_dir(), "Working directory is not a directory");
    let stdin = workload
        .stdin
        .map(|path| -> anyhow::Result<PathBuf> {
            let path = directory.join(path);
            let path = std::fs::canonicalize(&path)
                .with_context(|| format!("Cannot locate stdin file {}", path.display()))?;
            if !path.is_file() {
                bail!("Stdin path is not a regular file");
            }
            File::open(&path).context("Cannot read stdin file")?;
            Ok(path)
        })
        .transpose()?;
    Ok(PreparedWorkload {
        args: workload.args,
        env: workload.env,
        cwd,
        stdin,
        repeat: workload.repeat,
    })
}
