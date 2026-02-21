//! APT plugin — installs Debian/Ubuntu packages (.deb).
//!
//! Syncs the Packages.gz index from a configured mirror, then downloads and
//! extracts .deb files using the ar+tar pipeline in `deb.rs`.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.apt]
//! mirror     = "http://archive.ubuntu.com/ubuntu"  # or deb.debian.org/debian
//! suite      = "noble"          # noble, bookworm, focal, jammy, etc.
//! components = ["main", "universe"]
//! arch       = "amd64"          # usually auto-detected
//! ```
//!
//! Usage:  zl install vim --from apt
//!         zl search python3 --from apt

pub mod deb;
pub mod index;

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

use index::AptEntry;

// ── Defaults ──────────────────────────────────────────────────────────────────

const DEFAULT_MIRROR: &str = "http://archive.ubuntu.com/ubuntu";
const DEFAULT_SUITE: &str = "noble";
const DEFAULT_COMPONENTS: &[&str] = &["main", "universe"];

// ── Plugin struct ─────────────────────────────────────────────────────────────

pub struct AptPlugin {
    mirror: String,
    suite: String,
    components: Vec<String>,
    arch: String,
    cache_dir: PathBuf,
    /// In-memory package index: component → entries
    db: RwLock<Vec<AptEntry>>,
}

impl AptPlugin {
    pub fn new() -> Self {
        Self {
            mirror: DEFAULT_MIRROR.to_string(),
            suite: DEFAULT_SUITE.to_string(),
            components: DEFAULT_COMPONENTS.iter().map(|s| s.to_string()).collect(),
            arch: detect_deb_arch(),
            cache_dir: PathBuf::new(),
            db: RwLock::new(Vec::new()),
        }
    }

    fn entry_to_candidate(&self, entry: &AptEntry) -> PackageCandidate {
        let download_url = format!("{}/{}", self.mirror.trim_end_matches('/'), entry.filename);
        PackageCandidate {
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone(),
            arch: entry.arch.clone(),
            source: format!("apt/{}", self.suite),
            dependencies: entry.depends.clone(),
            provides: entry.provides.clone(),
            conflicts: entry.conflicts.clone(),
            installed_size: entry.installed_size * 1024, // convert KiB → bytes
            download_url,
            checksum: entry.sha256.clone(),
        }
    }

    fn packages_url(&self, component: &str) -> String {
        format!(
            "{}/dists/{}/{}/binary-{}/Packages.gz",
            self.mirror.trim_end_matches('/'),
            self.suite,
            component,
            self.arch
        )
    }

    fn packages_cache_path(&self, component: &str) -> PathBuf {
        self.cache_dir
            .join(format!("{}-{}-{}.gz", self.suite, component, self.arch))
    }

    /// Download and parse one component's Packages.gz, merging into db
    fn sync_component(&self, component: &str) -> ZlResult<Vec<AptEntry>> {
        let url = self.packages_url(component);
        let cache_path = self.packages_cache_path(component);

        tracing::info!("Syncing APT index: {}", url);

        let bytes = crate::error::retry_with_backoff(3, 1000, |attempt| {
            if attempt > 1 {
                tracing::info!("Retry {}/3 for {}", attempt, url);
            }
            let resp = reqwest::blocking::Client::new()
                .get(&url)
                .timeout(std::time::Duration::from_secs(120))
                .send()
                .map_err(|e| ZlError::DownloadFailed {
                    url: url.clone(),
                    attempts: attempt,
                    message: e.to_string(),
                })?;

            if !resp.status().is_success() {
                return Err(ZlError::DownloadFailed {
                    url: url.clone(),
                    attempts: attempt,
                    message: format!("HTTP {}", resp.status()),
                });
            }
            Ok(resp.bytes().map_err(|e| ZlError::DownloadFailed {
                url: url.clone(),
                attempts: attempt,
                message: e.to_string(),
            })?)
        })?;

        std::fs::write(&cache_path, &bytes)?;

        let content = decompress_gz(&bytes)?;
        let entries = index::parse(&content);
        tracing::info!(
            "APT {}/{}: {} packages",
            self.suite,
            component,
            entries.len()
        );
        Ok(entries)
    }

    /// Load cached Packages index from disk (if available)
    fn load_cache(&self) -> Vec<AptEntry> {
        let mut all = Vec::new();
        for component in &self.components {
            let path = self.packages_cache_path(component);
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(content) = decompress_gz(&bytes) {
                    all.extend(index::parse(&content));
                }
            }
        }
        all
    }
}

// ── SourcePlugin implementation ───────────────────────────────────────────────

impl SourcePlugin for AptPlugin {
    fn name(&self) -> &str {
        "apt"
    }

    fn display_name(&self) -> &str {
        "APT (Debian/Ubuntu)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        std::fs::create_dir_all(&self.cache_dir)?;

        if let Some(m) = config.extra.get("mirror").and_then(|v| v.as_str()) {
            self.mirror = m.to_string();
        }
        if let Some(s) = config.extra.get("suite").and_then(|v| v.as_str()) {
            self.suite = s.to_string();
        }
        if let Some(a) = config.extra.get("arch").and_then(|v| v.as_str()) {
            self.arch = a.to_string();
        }
        if let Some(comps) = config.extra.get("components").and_then(|v| v.as_array()) {
            let custom: Vec<String> = comps
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !custom.is_empty() {
                self.components = custom;
            }
        }

        // Load cached index (if exists from previous sync)
        let cached = self.load_cache();
        if !cached.is_empty() {
            tracing::info!(
                "APT: loaded {} packages from cache (run `zl update --from apt` to refresh)",
                cached.len()
            );
            *self.db.write().unwrap() = cached;
        } else {
            tracing::info!(
                "APT: no local cache found — run `zl update --from apt` to sync the index"
            );
        }

        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let db = self.db.read().unwrap();
        let q = query.to_lowercase();
        Ok(db
            .iter()
            .filter(|e| e.name.to_lowercase().contains(&q) || e.description.to_lowercase().contains(&q))
            .map(|e| self.entry_to_candidate(e))
            .collect())
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        let db = self.db.read().unwrap();
        Ok(db
            .iter()
            .find(|e| {
                e.name == name && version.map_or(true, |v| e.version == v)
            })
            .map(|e| self.entry_to_candidate(e)))
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        deb::download_deb(
            &candidate.download_url,
            candidate.checksum.as_deref(),
            dest_dir,
        )
    }

    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage> {
        let name = archive_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let placeholder = PackageCandidate {
            name,
            version: String::new(),
            description: String::new(),
            arch: self.arch.clone(),
            source: "apt".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };

        deb::extract(archive_path, placeholder)
    }

    fn sync(&self) -> ZlResult<()> {
        let mut all_entries = Vec::new();

        for component in &self.components {
            match self.sync_component(component) {
                Ok(entries) => all_entries.extend(entries),
                Err(e) => tracing::warn!("Failed to sync {}/{}: {}", self.suite, component, e),
            }
        }

        if all_entries.is_empty() {
            return Err(ZlError::Plugin {
                plugin: "apt".into(),
                message: "No packages synced — check mirror and suite config".into(),
            });
        }

        *self.db.write().unwrap() = all_entries;
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Decompress a gzip-compressed byte slice into a UTF-8 string
fn decompress_gz(bytes: &[u8]) -> ZlResult<String> {
    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut content = String::new();
    decoder
        .read_to_string(&mut content)
        .map_err(|e| ZlError::Archive(format!("Failed to decompress Packages.gz: {}", e)))?;
    Ok(content)
}

/// Map Rust's std::env::consts::ARCH to Debian architecture names
fn detect_deb_arch() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "arm" => "armhf",
        "riscv64" => "riscv64",
        "i686" => "i386",
        "powerpc64" => "ppc64el",
        "s390x" => "s390x",
        _ => "amd64",
    }
    .to_string()
}
