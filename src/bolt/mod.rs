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

fn bolt_common_rustflags() -> Vec<String> {
    vec!["-Clink-args=-Wl,-q".to_string()]
}

fn bolt_pgo_rustflags(ctx: &CargoContext, with_pgo: bool) -> anyhow::Result<Vec<String>> {
    let flags = match with_pgo {
        true => {
            let pgo_env = get_pgo_env()?;
            let pgo_dir = ctx.get_pgo_directory()?;
            let mut flags = prepare_pgo_optimization_flags(&pgo_env, &pgo_dir)?;
            flags.extend(bolt_common_rustflags());
            flags
        }
        false => bolt_common_rustflags(),
    };
    Ok(flags)
}

fn get_binary_profile_dir(bolt_dir: &Path, artifact: &Artifact) -> PathBuf {
    let name = &artifact.target.name;
    bolt_dir.join(name)
}
