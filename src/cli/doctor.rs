//! `zl doctor` — diagnose system and ZL health.
//!
//! Checks:
//! - Database integrity (can read all packages)
//! - Broken symlinks in bin/ and lib/
//! - Missing shared libraries for installed packages
//! - Orphaned packages (deps no longer needed)
//! - Disk usage summary
//! - System profile consistency

use console::style;

use crate::core::db::ops::ZlDatabase;
use crate::error::ZlResult;
use crate::system::SystemProfile;

use super::AppContext;

pub fn handle(ctx: &AppContext) -> ZlResult<()> {
    println!("{}", style("ZL Doctor — System Diagnostics").bold().cyan());
    println!();

    let mut issues = 0;
    let mut warnings = 0;

    // 1. Database check
    print!("  Checking database... ");
    match check_database(ctx.db) {
        Ok(count) => println!("{} ({} packages)", style("OK").green(), count),
        Err(e) => {
            println!("{} ({})", style("ERROR").red(), e);
            issues += 1;
        }
    }

    // 2. Broken symlinks in bin/
    print!("  Checking bin/ symlinks... ");
    let broken_bins = check_broken_symlinks(&ctx.paths.bin);
    if broken_bins.is_empty() {
        println!("{}", style("OK").green());
    } else {
        println!("{} ({} broken)", style("WARN").yellow(), broken_bins.len());
        for path in &broken_bins {
            println!("    -> {}", path);
        }
        warnings += broken_bins.len();
    }

    // 3. Broken symlinks in lib/
    print!("  Checking lib/ symlinks... ");
    let broken_libs = check_broken_symlinks(&ctx.paths.lib);
    if broken_libs.is_empty() {
        println!("{}", style("OK").green());
    } else {
        println!("{} ({} broken)", style("WARN").yellow(), broken_libs.len());
        for path in &broken_libs {
            println!("    -> {}", path);
        }
        warnings += broken_libs.len();
    }

    // 4. Missing shared libraries
    print!("  Checking shared library deps... ");
    let missing = check_missing_libs(ctx.db, ctx.profile)?;
    if missing.is_empty() {
        println!("{}", style("OK").green());
    } else {
        println!(
            "{} ({} missing across packages)",
            style("WARN").yellow(),
            missing.len()
        );
        for (pkg, lib) in &missing {
            println!("    {} needs {}", pkg, lib);
        }
        warnings += missing.len();
    }

    // 5. Orphaned packages
    print!("  Checking for orphans... ");
    let orphans = check_orphans(ctx.db)?;
    if orphans.is_empty() {
        println!("{}", style("OK").green());
    } else {
        println!("{} ({} orphaned)", style("INFO").blue(), orphans.len());
        for name in &orphans {
            println!("    - {}", name);
        }
        println!("    hint: remove with `zl remove <pkg> --cascade` or reinstall as explicit");
    }

    // 6. Disk usage
    print!("  Computing disk usage... ");
    let total_size = compute_total_size(&ctx.paths.packages);
    let cache_size = compute_total_size(&ctx.paths.cache);
    println!(
        "{} packages, {} cache",
        format_size(total_size),
        format_size(cache_size)
    );

    // 7. System profile
    println!("  System: {} {}", ctx.profile.arch, ctx.profile.libc);
    println!(
        "  Layout: {} ({} lib dirs)",
        ctx.profile.layout,
        ctx.profile.lib_dirs.len()
    );
    println!("  Interpreter: {}", ctx.profile.interpreter.display());

    // Summary
    println!();
    if issues > 0 {
        println!(
            "{} {} issue(s) found. Run with -vv for details.",
            style("!").red().bold(),
            issues
        );
    } else if warnings > 0 {
        println!(
            "{} {} warning(s). Everything functional but some cleanup recommended.",
            style("~").yellow().bold(),
            warnings
        );
    } else {
        println!("{} Everything looks healthy!", style("✓").green().bold());
    }

    Ok(())
}

fn check_database(db: &ZlDatabase) -> ZlResult<usize> {
    let packages = db.list_packages()?;
    Ok(packages.len())
}

fn check_broken_symlinks(dir: &std::path::Path) -> Vec<String> {
    let mut broken = Vec::new();
    if !dir.is_dir() {
        return broken;
    }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.symlink_metadata().is_ok() && !path.exists() {
                broken.push(path.to_string_lossy().into_owned());
            }
        }
    }
    broken
}

fn check_missing_libs(db: &ZlDatabase, profile: &SystemProfile) -> ZlResult<Vec<(String, String)>> {
    let mut missing = Vec::new();
    let packages = db.list_packages()?;

    for pkg in &packages {
        for lib in &pkg.needs_libs {
            if db.lib_provider(lib)?.is_some() {
                continue;
            }
            if profile.system_lib_exists(lib) {
                continue;
            }
            if pkg.provides_libs.contains_key(lib) {
                continue;
            }
            missing.push((pkg.id.name.clone(), lib.clone()));
        }
    }

    Ok(missing)
}

fn check_orphans(db: &ZlDatabase) -> ZlResult<Vec<String>> {
    let packages = db.list_packages()?;
    let mut orphans = Vec::new();

    for pkg in &packages {
        if pkg.explicit {
            continue;
        }

        let has_dependents = packages.iter().any(|other| {
            if other.id.name == pkg.id.name {
                return false;
            }
            let key = format!("{}-{}", other.id.name, other.id.version);
            db.get_dependencies(&key)
                .unwrap_or_default()
                .iter()
                .any(|dep| {
                    let dep_name = dep.split(&['>', '<', '=', ':'][..]).next().unwrap_or(dep);
                    dep_name == pkg.id.name
                })
        });

        if !has_dependents {
            orphans.push(format!("{}-{}", pkg.id.name, pkg.id.version));
        }
    }

    Ok(orphans)
}

fn compute_total_size(dir: &std::path::Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
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
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1500), "1.5 KB");
        assert_eq!(format_size(1_500_000), "1.5 MB");
        assert_eq!(format_size(1_500_000_000), "1.5 GB");
    }

    #[test]
    fn test_check_broken_symlinks_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let broken = check_broken_symlinks(tmp.path());
        assert!(broken.is_empty());
    }

    #[test]
    fn test_check_broken_symlinks_finds_broken() {
        let tmp = tempfile::tempdir().unwrap();
        let link = tmp.path().join("broken_link");
        std::os::unix::fs::symlink("/nonexistent/target", &link).unwrap();
        let broken = check_broken_symlinks(tmp.path());
        assert_eq!(broken.len(), 1);
    }

    #[test]
    fn test_check_database() {
        let db_file = tempfile::NamedTempFile::new().unwrap();
        let db = ZlDatabase::open(db_file.path()).unwrap();
        let count = check_database(&db).unwrap();
        assert_eq!(count, 0);
    }
}
