use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};
use crate::paths::ZlPaths;

use super::RemoveArgs;

pub fn handle(
    args: RemoveArgs,
    paths: &ZlPaths,
    db: &ZlDatabase,
    auto_yes: bool,
) -> ZlResult<()> {
    // 1. Find package in DB
    let node = db
        .get_package_by_name(&args.package)?
        .ok_or_else(|| ZlError::PackageNotFound(args.package.clone()))?;

    let pkg_key = format!("{}-{}", node.id.name, node.id.version);

    // 2. Confirm
    println!(
        "Package: {}-{} ({} files)",
        node.id.name,
        node.id.version,
        node.installed_files.len()
    );

    if !auto_yes {
        print!("Remove this package? [Y/n] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();
        if !input.is_empty() && input != "y" && input != "yes" {
            println!("Removal cancelled.");
            return Ok(());
        }
    }

    // 3. Remove symlinks from bin/
    remove_bin_symlinks(&node.installed_files, &paths.bin)?;

    // 4. Remove lib symlinks
    for (soname, _) in &node.provides_libs {
        let link_path = paths.lib.join(soname);
        if link_path.symlink_metadata().is_ok() {
            std::fs::remove_file(&link_path)?;
            tracing::debug!("Removed lib symlink: {}", soname);
        }
    }

    // 5. Remove package directory
    let pkg_dir = paths
        .packages
        .join(format!("{}-{}", node.id.name, node.id.version));
    if pkg_dir.exists() {
        std::fs::remove_dir_all(&pkg_dir)?;
    }

    // 6. Remove from DB
    db.remove_files_for_package(&pkg_key)?;
    db.remove_package(&node.id.name, &node.id.version)?;

    println!("Removed {}-{}.", node.id.name, node.id.version);

    // 7. Cascade: remove orphans if requested
    if args.cascade {
        remove_orphans(paths, db)?;
    }

    Ok(())
}

/// Remove symlinks from bin/ that point into the package's installed files
fn remove_bin_symlinks(
    installed_files: &[std::path::PathBuf],
    bin_dir: &std::path::Path,
) -> ZlResult<()> {
    if !bin_dir.is_dir() {
        return Ok(());
    }

    let entries = std::fs::read_dir(bin_dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.symlink_metadata().is_ok() {
            if let Ok(target) = std::fs::read_link(&path) {
                // Check if symlink target belongs to this package
                for installed in installed_files {
                    if target == *installed || target.starts_with(installed) {
                        std::fs::remove_file(&path)?;
                        tracing::debug!(
                            "Removed bin symlink: {}",
                            entry.file_name().to_string_lossy()
                        );
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Find and remove orphaned packages (dependencies not needed by any explicit package)
fn remove_orphans(paths: &ZlPaths, db: &ZlDatabase) -> ZlResult<()> {
    let all_packages = db.list_packages()?;

    // Find packages that are not explicit and not depended on by any explicit package
    let orphans: Vec<_> = all_packages
        .iter()
        .filter(|pkg| !pkg.explicit)
        .filter(|pkg| {
            // Check if any explicit package needs libs provided by this one
            let provides: std::collections::HashSet<&str> =
                pkg.provides_libs.keys().map(|s| s.as_str()).collect();
            !all_packages.iter().any(|other| {
                other.explicit && other.needs_libs.iter().any(|lib| provides.contains(lib.as_str()))
            })
        })
        .collect();

    if orphans.is_empty() {
        return Ok(());
    }

    println!("\nRemoving {} orphaned dependencies:", orphans.len());
    for orphan in &orphans {
        let pkg_key = format!("{}-{}", orphan.id.name, orphan.id.version);
        println!("  - {}-{}", orphan.id.name, orphan.id.version);

        // Remove lib symlinks
        for (soname, _) in &orphan.provides_libs {
            let link_path = paths.lib.join(soname);
            if link_path.symlink_metadata().is_ok() {
                std::fs::remove_file(&link_path)?;
            }
        }

        // Remove package directory
        let pkg_dir = paths
            .packages
            .join(format!("{}-{}", orphan.id.name, orphan.id.version));
        if pkg_dir.exists() {
            std::fs::remove_dir_all(&pkg_dir)?;
        }

        // Remove from DB
        db.remove_files_for_package(&pkg_key)?;
        db.remove_package(&orphan.id.name, &orphan.id.version)?;
    }

    Ok(())
}
