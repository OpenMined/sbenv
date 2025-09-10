use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sbenv")]
#[command(author, version, about = "SyftBox Env - virtualenv for SyftBox", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new SyftBox environment
    Init {
        /// Name of the environment
        #[arg(short, long)]
        name: Option<String>,
    },
    /// Activate a SyftBox environment
    Activate {
        /// Name of the environment to activate
        name: Option<String>,
    },
    /// Deactivate the current SyftBox environment
    Deactivate,
    /// List all SyftBox environments
    List,
    /// Remove a SyftBox environment
    Remove {
        /// Name of the environment to remove
        name: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Init { name }) => {
            println!("Init command called with name: {:?}", name);
        }
        Some(Commands::Activate { name }) => {
            println!("Activate command called with name: {:?}", name);
        }
        Some(Commands::Deactivate) => {
            println!("Deactivate command called");
        }
        Some(Commands::List) => {
            println!("List command called");
        }
        Some(Commands::Remove { name }) => {
            println!("Remove command called with name: {}", name);
        }
        None => {
            println!("SyftBox Env (sbenv) - virtualenv for SyftBox");
            println!("Use --help for more information");
        }
    }

    Ok(())
}