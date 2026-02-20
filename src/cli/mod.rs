pub mod cache;
pub mod completions;
pub mod deps;
pub mod info;
pub mod install;
pub mod list;
pub mod lockfile;
pub mod pin;
pub mod remove;
pub mod search;
pub mod update;

use clap::{Args, Parser, Subcommand};
use clap_complete::Shell;

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
    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// ZL root directory (default: ~/.local/share/zl)
    #[arg(long, global = true)]
    pub root: Option<std::path::PathBuf>,

    /// Suppress interactive prompts
    #[arg(short = 'y', long, global = true)]
    pub yes: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Install a package
    Install(InstallArgs),
    /// Remove a package
    Remove(RemoveArgs),
    /// Update installed packages
    Update(UpdateArgs),
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
}

#[derive(Args)]
pub struct UpdateArgs {
    /// Specific package (default: all)
    pub package: Option<String>,
}

#[derive(Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,
    /// Limit to a specific source
    #[arg(long)]
    pub from: Option<String>,
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
