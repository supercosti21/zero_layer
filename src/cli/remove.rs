use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};
use crate::paths::ZlPaths;

use super::{AppContext, RemoveArgs};

pub fn handle(args: RemoveArgs, ctx: &AppContext) -> ZlResult<()> {
    let paths = ctx.paths;
    let db = ctx.db;
    let auto_yes = ctx.auto_yes;
    let dry_run = ctx.dry_run;
    // If a specific version was requested, remove only that version
    if let Some(ref version) = args.version {
        return remove_specific_version(
            &args.package,
            version,
            paths,
            db,
            auto_yes,
            dry_run,
            args.cascade,
        );
    }

    // 1. Find package in DB
    let node = db
        .get_package_by_name(&args.package)?
        .ok_or_else(|| ZlError::PackageNotFound {
            name: args.package.clone(),
        })?;

    // Check if multiple versions exist
    let all_versions = db.get_all_versions(&args.package)?;
    if all_versions.len() > 1 {
        println!("Multiple versions of {} are installed:", args.package);
        for v in &all_versions {
            println!("  - {}", v.id.version);
        }
        println!("\nRemoving all versions...");

        if args.cascade {
            // Preview cascade before any removal
            preview_cascade_plan(&args.package, db)?;
        }

        if dry_run {
            println!(
                "[DRY-RUN] Would remove {} version(s) of {}. No changes made.",
                all_versions.len(),
                args.package
            );
            return Ok(());
        }
        for v in &all_versions {
            remove_single(v, paths, db)?;
        }
        if args.cascade {
            remove_orphans(paths, db, dry_run)?;
        }
        return Ok(());
    }

    let pkg_key = format!("{}-{}", node.id.name, node.id.version);

    // 2. Show what will happen
    println!(
        "Package: {}-{} ({} files)",
        node.id.name,
        node.id.version,
        node.installed_files.len()
    );

    if args.cascade {
        preview_cascade_plan(&args.package, db)?;
    }

    if dry_run {
        println!(
            "[DRY-RUN] Would remove {}-{}. No changes made.",
            node.id.name, node.id.version
        );
        return Ok(());
    }

    if !auto_yes {
        let prompt = if args.cascade {
            "Remove this package and its orphaned dependencies? [Y/n] "
        } else {
            "Remove this package? [Y/n] "
        };
        print!("{}", prompt);
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
    for soname in node.provides_libs.keys() {
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

    // 6. Remove from DB (including dependency records)
    db.remove_files_for_package(&pkg_key)?;
    db.remove_dependencies(&pkg_key)?;
    db.remove_package(&node.id.name, &node.id.version)?;

    println!("Removed {}-{}.", node.id.name, node.id.version);

    // Record in history
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = db.record_history(&crate::core::db::ops::HistoryEntry {
        timestamp: now,
        action: crate::core::db::ops::HistoryAction::Remove,
        packages: vec![format!("{}-{}", node.id.name, node.id.version)],
    });

    // 7. Cascade: remove orphans if requested
    if args.cascade {
        remove_orphans(paths, db, dry_run)?;
    }

    Ok(())
}

/// Remove a specific version of a package
fn remove_specific_version(
    name: &str,
    version: &str,
    paths: &ZlPaths,
    db: &ZlDatabase,
    auto_yes: bool,
    dry_run: bool,
    cascade: bool,
) -> ZlResult<()> {
    let node = db
        .get_package(name, version)?
        .ok_or_else(|| ZlError::Config(format!("{}-{} is not installed", name, version)))?;

    println!(
        "Package: {}-{} ({} files)",
        node.id.name,
        node.id.version,
        node.installed_files.len()
    );

    if cascade {
        preview_cascade_plan(name, db)?;
    }

    if dry_run {
        println!(
            "[DRY-RUN] Would remove {}-{}. No changes made.",
            name, version
        );
        return Ok(());
    }

    if !auto_yes {
        print!("Remove this version? [Y/n] ");
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

    remove_single(&node, paths, db)?;

    // If there are other versions, switch symlinks to the next available version
    let remaining = db.get_all_versions(name)?;
    if !remaining.is_empty() {
        let next = &remaining[0];
        let next_pkg_dir = paths
            .packages
            .join(format!("{}-{}", next.id.name, next.id.version));
        let mut txn = crate::core::transaction::Transaction::new();
        super::install::create_bin_symlinks(&next_pkg_dir, &paths.bin, &mut txn)?;
        txn.commit();
        println!(
            "Active version switched to {}-{}.",
            next.id.name, next.id.version
        );
    }

    if cascade {
        remove_orphans(paths, db, dry_run)?;
    }

    Ok(())
}

/// Remove a single package version (no prompts)
fn remove_single(
    node: &crate::core::graph::model::PackageNode,
    paths: &ZlPaths,
    db: &ZlDatabase,
) -> ZlResult<()> {
    let pkg_key = format!("{}-{}", node.id.name, node.id.version);

    remove_bin_symlinks(&node.installed_files, &paths.bin)?;

    for soname in node.provides_libs.keys() {
        let link_path = paths.lib.join(soname);
        if link_path.symlink_metadata().is_ok() {
            std::fs::remove_file(&link_path)?;
        }
    }

    let pkg_dir = paths
        .packages
        .join(format!("{}-{}", node.id.name, node.id.version));
    if pkg_dir.exists() {
        std::fs::remove_dir_all(&pkg_dir)?;
    }

    db.remove_files_for_package(&pkg_key)?;
    db.remove_dependencies(&pkg_key)?;
    db.remove_package(&node.id.name, &node.id.version)?;

    println!("Removed {}-{}.", node.id.name, node.id.version);
    Ok(())
}

/// Public wrapper for use by the history/rollback module
pub fn remove_bin_symlinks_public(
    installed_files: &[std::path::PathBuf],
    bin_dir: &std::path::Path,
) -> ZlResult<()> {
    remove_bin_symlinks(installed_files, bin_dir)
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
        if path.symlink_metadata().is_ok()
            && let Ok(target) = std::fs::read_link(&path)
        {
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
    Ok(())
}

/// Preview what `--cascade` would remove BEFORE actually doing anything.
/// Shows orphans that would be removed and dependencies that are kept (shared).
fn preview_cascade_plan(removing_name: &str, db: &ZlDatabase) -> ZlResult<()> {
    let all_packages = db.list_packages()?;

    // Simulate: what packages become orphaned if we remove `removing_name`?
    let (orphans, kept) = find_orphans_after_removal(removing_name, &all_packages, db)?;

    if !orphans.is_empty() {
        println!("\n  Cascade will also remove (ZL-only dependencies):");
        for orphan in &orphans {
            println!("    - {}-{} [{}]", orphan.0, orphan.1, orphan.2);
        }
    }

    if !kept.is_empty() {
        println!("\n  Keeping (shared with other packages):");
        for (name, version, needed_by) in &kept {
            println!("    - {}-{} (needed by {})", name, version, needed_by);
        }
    }

    if orphans.is_empty() {
        println!("\n  Cascade: no orphaned dependencies to remove.");
    }

    Ok(())
}

/// Determine which packages would become orphaned if `removing_name` is removed.
/// Returns (orphans, kept) where:
/// - orphans: Vec<(name, version, source)> — will be removed
/// - kept: Vec<(name, version, needed_by)> — shared deps that stay
#[allow(clippy::type_complexity)]
fn find_orphans_after_removal(
    removing_name: &str,
    all_packages: &[crate::core::graph::model::PackageNode],
    db: &ZlDatabase,
) -> ZlResult<(Vec<(String, String, String)>, Vec<(String, String, String)>)> {
    let mut orphans = Vec::new();
    let mut kept = Vec::new();

    for pkg in all_packages {
        // Only consider implicit (non-explicit) packages as cascade candidates
        if pkg.explicit || pkg.id.name == removing_name {
            continue;
        }

        let pkg_name = &pkg.id.name;

        // Check if any REMAINING package (other than the one being removed) depends on this
        let has_other_dependents = all_packages.iter().any(|other| {
            if other.id.name == *pkg_name || other.id.name == removing_name {
                return false;
            }
            let other_key = format!("{}-{}", other.id.name, other.id.version);
            db.get_dependencies(&other_key)
                .unwrap_or_default()
                .iter()
                .any(|dep| {
                    let dep_name = dep.split(&['>', '<', '=', ':'][..]).next().unwrap_or(dep);
                    dep_name == pkg_name
                })
        });

        // Also check shared lib dependencies
        let provides: std::collections::HashSet<&str> =
            pkg.provides_libs.keys().map(|s| s.as_str()).collect();
        let has_lib_dependents = all_packages.iter().any(|other| {
            other.id.name != *pkg_name
                && other.id.name != removing_name
                && other
                    .needs_libs
                    .iter()
                    .any(|lib| provides.contains(lib.as_str()))
        });

        if has_other_dependents || has_lib_dependents {
            // Find who needs it (for the display message)
            let needed_by = all_packages
                .iter()
                .filter(|other| other.id.name != *pkg_name && other.id.name != removing_name)
                .find(|other| {
                    let other_key = format!("{}-{}", other.id.name, other.id.version);
                    let has_dep = db
                        .get_dependencies(&other_key)
                        .unwrap_or_default()
                        .iter()
                        .any(|dep| {
                            let dep_name =
                                dep.split(&['>', '<', '=', ':'][..]).next().unwrap_or(dep);
                            dep_name == pkg_name
                        });
                    let has_lib = other
                        .needs_libs
                        .iter()
                        .any(|lib| provides.contains(lib.as_str()));
                    has_dep || has_lib
                })
                .map(|p| p.id.name.clone())
                .unwrap_or_else(|| "unknown".into());

            kept.push((pkg.id.name.clone(), pkg.id.version.clone(), needed_by));
        } else {
            // Check if this package is actually a dependency of the one being removed
            let is_dep_of_removing = all_packages
                .iter()
                .filter(|p| p.id.name == removing_name)
                .any(|p| {
                    let key = format!("{}-{}", p.id.name, p.id.version);
                    db.get_dependencies(&key)
                        .unwrap_or_default()
                        .iter()
                        .any(|dep| {
                            let dep_name =
                                dep.split(&['>', '<', '=', ':'][..]).next().unwrap_or(dep);
                            dep_name == pkg_name
                        })
                });

            // Also consider transitive: a dep of a dep
            let is_dep_of_removing = is_dep_of_removing || {
                // Check if any already-identified orphan depends on this
                // (simplified: just check if this package has no other dependents at all)
                !all_packages.iter().any(|other| {
                    other.id.name != *pkg_name
                        && other.id.name != removing_name
                        && other.explicit
                        && {
                            let other_key = format!("{}-{}", other.id.name, other.id.version);
                            db.get_dependencies(&other_key)
                                .unwrap_or_default()
                                .iter()
                                .any(|dep| {
                                    let dep_name =
                                        dep.split(&['>', '<', '=', ':'][..]).next().unwrap_or(dep);
                                    dep_name == pkg_name
                                })
                        }
                })
            };

            if is_dep_of_removing {
                orphans.push((
                    pkg.id.name.clone(),
                    pkg.id.version.clone(),
                    pkg.id.source.clone(),
                ));
            }
        }
    }

    Ok((orphans, kept))
}

/// Find and remove orphaned packages (dependencies not needed by any remaining package).
///
/// Safety guarantees:
/// - ONLY removes packages tracked in ZL's own database (never system packages)
/// - ONLY removes packages marked as `explicit: false` (installed as dependencies)
/// - Checks BOTH declared dependencies AND shared library needs
/// - Always prints what will be removed before doing it
fn remove_orphans(paths: &ZlPaths, db: &ZlDatabase, dry_run: bool) -> ZlResult<()> {
    let all_packages = db.list_packages()?;

    // Find packages that are:
    // 1. NOT explicit (were installed as dependencies by ZL)
    // 2. NOT depended on by any remaining explicit or non-orphan package
    let orphans: Vec<_> = all_packages
        .iter()
        .filter(|pkg| !pkg.explicit)
        .filter(|pkg| {
            let pkg_name = &pkg.id.name;

            // Check if any remaining package has a registered dependency on this one
            let has_dependents = all_packages.iter().any(|other| {
                if other.id.name == *pkg_name {
                    return false;
                }
                let other_key = format!("{}-{}", other.id.name, other.id.version);
                db.get_dependencies(&other_key)
                    .unwrap_or_default()
                    .iter()
                    .any(|dep| {
                        // Strip version constraints for comparison
                        let dep_name = dep.split(&['>', '<', '=', ':'][..]).next().unwrap_or(dep);
                        dep_name == pkg_name
                    })
            });

            if has_dependents {
                return false;
            }

            // Also check shared lib dependencies (fallback)
            let provides: std::collections::HashSet<&str> =
                pkg.provides_libs.keys().map(|s| s.as_str()).collect();
            !all_packages.iter().any(|other| {
                other.id.name != *pkg_name
                    && other
                        .needs_libs
                        .iter()
                        .any(|lib| provides.contains(lib.as_str()))
            })
        })
        .collect();

    if orphans.is_empty() {
        return Ok(());
    }

    println!("\nOrphaned dependencies to remove ({}):", orphans.len());
    for orphan in &orphans {
        println!(
            "  - {}-{} [{}]",
            orphan.id.name, orphan.id.version, orphan.id.source
        );
    }

    if dry_run {
        println!(
            "\n[DRY-RUN] Would remove {} orphaned dependency(ies). No changes made.",
            orphans.len()
        );
        return Ok(());
    }

    println!();
    for orphan in &orphans {
        let pkg_key = format!("{}-{}", orphan.id.name, orphan.id.version);

        // Remove bin symlinks
        remove_bin_symlinks(&orphan.installed_files, &paths.bin)?;

        // Remove lib symlinks
        for soname in orphan.provides_libs.keys() {
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
        db.remove_dependencies(&pkg_key)?;
        db.remove_package(&orphan.id.name, &orphan.id.version)?;

        println!("  Removed orphan: {}-{}", orphan.id.name, orphan.id.version);
    }

    Ok(())
}
