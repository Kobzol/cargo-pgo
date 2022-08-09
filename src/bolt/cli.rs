#[derive(clap::Parser, Debug)]
pub struct BoltArgs {
    /// Flags that will be passed to the BOLT command.
    /// Using this flag will override BOLT flags normally used by `cargo-pgo`.
    /// If you want to disable all flags, pass an empty string (`--bolt-args ""`).
    #[clap(long, allow_hyphen_values(true))]
    pub(crate) bolt_args: Option<String>,
}

pub fn add_bolt_args(args: &mut Vec<String>, bolt_args: &str) -> anyhow::Result<()> {
    let bolt_args = shellwords::split(bolt_args)
        .map_err(|error| anyhow::anyhow!("Could not parse BOLT args: {:?}", error))?;
    for arg in bolt_args {
        args.push(arg);
    }

    Ok(())
}
