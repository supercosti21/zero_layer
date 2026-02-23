use console::style;

use crate::core::db::ops::ZlDatabase;
use crate::error::ZlResult;

use super::ListArgs;

pub fn handle(args: ListArgs, db: &ZlDatabase) -> ZlResult<()> {
    let packages = db.list_packages()?;

    if packages.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    // Apply filters
    let filtered: Vec<_> = packages
        .iter()
        .filter(|pkg| {
            if args.explicit && !pkg.explicit {
                return false;
            }
            if args.deps && pkg.explicit {
                return false;
            }
            true
        })
        .collect();

    // If --orphans flag: find deps that are no longer needed
    if args.orphans {
        return handle_orphans(&packages, db);
    }

    if filtered.is_empty() {
        println!("No packages match the filter.");
        return Ok(());
    }

    let pinned_list = db.list_pinned().unwrap_or_default();
    let pinned_names: std::collections::HashSet<String> =
        pinned_list.into_iter().map(|(name, _)| name).collect();

    println!(
        "{:<30} {:<20} {:<15} {:>6} {}",
        style("Name").bold(),
        style("Version").bold(),
        style("Source").bold(),
        style("Files").bold(),
        style("Status").bold()
    );
    println!("{}", "-".repeat(85));

    for pkg in &filtered {
        let mut status_parts = Vec::new();
        if pkg.explicit {
            status_parts.push(style("explicit").green().to_string());
        } else {
            status_parts.push(style("dep").dim().to_string());
        }
        if pinned_names.contains(&pkg.id.name) {
            status_parts.push(style("pinned").yellow().to_string());
        }

        println!(
            "{:<30} {:<20} {:<15} {:>6} [{}]",
            style(&pkg.id.name).white().bold(),
            style(&pkg.id.version).yellow(),
            style(&pkg.id.source).cyan(),
            pkg.installed_files.len(),
            status_parts.join(", ")
        );
    }

    println!("\n{} package(s) listed.", style(filtered.len()).bold());
    Ok(())
}

fn handle_orphans(
    packages: &[crate::core::graph::model::PackageNode],
    db: &ZlDatabase,
) -> ZlResult<()> {
    let orphans: Vec<_> = packages
        .iter()
        .filter(|pkg| !pkg.explicit)
        .filter(|pkg| {
            let pkg_name = &pkg.id.name;
            // Check if any other package depends on this one
            let has_dependents = packages.iter().any(|other| {
                if other.id.name == *pkg_name {
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

            if has_dependents {
                return false;
            }

            // Also check shared lib needs
            let provides: std::collections::HashSet<&str> =
                pkg.provides_libs.keys().map(|s| s.as_str()).collect();
            !packages.iter().any(|other| {
                other.id.name != *pkg_name
                    && other
                        .needs_libs
                        .iter()
                        .any(|lib| provides.contains(lib.as_str()))
            })
        })
        .collect();

    if orphans.is_empty() {
        println!("No orphaned packages found.");
    } else {
        println!("Orphaned packages ({}):", orphans.len());
        for pkg in &orphans {
            println!(
                "  {}-{} (from {})",
                pkg.id.name, pkg.id.version, pkg.id.source
            );
        }
        println!("\nTo remove orphans: zl remove <name> --cascade");
    }

    Ok(())
}
