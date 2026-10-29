use std::path::Path;
use std::process::Command;

#[test]
fn training_plan_cli() {
    let python = std::env::var_os("PYTHON").unwrap_or_else(|| {
        if cfg!(windows) {
            "python".into()
        } else {
            "python3".into()
        }
    });
    let output = Command::new(python)
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/training_plan.py"))
        .args(["--binary", env!("CARGO_BIN_EXE_cargo-pgo")])
        .output()
        .expect("run the Python 3 CLI integration suite (override with PYTHON)");
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
