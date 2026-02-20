use clap::{Args, Parser, Subcommand};

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
    List,
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
