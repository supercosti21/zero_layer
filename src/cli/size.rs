//! `zl size` — show disk usage per package.

use console::style;

use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};

use super::SizeArgs;

pub fn handle(args: SizeArgs, db: &ZlDatabase) -> ZlResult<()> {
    if let Some(ref name) = args.package {
        return show_single(name, db);
    }

    let packages = db.list_packages()?;

    if packages.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }

    let mut entries: Vec<(String, String, u64, usize)> = packages
        .iter()
        .map(|pkg| {
            let size: u64 = pkg
                .installed_files
                .iter()
                .filter_map(|f| std::fs::metadata(f).ok())
                .map(|m| m.len())
                .sum();
            (
                pkg.id.name.clone(),
                pkg.id.version.clone(),
                size,
                pkg.installed_files.len(),
            )
        })
        .collect();

    if args.sort {
        entries.sort_by(|a, b| b.2.cmp(&a.2));
    } else {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
    }

    println!(
        "{:<30} {:<15} {:>10} {:>8}",
        style("Package").bold(),
        style("Version").bold(),
        style("Size").bold(),
        style("Files").bold()
    );
    println!("{}", "-".repeat(65));

    let mut total_size = 0u64;
    let mut total_files = 0usize;

    for (name, version, size, files) in &entries {
        println!(
            "{:<30} {:<15} {:>10} {:>8}",
            name,
            version,
            format_size(*size),
            files
        );
        total_size += size;
        total_files += files;
    }

    println!("{}", "-".repeat(65));
    println!(
        "{:<30} {:<15} {:>10} {:>8}",
        style("Total").bold(),
        format!("{} packages", entries.len()),
        style(format_size(total_size)).bold(),
        total_files
    );

    Ok(())
}

fn show_single(name: &str, db: &ZlDatabase) -> ZlResult<()> {
    let pkg = db
        .get_package_by_name(name)?
        .ok_or_else(|| ZlError::PackageNotFound {
            name: name.to_string(),
        })?;

    let mut file_sizes: Vec<(String, u64)> = pkg
        .installed_files
        .iter()
        .filter_map(|f| {
            std::fs::metadata(f)
                .ok()
                .map(|m| (f.to_string_lossy().into_owned(), m.len()))
        })
        .collect();

    file_sizes.sort_by(|a, b| b.1.cmp(&a.1));

    let total: u64 = file_sizes.iter().map(|(_, s)| s).sum();

    println!(
        "{}-{} — {} ({} files)\n",
        style(&pkg.id.name).bold(),
        pkg.id.version,
        format_size(total),
        pkg.installed_files.len()
    );

    // Show top 20 largest files
    println!("  Largest files:");
    for (path, size) in file_sizes.iter().take(20) {
        println!("    {:>10}  {}", format_size(*size), path);
    }

    if file_sizes.len() > 20 {
        println!("    ... and {} more files", file_sizes.len() - 20);
    }

    // Show lib breakdown
    if !pkg.provides_libs.is_empty() {
        println!("\n  Shared libraries provided:");
        for (soname, path) in &pkg.provides_libs {
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            println!(
                "    {:>10}  {} -> {}",
                format_size(size),
                soname,
                path.display()
            );
        }
    }

    // Dependency cost
    let deps = db
        .get_dependencies(&format!("{}-{}", pkg.id.name, pkg.id.version))
        .unwrap_or_default();
    if !deps.is_empty() {
        println!("\n  Dependencies ({}):", deps.len());
        let mut dep_total = 0u64;
        for dep_name in &deps {
            if let Some(dep_pkg) = db.get_package_by_name(dep_name).ok().flatten() {
                let size: u64 = dep_pkg
                    .installed_files
                    .iter()
                    .filter_map(|f| std::fs::metadata(f).ok())
                    .map(|m| m.len())
                    .sum();
                println!(
                    "    {:>10}  {}-{}",
                    format_size(size),
                    dep_pkg.id.name,
                    dep_pkg.id.version
                );
                dep_total += size;
            }
        }
        println!("\n  Total with deps: {}", format_size(total + dep_total));
    }

    Ok(())
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.1} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1500), "1.5 KB");
        assert_eq!(format_size(1_500_000), "1.5 MB");
    }
}
