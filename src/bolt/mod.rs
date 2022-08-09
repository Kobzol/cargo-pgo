use crate::pgo::optimize::{get_pgo_env, prepare_pgo_optimization_flags};
use crate::workspace::CargoContext;
use cargo_metadata::Artifact;
use std::path::{Path, PathBuf};

pub mod cli;
pub(crate) mod env;
pub mod instrument;
pub mod optimize;

pub fn llvm_bolt_install_hint() -> &'static str {
    "Build LLVM with BOLT and add its `bin` directory to PATH."
}

fn bolt_common_rustflags() -> &'static str {
    "-C link-args=-Wl,-q"
}

fn bolt_pgo_rustflags(ctx: &CargoContext, with_pgo: bool) -> anyhow::Result<String> {
    let flags = match with_pgo {
        true => {
            let pgo_env = get_pgo_env()?;
            let pgo_dir = ctx.get_pgo_directory()?;
            let flags = prepare_pgo_optimization_flags(&pgo_env, &pgo_dir)?;
            format!("{} {}", flags, bolt_common_rustflags())
        }
        false => bolt_common_rustflags().to_string(),
    };
    Ok(flags)
}

fn get_binary_profile_dir(bolt_dir: &Path, artifact: &Artifact) -> PathBuf {
    let name = &artifact.target.name;
    bolt_dir.join(name)
}
