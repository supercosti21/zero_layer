use std::sync::Mutex;

use console::style;

use crate::error::ZlResult;
use crate::plugin::{PackageCandidate, PluginRegistry, SourcePlugin};

use super::{SearchArgs, SortOrder};

/// Maximum results shown per source. Use --limit to override.
const DEFAULT_LIMIT: usize = 20;

/// A search result with relevance scoring
struct ScoredResult {
    candidate: PackageCandidate,
    score: u32,
    tag: &'static str,
}

/// Compute a relevance score for a candidate against the query.
///
/// Higher = more relevant:
/// - 100: exact name match
/// -  80: name starts with query
/// -  60: name contains query
/// -  30: description contains query
/// -  10: fallback (matched by plugin but not by our heuristics)
fn score_candidate(candidate: &PackageCandidate, query: &str) -> (u32, &'static str) {
    let name = candidate.name.to_lowercase();
    let q = query.to_lowercase();

    if name == q {
        (100, "exact")
    } else if name.starts_with(&q) {
        (80, "name")
    } else if name.contains(&q) {
        (60, "name")
    } else if candidate.description.to_lowercase().contains(&q) {
        (30, "desc")
    } else {
        (10, "")
    }
}

/// Search all plugins in parallel using thread::scope.
fn search_parallel<'a>(
    plugins: &[&'a dyn SourcePlugin],
    query: &str,
) -> Vec<(&'a dyn SourcePlugin, Vec<PackageCandidate>)> {
    let results: Mutex<Vec<(&dyn SourcePlugin, Vec<PackageCandidate>)>> =
        Mutex::new(Vec::with_capacity(plugins.len()));

    std::thread::scope(|scope| {
        let results = &results;
        let mut handles = Vec::new();

        for &plugin in plugins {
            handles.push(scope.spawn(move || {
                // Sync before searching
                if let Err(e) = plugin.sync() {
                    tracing::warn!("Failed to sync {}: {}", plugin.name(), e);
                    return;
                }

                match plugin.search(query) {
                    Ok(candidates) => {
                        if !candidates.is_empty() {
                            let mut r = results.lock().unwrap();
                            r.push((plugin, candidates));
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Search failed for {}: {}", plugin.name(), e);
                    }
                }
            }));
        }

        for handle in handles {
            if handle.join().is_err() {
                tracing::warn!("A search thread panicked");
            }
        }
    });

    results.into_inner().unwrap_or_default()
}

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

    // Search all plugins in parallel
    let all_results = search_parallel(&plugins, &args.query);

    if all_results.is_empty() {
        println!("No packages found for '{}'.", args.query);
        return Ok(());
    }

    let mut total_shown = 0;

    for (plugin, candidates) in &all_results {
        // Score and filter results
        let mut scored: Vec<ScoredResult> = candidates
            .iter()
            .map(|c| {
                let (score, tag) = score_candidate(c, &args.query);
                ScoredResult {
                    candidate: c.clone(),
                    score,
                    tag,
                }
            })
            .collect();

        // If --exact, keep only exact name matches
        if args.exact {
            scored.retain(|s| s.score == 100);
        }

        if scored.is_empty() {
            continue;
        }

        // Sort by requested order
        match args.sort {
            SortOrder::Relevance => scored.sort_by(|a, b| b.score.cmp(&a.score)),
            SortOrder::Name => {
                scored.sort_by(|a, b| a.candidate.name.cmp(&b.candidate.name));
            }
            SortOrder::Version => {
                scored.sort_by(|a, b| b.candidate.version.cmp(&a.candidate.version));
            }
        }

        let total_count = scored.len();
        let shown = total_count.min(limit);

        println!(
            "{} ({} result{})",
            style(format!("── {} ──", plugin.display_name()))
                .cyan()
                .bold(),
            total_count,
            if total_count == 1 { "" } else { "s" }
        );

        for entry in scored.iter().take(limit) {
            let tag_str = if entry.tag.is_empty() {
                String::new()
            } else {
                format!(" {}", style(format!("[{}]", entry.tag)).dim())
            };

            // Truncate description to 55 chars
            let desc: String = if entry.candidate.description.len() > 55 {
                format!("{}...", &entry.candidate.description[..52])
            } else {
                entry.candidate.description.clone()
            };

            let name_styled = if entry.score == 100 {
                style(format!("{:<30}", entry.candidate.name))
                    .green()
                    .bold()
                    .to_string()
            } else {
                style(format!("{:<30}", entry.candidate.name))
                    .white()
                    .to_string()
            };

            println!(
                "  {} {:<15} {}{}",
                name_styled,
                style(&entry.candidate.version).yellow(),
                desc,
                tag_str
            );
        }

        if total_count > limit {
            println!(
                "  ... and {} more (use --limit {} or --from {} to see all)",
                total_count - limit,
                total_count,
                plugin.name()
            );
        }
        println!();
        total_shown += shown;
    }

    if total_shown == 0 {
        if args.exact {
            println!(
                "No exact matches for '{}'. Try without --exact.",
                args.query
            );
        } else {
            println!("No packages found for '{}'.", args.query);
        }
    } else {
        println!(
            "{} result(s) shown across {} source(s).",
            total_shown,
            all_results.len()
        );
        if !args.exact {
            println!(
                "Tip: use `zl search {} --exact` for exact matches only.",
                args.query
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_exact_match() {
        let candidate = PackageCandidate {
            name: "firefox".into(),
            version: "120.0".into(),
            description: "Web browser".into(),
            arch: "x86_64".into(),
            source: "pacman".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };
        let (score, tag) = score_candidate(&candidate, "firefox");
        assert_eq!(score, 100);
        assert_eq!(tag, "exact");
    }

    #[test]
    fn test_score_starts_with() {
        let candidate = PackageCandidate {
            name: "firefox-esr".into(),
            version: "120.0".into(),
            description: "Web browser ESR".into(),
            arch: "x86_64".into(),
            source: "pacman".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };
        let (score, tag) = score_candidate(&candidate, "firefox");
        assert_eq!(score, 80);
        assert_eq!(tag, "name");
    }

    #[test]
    fn test_score_contains_in_name() {
        let candidate = PackageCandidate {
            name: "lib32-firefox".into(),
            version: "1.0".into(),
            description: "32-bit compat".into(),
            arch: "x86_64".into(),
            source: "pacman".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };
        let (score, tag) = score_candidate(&candidate, "firefox");
        assert_eq!(score, 60);
        assert_eq!(tag, "name");
    }

    #[test]
    fn test_score_description_only() {
        let candidate = PackageCandidate {
            name: "iceweasel".into(),
            version: "1.0".into(),
            description: "Rebranded firefox web browser".into(),
            arch: "x86_64".into(),
            source: "apt".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };
        let (score, tag) = score_candidate(&candidate, "firefox");
        assert_eq!(score, 30);
        assert_eq!(tag, "desc");
    }

    #[test]
    fn test_score_case_insensitive() {
        let candidate = PackageCandidate {
            name: "Firefox".into(),
            version: "1.0".into(),
            description: String::new(),
            arch: "x86_64".into(),
            source: "test".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };
        let (score, _) = score_candidate(&candidate, "firefox");
        assert_eq!(score, 100);
    }
}
