//! `zl diff <package>` — show what would change if a package is updated.

use console::style;

use crate::error::{ZlError, ZlResult};

use super::{AppContext, DiffArgs};

pub fn handle(args: DiffArgs, ctx: &AppContext) -> ZlResult<()> {
    let installed =
        ctx.db
            .get_package_by_name(&args.package)?
            .ok_or_else(|| ZlError::PackageNotFound {
                name: args.package.clone(),
            })?;

    let from = args.from.as_deref().unwrap_or(&installed.id.source);

    let plugin = ctx.registry.get(from).ok_or_else(|| ZlError::Plugin {
        plugin: from.to_string(),
        message: "Source plugin not found".into(),
    })?;

    // Sync and resolve latest
    plugin.sync()?;
    let latest = plugin
        .resolve(&args.package, None)?
        .ok_or_else(|| ZlError::PackageNotFound {
            name: args.package.clone(),
        })?;

    println!(
        "{} {} — installed vs latest\n",
        style(&args.package).bold(),
        style(format!("[{}]", from)).dim()
    );

    // Version diff
    if installed.id.version == latest.version {
        println!(
            "  Version: {} ({})",
            installed.id.version,
            style("up to date").green()
        );
        return Ok(());
    }

    println!(
        "  Version: {} -> {}",
        style(&installed.id.version).red(),
        style(&latest.version).green()
    );

    // Dependency diff
    let installed_deps: std::collections::HashSet<String> = ctx
        .db
        .get_dependencies(&format!("{}-{}", installed.id.name, installed.id.version))
        .unwrap_or_default()
        .into_iter()
        .collect();

    let new_deps: std::collections::HashSet<String> = latest.dependencies.iter().cloned().collect();

    let added: Vec<&String> = new_deps.difference(&installed_deps).collect();
    let removed: Vec<&String> = installed_deps.difference(&new_deps).collect();

    if !added.is_empty() {
        println!("\n  New dependencies:");
        for dep in &added {
            println!("    {} {}", style("+").green(), dep);
        }
    }

    if !removed.is_empty() {
        println!("\n  Removed dependencies:");
        for dep in &removed {
            println!("    {} {}", style("-").red(), dep);
        }
    }

    if added.is_empty() && removed.is_empty() {
        println!("  Dependencies: unchanged");
    }

    // Size comparison
    let current_size: u64 = installed
        .installed_files
        .iter()
        .filter_map(|f| std::fs::metadata(f).ok())
        .map(|m| m.len())
        .sum();

    println!(
        "\n  Current size: {:.1} MB ({} files)",
        current_size as f64 / 1_000_000.0,
        installed.installed_files.len()
    );

    if latest.installed_size > 0 {
        let delta = latest.installed_size as i64 - current_size as i64;
        let delta_str = if delta > 0 {
            format!("+{:.1} MB", delta as f64 / 1_000_000.0)
        } else if delta < 0 {
            format!("{:.1} MB", delta as f64 / 1_000_000.0)
        } else {
            "no change".to_string()
        };
        println!(
            "  New size: {:.1} MB ({})",
            latest.installed_size as f64 / 1_000_000.0,
            delta_str
        );
    }

    println!(
        "\n  hint: run `zl update {}` to apply this update",
        args.package
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_placeholder() {
        // Integration testing requires live plugin; unit tests are in individual modules
        assert!(true);
    }
}
