mod cli;
mod config;
mod core;
mod error;
mod paths;
mod plugin;
mod system;

use clap::Parser;

use config::ZlConfig;
use core::db::ops::ZlDatabase;
use paths::ZlPaths;
use plugin::apt::AptPlugin;
use plugin::aur::AurPlugin;
use plugin::github::GithubPlugin;
use plugin::pacman::PacmanPlugin;
use plugin::{PluginRegistry, SourcePlugin};
use system::SystemProfile;

fn main() {
    let cli_args = cli::Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(match cli_args.global.verbose {
            0 => "warn",
            1 => "info",
            _ => "debug",
        })
        .init();

    if let Err(e) = run(cli_args) {
        eprintln!("error: {}", e);
        if let Some(zl_err) = e.downcast_ref::<error::ZlError>() {
            if let Some(hint) = zl_err.suggestion() {
                eprintln!("  hint: {}", hint);
            }
        }
        std::process::exit(1);
    }
}

fn run(cli_args: cli::Cli) -> anyhow::Result<()> {
    // Shell completions can run without any setup
    if let cli::Commands::Completions(ref args) = cli_args.command {
        cli::completions::handle(cli::CompletionsArgs { shell: args.shell })?;
        return Ok(());
    }

    // Self-update can run without full setup
    if let cli::Commands::SelfUpdate = cli_args.command {
        cli::selfupdate::handle()?;
        return Ok(());
    }

    // Load config
    let config = ZlConfig::load()?;

    // Detect system profile (replaces all hardcoded FHS assumptions)
    let mut profile = SystemProfile::detect();
    profile.apply_overrides(&config.system);
    tracing::debug!("System profile: {}", profile);

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

    let mut aur = AurPlugin::new();
    aur.init(&config.plugin_config("aur"))?;
    registry.register(Box::new(aur));

    let mut apt = AptPlugin::new();
    let mut apt_config = config.plugin_config("apt");
    apt_config.cache_dir = zl_paths.cache.join("apt");
    apt.init(&apt_config)?;
    registry.register(Box::new(apt));

    let mut github = GithubPlugin::new();
    github.init(&config.plugin_config("github"))?;
    registry.register(Box::new(github));

    let auto_yes = cli_args.global.yes || config.general.auto_confirm;
    let dry_run = cli_args.global.dry_run;
    let skip_verify = cli_args.global.skip_verify;

    // Dispatch commands
    match cli_args.command {
        cli::Commands::Install(args) => {
            cli::install::handle(
                args,
                &zl_paths,
                &db,
                &registry,
                &profile,
                auto_yes,
                dry_run,
                skip_verify,
            )?;
        }
        cli::Commands::Remove(args) => {
            cli::remove::handle(args, &zl_paths, &db, auto_yes, dry_run)?;
        }
        cli::Commands::Search(args) => {
            cli::search::handle(args, &registry)?;
        }
        cli::Commands::Update(args) => {
            cli::update::handle(
                args,
                &zl_paths,
                &db,
                &registry,
                &profile,
                auto_yes,
                dry_run,
                skip_verify,
            )?;
        }
        cli::Commands::Upgrade(args) => {
            cli::upgrade::handle(
                args,
                &zl_paths,
                &db,
                &registry,
                &profile,
                auto_yes,
                dry_run,
                skip_verify,
            )?;
        }
        cli::Commands::List(args) => {
            cli::list::handle(args, &db)?;
        }
        cli::Commands::Info(args) => {
            cli::info::handle(args, &db)?;
        }
        cli::Commands::Cache(cmd) => {
            cli::cache::handle(cmd, &zl_paths)?;
        }
        cli::Commands::Completions(_) => {
            unreachable!("handled above");
        }
        cli::Commands::Pin(args) => {
            cli::pin::handle_pin(args, &db)?;
        }
        cli::Commands::Unpin(args) => {
            cli::pin::handle_unpin(args, &db)?;
        }
        cli::Commands::Export(args) => {
            cli::lockfile::handle_export(args, &db)?;
        }
        cli::Commands::Import(args) => {
            cli::lockfile::handle_import(args, &db)?;
        }
        cli::Commands::Switch(args) => {
            cli::install::handle_switch(args, &zl_paths, &db)?;
        }
        cli::Commands::SelfUpdate => {
            unreachable!("handled above");
        }
        cli::Commands::Env(cmd) => {
            cli::env::handle(cmd, &zl_paths, &config, &profile)?;
        }
    }

    Ok(())
}
