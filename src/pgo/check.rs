use super::llvm_profdata_install_hint;
use crate::cli::cli_format_path;
use crate::pgo::env::find_pgo_env;
use anyhow::anyhow;
use colored::Colorize;

/// Check that binaries required for performing PGO can be found.
pub fn pgo_check() -> anyhow::Result<()> {
    match find_pgo_env() {
        Ok(env) => {
            println!(
                "{}: {} at {}",
                "[llvm-profdata]".bold(),
                "found".green(),
                cli_format_path(&env.llvm_profdata.display())
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
