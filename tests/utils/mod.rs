use cargo_fdo::get_default_target;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

pub struct CargoProject {
    name: String,
    pub dir: PathBuf,
    _tempdir: TempDir,
}

impl CargoProject {
    pub fn run(&self, args: &[&str]) -> anyhow::Result<Output> {
        let mut command = Command::new("cargo");
        command.arg("fdo");
        for arg in args {
            command.arg(arg);
        }
        command.current_dir(&self.dir);

        let path = std::env::var("PATH").unwrap_or_default();
        let path = format!("{}:{}", cargo_fdo_target_dir().display(), path);

        command.env("PATH", path);

        let output = command.output()?;
        Ok(output)
    }

    pub fn path<P: Into<PathBuf>>(&self, path: P) -> PathBuf {
        let path = path.into();
        self.dir.join(path)
    }

    pub fn main_binary(&self) -> PathBuf {
        self.dir
            .join("target")
            .join(get_default_target().unwrap())
            .join("release")
            .join(&self.name)
    }

    pub fn default_profile_dir(&self) -> PathBuf {
        self.path("target/pgo-profiles")
    }
}

impl Drop for CargoProject {
    fn drop(&mut self) {
        if std::thread::panicking() {
            // Do not delete the directory if an error has occurred
            std::mem::replace(&mut self._tempdir, TempDir::new().unwrap()).into_path();
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
            eprintln!("{}", self.stdout());
            eprintln!("{}", self.stderr());
            panic!("Output was not successful");
        }
        self
    }

    fn assert_error(self) -> Self {
        if self.status.success() {
            eprintln!("{}", self.stdout());
            eprintln!("{}", self.stderr());
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
        .args(&["new", "--bin", name])
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

fn cargo_fdo_target_dir() -> PathBuf {
    let mut target_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    target_dir.push("target");
    target_dir.push("debug");
    target_dir
}

pub fn run_command(path: &Path) -> anyhow::Result<()> {
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
