use crate::error::ZlResult;
use crate::plugin::PluginRegistry;

use super::SearchArgs;

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

                println!("── {} ──", plugin.display_name());
                for candidate in &results {
                    println!(
                        "  {:<30} {:<15} {}",
                        candidate.name, candidate.version, candidate.description
                    );
                }
                println!();
                total += results.len();
            }
            Err(e) => {
                tracing::warn!("Search failed for {}: {}", plugin.name(), e);
            }
        }
    }

    if total == 0 {
        println!("No packages found for '{}'.", args.query);
    } else {
        println!("{} result(s) found.", total);
    }

    Ok(())
}
