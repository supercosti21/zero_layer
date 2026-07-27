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

/// Convert days since the Unix epoch to a (year, month, day) UTC civil date.
///
/// Howard Hinnant's `civil_from_days`, which handles leap years and real month
/// lengths exactly. A 365-day-year / 30-day-month approximation drifts by more
/// than two weeks by 2026.
fn civil_from_days(days_since_epoch: u64) -> (u64, u64, u64) {
    // Shift the epoch to 0000-03-01 so leap days land at the end of the cycle
    let z = days_since_epoch + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097; // day of era, [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // March-based month, [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]

    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return "unknown".to_string();
    }
    const SECS_PER_DAY: u64 = 86_400;

    let (year, month, day) = civil_from_days(ts / SECS_PER_DAY);
    let secs_of_day = ts % SECS_PER_DAY;
    let hour = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;

    format!("{:04}-{:02}-{:02} {:02}:{:02}", year, month, day, hour, min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp() {
        // 2024-02-23 14:53:20 UTC
        assert_eq!(format_timestamp(1_708_700_000), "2024-02-23 14:53");
    }

    #[test]
    fn test_format_timestamp_epoch_and_boundaries() {
        assert_eq!(format_timestamp(1), "1970-01-01 00:00");
        // 2000-02-29: leap day of a century year that *is* a leap year
        assert_eq!(format_timestamp(951_782_400), "2000-02-29 00:00");
        // 2100-03-01: the day after 2100-02-28, a century year that is not leap
        assert_eq!(format_timestamp(4_107_542_400), "2100-03-01 00:00");
        // Last second of a year
        assert_eq!(format_timestamp(1_767_225_599), "2025-12-31 23:59");
    }

    #[test]
    fn test_format_timestamp_zero() {
        assert_eq!(format_timestamp(0), "unknown");
    }
}
