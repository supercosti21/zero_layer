use std::fmt;
use std::path::Path;

use crate::core::db::ops::ZlDatabase;
use crate::error::ZlResult;
use crate::paths::ZlPaths;
use crate::plugin::PackageCandidate;

// ── Conflict types ──

/// A single detected conflict between a candidate package and the installed set.
#[derive(Debug)]
pub enum Conflict {
    /// Two packages claim ownership of the same filesystem path.
    FileOwnership {
        path: String,
        existing_owner: String,
        new_package: String,
    },
    /// Two packages provide a binary with the same name in the `bin/` directory.
    BinaryName {
        name: String,
        existing_package: String,
        new_package: String,
    },
    /// Two packages provide a shared library with the same soname in the `lib/` directory.
    LibrarySoname {
        soname: String,
        existing_package: String,
        new_package: String,
    },
    /// The candidate's `conflicts` field lists an installed package.
    DeclaredConflict {
        package: String,
        conflicts_with: String,
    },
    /// A dependency version constraint cannot be satisfied by the installed version.
    VersionConflict {
        dependency: String,
        required_by: String,
        required_version: String,
        installed_version: String,
    },
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Conflict::FileOwnership {
                path,
                existing_owner,
                new_package,
            } => write!(
                f,
                "file conflict: '{path}' is owned by '{existing_owner}', \
                 also provided by '{new_package}'"
            ),
            Conflict::BinaryName {
                name,
                existing_package,
                new_package,
            } => write!(
                f,
                "binary conflict: '{name}' is provided by '{existing_package}', \
                 also provided by '{new_package}'"
            ),
            Conflict::LibrarySoname {
                soname,
                existing_package,
                new_package,
            } => write!(
                f,
                "library conflict: '{soname}' is provided by '{existing_package}', \
                 also provided by '{new_package}'"
            ),
            Conflict::DeclaredConflict {
                package,
                conflicts_with,
            } => write!(
                f,
                "declared conflict: '{package}' conflicts with installed '{conflicts_with}'"
            ),
            Conflict::VersionConflict {
                dependency,
                required_by,
                required_version,
                installed_version,
            } => write!(
                f,
                "version conflict: '{required_by}' requires '{dependency} {required_version}', \
                 but '{installed_version}' is installed"
            ),
        }
    }
}

// ── Conflict report ──

/// Aggregated result of a conflict check across one or more candidate packages.
#[derive(Debug)]
pub struct ConflictReport {
    pub conflicts: Vec<Conflict>,
}

impl ConflictReport {
    /// Returns `true` if there are any conflicts.
    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }

    /// Print a human-readable conflict report to stderr.
    pub fn display(&self) {
        if self.conflicts.is_empty() {
            return;
        }
        eprintln!("Conflicts detected ({}):", self.conflicts.len());
        for (i, conflict) in self.conflicts.iter().enumerate() {
            eprintln!("  {}. {}", i + 1, conflict);
        }
    }
}

// ── Main entry point ──

/// Check all candidate packages for conflicts against already-installed packages.
///
/// This examines:
/// 1. **File ownership** — whether any file the candidate would install is already owned
/// 2. **Binary name** — whether the candidate provides a binary name that already exists
/// 3. **Library soname** — whether the candidate provides a soname already provided
/// 4. **Declared conflicts** — whether the candidate's `conflicts` list matches installed packages
/// 5. **Version constraints** — whether dependency version constraints are satisfiable
pub fn check_conflicts(
    candidates: &[&PackageCandidate],
    db: &ZlDatabase,
    paths: &ZlPaths,
) -> ZlResult<ConflictReport> {
    let mut conflicts = Vec::new();

    let installed = db.list_packages()?;

    for candidate in candidates {
        let new_key = format!("{}-{}", candidate.name, candidate.version);

        // 1. File ownership conflicts
        // Check if any files the candidate would install are already owned.
        // We check common paths a package installs into (bin, lib, share, etc).
        check_file_ownership_conflicts(candidate, &new_key, db, paths, &mut conflicts)?;

        // 2. Binary name conflicts
        check_binary_conflicts(candidate, &new_key, paths, &mut conflicts)?;

        // 3. Library soname conflicts
        check_library_conflicts(candidate, &new_key, db, &mut conflicts)?;

        // 4. Declared conflicts
        for installed_pkg in &installed {
            let installed_name = &installed_pkg.id.name;
            // Check if the candidate declares a conflict with this installed package
            for conflict_pattern in &candidate.conflicts {
                if matches_package_name(conflict_pattern, installed_name) {
                    conflicts.push(Conflict::DeclaredConflict {
                        package: new_key.clone(),
                        conflicts_with: format!(
                            "{}-{}",
                            installed_pkg.id.name, installed_pkg.id.version
                        ),
                    });
                }
            }
        }

        // 5. Version conflicts
        // Check if the candidate's dependencies have version constraints that clash
        // with installed versions.
        for dep_spec in &candidate.dependencies {
            let (dep_name, constraint) = parse_dependency_spec(dep_spec);
            if let Some(constraint) = constraint {
                // Look up installed version of this dependency
                if let Some(installed_dep) = db.get_package_by_name(dep_name)? {
                    if !satisfies_constraint(&installed_dep.id.version, constraint) {
                        conflicts.push(Conflict::VersionConflict {
                            dependency: dep_name.to_string(),
                            required_by: new_key.clone(),
                            required_version: constraint.to_string(),
                            installed_version: installed_dep.id.version.clone(),
                        });
                    }
                }
            }
        }
    }

    Ok(ConflictReport { conflicts })
}

// ── File ownership conflicts ──

fn check_file_ownership_conflicts(
    candidate: &PackageCandidate,
    new_key: &str,
    db: &ZlDatabase,
    paths: &ZlPaths,
    conflicts: &mut Vec<Conflict>,
) -> ZlResult<()> {
    // Check expected file locations that would be installed
    let pkg_dir = paths.packages.join(format!("{}-{}", candidate.name, candidate.version));

    // If the package dir already exists on disk, scan its files
    if pkg_dir.is_dir() {
        scan_dir_for_ownership_conflicts(&pkg_dir, new_key, db, conflicts)?;
    }

    Ok(())
}

fn scan_dir_for_ownership_conflicts(
    dir: &Path,
    new_key: &str,
    db: &ZlDatabase,
    conflicts: &mut Vec<Conflict>,
) -> ZlResult<()> {
    let walker = walkdir::WalkDir::new(dir).into_iter().filter_map(|e| e.ok());
    for entry in walker {
        if entry.file_type().is_file() || entry.file_type().is_symlink() {
            let path_str = entry.path().to_string_lossy().to_string();
            if let Some(owner) = db.file_owner(&path_str)? {
                if owner != new_key {
                    conflicts.push(Conflict::FileOwnership {
                        path: path_str,
                        existing_owner: owner,
                        new_package: new_key.to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

// ── Binary name conflicts ──

fn check_binary_conflicts(
    candidate: &PackageCandidate,
    new_key: &str,
    paths: &ZlPaths,
    conflicts: &mut Vec<Conflict>,
) -> ZlResult<()> {
    // Check if a binary symlink by this package's name already exists and is owned
    // by a different package. We check the ZL bin directory.
    let bin_path = paths.bin.join(&candidate.name);
    if bin_path.symlink_metadata().is_ok() {
        // A binary with this name already exists — check who owns it
        let path_str = bin_path.to_string_lossy().to_string();
        // We read the symlink target to try to determine the owning package
        if let Ok(target) = std::fs::read_link(&bin_path) {
            let target_str = target.to_string_lossy();
            // Package dirs are named "name-version" under packages/
            if let Some(owner) = extract_package_key_from_path(&target_str) {
                if owner != new_key {
                    conflicts.push(Conflict::BinaryName {
                        name: candidate.name.clone(),
                        existing_package: owner,
                        new_package: new_key.to_string(),
                    });
                }
            }
        } else {
            // Not a symlink but a regular file — still a conflict
            conflicts.push(Conflict::BinaryName {
                name: candidate.name.clone(),
                existing_package: format!("(unknown owner of {})", path_str),
                new_package: new_key.to_string(),
            });
        }
    }

    // Also check the `provides` list: each provided name gets a symlink in bin/
    for provided in &candidate.provides {
        let provided_bin = paths.bin.join(provided);
        if provided_bin.symlink_metadata().is_ok() {
            if let Ok(target) = std::fs::read_link(&provided_bin) {
                let target_str = target.to_string_lossy();
                if let Some(owner) = extract_package_key_from_path(&target_str) {
                    if owner != new_key {
                        conflicts.push(Conflict::BinaryName {
                            name: provided.clone(),
                            existing_package: owner,
                            new_package: new_key.to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

// ── Library soname conflicts ──

fn check_library_conflicts(
    candidate: &PackageCandidate,
    new_key: &str,
    db: &ZlDatabase,
    conflicts: &mut Vec<Conflict>,
) -> ZlResult<()> {
    // Check if any soname provided by the candidate is already provided by another package
    for provided in &candidate.provides {
        // Library provides are typically sonames like "libfoo.so.3"
        if provided.contains(".so") {
            if let Some(existing_provider) = db.lib_provider(provided)? {
                if existing_provider != new_key {
                    conflicts.push(Conflict::LibrarySoname {
                        soname: provided.clone(),
                        existing_package: existing_provider,
                        new_package: new_key.to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

// ── Declared conflict matching ──

/// Check if a conflict pattern matches a package name.
/// Supports exact name matching (e.g. "firefox" matches "firefox").
fn matches_package_name(pattern: &str, package_name: &str) -> bool {
    // Strip any version constraint from the pattern (e.g. "foo>=2.0" => "foo")
    let (name_part, _) = parse_dependency_spec(pattern);
    name_part == package_name
}

// ── Version constraint parsing and checking ──

/// Parse a dependency specification like "glibc>=2.17" into ("glibc", Some(">=2.17")).
/// If there is no constraint, returns (full_spec, None).
fn parse_dependency_spec(spec: &str) -> (&str, Option<&str>) {
    // Find the first occurrence of a comparison operator
    for (i, _) in spec.char_indices() {
        let rest = &spec[i..];
        if rest.starts_with(">=")
            || rest.starts_with("<=")
            || rest.starts_with('>')
            || rest.starts_with('<')
            || rest.starts_with('=')
        {
            return (&spec[..i], Some(&spec[i..]));
        }
    }
    (spec, None)
}

/// Check whether `version` satisfies a version `constraint`.
///
/// Supported constraint formats:
/// - `>=X.Y` — greater than or equal
/// - `<=X.Y` — less than or equal
/// - `>X.Y`  — strictly greater
/// - `<X.Y`  — strictly less
/// - `=X.Y`  — exact match
/// - empty / no constraint — always satisfies
///
/// Versions are compared component-by-component (split on `.`).
/// Each component is compared numerically if both sides parse as u64,
/// otherwise compared lexicographically.
pub fn satisfies_constraint(version: &str, constraint: &str) -> bool {
    let constraint = constraint.trim();
    if constraint.is_empty() {
        return true;
    }

    let (op, req_version) = if constraint.starts_with(">=") {
        (">=", constraint[2..].trim())
    } else if constraint.starts_with("<=") {
        ("<=", constraint[2..].trim())
    } else if constraint.starts_with('>') {
        (">", constraint[1..].trim())
    } else if constraint.starts_with('<') {
        ("<", constraint[1..].trim())
    } else if constraint.starts_with('=') {
        ("=", constraint[1..].trim())
    } else {
        // No recognised operator — treat as exact match
        ("=", constraint)
    };

    let ordering = compare_versions(version, req_version);

    match op {
        ">=" => ordering != std::cmp::Ordering::Less,
        "<=" => ordering != std::cmp::Ordering::Greater,
        ">" => ordering == std::cmp::Ordering::Greater,
        "<" => ordering == std::cmp::Ordering::Less,
        "=" => ordering == std::cmp::Ordering::Equal,
        _ => false,
    }
}

/// Compare two version strings component-by-component.
/// Each component (split on `.`) is compared numerically if both sides are
/// valid integers, otherwise compared lexicographically.
fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts_a: Vec<&str> = a.split('.').collect();
    let parts_b: Vec<&str> = b.split('.').collect();

    let max_len = parts_a.len().max(parts_b.len());

    for i in 0..max_len {
        let pa = parts_a.get(i).copied().unwrap_or("0");
        let pb = parts_b.get(i).copied().unwrap_or("0");

        // Try numeric comparison first
        let ord = match (pa.parse::<u64>(), pb.parse::<u64>()) {
            (Ok(na), Ok(nb)) => na.cmp(&nb),
            _ => pa.cmp(pb),
        };

        if ord != std::cmp::Ordering::Equal {
            return ord;
        }
    }

    std::cmp::Ordering::Equal
}

/// Try to extract a package key ("name-version") from a file path
/// that lives under the `packages/` directory.
///
/// Example: `/home/user/.local/share/zl/packages/firefox-120.0/bin/firefox`
///          → `"firefox-120.0"`
fn extract_package_key_from_path(path: &str) -> Option<String> {
    let parts: Vec<&str> = path.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if *part == "packages" {
            if let Some(key) = parts.get(i + 1) {
                return Some(key.to_string());
            }
        }
    }
    None
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::ops::ZlDatabase;
    use crate::core::graph::model::{PackageId, PackageNode};
    use std::collections::HashMap;

    // ── Version constraint tests ──

    #[test]
    fn test_satisfies_constraint_gte() {
        assert!(satisfies_constraint("3.5", ">=3.0"));
        assert!(satisfies_constraint("3.0", ">=3.0"));
        assert!(!satisfies_constraint("2.9", ">=3.0"));
    }

    #[test]
    fn test_satisfies_constraint_lte() {
        assert!(satisfies_constraint("3.0", "<=3.0"));
        assert!(satisfies_constraint("2.5", "<=3.0"));
        assert!(!satisfies_constraint("3.1", "<=3.0"));
    }

    #[test]
    fn test_satisfies_constraint_gt() {
        assert!(satisfies_constraint("3.1", ">3.0"));
        assert!(!satisfies_constraint("3.0", ">3.0"));
        assert!(!satisfies_constraint("2.9", ">3.0"));
    }

    #[test]
    fn test_satisfies_constraint_lt() {
        assert!(satisfies_constraint("2.9", "<3.0"));
        assert!(!satisfies_constraint("3.0", "<3.0"));
        assert!(!satisfies_constraint("3.1", "<3.0"));
    }

    #[test]
    fn test_satisfies_constraint_eq() {
        assert!(satisfies_constraint("3.5", "=3.5"));
        assert!(!satisfies_constraint("3.4", "=3.5"));
        assert!(!satisfies_constraint("3.6", "=3.5"));
    }

    #[test]
    fn test_satisfies_constraint_empty() {
        assert!(satisfies_constraint("1.0", ""));
        assert!(satisfies_constraint("999.0", ""));
    }

    #[test]
    fn test_satisfies_constraint_multipart() {
        assert!(satisfies_constraint("2.17.3", ">=2.17"));
        assert!(satisfies_constraint("2.17.0", ">=2.17"));
        assert!(!satisfies_constraint("2.16.99", ">=2.17"));
    }

    #[test]
    fn test_satisfies_constraint_different_lengths() {
        // 3.0 vs 3.0.1: 3.0 is treated as 3.0.0 which is < 3.0.1
        assert!(satisfies_constraint("3.0.1", ">3.0"));
        assert!(!satisfies_constraint("3.0", ">3.0"));
        assert!(satisfies_constraint("3.0", ">=3.0"));
    }

    // ── Version comparison tests ──

    #[test]
    fn test_compare_versions() {
        assert_eq!(compare_versions("1.0", "1.0"), std::cmp::Ordering::Equal);
        assert_eq!(compare_versions("2.0", "1.0"), std::cmp::Ordering::Greater);
        assert_eq!(compare_versions("1.0", "2.0"), std::cmp::Ordering::Less);
        assert_eq!(
            compare_versions("1.10", "1.9"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_versions("1.0.0", "1.0"),
            std::cmp::Ordering::Equal
        );
    }

    // ── Dependency spec parsing tests ──

    #[test]
    fn test_parse_dependency_spec() {
        assert_eq!(parse_dependency_spec("glibc>=2.17"), ("glibc", Some(">=2.17")));
        assert_eq!(parse_dependency_spec("openssl<4.0"), ("openssl", Some("<4.0")));
        assert_eq!(parse_dependency_spec("zlib=1.3"), ("zlib", Some("=1.3")));
        assert_eq!(parse_dependency_spec("curl"), ("curl", None));
        assert_eq!(parse_dependency_spec("libfoo<=2.0"), ("libfoo", Some("<=2.0")));
        assert_eq!(parse_dependency_spec("bar>1.5"), ("bar", Some(">1.5")));
    }

    // ── Package name matching tests ──

    #[test]
    fn test_matches_package_name() {
        assert!(matches_package_name("firefox", "firefox"));
        assert!(!matches_package_name("firefox", "chromium"));
        // With version constraint attached to pattern
        assert!(matches_package_name("gtk3>=3.24", "gtk3"));
        assert!(!matches_package_name("gtk3>=3.24", "gtk4"));
    }

    // ── extract_package_key_from_path tests ──

    #[test]
    fn test_extract_package_key_from_path() {
        assert_eq!(
            extract_package_key_from_path("/home/user/.local/share/zl/packages/firefox-120.0/bin/firefox"),
            Some("firefox-120.0".to_string())
        );
        assert_eq!(
            extract_package_key_from_path("/no/pkgs/here"),
            None
        );
    }

    // ── Integration: check_conflicts ──

    fn make_db() -> (tempfile::NamedTempFile, ZlDatabase) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let db = ZlDatabase::open(tmp.path()).unwrap();
        (tmp, db)
    }

    fn make_node(name: &str, version: &str) -> PackageNode {
        PackageNode {
            id: PackageId {
                name: name.into(),
                version: version.into(),
                source: "test".into(),
            },
            installed_files: vec![],
            provides_libs: HashMap::new(),
            needs_libs: vec![],
            installed_at: 0,
            explicit: true,
        }
    }

    #[test]
    fn test_no_conflicts_empty_db() {
        let (_tmp, db) = make_db();
        let tmp_root = tempfile::TempDir::new().unwrap();
        let paths = ZlPaths::new(Some(tmp_root.path()));

        let candidate = PackageCandidate {
            name: "newpkg".into(),
            version: "1.0".into(),
            description: "A new package".into(),
            arch: "x86_64".into(),
            source: "test".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };

        let report = check_conflicts(&[&candidate], &db, &paths).unwrap();
        assert!(!report.has_conflicts());
    }

    #[test]
    fn test_declared_conflict() {
        let (_tmp, db) = make_db();
        let tmp_root = tempfile::TempDir::new().unwrap();
        let paths = ZlPaths::new(Some(tmp_root.path()));

        // Install a package
        db.put_package(&make_node("libpng", "1.6")).unwrap();

        // Candidate declares conflict with libpng
        let candidate = PackageCandidate {
            name: "libpng-ng".into(),
            version: "2.0".into(),
            description: "Next-gen PNG".into(),
            arch: "x86_64".into(),
            source: "test".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec!["libpng".into()],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };

        let report = check_conflicts(&[&candidate], &db, &paths).unwrap();
        assert!(report.has_conflicts());
        assert_eq!(report.conflicts.len(), 1);
        match &report.conflicts[0] {
            Conflict::DeclaredConflict {
                package,
                conflicts_with,
            } => {
                assert_eq!(package, "libpng-ng-2.0");
                assert!(conflicts_with.starts_with("libpng-"));
            }
            other => panic!("expected DeclaredConflict, got: {:?}", other),
        }
    }

    #[test]
    fn test_version_conflict() {
        let (_tmp, db) = make_db();
        let tmp_root = tempfile::TempDir::new().unwrap();
        let paths = ZlPaths::new(Some(tmp_root.path()));

        // Install glibc 2.17
        db.put_package(&make_node("glibc", "2.17")).unwrap();

        // Candidate requires glibc >= 2.34
        let candidate = PackageCandidate {
            name: "modernapp".into(),
            version: "1.0".into(),
            description: "Needs new glibc".into(),
            arch: "x86_64".into(),
            source: "test".into(),
            dependencies: vec!["glibc>=2.34".into()],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };

        let report = check_conflicts(&[&candidate], &db, &paths).unwrap();
        assert!(report.has_conflicts());
        assert_eq!(report.conflicts.len(), 1);
        match &report.conflicts[0] {
            Conflict::VersionConflict {
                dependency,
                required_by,
                required_version,
                installed_version,
            } => {
                assert_eq!(dependency, "glibc");
                assert_eq!(required_by, "modernapp-1.0");
                assert_eq!(required_version, ">=2.34");
                assert_eq!(installed_version, "2.17");
            }
            other => panic!("expected VersionConflict, got: {:?}", other),
        }
    }

    #[test]
    fn test_library_soname_conflict() {
        let (_tmp, db) = make_db();
        let tmp_root = tempfile::TempDir::new().unwrap();
        let paths = ZlPaths::new(Some(tmp_root.path()));

        // Register a library from an existing package
        db.register_lib("libssl.so.3", "openssl-3.1").unwrap();

        // Candidate also provides libssl.so.3
        let candidate = PackageCandidate {
            name: "libressl".into(),
            version: "3.8".into(),
            description: "LibreSSL".into(),
            arch: "x86_64".into(),
            source: "test".into(),
            dependencies: vec![],
            provides: vec!["libssl.so.3".into(), "libcrypto.so.3".into()],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };

        let report = check_conflicts(&[&candidate], &db, &paths).unwrap();
        assert!(report.has_conflicts());
        assert_eq!(report.conflicts.len(), 1);
        match &report.conflicts[0] {
            Conflict::LibrarySoname {
                soname,
                existing_package,
                new_package,
            } => {
                assert_eq!(soname, "libssl.so.3");
                assert_eq!(existing_package, "openssl-3.1");
                assert_eq!(new_package, "libressl-3.8");
            }
            other => panic!("expected LibrarySoname, got: {:?}", other),
        }
    }

    #[test]
    fn test_multiple_conflicts() {
        let (_tmp, db) = make_db();
        let tmp_root = tempfile::TempDir::new().unwrap();
        let paths = ZlPaths::new(Some(tmp_root.path()));

        // Install packages and register libs
        db.put_package(&make_node("openssl", "3.1")).unwrap();
        db.register_lib("libssl.so.3", "openssl-3.1").unwrap();
        db.put_package(&make_node("glibc", "2.17")).unwrap();

        let candidate = PackageCandidate {
            name: "conflicting-pkg".into(),
            version: "1.0".into(),
            description: "A very conflicting package".into(),
            arch: "x86_64".into(),
            source: "test".into(),
            dependencies: vec!["glibc>=2.34".into()],
            provides: vec!["libssl.so.3".into()],
            conflicts: vec!["openssl".into()],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };

        let report = check_conflicts(&[&candidate], &db, &paths).unwrap();
        assert!(report.has_conflicts());
        // Should have: LibrarySoname, DeclaredConflict, VersionConflict
        assert_eq!(report.conflicts.len(), 3);
    }

    #[test]
    fn test_conflict_report_display() {
        let report = ConflictReport {
            conflicts: vec![
                Conflict::DeclaredConflict {
                    package: "a-1.0".into(),
                    conflicts_with: "b-2.0".into(),
                },
            ],
        };
        // Just verify it doesn't panic
        report.display();
        assert!(report.has_conflicts());
    }

    #[test]
    fn test_satisfied_version_no_conflict() {
        let (_tmp, db) = make_db();
        let tmp_root = tempfile::TempDir::new().unwrap();
        let paths = ZlPaths::new(Some(tmp_root.path()));

        // Install glibc 2.34
        db.put_package(&make_node("glibc", "2.34")).unwrap();

        // Candidate requires glibc >= 2.17 — should be satisfied
        let candidate = PackageCandidate {
            name: "myapp".into(),
            version: "1.0".into(),
            description: "My app".into(),
            arch: "x86_64".into(),
            source: "test".into(),
            dependencies: vec!["glibc>=2.17".into()],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };

        let report = check_conflicts(&[&candidate], &db, &paths).unwrap();
        assert!(!report.has_conflicts());
    }
}
