use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};

use super::InfoArgs;

pub fn handle(args: InfoArgs, db: &ZlDatabase) -> ZlResult<()> {
    let pkg = db
        .get_package_by_name(&args.package)?
        .ok_or_else(|| ZlError::PackageNotFound {
            name: args.package.clone(),
        })?;

    let pkg_key = format!("{}-{}", pkg.id.name, pkg.id.version);
    let deps = db.get_dependencies(&pkg_key).unwrap_or_default();
    let rdeps = db.reverse_dependencies(&pkg.id.name).unwrap_or_default();
    let is_pinned = db.is_pinned(&pkg.id.name).unwrap_or(false);

    println!("Name:         {}", pkg.id.name);
    println!("Version:      {}", pkg.id.version);
    println!("Source:       {}", pkg.id.source);
    println!(
        "Status:       {}{}",
        if pkg.explicit {
            "explicitly installed"
        } else {
            "installed as dependency"
        },
        if is_pinned { " [PINNED]" } else { "" }
    );
    println!(
        "Installed:    {}",
        format_timestamp(pkg.installed_at)
    );
    println!("Files:        {}", pkg.installed_files.len());

    // Shared libraries provided
    if !pkg.provides_libs.is_empty() {
        println!("Provides:");
        for (soname, path) in &pkg.provides_libs {
            println!("  {} -> {}", soname, path.display());
        }
    }

    // Libraries needed
    if !pkg.needs_libs.is_empty() {
        println!("Needs libs:   {}", pkg.needs_libs.join(", "));
    }

    // Dependencies
    if !deps.is_empty() {
        println!("Depends on:   {}", deps.join(", "));
    }

    // Reverse dependencies
    if !rdeps.is_empty() {
        println!("Required by:  {}", rdeps.join(", "));
    }

    // Total size on disk
    let total_size: u64 = pkg
        .installed_files
        .iter()
        .filter_map(|f| std::fs::metadata(f).ok())
        .map(|m| m.len())
        .sum();
    println!(
        "Disk usage:   {:.1} MB",
        total_size as f64 / 1_000_000.0
    );

    Ok(())
}

fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return "unknown".to_string();
    }
    // Simple date formatting without external crate
    let secs_per_day = 86400u64;
    let days_since_epoch = ts / secs_per_day;
    // Approximate date calculation
    let years = 1970 + days_since_epoch / 365;
    let remaining_days = days_since_epoch % 365;
    let months = remaining_days / 30 + 1;
    let days = remaining_days % 30 + 1;
    format!("{:04}-{:02}-{:02}", years, months, days)
}
