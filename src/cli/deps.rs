use std::collections::HashSet;
use std::path::Path;

use console::style;

use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{PackageCandidate, PluginRegistry, SourcePlugin};
use crate::system::SystemProfile;

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
///
/// When a dependency is not found in the primary source, queries all other
/// registered plugins and lets the user choose where to install it from.
pub fn resolve_with_deps(
    name: &str,
    version: Option<&str>,
    source_name: Option<&str>,
    db: &ZlDatabase,
    registry: &PluginRegistry,
    profile: &SystemProfile,
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
        registry,
        profile,
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

#[allow(clippy::too_many_arguments)]
fn resolve_recursive(
    candidate: &PackageCandidate,
    explicit: bool,
    plugin: &dyn SourcePlugin,
    db: &ZlDatabase,
    registry: &PluginRegistry,
    profile: &SystemProfile,
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

    // Already installed in the ZL database
    if db.get_package_by_name(name)?.is_some() {
        tracing::debug!("{} is already installed, skipping", name);
        already_installed.push(name.clone());
        resolved.insert(name.clone());
        return Ok(());
    }

    // Already provided by the host system (e.g. libc6 on Arch = libc.so.6 already present)
    if is_system_provided(name, &profile.lib_dirs) {
        tracing::debug!("{} is provided by the host system, skipping", name);
        already_installed.push(format!("{} (system)", name));
        resolved.insert(name.clone());
        return Ok(());
    }

    // Cycle detection — circular deps (e.g. libc6 ↔ libgcc-s1 in Debian) are common
    // in base system packages. Treat as co-dependency: the package is already being
    // resolved and will be installed, so just skip to avoid infinite recursion.
    if resolving_stack.contains(name) {
        tracing::debug!(
            "Circular dependency: {} is already being resolved (co-dependency), skipping",
            name
        );
        return Ok(());
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

        // Skip if provided by the host system
        if is_system_provided(dep_name, &profile.lib_dirs) {
            tracing::debug!("Dep '{}' provided by host system, skipping", dep_name);
            resolved.insert(dep_name.to_string());
            continue;
        }

        // Try to resolve the dependency via the primary plugin
        match plugin.resolve(dep_name, None)? {
            Some(dep_candidate) => {
                resolve_recursive(
                    &dep_candidate,
                    false,
                    plugin,
                    db,
                    registry,
                    profile,
                    resolved,
                    resolving_stack,
                    plan,
                    unresolvable,
                    already_installed,
                )?;
            }
            None => {
                // Cross-source resolution: try other plugins
                if let Some(cross_candidate) =
                    try_cross_source_resolve(dep_name, plugin.name(), registry)?
                {
                    let cross_plugin = registry.get(&cross_candidate.source).unwrap_or(plugin);
                    resolve_recursive(
                        &cross_candidate,
                        false,
                        cross_plugin,
                        db,
                        registry,
                        profile,
                        resolved,
                        resolving_stack,
                        plan,
                        unresolvable,
                        already_installed,
                    )?;
                } else if !unresolvable.contains(&dep_str.to_string()) {
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

/// Try to resolve a dependency from other sources when the primary source fails.
/// Presents the user with options if found in multiple sources.
fn try_cross_source_resolve(
    dep_name: &str,
    primary_source: &str,
    registry: &PluginRegistry,
) -> ZlResult<Option<PackageCandidate>> {
    let mut found: Vec<(String, PackageCandidate)> = Vec::new();

    for plugin in registry.all() {
        if plugin.name() == primary_source {
            continue;
        }
        match plugin.resolve(dep_name, None) {
            Ok(Some(candidate)) => {
                found.push((plugin.name().to_string(), candidate));
            }
            _ => continue,
        }
    }

    match found.len() {
        0 => Ok(None),
        1 => {
            let (source, candidate) = found.into_iter().next().unwrap();
            eprintln!(
                "  {} Dependency '{}' not in primary source, found in {}",
                style("~").yellow(),
                dep_name,
                style(&source).cyan()
            );
            Ok(Some(candidate))
        }
        _ => {
            // Multiple sources — let user choose
            eprintln!(
                "\n  {} Dependency '{}' not in primary source, found in {} other source(s):",
                style("?").yellow().bold(),
                style(dep_name).bold(),
                found.len()
            );

            let items: Vec<String> = found
                .iter()
                .map(|(source, c)| format!("{} {} [{}]", c.name, c.version, source))
                .collect();

            // Add "skip" option
            let mut all_items: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
            all_items.push("Skip (don't install this dependency)");

            let selection =
                dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt(format!("Install '{}' from", dep_name))
                    .items(&all_items)
                    .default(0)
                    .interact()
                    .unwrap_or(all_items.len() - 1); // default to skip on error

            if selection >= found.len() {
                Ok(None) // user chose skip
            } else {
                let (_, candidate) = found.into_iter().nth(selection).unwrap();
                Ok(Some(candidate))
            }
        }
    }
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

fn lib_dir_contains_lib(lib_dir: &Path, base: &str) -> bool {
    std::fs::read_dir(lib_dir)
        .ok()
        .map(|entries| {
            entries.flatten().any(|e| {
                let fname = e.file_name();
                let s = fname.to_string_lossy();
                s.starts_with(base) && s.contains(".so")
            })
        })
        .unwrap_or(false)
}

/// Check if a dependency is already provided by the host system.
///
/// This prevents downloading Ubuntu/Debian system libs (libc6, libgcc-s1,
/// zlib1g, etc.) when the equivalent library is already present on the host
/// (e.g. libc.so.6, libgcc_s.so.1, libz.so.1 on Arch Linux).
///
/// Two checks:
/// 1. Pure metadata packages (no binaries) — always skip on non-Debian systems
/// 2. Library packages — check if a matching `.so` file exists in system lib dirs
fn is_system_provided(dep_name: &str, lib_dirs: &[std::path::PathBuf]) -> bool {
    // Pure metadata/config packages with no installable binaries
    const METADATA_ONLY: &[&str] = &[
        "debconf",
        "tzdata",
        "netbase",
        "media-types",
        "base-files",
        "readline-common",
        "sensible-utils",
        "lsb-base",
        "init-system-helpers",
        "dpkg",
        "apt",
        "perl-base",
        "perl",
        "libperl5.38",
        "ucf",
        "adduser",
        "login",
        "passwd",
    ];
    if METADATA_ONLY.contains(&dep_name) {
        return true;
    }

    // Library packages: derive the base lib name and check system lib dirs.
    // Try both underscore form ("libgcc_s") and hyphen form ("libpcre2-8")
    // since different libraries use different conventions in their soname.
    if dep_name.starts_with("lib") {
        let base_underscore = derive_lib_base(dep_name);
        let base_hyphen = base_underscore.replace('_', "-");
        for lib_dir in lib_dirs {
            if lib_dir_contains_lib(lib_dir, &base_underscore)
                || lib_dir_contains_lib(lib_dir, &base_hyphen)
            {
                return true;
            }
        }
    }

    false
}

/// Derive the base library filename prefix from an APT/dpkg package name.
///
/// Examples:
/// - `libc6`        → `libc`
/// - `libgcc-s1`    → `libgcc_s`
/// - `libssl3t64`   → `libssl`
/// - `libacl1`      → `libacl`
/// - `libbz2-1.0`   → `libbz2`
/// - `libncursesw6` → `libncursesw`
/// - `libpcre2-8-0` → `libpcre2_8`
fn derive_lib_base(pkg_name: &str) -> String {
    let mut s = pkg_name.to_string();
    let mut stripped_hyphen_version = false;

    // Strip common Debian multiarch/compat suffixes (t64, i386, amd64, arm64, armhf)
    for suffix in &["t64", "i386", "amd64", "arm64", "armhf", "armel"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
            break;
        }
    }

    // Strip trailing hyphen-version suffix if everything after the last `-` is digits/dots.
    // e.g. "libbz2-1.0" → strip "-1.0" → "libbz2"
    //      "libpcre2-8-0" → strip "-0" → "libpcre2-8"
    if let Some(pos) = s.rfind('-') {
        let after = &s[pos + 1..];
        if !after.is_empty() && after.chars().all(|c| c.is_ascii_digit() || c == '.') {
            s.truncate(pos);
            stripped_hyphen_version = true;
        }
    }

    // Trim trailing standalone digits only if we haven't already stripped a hyphen-version.
    // e.g. "libc6" → "libc", "libgcc-s1" → "libgcc-s", but "libbz2" stays "libbz2"
    let base = if !stripped_hyphen_version {
        s.trim_end_matches(|c: char| c.is_ascii_digit()).to_string()
    } else {
        s
    };

    // Replace hyphens with underscores to match soname convention (libgcc-s → libgcc_s)
    base.replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_lib_base() {
        assert_eq!(derive_lib_base("libc6"), "libc");
        assert_eq!(derive_lib_base("libgcc-s1"), "libgcc_s");
        assert_eq!(derive_lib_base("libssl3t64"), "libssl");
        assert_eq!(derive_lib_base("libacl1"), "libacl");
        assert_eq!(derive_lib_base("libbz2-1.0"), "libbz2");
        assert_eq!(derive_lib_base("libncursesw6"), "libncursesw");
        assert_eq!(derive_lib_base("libpcre2-8-0"), "libpcre2_8");
    }

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
