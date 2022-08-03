use cargo_metadata::Artifact;
use std::path::{Path, PathBuf};

pub(crate) mod env;
pub mod instrument;
pub mod optimize;

pub fn llvm_bolt_install_hint() -> &'static str {
    "Build LLVM with BOLT and add its `bin` directory to PATH."
}

fn bolt_rustflags() -> &'static str {
    "-C link-args=-Wl,-q"
}

fn get_binary_profile_dir(bolt_dir: &Path, artifact: &Artifact) -> PathBuf {
    let name = &artifact.target.name;
    bolt_dir.join(name)
}
