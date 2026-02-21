use crate::error::ZlResult;
use crate::plugin::PluginRegistry;

use super::SearchArgs;

/// Maximum results shown per source. Use --limit to override.
const DEFAULT_LIMIT: usize = 20;

pub fn handle(args: SearchArgs, registry: &PluginRegistry) -> ZlResult<()> {
    let plugins = match args.from.as_deref() {
        Some(name) => match registry.get(name) {
            Some(p) => vec![p],
            None => {
                eprintln!("Unknown source: {}", name);
                return Ok(());
            }
        },
        None => registry.all(),
    };

    let limit = args.limit.unwrap_or(DEFAULT_LIMIT);
    let mut total = 0;

    for plugin in plugins {
        // Sync before searching
        if let Err(e) = plugin.sync() {
            tracing::warn!("Failed to sync {}: {}", plugin.name(), e);
            continue;
        }

        match plugin.search(&args.query) {
            Ok(results) => {
                if results.is_empty() {
                    continue;
                }

                let shown = results.len().min(limit);
                println!("── {} ──", plugin.display_name());
                for candidate in results.iter().take(limit) {
                    println!(
                        "  {:<30} {:<15} {}",
                        candidate.name, candidate.version, candidate.description
                    );
                }
                if results.len() > limit {
                    println!(
                        "  ... and {} more (use --limit N or --from {} to narrow down)",
                        results.len() - limit,
                        plugin.name()
                    );
                }
                println!();
                total += shown;
            }
            Err(e) => {
                tracing::warn!("Search failed for {}: {}", plugin.name(), e);
            }
        }
    }

    if total == 0 {
        println!("No packages found for '{}'.", args.query);
    } else {
        println!("{} result(s) shown.", total);
    }

    Ok(())
}
