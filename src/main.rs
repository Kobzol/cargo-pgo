use clap::Parser;

use cargo_bolt::find_bolt_env;

#[derive(clap::Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(bin_name("cargo"))]
enum Args {
    #[clap(subcommand)]
    Bolt(Subcommand),
}

#[derive(clap::Parser, Debug)]
enum Subcommand {
    /// Check that BOLT binaries can be found.
    Check,
}

fn run() -> anyhow::Result<()> {
    let args = Args::parse();

    let Args::Bolt(args) = args;
    match args {
        Subcommand::Check => {
            let env = find_bolt_env()?;
            println!("Found `llvm-bolt` at {}", env.bolt.display());
            println!("Found `merge-fdata` at {}", env.merge_fdata.display());
        }
    }

    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Error: {:?}", error);
        std::process::exit(1);
    }
}
