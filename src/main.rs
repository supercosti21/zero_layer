mod cli;
mod config;
mod core;
mod error;
mod paths;
mod plugin;

use clap::Parser;

fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(if cli.global.verbose { "debug" } else { "info" })
        .init();

    match cli.command {
        cli::Commands::Install(args) => {
            println!("install: {} (not yet implemented)", args.package);
        }
        cli::Commands::Remove(args) => {
            println!("remove: {} (not yet implemented)", args.package);
        }
        cli::Commands::Search(args) => {
            println!("search: {} (not yet implemented)", args.query);
        }
        cli::Commands::Update(args) => {
            println!(
                "update: {} (not yet implemented)",
                args.package.as_deref().unwrap_or("all")
            );
        }
        cli::Commands::List => {
            println!("list (not yet implemented)");
        }
    }

    Ok(())
}
