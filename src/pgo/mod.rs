use colored::Colorize;

pub(crate) mod env;
pub mod instrument;
pub mod optimize;

pub fn llvm_profdata_install_hint() -> String {
    format!(
        "Try installing `llvm-profdata` using `{}` or build LLVM manually and \
add its `bin` directory to PATH.",
        "rustup component add llvm-tools-preview".blue()
    )
}
