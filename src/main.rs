mod cli;
mod config;
mod core;
mod error;
mod paths;
mod plugin;

use clap::Parser;

use config::ZlConfig;
use core::db::ops::ZlDatabase;
use paths::ZlPaths;
use plugin::pacman::PacmanPlugin;
use plugin::{PluginRegistry, SourcePlugin};

fn main() -> anyhow::Result<()> {
    let cli_args = cli::Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(if cli_args.global.verbose {
            "debug"
        } else {
            "info"
        })
        .init();

    // Load config
    let config = ZlConfig::load()?;

    // Setup paths (CLI --root overrides config)
    let root_override = cli_args
        .global
        .root
        .as_deref()
        .or(config.general.root.as_deref());
    let zl_paths = ZlPaths::new(root_override);
    zl_paths.ensure_dirs()?;

    // Open database
    let db = ZlDatabase::open(&zl_paths.db_file)?;

    // Setup plugin registry
    let mut registry = PluginRegistry::new();
    let mut pacman = PacmanPlugin::new();
    let mut pacman_config = config.plugin_config("pacman");
    pacman_config.cache_dir = zl_paths.cache.join("pacman");
    pacman.init(&pacman_config)?;
    registry.register(Box::new(pacman));

    let auto_yes = cli_args.global.yes || config.general.auto_confirm;

    // Dispatch commands
    match cli_args.command {
        cli::Commands::Install(args) => {
            cli::install::handle(args, &zl_paths, &db, &registry, auto_yes)?;
        }
        cli::Commands::Remove(args) => {
            cli::remove::handle(args, &zl_paths, &db, auto_yes)?;
        }
        cli::Commands::Search(args) => {
            cli::search::handle(args, &registry)?;
        }
        cli::Commands::Update(args) => {
            cli::update::handle(args, &zl_paths, &db, &registry, auto_yes)?;
        }
        cli::Commands::List => {
            cli::list::handle(&db)?;
        }
    }

    Ok(())
}
