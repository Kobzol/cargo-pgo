use crate::cli::cli_format_path;
use crate::pgo::env::find_pgo_env;
use crate::pgo::llvm_profdata_install_hint;
use crate::workspace::get_cargo_workspace;
use anyhow::anyhow;
use colored::Colorize;

/// Check that binaries required for performing PGO and BOLT can be found.
pub fn environment_check() -> anyhow::Result<()> {
    let mut success = true;
    success &= check_rustc_version();
    success &= check_pgo_env();

    if success {
        Ok(())
    } else {
        Err(anyhow!("Some dependencies were not satisfied"))
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

fn check_pgo_env() -> bool {
    match find_pgo_env() {
        Ok(env) => {
            println!(
                "{}: found at {}",
                "[llvm-profdata]".bold().green(),
                cli_format_path(&env.llvm_profdata.display())
            );
            true
        }
        Err(_) => {
            println!("{}: could not be found", "[llvm-profdata]".bold().red(),);
            log::warn!("{}", llvm_profdata_install_hint());
            false
        }
    }
}

fn get_rustc_version() -> anyhow::Result<semver::Version> {
    let config = cargo::Config::default()?;
    let workspace = get_cargo_workspace(&config)?;
    let rustc = workspace.config().load_global_rustc(Some(&workspace))?;
    Ok(rustc.version)
}
