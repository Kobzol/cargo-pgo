use cargo_pgo::get_default_target;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

pub struct CargoProject {
    name: String,
    pub dir: PathBuf,
    _tempdir: TempDir,
}

impl CargoProject {
    pub fn run(&self, args: &[&str]) -> anyhow::Result<Output> {
        self.run_with_input(args, &[])
    }

    pub fn cmd(&self, args: &[&str]) -> Cmd {
        Cmd::default()
            .cwd(&self.dir)
            .args(&["cargo", "pgo"])
            .args(args)
    }

    pub fn run_with_input(&self, args: &[&str], stdin: &[u8]) -> anyhow::Result<Output> {
        let mut command = Command::new("cargo");
        command.arg("pgo");
        for arg in args {
            command.arg(arg);
        }
        command.current_dir(&self.dir);
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let path = std::env::var("PATH").unwrap_or_default();
        let path = format!("{}:{}", cargo_pgo_target_dir().display(), path);

        command.env("PATH", path);

        let mut child = command.spawn()?;
        {
            let mut child_stdin = child.stdin.take().unwrap();
            child_stdin.write_all(stdin)?;
        }

        let output = child.wait_with_output()?;
        Ok(output)
    }

    pub fn path<P: Into<PathBuf>>(&self, path: P) -> PathBuf {
        let path = path.into();
        self.dir.join(path)
    }

    pub fn file<P: AsRef<Path>>(&mut self, path: P, code: &str) -> &mut Self {
        let path = self.path(path.as_ref());
        std::fs::create_dir_all(path.parent().unwrap()).expect("Cannot create parent directory");
        std::fs::write(path, code).expect("Could not write project file");
        self
    }

    pub fn main_binary(&self) -> PathBuf {
        self.dir
            .join("target")
            .join(get_default_target().unwrap())
            .join("release")
            .join(&self.name)
    }

    pub fn bolt_instrumented_binary(&self) -> PathBuf {
        let binary = self.main_binary();
        binary.with_file_name(format!(
            "{}-bolt-instrumented",
            binary.file_stem().unwrap().to_str().unwrap()
        ))
    }

    pub fn bolt_optimized_binary(&self) -> PathBuf {
        let binary = self.main_binary();
        binary.with_file_name(format!(
            "{}-bolt-optimized",
            binary.file_stem().unwrap().to_str().unwrap()
        ))
    }

    pub fn default_pgo_profile_dir(&self) -> PathBuf {
        self.path("target/pgo-profiles")
    }

    pub fn default_bolt_profile_dir(&self) -> PathBuf {
        self.path("target/bolt-profiles")
    }
}

impl Drop for CargoProject {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // Do not delete the directory if an error has occurred
            let path = std::mem::replace(&mut self._tempdir, TempDir::new().unwrap()).into_path();
            eprintln!("Directory of failed test located at: {}", path.display());
        }
    }
}

#[derive(Default)]
pub struct Cmd {
    arguments: Vec<String>,
    cwd: Option<PathBuf>,
    stdin: Vec<u8>,
    env: HashMap<String, String>,
}

impl Cmd {
    pub fn run(self) -> anyhow::Result<Output> {
        let mut command = Command::new(&self.arguments[0]);
        for arg in &self.arguments[1..] {
            command.arg(arg);
        }
        if let Some(cwd) = self.cwd {
            command.current_dir(&cwd);
        }
        command.stdin(Stdio::piped());
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());

        let path = std::env::var("PATH").unwrap_or_default();
        let path = format!("{}:{}", cargo_pgo_target_dir().display(), path);

        command.env("PATH", path);

        for (key, value) in self.env {
            command.env(key, value);
        }

        let mut child = command.spawn()?;
        {
            let mut child_stdin = child.stdin.take().unwrap();
            child_stdin.write_all(&self.stdin)?;
        }

        let output = child.wait_with_output()?;
        Ok(output)
    }

    pub fn args(mut self, args: &[&str]) -> Self {
        self.arguments.extend(args.iter().map(|s| s.to_string()));
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.insert(key.to_string(), value.to_string());
        self
    }

    pub fn cwd(self, cwd: &Path) -> Self {
        Self {
            cwd: Some(cwd.to_path_buf()),
            ..self
        }
    }
}

pub trait OutputExt {
    fn assert_ok(self) -> Self;
    fn assert_error(self) -> Self;

    fn stdout(&self) -> String;
    fn stderr(&self) -> String;
}

impl OutputExt for Output {
    fn assert_ok(self) -> Self {
        if !self.status.success() {
            eprintln!("Stdout: {}", self.stdout());
            eprintln!("Stderr: {}", self.stderr());
            panic!("Output was not successful");
        }
        self
    }

    fn assert_error(self) -> Self {
        if self.status.success() {
            eprintln!("Stdout: {}", self.stdout());
            eprintln!("Stderr: {}", self.stderr());
            panic!("Output was successful");
        }
        self
    }

    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout).to_string()
    }

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.stderr).to_string()
    }
}

pub fn init_cargo_project() -> anyhow::Result<CargoProject> {
    let dir = tempfile::tempdir()?;

    let name = "foo";
    let status = Command::new("cargo")
        .args(["new", "--bin", name])
        .current_dir(dir.path())
        .status()?;
    assert!(status.success());

    let path = dir.path().join(name);

    println!("Created Cargo project {} at {}", name, path.display());

    Ok(CargoProject {
        name: name.to_string(),
        dir: path,
        _tempdir: dir,
    })
}

fn cargo_pgo_target_dir() -> PathBuf {
    let mut target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    target_dir.push("target");
    target_dir.push("debug");
    target_dir
}

pub fn run_command<S: AsRef<OsStr>>(path: S) -> anyhow::Result<()> {
    let status = Command::new(path).status()?;
    match status.success() {
        true => Ok(()),
        false => Err(anyhow::anyhow!("Command failed")),
    }
}

pub fn get_dir_files(directory: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = vec![];

    for entry in directory.read_dir()? {
        files.push(entry?.path());
    }

    Ok(files)
}
