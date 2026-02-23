use crate::error::{ZlError, ZlResult};

use super::{AppContext, RemoveArgs, UpdateArgs};

pub fn handle(args: UpdateArgs, ctx: &AppContext) -> ZlResult<()> {
    if ctx.dry_run {
        println!("[DRY-RUN] Simulating update...");
    }

    // Get list of packages to update
    let packages = match args.package {
        Some(ref name) => {
            let pkg = ctx
                .db
                .get_package_by_name(name)?
                .ok_or_else(|| ZlError::PackageNotFound { name: name.clone() })?;
            vec![pkg]
        }
        None => ctx.db.list_packages()?,
    };

    if packages.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    // Sync all plugins first
    for plugin in ctx.registry.all() {
        if let Err(e) = plugin.sync() {
            tracing::warn!("Failed to sync {}: {}", plugin.name(), e);
        }
    }

    let mut updated = 0;
    let mut skipped_pinned = 0;

    for pkg in &packages {
        // Only update explicitly installed packages
        if !pkg.explicit {
            continue;
        }

        // Skip pinned packages
        if ctx.db.is_pinned(&pkg.id.name)? {
            tracing::info!("{} is pinned, skipping update", pkg.id.name);
            skipped_pinned += 1;
            continue;
        }

        // Find the plugin that manages this package
        let source_name = pkg.id.source.split('/').next().unwrap_or(&pkg.id.source);
        let plugin = match ctx.registry.get(source_name) {
            Some(p) => p,
            None => {
                tracing::warn!("No plugin found for source '{}', skipping", pkg.id.source);
                continue;
            }
        };

        // Check for newer version
        match plugin.resolve(&pkg.id.name, None)? {
            Some(candidate) if candidate.version != pkg.id.version => {
                println!(
                    "{}: {} -> {}",
                    pkg.id.name, pkg.id.version, candidate.version
                );

                if ctx.dry_run {
                    updated += 1;
                    continue;
                }

                // Remove old version
                let remove_args = RemoveArgs {
                    package: pkg.id.name.clone(),
                    cascade: false,
                    version: Some(pkg.id.version.clone()),
                };
                super::remove::handle(remove_args, ctx)?;

                // Install new version directly (skip dep resolution for updates)
                super::install::install_single_package(
                    &candidate,
                    true, // maintain explicit status
                    ctx.paths,
                    ctx.db,
                    plugin,
                    ctx.profile,
                    ctx.skip_verify,
                )?;

                updated += 1;
            }
            Some(_) => {
                tracing::debug!("{} is up to date", pkg.id.name);
            }
            None => {
                tracing::warn!("Could not resolve {} in {}", pkg.id.name, source_name);
            }
        }
    }

    if ctx.dry_run {
        if updated == 0 {
            println!("[DRY-RUN] All packages are up to date.");
        } else {
            println!(
                "[DRY-RUN] Would update {} package(s). No changes made.",
                updated
            );
        }
    } else if updated == 0 {
        println!("All packages are up to date.");
    } else {
        println!("\n{} package(s) updated.", updated);
    }

    if skipped_pinned > 0 {
        println!("{} pinned package(s) skipped.", skipped_pinned);
    }

    Ok(())
}
