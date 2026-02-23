//! `zl sources` command — manage which package sources ZL uses.
//!
//! Subcommands:
//! - `zl sources list`              — show all sources (enabled/disabled)
//! - `zl sources enable <names...>` — enable specific sources
//! - `zl sources disable <names...>`— disable specific sources
//! - `zl sources only <names...>`   — enable ONLY these sources (disable rest)
//! - `zl sources reset`             — remove source filter (enable all)

use console::style;

use crate::config::ZlConfig;
use crate::error::ZlResult;
use crate::plugin::PluginRegistry;

use super::SourcesCommand;

/// All known plugin names (built-in).
pub const ALL_PLUGIN_NAMES: &[&str] = &[
    "pacman", "aur", "apt", "github", "dnf", "zypper", "apk", "xbps", "portage", "nix", "flatpak",
    "snap", "appimage",
];

pub fn handle(cmd: SourcesCommand, registry: &PluginRegistry) -> ZlResult<()> {
    match cmd {
        SourcesCommand::List => handle_list(registry),
        SourcesCommand::Enable(args) => handle_enable(&args.names),
        SourcesCommand::Disable(args) => handle_disable(&args.names),
        SourcesCommand::Only(args) => handle_only(&args.names),
        SourcesCommand::Reset => handle_reset(),
    }
}

fn handle_list(registry: &PluginRegistry) -> ZlResult<()> {
    let config = ZlConfig::load()?;
    let enabled_sources = config.enabled_sources();

    println!("{}", style("Package Sources").bold());
    println!();

    let registered: Vec<&str> = registry.names();

    for &name in ALL_PLUGIN_NAMES {
        let is_registered = registered.contains(&name);
        let is_enabled = enabled_sources.is_none_or(|s| s.iter().any(|x| x == name));
        let per_plugin_enabled = config.plugin_config(name).enabled;

        let status = if is_enabled && per_plugin_enabled {
            style("enabled").green().bold()
        } else {
            style("disabled").dim()
        };

        let loaded = if is_registered {
            style(" (loaded)").dim()
        } else {
            style("         ").dim()
        };

        println!("  {:12} {}{}", name, status, loaded);
    }

    if let Some(sources) = enabled_sources {
        println!();
        println!("  Active filter: {}", style(sources.join(", ")).cyan());
    } else {
        println!();
        println!("  {}", style("All sources enabled (no filter)").dim());
    }

    Ok(())
}

fn handle_enable(names: &[String]) -> ZlResult<()> {
    let mut config = ZlConfig::load()?;
    let current = config
        .general
        .sources
        .get_or_insert_with(|| ALL_PLUGIN_NAMES.iter().map(|s| s.to_string()).collect());

    for name in names {
        validate_source_name(name)?;
        if !current.iter().any(|s| s == name) {
            current.push(name.clone());
        }
    }

    config.save()?;
    println!(
        "Enabled: {}. Restart zl for changes to take effect.",
        style(names.join(", ")).green()
    );
    Ok(())
}

fn handle_disable(names: &[String]) -> ZlResult<()> {
    let mut config = ZlConfig::load()?;
    let current = config
        .general
        .sources
        .get_or_insert_with(|| ALL_PLUGIN_NAMES.iter().map(|s| s.to_string()).collect());

    for name in names {
        validate_source_name(name)?;
        current.retain(|s| s != name);
    }

    config.save()?;
    println!(
        "Disabled: {}. Restart zl for changes to take effect.",
        style(names.join(", ")).yellow()
    );
    Ok(())
}

fn handle_only(names: &[String]) -> ZlResult<()> {
    for name in names {
        validate_source_name(name)?;
    }

    let mut config = ZlConfig::load()?;
    config.general.sources = Some(names.to_vec());
    config.save()?;
    println!("Set active sources to: {}", style(names.join(", ")).green());
    Ok(())
}

fn handle_reset() -> ZlResult<()> {
    let mut config = ZlConfig::load()?;
    config.general.sources = None;
    config.save()?;
    println!("Source filter removed. All sources are now enabled.");
    Ok(())
}

fn validate_source_name(name: &str) -> ZlResult<()> {
    if ALL_PLUGIN_NAMES.contains(&name) {
        Ok(())
    } else {
        Err(crate::error::ZlError::Plugin {
            plugin: "sources".into(),
            message: format!(
                "Unknown source '{}'. Available: {}",
                name,
                ALL_PLUGIN_NAMES.join(", ")
            ),
        })
    }
}

/// First-run wizard: detect distro and let user pick sources.
/// Called when config.toml doesn't exist yet.
pub fn first_run_wizard(profile: &crate::system::SystemProfile) -> ZlResult<Option<Vec<String>>> {
    use dialoguer::MultiSelect;

    println!();
    println!(
        "{}",
        style("Welcome to Zero Layer! Let's configure your package sources.").bold()
    );
    println!();

    // Suggest sources based on detected system
    let suggested = suggest_sources(profile);

    let items: Vec<String> = ALL_PLUGIN_NAMES.iter().map(|s| s.to_string()).collect();
    let defaults: Vec<bool> = items
        .iter()
        .map(|name| suggested.contains(&name.as_str()))
        .collect();

    println!("Select which package sources to enable:");
    println!("  (Use space to toggle, enter to confirm)");
    println!();

    let selected = MultiSelect::new()
        .items(&items)
        .defaults(&defaults)
        .interact()
        .map_err(|e| crate::error::ZlError::Plugin {
            plugin: "wizard".into(),
            message: format!("Selection cancelled: {}", e),
        })?;

    if selected.is_empty() {
        println!("No sources selected — enabling all sources by default.");
        return Ok(None);
    }

    let chosen: Vec<String> = selected.into_iter().map(|i| items[i].clone()).collect();
    println!();
    println!(
        "Enabled sources: {}",
        style(chosen.join(", ")).green().bold()
    );
    println!(
        "  You can change this anytime with: {}",
        style("zl sources").cyan()
    );
    println!();

    Ok(Some(chosen))
}

/// Suggest default sources based on detected system profile.
fn suggest_sources(profile: &crate::system::SystemProfile) -> Vec<&'static str> {
    let mut sources = vec!["github"]; // Always suggest GitHub

    // Detect distro from various signals
    let layout = format!("{:?}", profile.layout);
    let interp = profile.interpreter.to_string_lossy().to_string();

    if layout.contains("Nix") || interp.contains("nix") {
        sources.push("nix");
    }

    // Check /etc/os-release for distro detection
    if let Ok(os_release) = std::fs::read_to_string("/etc/os-release") {
        let os_lower = os_release.to_lowercase();
        if os_lower.contains("arch") {
            sources.extend(["pacman", "aur"]);
        }
        if os_lower.contains("debian") || os_lower.contains("ubuntu") || os_lower.contains("mint") {
            sources.push("apt");
        }
        if os_lower.contains("fedora")
            || os_lower.contains("rhel")
            || os_lower.contains("centos")
            || os_lower.contains("rocky")
            || os_lower.contains("alma")
        {
            sources.push("dnf");
        }
        if os_lower.contains("opensuse") || os_lower.contains("sles") {
            sources.push("zypper");
        }
        if os_lower.contains("alpine") {
            sources.push("apk");
        }
        if os_lower.contains("void") {
            sources.push("xbps");
        }
        if os_lower.contains("gentoo") {
            sources.push("portage");
        }
    }

    // Always suggest flatpak and appimage as universals
    sources.push("flatpak");
    sources.push("appimage");

    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_source_name_valid() {
        assert!(validate_source_name("pacman").is_ok());
        assert!(validate_source_name("dnf").is_ok());
        assert!(validate_source_name("nix").is_ok());
    }

    #[test]
    fn test_validate_source_name_invalid() {
        assert!(validate_source_name("invalid").is_err());
        assert!(validate_source_name("yum").is_err());
    }

    #[test]
    fn test_all_plugin_names_count() {
        assert_eq!(ALL_PLUGIN_NAMES.len(), 13);
    }
}
