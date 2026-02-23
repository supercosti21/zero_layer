//! `zl history` — show install/remove history and rollback changes.

use crate::core::db::ops::ZlDatabase;
use crate::error::ZlResult;

use super::{AppContext, HistoryCommand, RollbackArgs};

pub fn handle(cmd: HistoryCommand, ctx: &AppContext) -> ZlResult<()> {
    match cmd {
        HistoryCommand::List => handle_list(ctx.db),
        HistoryCommand::Rollback(args) => handle_rollback(args, ctx),
    }
}

fn handle_list(db: &ZlDatabase) -> ZlResult<()> {
    let entries = db.list_history(50)?;

    if entries.is_empty() {
        println!("No history recorded yet.");
        return Ok(());
    }

    println!("{:<22} {:<10} Packages", "Date", "Action");
    println!("{}", "-".repeat(70));

    for entry in &entries {
        let date = format_timestamp(entry.timestamp);
        let pkgs = entry.packages.join(", ");
        println!("{:<22} {:<10} {}", date, entry.action, pkgs);
    }

    Ok(())
}

fn handle_rollback(args: RollbackArgs, ctx: &AppContext) -> ZlResult<()> {
    use crate::core::db::ops::HistoryAction;

    let entries = ctx.db.list_history(args.count)?;

    if entries.is_empty() {
        println!("No history to rollback.");
        return Ok(());
    }

    println!(
        "Rolling back {} operation(s)...\n",
        entries.len().min(args.count)
    );

    for entry in entries.iter().take(args.count) {
        match entry.action {
            HistoryAction::Install => {
                // Undo install = remove the packages
                println!("  Undoing install of: {}", entry.packages.join(", "));
                for pkg_name in &entry.packages {
                    // Parse "name-version" into name
                    let name = pkg_name
                        .rfind('-')
                        .map(|pos| &pkg_name[..pos])
                        .unwrap_or(pkg_name);
                    if let Some(node) = ctx.db.get_package_by_name(name)? {
                        let pkg_key = format!("{}-{}", node.id.name, node.id.version);
                        let pkg_dir = ctx.paths.packages.join(&pkg_key);

                        // Remove bin symlinks
                        super::remove::remove_bin_symlinks_public(
                            &node.installed_files,
                            &ctx.paths.bin,
                        )?;

                        // Remove lib symlinks
                        for soname in node.provides_libs.keys() {
                            let link = ctx.paths.lib.join(soname);
                            if link.symlink_metadata().is_ok() {
                                std::fs::remove_file(&link)?;
                            }
                        }

                        // Remove package dir
                        if pkg_dir.exists() {
                            std::fs::remove_dir_all(&pkg_dir)?;
                        }

                        // Remove from DB
                        ctx.db.remove_files_for_package(&pkg_key)?;
                        ctx.db.remove_dependencies(&pkg_key)?;
                        ctx.db.remove_package(&node.id.name, &node.id.version)?;

                        println!("    Removed {}", pkg_key);
                    }
                }
            }
            HistoryAction::Remove => {
                // Undo remove = we can't restore deleted packages
                println!(
                    "  Cannot undo removal of: {} (packages already deleted)",
                    entry.packages.join(", ")
                );
                println!("    hint: reinstall them with `zl install`");
            }
            HistoryAction::Upgrade => {
                println!(
                    "  Cannot undo upgrade of: {} (previous version not cached)",
                    entry.packages.join(", ")
                );
                println!("    hint: install the old version explicitly with --version");
            }
            HistoryAction::Rollback => {
                println!("  Skipping rollback entry (already a rollback)");
            }
        }
    }

    // Record this rollback in history
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    ctx.db.record_history(&crate::core::db::ops::HistoryEntry {
        timestamp: now,
        action: HistoryAction::Rollback,
        packages: entries
            .iter()
            .take(args.count)
            .flat_map(|e| e.packages.clone())
            .collect(),
    })?;

    println!("\nRollback complete.");
    Ok(())
}

fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return "unknown".to_string();
    }
    let secs_per_day = 86400u64;
    let days_since_epoch = ts / secs_per_day;
    let years = 1970 + days_since_epoch / 365;
    let remaining_days = days_since_epoch % 365;
    let months = remaining_days / 30 + 1;
    let days = remaining_days % 30 + 1;
    let hour = (ts % secs_per_day) / 3600;
    let min = (ts % 3600) / 60;
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        years, months, days, hour, min
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp() {
        let ts = 1708700000; // ~Feb 2024
        let s = format_timestamp(ts);
        assert!(s.contains("2024"));
        assert!(s.contains(":"));
    }

    #[test]
    fn test_format_timestamp_zero() {
        assert_eq!(format_timestamp(0), "unknown");
    }
}
