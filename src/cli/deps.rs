use std::collections::HashSet;

use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{PackageCandidate, PluginRegistry, SourcePlugin};

/// A single entry in the install plan
#[derive(Debug, Clone)]
pub struct InstallEntry {
    pub candidate: PackageCandidate,
    /// true if this is the package the user explicitly requested
    pub explicit: bool,
}

/// The result of dependency resolution: an ordered install plan
#[derive(Debug)]
pub struct InstallPlan {
    /// Packages to install, in dependency-first order (deps before dependents)
    pub packages: Vec<InstallEntry>,
    /// Dependencies that could not be resolved (name strings)
    pub unresolvable: Vec<String>,
    /// Packages that are already installed (skipped)
    pub already_installed: Vec<String>,
}

impl InstallPlan {
    pub fn total_installed_size(&self) -> u64 {
        self.packages
            .iter()
            .map(|e| e.candidate.installed_size)
            .sum()
    }

    pub fn dep_count(&self) -> usize {
        self.packages.iter().filter(|e| !e.explicit).count()
    }
}

/// Strip version constraint from a dependency string.
/// "glibc>=2.33" -> "glibc", "openssl>1.0" -> "openssl", "sh" -> "sh"
fn strip_version_constraint(dep: &str) -> &str {
    for (i, c) in dep.char_indices() {
        if c == '>' || c == '<' || c == '=' || c == ':' {
            return &dep[..i];
        }
    }
    dep
}

/// Resolve a package and all its transitive dependencies.
/// Returns an InstallPlan with packages in dependency-first order.
pub fn resolve_with_deps(
    name: &str,
    version: Option<&str>,
    source_name: Option<&str>,
    db: &ZlDatabase,
    registry: &PluginRegistry,
) -> ZlResult<InstallPlan> {
    let plugin = registry
        .get_or_default(source_name)
        .ok_or_else(|| ZlError::Plugin {
            plugin: source_name.unwrap_or("default").into(),
            message: "No matching source plugin found".into(),
        })?;

    // Resolve the target package
    let target = plugin
        .resolve(name, version)?
        .ok_or_else(|| ZlError::PackageNotFound {
            name: name.to_string(),
        })?;

    let mut plan_entries: Vec<InstallEntry> = Vec::new();
    let mut resolved: HashSet<String> = HashSet::new();
    let mut resolving_stack: Vec<String> = Vec::new();
    let mut unresolvable: Vec<String> = Vec::new();
    let mut already_installed: Vec<String> = Vec::new();

    resolve_recursive(
        &target,
        true,
        plugin,
        db,
        &mut resolved,
        &mut resolving_stack,
        &mut plan_entries,
        &mut unresolvable,
        &mut already_installed,
    )?;

    Ok(InstallPlan {
        packages: plan_entries,
        unresolvable,
        already_installed,
    })
}

fn resolve_recursive(
    candidate: &PackageCandidate,
    explicit: bool,
    plugin: &dyn SourcePlugin,
    db: &ZlDatabase,
    resolved: &mut HashSet<String>,
    resolving_stack: &mut Vec<String>,
    plan: &mut Vec<InstallEntry>,
    unresolvable: &mut Vec<String>,
    already_installed: &mut Vec<String>,
) -> ZlResult<()> {
    let name = &candidate.name;

    // Already in our resolved set for this run
    if resolved.contains(name) {
        return Ok(());
    }

    // Already installed in the database
    if db.get_package_by_name(name)?.is_some() {
        tracing::debug!("{} is already installed, skipping", name);
        already_installed.push(name.clone());
        resolved.insert(name.clone());
        return Ok(());
    }

    // Cycle detection
    if resolving_stack.contains(name) {
        let mut cycle_chain: Vec<String> = resolving_stack
            .iter()
            .skip_while(|n| n.as_str() != name)
            .cloned()
            .collect();
        cycle_chain.push(name.clone());
        return Err(ZlError::DependencyCycle { chain: cycle_chain });
    }

    resolving_stack.push(name.clone());

    // Resolve each dependency first (depth-first)
    for dep_str in &candidate.dependencies {
        let dep_name = strip_version_constraint(dep_str);

        // Skip if already resolved or installed
        if resolved.contains(dep_name) {
            continue;
        }
        if db.get_package_by_name(dep_name)?.is_some() {
            already_installed.push(dep_name.to_string());
            resolved.insert(dep_name.to_string());
            continue;
        }

        // Try to resolve the dependency via the plugin
        match plugin.resolve(dep_name, None)? {
            Some(dep_candidate) => {
                resolve_recursive(
                    &dep_candidate,
                    false, // dependencies are implicit
                    plugin,
                    db,
                    resolved,
                    resolving_stack,
                    plan,
                    unresolvable,
                    already_installed,
                )?;
            }
            None => {
                // Dependency not found in this source — track but don't fail
                if !unresolvable.contains(&dep_str.to_string()) {
                    unresolvable.push(dep_str.clone());
                }
            }
        }
    }

    // Add this package to the plan (after its deps, so deps-first order)
    resolved.insert(name.clone());
    plan.push(InstallEntry {
        candidate: candidate.clone(),
        explicit,
    });

    resolving_stack.pop();
    Ok(())
}

/// Display the install plan to the user
pub fn display_plan(plan: &InstallPlan) {
    let dep_count = plan.dep_count();
    let explicit_count = plan.packages.len() - dep_count;

    if dep_count > 0 {
        println!("\nDependencies to install ({}):", dep_count);
        for entry in &plan.packages {
            if !entry.explicit {
                println!(
                    "  {} {} ({:.1} MB)",
                    entry.candidate.name,
                    entry.candidate.version,
                    entry.candidate.installed_size as f64 / 1_000_000.0
                );
            }
        }
    }

    println!("\nPackages to install ({}):", explicit_count);
    for entry in &plan.packages {
        if entry.explicit {
            println!(
                "  {} {} ({:.1} MB)",
                entry.candidate.name,
                entry.candidate.version,
                entry.candidate.installed_size as f64 / 1_000_000.0
            );
        }
    }

    if !plan.already_installed.is_empty() {
        println!(
            "\nAlready installed ({}): {}",
            plan.already_installed.len(),
            plan.already_installed.join(", ")
        );
    }

    if !plan.unresolvable.is_empty() {
        println!("\nCould not resolve ({}):", plan.unresolvable.len());
        for dep in &plan.unresolvable {
            println!("  - {}", dep);
        }
    }

    println!(
        "\nTotal installed size: {:.1} MB",
        plan.total_installed_size() as f64 / 1_000_000.0
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_version_constraint() {
        assert_eq!(strip_version_constraint("glibc>=2.33"), "glibc");
        assert_eq!(strip_version_constraint("openssl>1.0"), "openssl");
        assert_eq!(strip_version_constraint("libfoo<=3.0"), "libfoo");
        assert_eq!(strip_version_constraint("sh=5.2"), "sh");
        assert_eq!(strip_version_constraint("zlib"), "zlib");
        assert_eq!(strip_version_constraint("python:3"), "python");
    }
}
