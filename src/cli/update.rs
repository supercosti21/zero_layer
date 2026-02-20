use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};
use crate::paths::ZlPaths;
use crate::plugin::PluginRegistry;
use crate::system::SystemProfile;

use super::{InstallArgs, RemoveArgs, UpdateArgs};

pub fn handle(
    args: UpdateArgs,
    paths: &ZlPaths,
    db: &ZlDatabase,
    registry: &PluginRegistry,
    profile: &SystemProfile,
    _auto_yes: bool,
) -> ZlResult<()> {
    // Get list of packages to update
    let packages = match args.package {
        Some(ref name) => {
            let pkg = db
                .get_package_by_name(name)?
                .ok_or_else(|| ZlError::PackageNotFound(name.clone()))?;
            vec![pkg]
        }
        None => db.list_packages()?,
    };

    if packages.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    // Sync all plugins first
    for plugin in registry.all() {
        if let Err(e) = plugin.sync() {
            tracing::warn!("Failed to sync {}: {}", plugin.name(), e);
        }
    }

    let mut updated = 0;

    for pkg in &packages {
        // Find the plugin that manages this package
        let source_name = pkg.id.source.split('/').next().unwrap_or(&pkg.id.source);
        let plugin = match registry.get(source_name) {
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

                // Remove old version
                let remove_args = RemoveArgs {
                    package: pkg.id.name.clone(),
                    cascade: false,
                };
                super::remove::handle(remove_args, paths, db, true)?;

                // Install new version
                let install_args = InstallArgs {
                    package: pkg.id.name.clone(),
                    from: Some(source_name.to_string()),
                    version: Some(candidate.version.clone()),
                };
                super::install::handle(install_args, paths, db, registry, profile, true)?;

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

    if updated == 0 {
        println!("All packages are up to date.");
    } else {
        println!("\n{} package(s) updated.", updated);
    }

    Ok(())
}
