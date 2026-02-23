use crate::error::ZlResult;
use crate::plugin::PackageCandidate;

use super::{AppContext, RemoveArgs, UpgradeArgs};

/// An available upgrade for a single package
struct UpgradeEntry {
    name: String,
    old_version: String,
    new_version: String,
    candidate: PackageCandidate,
    source_name: String,
}

pub fn handle(args: UpgradeArgs, ctx: &AppContext) -> ZlResult<()> {
    let packages = ctx.db.list_packages()?;

    if packages.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    // Sync all relevant plugins
    println!("Syncing package databases...");
    for plugin in ctx.registry.all() {
        if let Some(ref source_filter) = args.from
            && plugin.name() != source_filter
        {
            continue;
        }
        if let Err(e) = plugin.sync() {
            tracing::warn!("Failed to sync {}: {}", plugin.name(), e);
        }
    }

    // Collect all available upgrades
    let mut upgrades: Vec<UpgradeEntry> = Vec::new();
    let mut skipped_pinned = 0;
    let mut up_to_date = 0;

    for pkg in &packages {
        if !pkg.explicit {
            continue;
        }

        if ctx.db.is_pinned(&pkg.id.name)? {
            skipped_pinned += 1;
            continue;
        }

        let source_name = pkg.id.source.split('/').next().unwrap_or(&pkg.id.source);

        // If --from filter is set, skip packages from other sources
        if let Some(ref source_filter) = args.from
            && source_name != source_filter
        {
            continue;
        }

        let plugin = match ctx.registry.get(source_name) {
            Some(p) => p,
            None => continue,
        };

        match plugin.resolve(&pkg.id.name, None)? {
            Some(candidate) if candidate.version != pkg.id.version => {
                upgrades.push(UpgradeEntry {
                    name: pkg.id.name.clone(),
                    old_version: pkg.id.version.clone(),
                    new_version: candidate.version.clone(),
                    candidate,
                    source_name: source_name.to_string(),
                });
            }
            _ => {
                up_to_date += 1;
            }
        }
    }

    // Display summary
    if upgrades.is_empty() {
        println!("\nAll packages are up to date.");
        if skipped_pinned > 0 {
            println!("{} pinned package(s) skipped.", skipped_pinned);
        }
        return Ok(());
    }

    println!("\nAvailable upgrades ({}):", upgrades.len());
    let mut total_size = 0u64;
    for entry in &upgrades {
        println!(
            "  {} {} -> {} (from {})",
            entry.name, entry.old_version, entry.new_version, entry.source_name
        );
        total_size += entry.candidate.installed_size;
    }

    println!(
        "\nTotal: {} upgrade(s), {:.1} MB estimated",
        upgrades.len(),
        total_size as f64 / 1_000_000.0
    );

    if up_to_date > 0 {
        println!("{} package(s) already up to date.", up_to_date);
    }
    if skipped_pinned > 0 {
        println!("{} pinned package(s) skipped.", skipped_pinned);
    }

    // Check-only mode: just show what would be upgraded
    if args.check {
        return Ok(());
    }

    if ctx.dry_run {
        println!(
            "\n[DRY-RUN] Would upgrade {} package(s). No changes made.",
            upgrades.len()
        );
        return Ok(());
    }

    // Confirm
    print!("\nProceed with upgrade? [Y/n] ");
    use std::io::Write;
    std::io::stdout().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();
    if !input.is_empty() && input != "y" && input != "yes" {
        println!("Upgrade cancelled.");
        return Ok(());
    }

    // Perform upgrades
    let mut upgraded = 0;
    let mut failed = 0;

    for entry in &upgrades {
        println!(
            "\nUpgrading {} {} -> {}...",
            entry.name, entry.old_version, entry.new_version
        );

        let plugin = match ctx.registry.get(&entry.source_name) {
            Some(p) => p,
            None => {
                eprintln!("  Plugin {} not found, skipping", entry.source_name);
                failed += 1;
                continue;
            }
        };

        // Remove old version
        let remove_args = RemoveArgs {
            package: entry.name.clone(),
            cascade: false,
            version: Some(entry.old_version.clone()),
        };
        if let Err(e) = super::remove::handle(remove_args, ctx) {
            eprintln!("  Failed to remove old version: {}", e);
            failed += 1;
            continue;
        }

        // Install new version
        match super::install::install_single_package(
            &entry.candidate,
            true,
            ctx.paths,
            ctx.db,
            plugin,
            ctx.profile,
            ctx.skip_verify,
        ) {
            Ok(()) => {
                upgraded += 1;
            }
            Err(e) => {
                eprintln!("  Failed to install new version: {}", e);
                failed += 1;
            }
        }
    }

    println!("\n{} package(s) upgraded successfully.", upgraded);
    if failed > 0 {
        eprintln!("{} package(s) failed to upgrade.", failed);
    }

    Ok(())
}
