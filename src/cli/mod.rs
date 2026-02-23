pub mod cache;
pub mod completions;
pub mod deps;
pub mod env;
pub mod info;
pub mod install;
pub mod list;
pub mod lockfile;
pub mod pin;
pub mod remove;
pub mod search;
pub mod selfupdate;
pub mod update;
pub mod upgrade;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;

use crate::core::db::ops::ZlDatabase;
use crate::paths::ZlPaths;
use crate::plugin::PluginRegistry;
use crate::system::SystemProfile;

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum SortOrder {
    #[default]
    Relevance,
    Name,
    Version,
}

/// Shared application context passed to all command handlers.
pub struct AppContext<'a> {
    pub paths: &'a ZlPaths,
    pub db: &'a ZlDatabase,
    pub registry: &'a PluginRegistry,
    pub profile: &'a SystemProfile,
    pub auto_yes: bool,
    pub dry_run: bool,
    pub skip_verify: bool,
}

#[derive(Parser)]
#[command(
    name = "zl",
    version,
    about = "Zero Layer — Universal Linux package manager with native binary translation"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    #[command(flatten)]
    pub global: GlobalOpts,
}

#[derive(Args)]
pub struct GlobalOpts {
    /// Verbose output (-v = info, -vv = debug)
    #[arg(short, long, global = true, action = ArgAction::Count)]
    pub verbose: u8,

    /// ZL root directory (default: ~/.local/share/zl)
    #[arg(long, global = true)]
    pub root: Option<std::path::PathBuf>,

    /// Suppress interactive prompts
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,

    /// Dry-run mode: show what would happen without making changes
    #[arg(long, alias = "simulate", global = true)]
    pub dry_run: bool,

    /// Skip checksum and GPG signature verification
    #[arg(long, global = true)]
    pub skip_verify: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Install a package
    Install(InstallArgs),
    /// Remove a package
    Remove(RemoveArgs),
    /// Update installed packages
    Update(UpdateArgs),
    /// Upgrade all packages at once
    Upgrade(UpgradeArgs),
    /// Search for packages across sources
    Search(SearchArgs),
    /// List installed packages
    List(ListArgs),
    /// Show detailed info about an installed package
    Info(InfoArgs),
    /// Manage the download cache
    #[command(subcommand)]
    Cache(CacheCommand),
    /// Generate shell completions
    Completions(CompletionsArgs),
    /// Pin a package to prevent updates
    Pin(PinArgs),
    /// Unpin a package to allow updates
    Unpin(UnpinArgs),
    /// Export installed packages to a lockfile
    Export(ExportArgs),
    /// Import packages from a lockfile
    Import(ImportArgs),
    /// Switch active version of a multi-version package
    Switch(SwitchArgs),
    /// Update ZL itself to the latest version
    SelfUpdate,
    /// Manage ephemeral environments
    #[command(subcommand)]
    Env(EnvCommand),
}

#[derive(Args)]
pub struct InstallArgs {
    /// Package name
    pub package: String,
    /// Source to install from (e.g., pacman, apt)
    #[arg(long)]
    pub from: Option<String>,
    /// Specific version
    #[arg(long)]
    pub version: Option<String>,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Package name
    pub package: String,
    /// Also remove orphaned dependencies
    #[arg(long)]
    pub cascade: bool,
    /// Remove a specific version (default: all versions)
    #[arg(long)]
    pub version: Option<String>,
}

#[derive(Args)]
pub struct UpdateArgs {
    /// Specific package (default: all)
    pub package: Option<String>,
}

#[derive(Args)]
pub struct UpgradeArgs {
    /// Source to upgrade from (default: all sources)
    #[arg(long)]
    pub from: Option<String>,
    /// Only show what would be upgraded, don't actually upgrade
    #[arg(long)]
    pub check: bool,
}

#[derive(Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Limit to a specific source
    #[arg(long)]
    pub from: Option<String>,
    /// Maximum results per source (default: 20)
    #[arg(long)]
    pub limit: Option<usize>,
    /// Sort results: relevance (default), name, version
    #[arg(long, default_value = "relevance")]
    pub sort: SortOrder,
    /// Only show exact name matches
    #[arg(long)]
    pub exact: bool,
}

#[derive(Args)]
pub struct ListArgs {
    /// Show only explicitly installed packages
    #[arg(long)]
    pub explicit: bool,
    /// Show only packages installed as dependencies
    #[arg(long)]
    pub deps: bool,
    /// Show orphaned dependencies (no longer needed)
    #[arg(long)]
    pub orphans: bool,
}

#[derive(Args)]
pub struct InfoArgs {
    /// Package name
    pub package: String,
}

#[derive(Subcommand)]
pub enum CacheCommand {
    /// List cached files and their sizes
    List,
    /// Remove all cached files
    Clean,
}

#[derive(Args)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    pub shell: Shell,
}

#[derive(Args)]
pub struct PinArgs {
    /// Package name to pin
    pub package: String,
}

#[derive(Args)]
pub struct UnpinArgs {
    /// Package name to unpin
    pub package: String,
}

#[derive(Args)]
pub struct ExportArgs {
    /// Output file path (default: zl-lock.json)
    pub file: Option<std::path::PathBuf>,
}

#[derive(Args)]
pub struct ImportArgs {
    /// Lockfile path to import
    pub file: std::path::PathBuf,
}

#[derive(Args)]
pub struct SwitchArgs {
    /// Package name
    pub package: String,
    /// Version to activate
    pub version: String,
}

#[derive(Subcommand)]
pub enum EnvCommand {
    /// Create and enter an ephemeral environment (deleted on exit)
    Shell(EnvShellArgs),
    /// List existing named environments
    List,
    /// Delete a named environment
    Delete(EnvDeleteArgs),
}

#[derive(Args)]
pub struct EnvShellArgs {
    /// Environment name (omit for temporary, auto-deleted on exit)
    pub name: Option<String>,
}

#[derive(Args)]
pub struct EnvDeleteArgs {
    /// Name of the environment to delete
    pub name: String,
}
