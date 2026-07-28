//! Portage plugin — installs precompiled binary packages from Gentoo binhost.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.portage]
//! binhost = "https://distfiles.gentoo.org/releases/amd64/binpackages/23.0/x86-64"
//! arch = "amd64"
//! ```
//!
//! Usage:  zl install bash --from portage
//!         zl search vim --from portage
//!
//! Only uses binhost (precompiled packages), NOT source builds from ebuilds.

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

// Gentoo retired the 17.1 profiles; 23.0 is the current default. The binhost
// path mirrors the profile version. Overridable via `[plugins.portage] binhost`.
const DEFAULT_BINHOST: &str = "https://distfiles.gentoo.org/releases/amd64/binpackages/23.0/x86-64";

/// An entry from the Gentoo binhost Packages index.
#[derive(Debug, Clone)]
struct BinhostEntry {
    /// Category/name (e.g., "sys-apps/bash")
    cpv: String,
    /// Short name
    name: String,
    /// Version
    version: String,
    description: String,
    installed_size: u64,
    depends: Vec<String>,
    provides: Vec<String>,
    /// Relative path to the .tbz2/.gpkg.tar
    path: String,
    checksum: Option<String>,
}

pub struct PortagePlugin {
    binhost: String,
    arch: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
    packages: RwLock<Vec<BinhostEntry>>,
}

impl Default for PortagePlugin {
    fn default() -> Self {
        Self {
            binhost: DEFAULT_BINHOST.to_string(),
            arch: "amd64".to_string(),
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1")
                .timeout(std::time::Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
            packages: RwLock::new(Vec::new()),
        }
    }
}

impl PortagePlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn entry_to_candidate(&self, entry: &BinhostEntry) -> PackageCandidate {
        PackageCandidate {
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone(),
            arch: entry.arch().to_string(),
            source: "portage".into(),
            dependencies: entry.depends.clone(),
            provides: entry.provides.clone(),
            conflicts: vec![],
            installed_size: entry.installed_size,
            download_url: format!("{}/{}", self.binhost, entry.path),
            checksum: entry.checksum.clone(),
        }
    }
}

impl BinhostEntry {
    fn arch(&self) -> &str {
        "amd64"
    }
}

/// Parse the Gentoo binhost `Packages` index file.
/// Format: blocks separated by blank lines, key: value pairs.
fn parse_packages_index<R: Read>(reader: R) -> ZlResult<Vec<BinhostEntry>> {
    let buf = BufReader::new(reader);
    let mut entries = Vec::new();
    let mut current_cpv = String::new();
    let mut current_desc = String::new();
    let mut current_size: u64 = 0;
    let mut current_path = String::new();
    let mut current_sha256 = None;
    let mut current_depends = Vec::new();

    for line in buf.lines() {
        let line = line.map_err(|e| ZlError::Plugin {
            plugin: "portage".into(),
            message: format!("Packages index read error: {}", e),
        })?;

        if line.is_empty() {
            if !current_cpv.is_empty() && !current_path.is_empty() {
                // Parse "category/name-version" format
                let (name, version) = parse_cpv(&current_cpv);
                entries.push(BinhostEntry {
                    cpv: current_cpv.clone(),
                    name,
                    version,
                    description: current_desc.clone(),
                    installed_size: current_size,
                    depends: current_depends.clone(),
                    provides: vec![],
                    path: current_path.clone(),
                    checksum: current_sha256.clone(),
                });
            }
            current_cpv.clear();
            current_desc.clear();
            current_size = 0;
            current_path.clear();
            current_sha256 = None;
            current_depends.clear();
            continue;
        }

        if let Some((key, value)) = line.split_once(": ") {
            match key {
                "CPV" => current_cpv = value.to_string(),
                "DESC" => current_desc = value.to_string(),
                "SIZE" => current_size = value.parse().unwrap_or(0),
                "PATH" => current_path = value.to_string(),
                "SHA256" => current_sha256 = Some(value.to_string()),
                "RDEPEND" => {
                    current_depends = value
                        .split_whitespace()
                        .filter(|s| !s.starts_with('!') && !s.starts_with("||"))
                        .map(|s| {
                            // Strip version constraints like ">=sys-apps/bash-5.0"
                            s.trim_start_matches(">=")
                                .trim_start_matches("<=")
                                .trim_start_matches('>')
                                .trim_start_matches('<')
                                .trim_start_matches('=')
                                .trim_start_matches('~')
                                .to_string()
                        })
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                _ => {}
            }
        }
    }

    // Don't forget last entry
    if !current_cpv.is_empty() && !current_path.is_empty() {
        let (name, version) = parse_cpv(&current_cpv);
        entries.push(BinhostEntry {
            cpv: current_cpv,
            name,
            version,
            description: current_desc,
            installed_size: current_size,
            depends: current_depends,
            provides: vec![],
            path: current_path,
            checksum: current_sha256,
        });
    }

    Ok(entries)
}

/// Parse "category/name-version" into (name, version).
/// E.g., "sys-apps/bash-5.2_p26" → ("bash", "5.2_p26")
fn parse_cpv(cpv: &str) -> (String, String) {
    // Strip category
    let name_version = cpv.rsplit('/').next().unwrap_or(cpv);
    // Split at last hyphen followed by a digit
    if let Some(pos) = name_version
        .rmatch_indices('-')
        .find(|(i, _)| {
            name_version
                .as_bytes()
                .get(i + 1)
                .is_some_and(|b| b.is_ascii_digit())
        })
        .map(|(i, _)| i)
    {
        (
            name_version[..pos].to_string(),
            name_version[pos + 1..].to_string(),
        )
    } else {
        (name_version.to_string(), String::new())
    }
}

impl SourcePlugin for PortagePlugin {
    fn name(&self) -> &str {
        "portage"
    }

    fn display_name(&self) -> &str {
        "Gentoo Binhost (Portage)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        if let Some(binhost) = config.extra.get("binhost").and_then(|v| v.as_str()) {
            self.binhost = binhost.to_string();
        }
        if let Some(arch) = config.extra.get("arch").and_then(|v| v.as_str()) {
            self.arch = arch.to_string();
        }

        tracing::info!("Portage plugin initialized (binhost: {})", self.binhost);
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let packages = self.packages.read().unwrap();
        let q = query.to_lowercase();
        Ok(packages
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&q)
                    || e.cpv.to_lowercase().contains(&q)
                    || e.description.to_lowercase().contains(&q)
            })
            .take(50)
            .map(|e| self.entry_to_candidate(e))
            .collect())
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        let packages = self.packages.read().unwrap();
        let found = packages.iter().find(|e| {
            (e.name == name || e.cpv.ends_with(&format!("/{}", name)))
                && version.is_none_or(|v| e.version == v)
        });
        Ok(found.map(|e| self.entry_to_candidate(e)))
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        let filename = candidate
            .download_url
            .rsplit('/')
            .next()
            .unwrap_or("package.tbz2");
        let dest = dest_dir.join(filename);
        if dest.exists() {
            return Ok(dest);
        }

        crate::error::retry_with_backoff(3, 1000, |attempt| {
            let resp = self
                .client
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
        let name = archive_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();

        if name.ends_with(".tbz2") {
            // tbz2 = tar + bzip2
            let file = std::fs::File::open(archive_path)?;
            let bz = bzip2::read::BzDecoder::new(file);
            let mut tar = tar::Archive::new(bz);
            tar.set_preserve_permissions(false);
            tar.unpack(extract_dir.path())
                .map_err(|e| ZlError::Archive(format!("tbz2 extraction failed: {}", e)))?;
        } else if name.ends_with(".gpkg.tar") {
            // Gentoo binary package format v2
            let file = std::fs::File::open(archive_path)?;
            let mut tar = tar::Archive::new(file);
            tar.set_preserve_permissions(false);
            tar.unpack(extract_dir.path())
                .map_err(|e| ZlError::Archive(format!("gpkg extraction failed: {}", e)))?;
        } else {
            return Err(ZlError::Archive(format!(
                "Unknown Portage package format: {}",
                name
            )));
        }

        classify_extracted(extract_dir, archive_path)
    }

    fn sync(&self) -> ZlResult<()> {
        let url = format!("{}/Packages", self.binhost);
        let cache_path = self.cache_dir.join("Packages");

        tracing::info!("Portage: syncing from {}", url);

        let resp = self
            .client
            .get(&url)
            .send()
            .map_err(|e| ZlError::DownloadFailed {
                url: url.clone(),
                attempts: 1,
                message: e.to_string(),
            })?;

        if !resp.status().is_success() {
            tracing::warn!("Portage: failed to sync: HTTP {}", resp.status());
            if cache_path.exists() {
                let file = std::fs::File::open(&cache_path)?;
                let entries = parse_packages_index(file)?;
                let mut packages = self.packages.write().unwrap();
                *packages = entries;
            }
            return Ok(());
        }

        let bytes = resp.bytes().map_err(|e| ZlError::DownloadFailed {
            url: url.clone(),
            attempts: 1,
            message: e.to_string(),
        })?;

        if !self.cache_dir.as_os_str().is_empty() {
            let _ = std::fs::write(&cache_path, &bytes);
        }

        let entries = parse_packages_index(std::io::Cursor::new(bytes))?;
        let count = entries.len();
        let mut packages = self.packages.write().unwrap();
        *packages = entries;

        tracing::info!("Portage: {} packages loaded", count);
        Ok(())
    }
}

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
        source: "portage".into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_portage_plugin_default() {
        let p = PortagePlugin::new();
        assert_eq!(p.name(), "portage");
        assert_eq!(p.display_name(), "Gentoo Binhost (Portage)");
        // Must target the current 23.0 profile, not the retired 17.1 layout.
        assert!(p.binhost.contains("/23.0/"));
        assert!(!p.binhost.contains("/17.1/"));
    }

    #[test]
    fn test_parse_cpv() {
        let (name, ver) = parse_cpv("sys-apps/bash-5.2_p26");
        assert_eq!(name, "bash");
        assert_eq!(ver, "5.2_p26");

        let (name, ver) = parse_cpv("dev-libs/openssl-3.1.4");
        assert_eq!(name, "openssl");
        assert_eq!(ver, "3.1.4");
    }

    #[test]
    fn test_parse_packages_index() {
        let index = "CPV: sys-apps/bash-5.2_p26\nDESC: The standard GNU Bourne Again SHell\nSIZE: 8000000\nPATH: sys-apps/bash-5.2_p26.tbz2\nSHA256: abc123\n\nCPV: app-editors/vim-9.0.2092\nDESC: Vim, an improved vi-style text editor\nSIZE: 15000000\nPATH: app-editors/vim-9.0.2092.tbz2\n\n";

        let entries = parse_packages_index(index.as_bytes()).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "bash");
        assert_eq!(entries[0].version, "5.2_p26");
        assert_eq!(entries[0].checksum, Some("abc123".to_string()));
        assert_eq!(entries[1].name, "vim");
        assert_eq!(entries[1].version, "9.0.2092");
    }
}
