use crate::bolt::env::{find_llvm_bolt, find_merge_fdata};
use crate::bolt::llvm_bolt_install_hint;
use crate::cli::cli_format_path;
use crate::pgo::env::find_pgo_env;
use crate::pgo::llvm_profdata_install_hint;
use crate::workspace::get_cargo_workspace;
use anyhow::anyhow;
use colored::Colorize;
use std::path::PathBuf;

/// Check that binaries required for performing PGO and BOLT can be found.
pub fn environment_check() -> anyhow::Result<()> {
    let mut success = true;
    success &= check_rustc_version();
    success &= check_pgo_env();
    success &= check_bolt_env();

    if success {
        Ok(())
    } else {
        Err(anyhow!("Some requirements were not satisfied"))
    }
}

fn check_rustc_version() -> bool {
    match get_rustc_version() {
        Ok(version) => {
            if version >= semver::Version::new(1, 39, 0) {
                println!(
                    "{}: {} is recent enough",
                    "[rustc version]".bold().green(),
                    version.to_string().blue(),
                );
                true
            } else {
                println!(
                    "{}: {} is too old",
                    "[rustc version]".bold().red(),
                    version.to_string().blue(),
                );
                false
            }
        }
        Err(error) => {
            println!(
                "{}: cannot determine version ({})",
                "[rustc version]".bold().red(),
                error
            );
            false
        }
    }
}

fn check_binary_available(name: &str, resolved: anyhow::Result<PathBuf>, hint: &str) -> bool {
    match resolved {
        Ok(path) => {
            println!(
                "{}: found at {}",
                format!("[{}]", name).bold().green(),
                cli_format_path(&path.display())
            );
            true
        }
        Err(_) => {
            println!(
                "{}: could not be found ({})",
                format!("[{}]", name).bold().red(),
                hint
            );
            false
        }
    }
}

fn check_pgo_env() -> bool {
    check_binary_available(
        "llvm-profdata",
        find_pgo_env().map(|env| env.llvm_profdata),
        &llvm_profdata_install_hint(),
    )
}

fn check_bolt_env() -> bool {
    let hint = llvm_bolt_install_hint();

    let llvm_bolt = check_binary_available("llvm-bolt", find_llvm_bolt(), hint);
    let merge_fdata = check_binary_available("merge-fdata", find_merge_fdata(), hint);
    llvm_bolt && merge_fdata
}

fn get_rustc_version() -> anyhow::Result<semver::Version> {
    let config = cargo::Config::default()?;
    let workspace = get_cargo_workspace(&config)?;
    let rustc = workspace.config().load_global_rustc(Some(&workspace))?;
    Ok(rustc.version)
}
