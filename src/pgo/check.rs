use crate::env::pgo::find_pgo_env;
use anyhow::anyhow;
use colored::Colorize;

fn llvm_profdata_install_hint() -> String {
    format!(
        "Try installing `llvm-profdata` using `{}` or build LLVM manually and \
add its `bin` directory to PATH.",
        "rustup component add llvm-tools-preview".blue()
    )
}

/// Check that binaries required for performing PGO can be found.
pub fn pgo_check() -> anyhow::Result<()> {
    match find_pgo_env() {
        Ok(env) => {
            println!(
                "{}: {} at {}",
                "[llvm-profdata]".bold(),
                "found".green(),
                env.llvm_profdata.display()
            );
            Ok(())
        }
        Err(_) => {
            println!(
                "{}: {}",
                "[llvm-profdata]".bold(),
                "could not be found".red()
            );
            Err(anyhow!("{}", llvm_profdata_install_hint()))
        }
    }
}
