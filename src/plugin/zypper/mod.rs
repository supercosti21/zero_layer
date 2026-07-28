//! Zypper plugin — installs packages from openSUSE/SLES RPM repositories.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.zypper]
//! mirror = "https://download.opensuse.org"
//! release = "tumbleweed"
//! repos = ["oss", "update"]
//! arch = "x86_64"
//! ```
//!
//! Usage:  zl install bash --from zypper
//!         zl search vim --from zypper

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::rpm::repodata::RpmEntry;
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const DEFAULT_MIRROR: &str = "https://download.opensuse.org";
const DEFAULT_RELEASE: &str = "tumbleweed";

pub struct ZypperPlugin {
    mirror: String,
    release: String,
    repos: Vec<String>,
    arch: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
    packages: RwLock<Vec<(String, RpmEntry)>>, // (repo_name, entry)
}

impl Default for ZypperPlugin {
    fn default() -> Self {
        Self {
            mirror: DEFAULT_MIRROR.to_string(),
            release: DEFAULT_RELEASE.to_string(),
            repos: vec!["oss".into(), "update".into()],
            arch: std::env::consts::ARCH.to_string(),
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

impl ZypperPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn repomd_url(&self, repo: &str) -> String {
        format!("{}/repodata/repomd.xml", self.base_url(repo))
    }

    fn base_url(&self, repo: &str) -> String {
        if self.release == "tumbleweed" {
            format!("{}/tumbleweed/repo/{}", self.mirror, repo)
        } else {
            format!(
                "{}/distribution/leap/{}/repo/{}",
                self.mirror, self.release, repo
            )
        }
    }

    fn entry_to_candidate(&self, repo: &str, entry: &RpmEntry) -> PackageCandidate {
        PackageCandidate {
            name: entry.name.clone(),
            version: entry.evr(),
            description: entry.summary.clone(),
            arch: entry.arch.clone(),
            source: format!("zypper/{}", repo),
            dependencies: entry.requires.clone(),
            provides: entry.provides.clone(),
            conflicts: entry.conflicts.clone(),
            installed_size: entry.installed_size,
            download_url: format!("{}/{}", self.base_url(repo), entry.location_href),
            checksum: entry.checksum.clone(),
        }
    }
}

impl SourcePlugin for ZypperPlugin {
    fn name(&self) -> &str {
        "zypper"
    }

    fn display_name(&self) -> &str {
        "openSUSE/SLES (Zypper)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        if let Some(mirror) = config.extra.get("mirror").and_then(|v| v.as_str()) {
            self.mirror = mirror.to_string();
        }
        if let Some(release) = config.extra.get("release").and_then(|v| v.as_str()) {
            self.release = release.to_string();
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

        tracing::info!("Zypper plugin initialized (mirror: {})", self.mirror);
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let packages = self.packages.read().unwrap();
        let q = query.to_lowercase();
        Ok(packages
            .iter()
            .filter(|(_, e)| {
                e.name.to_lowercase().contains(&q) || e.summary.to_lowercase().contains(&q)
            })
            .take(50)
            .map(|(repo, e)| self.entry_to_candidate(repo, e))
            .collect())
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        let packages = self.packages.read().unwrap();
        let found = packages.iter().find(|(_, e)| {
            e.name == name && version.is_none_or(|v| e.version == v || e.evr() == v)
        });
        Ok(found.map(|(repo, e)| self.entry_to_candidate(repo, e)))
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        let filename = candidate
            .download_url
            .rsplit('/')
            .next()
            .unwrap_or("package.rpm");
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
        crate::plugin::rpm::extract::extract_rpm(archive_path, extract_dir.path())?;
        crate::plugin::dnf::classify_extracted_rpm(extract_dir, archive_path, "zypper")
    }

    fn sync(&self) -> ZlResult<()> {
        let mut all_entries = Vec::new();

        for repo in &self.repos {
            match self.sync_repo(repo) {
                Ok(entries) => {
                    for e in entries {
                        all_entries.push((repo.clone(), e));
                    }
                }
                Err(e) => tracing::warn!("Zypper: failed to sync {}: {}", repo, e),
            }
        }

        let mut packages = self.packages.write().unwrap();
        *packages = all_entries;

        tracing::info!("Zypper: {} packages loaded", packages.len());
        Ok(())
    }
}

impl ZypperPlugin {
    /// Fetch and parse a single repo's primary metadata, discovering the
    /// primary file through repomd.xml.
    fn sync_repo(&self, repo: &str) -> ZlResult<Vec<RpmEntry>> {
        use crate::plugin::rpm::repomd;

        let repomd_url = self.repomd_url(repo);
        tracing::info!("Zypper: syncing {} from {}", repo, repomd_url);

        let repomd_bytes = self.get_bytes(&repomd_url)?;
        let data = repomd::parse_repomd(std::io::Cursor::new(repomd_bytes))?;
        let href = repomd::primary_href(&data).ok_or_else(|| ZlError::Plugin {
            plugin: "zypper".into(),
            message: format!("no primary metadata listed in repomd.xml for {}", repo),
        })?;

        let primary_url = format!("{}/{}", self.base_url(repo), href);
        let primary_bytes = self.get_bytes(&primary_url)?;
        repomd::parse_primary_by_href(&href, primary_bytes)
    }

    /// GET a URL, returning its body bytes or a DownloadFailed error.
    fn get_bytes(&self, url: &str) -> ZlResult<Vec<u8>> {
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| ZlError::DownloadFailed {
                url: url.to_string(),
                attempts: 1,
                message: e.to_string(),
            })?;
        if !resp.status().is_success() {
            return Err(ZlError::DownloadFailed {
                url: url.to_string(),
                attempts: 1,
                message: format!("HTTP {}", resp.status()),
            });
        }
        resp.bytes()
            .map(|b| b.to_vec())
            .map_err(|e| ZlError::DownloadFailed {
                url: url.to_string(),
                attempts: 1,
                message: e.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zypper_plugin_default() {
        let p = ZypperPlugin::new();
        assert_eq!(p.name(), "zypper");
        assert_eq!(p.display_name(), "openSUSE/SLES (Zypper)");
        assert_eq!(p.release, "tumbleweed");
    }

    #[test]
    fn test_zypper_repomd_url_tumbleweed() {
        let p = ZypperPlugin::new();
        let url = p.repomd_url("oss");
        assert!(url.contains("tumbleweed"));
        assert!(url.ends_with("repodata/repomd.xml"));
    }

    #[test]
    fn test_zypper_repomd_url_leap() {
        let mut p = ZypperPlugin::new();
        p.release = "15.5".to_string();
        let url = p.repomd_url("oss");
        assert!(url.contains("leap/15.5"));
        assert!(url.ends_with("repodata/repomd.xml"));
    }
}
