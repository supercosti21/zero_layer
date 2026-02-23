use std::path::{Path, PathBuf};

use super::detect::SystemLayout;

/// Discover all library search directories on the host system.
///
/// Combines multiple sources:
/// 1. `ldconfig -p` output (most reliable, includes everything the linker knows)
/// 2. `/etc/ld.so.conf` and its includes
/// 3. Common fallback paths
/// 4. Layout-specific paths (NixOS, Guix, Termux)
pub fn discover_lib_dirs(layout: &SystemLayout) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Method 1: Parse ldconfig -p (the most authoritative source)
    dirs.extend(lib_dirs_from_ldconfig());

    // Method 2: Parse ld.so.conf directly (covers cases where ldconfig -p isn't available)
    dirs.extend(lib_dirs_from_ld_so_conf());

    // Method 3: LD_LIBRARY_PATH (user-set)
    if let Ok(val) = std::env::var("LD_LIBRARY_PATH") {
        for p in val.split(':') {
            if !p.is_empty() {
                dirs.push(PathBuf::from(p));
            }
        }
    }

    // Method 4: Layout-specific additions
    match layout {
        SystemLayout::NixOS => {
            dirs.extend(nix_lib_dirs());
        }
        SystemLayout::Guix => {
            if let Ok(profile) = std::env::var("GUIX_PROFILE") {
                dirs.push(PathBuf::from(format!("{}/lib", profile)));
            }
        }
        SystemLayout::Termux => {
            if let Ok(prefix) = std::env::var("PREFIX") {
                dirs.push(PathBuf::from(format!("{}/lib", prefix)));
            }
        }
        _ => {}
    }

    // Method 5: Standard fallbacks (always include these, even if found via other methods)
    let fallbacks = [
        "/usr/lib",
        "/usr/lib64",
        "/lib",
        "/lib64",
        "/usr/local/lib",
        "/usr/local/lib64",
    ];
    for fb in &fallbacks {
        dirs.push(PathBuf::from(fb));
    }

    // Deduplicate, preserving order (first occurrence wins)
    dedup_paths(&mut dirs);

    // Filter to only existing directories
    dirs.retain(|d| d.is_dir());

    dirs
}

/// Discover all binary directories on the host system.
pub fn discover_bin_dirs(layout: &SystemLayout) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // From PATH environment
    if let Ok(val) = std::env::var("PATH") {
        for p in val.split(':') {
            if !p.is_empty() {
                dirs.push(PathBuf::from(p));
            }
        }
    }

    // Layout-specific additions
    match layout {
        SystemLayout::NixOS => {
            dirs.push(PathBuf::from("/run/current-system/sw/bin"));
        }
        SystemLayout::Guix => {
            if let Ok(profile) = std::env::var("GUIX_PROFILE") {
                dirs.push(PathBuf::from(format!("{}/bin", profile)));
            }
        }
        SystemLayout::Termux => {
            if let Ok(prefix) = std::env::var("PREFIX") {
                dirs.push(PathBuf::from(format!("{}/bin", prefix)));
            }
        }
        _ => {}
    }

    // Standard fallbacks
    let fallbacks = ["/usr/bin", "/usr/sbin", "/bin", "/sbin", "/usr/local/bin"];
    for fb in &fallbacks {
        dirs.push(PathBuf::from(fb));
    }

    dedup_paths(&mut dirs);
    dirs.retain(|d| d.is_dir());
    dirs
}

/// Discover Debian-style multiarch tuple (e.g., "x86_64-linux-gnu").
/// Returns the tuple if `/usr/lib/<tuple>` exists.
pub fn detect_multiarch_tuple() -> Option<String> {
    // Try dpkg-architecture first
    if let Ok(output) = std::process::Command::new("dpkg-architecture")
        .arg("-qDEB_HOST_MULTIARCH")
        .output()
        && output.status.success()
    {
        let tuple = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !tuple.is_empty() && Path::new(&format!("/usr/lib/{}", tuple)).is_dir() {
            return Some(tuple);
        }
    }

    // Fallback: scan /usr/lib for known multiarch patterns
    let known_tuples = [
        "x86_64-linux-gnu",
        "aarch64-linux-gnu",
        "arm-linux-gnueabihf",
        "i386-linux-gnu",
        "riscv64-linux-gnu",
        "s390x-linux-gnu",
        "powerpc64le-linux-gnu",
    ];

    for tuple in &known_tuples {
        if Path::new(&format!("/usr/lib/{}", tuple)).is_dir() {
            return Some(tuple.to_string());
        }
    }

    None
}

/// Parse `ldconfig -p` output to extract library directories.
fn lib_dirs_from_ldconfig() -> Vec<PathBuf> {
    let output = match std::process::Command::new("ldconfig").arg("-p").output() {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let text = String::from_utf8_lossy(&output.stdout);
    let mut dirs = Vec::new();

    for line in text.lines().skip(1) {
        // Lines look like: "	libz.so.1 (libc6,x86-64) => /usr/lib/x86_64-linux-gnu/libz.so.1"
        if let Some(arrow_pos) = line.find("=>") {
            let lib_path = line[arrow_pos + 2..].trim();
            if let Some(parent) = Path::new(lib_path).parent() {
                dirs.push(parent.to_path_buf());
            }
        }
    }

    dirs
}

/// Parse /etc/ld.so.conf and all included files.
fn lib_dirs_from_ld_so_conf() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    parse_ld_so_conf(Path::new("/etc/ld.so.conf"), &mut dirs);
    dirs
}

fn parse_ld_so_conf(path: &Path, dirs: &mut Vec<PathBuf>) {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return,
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(pattern) = line.strip_prefix("include ") {
            let pattern = pattern.trim();
            // Glob expand the include pattern
            if let Ok(entries) = glob_paths(pattern) {
                for entry in entries {
                    parse_ld_so_conf(&entry, dirs);
                }
            }
        } else {
            dirs.push(PathBuf::from(line));
        }
    }
}

/// Simple glob expansion for ld.so.conf includes (e.g., "/etc/ld.so.conf.d/*.conf").
fn glob_paths(pattern: &str) -> std::io::Result<Vec<PathBuf>> {
    let mut results = Vec::new();

    let (dir_part, file_pattern) = match pattern.rfind('/') {
        Some(pos) => (&pattern[..pos], &pattern[pos + 1..]),
        None => (".", pattern),
    };

    let dir = Path::new(dir_part);
    if !dir.is_dir() {
        return Ok(results);
    }

    let entries = std::fs::read_dir(dir)?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if matches_simple_glob(file_pattern, &name_str) {
            results.push(entry.path());
        }
    }

    results.sort();
    Ok(results)
}

/// Match a simple glob pattern (only `*` wildcard).
fn matches_simple_glob(pattern: &str, name: &str) -> bool {
    if let Some(pos) = pattern.find('*') {
        let prefix = &pattern[..pos];
        let suffix = &pattern[pos + 1..];
        name.starts_with(prefix) && name.ends_with(suffix)
    } else {
        pattern == name
    }
}

/// NixOS: discover lib directories from /nix/store profiles.
fn nix_lib_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Current system profile
    let system_lib = Path::new("/run/current-system/sw/lib");
    if system_lib.is_dir() {
        dirs.push(system_lib.to_path_buf());
    }

    // User profile
    if let Ok(home) = std::env::var("HOME") {
        let user_lib = PathBuf::from(format!("{}/.nix-profile/lib", home));
        if user_lib.is_dir() {
            dirs.push(user_lib);
        }
    }

    // NIX_PROFILES environment variable
    if let Ok(profiles) = std::env::var("NIX_PROFILES") {
        for profile in profiles.split_whitespace() {
            let lib = PathBuf::from(format!("{}/lib", profile));
            if lib.is_dir() {
                dirs.push(lib);
            }
        }
    }

    dirs
}

/// Deduplicate paths preserving order (first occurrence wins).
fn dedup_paths(paths: &mut Vec<PathBuf>) {
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| {
        // Canonicalize to handle symlinks (e.g., /lib → /usr/lib)
        let canonical = std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        seen.insert(canonical)
    });
}

/// Compute the set of FHS source prefixes that packages might use.
/// These are the paths that package contents refer to and need remapping.
pub fn fhs_source_prefixes() -> Vec<(String, &'static str)> {
    // (FHS path, category: "lib" | "bin" | "share" | "etc")
    vec![
        ("/usr/lib64".into(), "lib"),
        ("/usr/lib".into(), "lib"),
        ("/lib64".into(), "lib"),
        ("/lib".into(), "lib"),
        ("/usr/bin".into(), "bin"),
        ("/usr/sbin".into(), "bin"),
        ("/bin".into(), "bin"),
        ("/sbin".into(), "bin"),
        ("/usr/share".into(), "share"),
        ("/etc".into(), "etc"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matches_simple_glob() {
        assert!(matches_simple_glob("*.conf", "foo.conf"));
        assert!(matches_simple_glob("*.conf", "bar.conf"));
        assert!(!matches_simple_glob("*.conf", "foo.txt"));
        assert!(matches_simple_glob("lib*", "libfoo"));
        assert!(matches_simple_glob("*", "anything"));
        assert!(matches_simple_glob("exact", "exact"));
        assert!(!matches_simple_glob("exact", "other"));
    }

    #[test]
    fn test_discover_lib_dirs_not_empty() {
        let dirs = discover_lib_dirs(&SystemLayout::Fhs);
        assert!(
            !dirs.is_empty(),
            "Should find at least some lib directories"
        );
        // At least /usr/lib or /lib should exist on any Linux
        assert!(
            dirs.iter().any(|d| d.to_string_lossy().contains("lib")),
            "Should include a lib directory"
        );
    }

    #[test]
    fn test_fhs_source_prefixes() {
        let prefixes = fhs_source_prefixes();
        assert!(prefixes.len() >= 10);
        // Should be sorted longest-first for correct prefix matching
        // (the caller is responsible for sorting, but verify content)
        assert!(prefixes.iter().any(|(p, c)| p == "/usr/lib" && *c == "lib"));
        assert!(prefixes.iter().any(|(p, c)| p == "/usr/bin" && *c == "bin"));
    }
}
