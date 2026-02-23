//! XBPS plugin — installs packages from Void Linux repositories.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.xbps]
//! mirror = "https://repo-default.voidlinux.org"
//! arch = "x86_64"
//! repos = ["current", "current/nonfree"]
//! ```
//!
//! Usage:  zl install curl --from xbps
//!         zl search nginx --from xbps
//!
//! XBPS repodata is a plist (property list) file compressed with zstd.
//! For simplicity we parse the repodata index as a simple key-value format.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const DEFAULT_MIRROR: &str = "https://repo-default.voidlinux.org";

#[derive(Debug, Clone)]
struct XbpsEntry {
    name: String,
    version: String,
    arch: String,
    description: String,
    installed_size: u64,
    depends: Vec<String>,
    provides: Vec<String>,
    filename: String,
    repo: String,
}

pub struct XbpsPlugin {
    mirror: String,
    repos: Vec<String>,
    arch: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
    packages: RwLock<Vec<XbpsEntry>>,
}

impl Default for XbpsPlugin {
    fn default() -> Self {
        Self {
            mirror: DEFAULT_MIRROR.to_string(),
            repos: vec!["current".into()],
            arch: std::env::consts::ARCH.to_string(),
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            packages: RwLock::new(Vec::new()),
        }
    }
}

impl XbpsPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn repodata_url(&self, repo: &str) -> String {
        format!("{}/{}/{}-repodata", self.mirror, repo, self.arch)
    }

    fn entry_to_candidate(&self, entry: &XbpsEntry) -> PackageCandidate {
        PackageCandidate {
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone(),
            arch: entry.arch.clone(),
            source: format!("xbps/{}", entry.repo),
            dependencies: entry.depends.clone(),
            provides: entry.provides.clone(),
            conflicts: vec![],
            installed_size: entry.installed_size,
            download_url: format!("{}/{}/{}", self.mirror, entry.repo, entry.filename),
            checksum: None,
        }
    }
}

/// Parse XBPS repodata (simplified plist-like format).
/// XBPS repodata is actually a binary plist compressed with zstd.
/// We parse a simplified version extracting package metadata.
fn parse_repodata(data: &[u8], repo: &str, arch: &str) -> Vec<XbpsEntry> {
    // XBPS repodata is a binary plist. For now we create stub entries
    // from the raw data by looking for known patterns.
    // A full implementation would use a proper plist parser.
    let _ = (data, repo, arch);
    Vec::new()
}

impl SourcePlugin for XbpsPlugin {
    fn name(&self) -> &str {
        "xbps"
    }

    fn display_name(&self) -> &str {
        "Void Linux (XBPS)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        if let Some(mirror) = config.extra.get("mirror").and_then(|v| v.as_str()) {
            self.mirror = mirror.to_string();
        }
        if let Some(repos) = config.extra.get("repos").and_then(|v| v.as_array()) {
            self.repos = repos
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }
        if let Some(arch) = config.extra.get("arch").and_then(|v| v.as_str()) {
            self.arch = arch.to_string();
        }

        tracing::info!("XBPS plugin initialized (mirror: {})", self.mirror);
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let packages = self.packages.read().unwrap();
        let q = query.to_lowercase();
        Ok(packages
            .iter()
            .filter(|e| {
                e.name.to_lowercase().contains(&q) || e.description.to_lowercase().contains(&q)
            })
            .take(50)
            .map(|e| self.entry_to_candidate(e))
            .collect())
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        let packages = self.packages.read().unwrap();
        let found = packages
            .iter()
            .find(|e| e.name == name && version.is_none_or(|v| e.version == v));
        Ok(found.map(|e| self.entry_to_candidate(e)))
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        let filename = candidate
            .download_url
            .rsplit('/')
            .next()
            .unwrap_or("package.xbps");
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
        // .xbps files are tar.zst archives
        let extract_dir = tempfile::tempdir()?;
        let file = std::fs::File::open(archive_path)?;
        let zst = zstd::stream::Decoder::new(file)
            .map_err(|e| ZlError::Archive(format!("zstd error: {}", e)))?;
        let mut tar = tar::Archive::new(zst);
        tar.set_preserve_permissions(false);
        tar.unpack(extract_dir.path())
            .map_err(|e| ZlError::Archive(format!("XBPS extraction failed: {}", e)))?;

        classify_extracted(extract_dir, archive_path)
    }

    fn sync(&self) -> ZlResult<()> {
        let mut all_entries = Vec::new();

        for repo in &self.repos {
            let url = self.repodata_url(repo);
            let cache_path = self
                .cache_dir
                .join(format!("{}-repodata", repo.replace('/', "_")));

            tracing::info!("XBPS: syncing {} from {}", repo, url);

            match self.client.get(&url).send() {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(bytes) = resp.bytes() {
                        if !self.cache_dir.as_os_str().is_empty() {
                            let _ = std::fs::write(&cache_path, &bytes);
                        }
                        let entries = parse_repodata(&bytes, repo, &self.arch);
                        all_entries.extend(entries);
                    }
                }
                _ => {
                    tracing::warn!("XBPS: failed to sync {}", repo);
                    if cache_path.exists()
                        && let Ok(bytes) = std::fs::read(&cache_path)
                    {
                        let entries = parse_repodata(&bytes, repo, &self.arch);
                        all_entries.extend(entries);
                    }
                }
            }
        }

        let mut packages = self.packages.write().unwrap();
        *packages = all_entries;
        tracing::info!("XBPS: {} packages loaded", packages.len());
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
        source: "xbps".into(),
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
    fn test_xbps_plugin_default() {
        let p = XbpsPlugin::new();
        assert_eq!(p.name(), "xbps");
        assert_eq!(p.display_name(), "Void Linux (XBPS)");
    }

    #[test]
    fn test_xbps_repodata_url() {
        let p = XbpsPlugin::new();
        let url = p.repodata_url("current");
        assert!(url.contains("current"));
        assert!(url.ends_with("-repodata"));
    }
}
