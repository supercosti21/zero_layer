# Plugin Development

This guide explains how to create a new package source plugin for Zero Layer. If you want to add support for a new package manager, repository, or package format, this is the page for you.

---

## Table of Contents

- [Overview](#overview)
- [The SourcePlugin Trait](#the-sourceplugin-trait)
- [Step-by-Step Guide](#step-by-step-guide)
- [Common Patterns](#common-patterns)
- [Testing](#testing)
- [Shared Utilities](#shared-utilities)

---

## Overview

ZL's plugin system is compile-time — plugins are Rust modules that implement the `SourcePlugin` trait. They're not dynamically loaded libraries. To add a new plugin, you:

1. Create `src/plugin/<name>/mod.rs`
2. Implement the `SourcePlugin` trait
3. Register it in `src/plugin/mod.rs` and `src/main.rs`
4. Add the name to `ALL_PLUGIN_NAMES` in `src/cli/sources.rs`

---

## The SourcePlugin Trait

Every plugin must implement this trait:

```rust
pub trait SourcePlugin: Send + Sync {
    /// Short name used in CLI (e.g., "pacman", "apt", "github")
    fn name(&self) -> &str;

    /// Human-readable display name (e.g., "Arch Linux (pacman)")
    fn display_name(&self) -> &str;

    /// Initialize the plugin with user configuration.
    /// Called once at startup after registration.
    fn init(&mut self, config: &PluginConfig) -> ZlResult<()>;

    /// Search for packages matching a query string.
    /// Returns up to ~50 results.
    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>>;

    /// Resolve a specific package by name and optional version.
    /// Returns None if the package doesn't exist.
    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>>;

    /// Download a package to the destination directory.
    /// Returns the path to the downloaded file.
    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf>;

    /// Extract a downloaded archive.
    /// Returns the extracted files with classification (ELF, scripts, etc.).
    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage>;

    /// Sync/refresh the package database.
    /// Called by `zl update --from <name>`.
    fn sync(&self) -> ZlResult<()>;
}
```

Key requirements:
- **`Send + Sync`** — plugins are shared across threads (for parallel search)
- **`&self` on query methods** — search, resolve, download, extract are called concurrently
- **`&mut self` only on init** — initialization happens before any concurrent access

---

## Step-by-Step Guide

### 1. Create the plugin module

Create `src/plugin/myplugin/mod.rs`:

```rust
//! MyPlugin — installs packages from MySource.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.myplugin]
//! mirror = "https://example.com/repo"
//! ```

use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

pub struct MyPlugin {
    mirror: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
}

impl Default for MyPlugin {
    fn default() -> Self {
        Self {
            mirror: "https://example.com/repo".to_string(),
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl MyPlugin {
    pub fn new() -> Self {
        Self::default()
    }
}
```

### 2. Implement SourcePlugin

```rust
impl SourcePlugin for MyPlugin {
    fn name(&self) -> &str {
        "myplugin"
    }

    fn display_name(&self) -> &str {
        "My Package Source"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        // Set up cache directory
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        // Read user configuration
        if let Some(mirror) = config.extra.get("mirror").and_then(|v| v.as_str()) {
            self.mirror = mirror.to_string();
        }

        tracing::info!("MyPlugin initialized (mirror: {})", self.mirror);
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        // Implement your search logic here
        // Return Vec<PackageCandidate> with results
        let q = query.to_lowercase();

        // Example: query an API
        // let resp = self.client.get(&url).send()?;
        // let results: Vec<...> = resp.json()?;

        Ok(vec![])  // Return your results
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        // Look up a specific package by exact name
        // Return None if not found
        Ok(None)
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        let filename = format!("{}-{}.pkg", candidate.name, candidate.version);
        let dest = dest_dir.join(&filename);

        // Skip if already cached
        if dest.exists() {
            return Ok(dest);
        }

        // Download with retry
        crate::error::retry_with_backoff(3, 1000, |attempt| {
            let resp = self.client
                .get(&candidate.download_url)
                .send()
                .map_err(|e| ZlError::DownloadFailed {
                    url: candidate.download_url.clone(),
                    attempts: attempt,
                    message: e.to_string(),
                })?;

            if !resp.status().is_success() {
                return Err(ZlError::DownloadFailed {
                    url: candidate.download_url.clone(),
                    attempts: attempt,
                    message: format!("HTTP {}", resp.status()),
                });
            }

            let bytes = resp.bytes().map_err(|e| ZlError::DownloadFailed {
                url: candidate.download_url.clone(),
                attempts: attempt,
                message: e.to_string(),
            })?;

            std::fs::write(&dest, &bytes)?;
            Ok(dest.clone())
        })
    }

    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage> {
        let extract_dir = tempfile::tempdir()?;

        // Extract your archive format here
        // Example for tar.gz:
        // let file = std::fs::File::open(archive_path)?;
        // let gz = flate2::read::GzDecoder::new(file);
        // let mut tar = tar::Archive::new(gz);
        // tar.unpack(extract_dir.path())?;

        classify_extracted(extract_dir, archive_path)
    }

    fn sync(&self) -> ZlResult<()> {
        // Download/refresh your package index
        // For live APIs, this can be a no-op:
        tracing::info!("MyPlugin: nothing to sync (live API)");
        Ok(())
    }
}
```

### 3. Add the file classification helper

```rust
fn classify_extracted(
    extract_dir: tempfile::TempDir,
    archive_path: &Path,
) -> ZlResult<ExtractedPackage> {
    use crate::core::elf::analysis;

    let mut files = Vec::new();
    let mut elf_files = Vec::new();
    let mut script_files = Vec::new();

    for entry in walkdir::WalkDir::new(extract_dir.path())
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path().to_path_buf();
        if analysis::is_elf_file(&path) {
            elf_files.push(path.clone());
        } else if is_script(&path) {
            script_files.push(path.clone());
        }
        files.push(path);
    }

    let fname = archive_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let metadata = PackageCandidate {
        name: fname,
        version: String::new(),
        description: String::new(),
        arch: std::env::consts::ARCH.to_string(),
        source: "myplugin".into(),
        dependencies: vec![],
        provides: vec![],
        conflicts: vec![],
        installed_size: 0,
        download_url: String::new(),
        checksum: None,
    };

    Ok(ExtractedPackage {
        extract_dir,
        metadata,
        files,
        elf_files,
        script_files,
    })
}

fn is_script(path: &Path) -> bool {
    use std::io::Read;
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy();
        if matches!(ext.as_ref(), "sh" | "bash" | "py" | "pl" | "rb") {
            return true;
        }
    }
    if let Ok(mut f) = std::fs::File::open(path) {
        let mut buf = [0u8; 2];
        if f.read_exact(&mut buf).is_ok() && buf == *b"#!" {
            return true;
        }
    }
    false
}
```

### 4. Register the plugin

**In `src/plugin/mod.rs`:**

```rust
pub mod myplugin;
```

**In `src/main.rs` (plugin registration section):**

```rust
registry.register(Box::new(plugin::myplugin::MyPlugin::new()));
```

**In `src/cli/sources.rs` (ALL_PLUGIN_NAMES):**

```rust
pub const ALL_PLUGIN_NAMES: &[&str] = &[
    "pacman", "aur", "apt", "github", "dnf", "zypper", "apk",
    "xbps", "portage", "nix", "flatpak", "snap", "appimage",
    "myplugin",  // Add your plugin
];
```

### 5. Add tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_myplugin_default() {
        let p = MyPlugin::new();
        assert_eq!(p.name(), "myplugin");
        assert_eq!(p.display_name(), "My Package Source");
    }

    #[test]
    fn test_myplugin_search_empty() {
        let p = MyPlugin::new();
        let results = p.search("nonexistent").unwrap();
        assert!(results.is_empty());
    }
}
```

---

## Common Patterns

### Cached database plugins (like pacman, apt, dnf)

```rust
struct MyPlugin {
    packages: RwLock<Vec<MyEntry>>,  // Thread-safe cached data
    cache_dir: PathBuf,
}

// sync() downloads and parses the index
fn sync(&self) -> ZlResult<()> {
    let data = download_index()?;
    let entries = parse_index(&data)?;
    let mut packages = self.packages.write().unwrap();
    *packages = entries;
    Ok(())
}

// search() reads from cache
fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
    let packages = self.packages.read().unwrap();
    Ok(packages.iter().filter(|e| e.name.contains(query)).collect())
}
```

### Live API plugins (like github, nix, snap)

```rust
struct MyPlugin {
    client: reqwest::blocking::Client,
}

// sync() is a no-op
fn sync(&self) -> ZlResult<()> {
    tracing::info!("MyPlugin: live API, nothing to sync");
    Ok(())
}

// search() queries the API each time
fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
    let url = format!("https://api.example.com/search?q={}", query);
    let resp = self.client.get(&url).send()?;
    let results: ApiResponse = resp.json()?;
    // Convert to PackageCandidate...
    Ok(candidates)
}
```

### Delegating to external tools (like flatpak, snap)

```rust
fn download(&self, candidate: &PackageCandidate, _dest: &Path) -> ZlResult<PathBuf> {
    let output = std::process::Command::new("flatpak")
        .args(["install", "--noninteractive", &candidate.name])
        .output()
        .map_err(|_| ZlError::BuildToolMissing {
            tool: "flatpak".into(),
        })?;

    if !output.status.success() {
        return Err(ZlError::Plugin {
            plugin: "flatpak".into(),
            message: String::from_utf8_lossy(&output.stderr).to_string(),
        });
    }
    // ...
}
```

---

## Testing

All tests are unit tests inside `#[cfg(test)]` modules in the same file:

```bash
# Run all tests
cargo test

# Run tests for a specific plugin
cargo test myplugin

# Run a specific test
cargo test test_myplugin_search_empty

# Run with output visible
cargo test -- --nocapture
```

### What to test

1. **Default construction** — `MyPlugin::new()` returns valid defaults
2. **Name and display_name** — correct strings
3. **Search with no data** — returns empty vec (not error)
4. **URL generation** — if your plugin builds URLs, test them
5. **Index parsing** — if you parse a custom format, test with sample data

---

## Shared Utilities

### RPM module (`plugin/rpm/`)

If your plugin uses RPM packages, reuse the shared RPM module:

```rust
use crate::plugin::rpm::{extract, repodata};

// Parse primary.xml.gz
let entries = repodata::parse_primary_xml_gz(&bytes)?;

// Extract an RPM file
let (extracted_dir, files) = extract::extract_rpm(archive_path)?;
```

### Error types

Use ZL's standard error types:

```rust
// Plugin-specific errors
ZlError::Plugin { plugin: "myplugin".into(), message: "...".into() }

// Download failures (triggers retry)
ZlError::DownloadFailed { url, attempts, message }

// Missing external tool
ZlError::BuildToolMissing { tool: "some-tool".into() }

// Archive extraction errors
ZlError::Archive("extraction failed: ...".into())
```

### Retry with backoff

For network operations:

```rust
crate::error::retry_with_backoff(3, 1000, |attempt| {
    // attempt = 1, 2, 3
    // delays: 1000ms, 2000ms, 4000ms between retries
    do_something()?;
    Ok(result)
})
```

### ELF analysis

```rust
use crate::core::elf::analysis;

// Check if a file is an ELF binary
analysis::is_elf_file(&path)
```

---

## PackageCandidate Fields

When returning `PackageCandidate` from search/resolve, fill in as many fields as possible:

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Package name |
| `version` | Yes | Package version |
| `description` | Recommended | One-line description |
| `arch` | Recommended | Architecture (e.g., "x86_64") |
| `source` | Yes | Source identifier (e.g., "myplugin/main") |
| `dependencies` | Recommended | List of dependency names |
| `provides` | Optional | Virtual packages this provides |
| `conflicts` | Optional | Packages this conflicts with |
| `installed_size` | Optional | Size in bytes when installed |
| `download_url` | Required for download | URL to download the package |
| `checksum` | Recommended | SHA256 hex string for verification |

---

## Next Steps

- [[Architecture]] — How plugins fit into the overall system
- [[Contributing]] — Development workflow and code style
- [[Package Sources]] — See existing plugins for reference
