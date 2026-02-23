//! AppImage plugin — installs AppImages from AppImageHub and GitHub.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.appimage]
//! # No configuration needed
//! ```
//!
//! Usage:  zl install kdenlive --from appimage
//!         zl search blender --from appimage
//!
//! Uses the AppImageHub feed (appimage.github.io) for searching.

use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const APPIMAGE_FEED: &str = "https://appimage.github.io/feed.json";

#[derive(serde::Deserialize)]
struct AppImageFeed {
    items: Vec<AppImageItem>,
}

#[derive(serde::Deserialize)]
struct AppImageItem {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    links: Vec<AppImageLink>,
}

#[derive(serde::Deserialize)]
struct AppImageLink {
    #[serde(rename = "type")]
    link_type: String,
    url: String,
}

pub struct AppImagePlugin {
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
    /// Cached feed data
    feed: std::sync::RwLock<Vec<AppImageItem>>,
}

impl Default for AppImagePlugin {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            feed: std::sync::RwLock::new(Vec::new()),
        }
    }
}

impl AppImagePlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn item_to_candidate(&self, item: &AppImageItem) -> PackageCandidate {
        let download_url = item
            .links
            .iter()
            .find(|l| l.link_type == "Download")
            .map(|l| l.url.clone())
            .unwrap_or_default();

        PackageCandidate {
            name: item.name.clone(),
            version: String::new(), // AppImageHub doesn't always have version
            description: item.description.clone(),
            arch: std::env::consts::ARCH.to_string(),
            source: "appimage".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url,
            checksum: None,
        }
    }
}

impl SourcePlugin for AppImagePlugin {
    fn name(&self) -> &str {
        "appimage"
    }

    fn display_name(&self) -> &str {
        "AppImageHub"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }
        tracing::info!("AppImage plugin initialized");
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let feed = self.feed.read().unwrap();
        let q = query.to_lowercase();
        Ok(feed
            .iter()
            .filter(|item| {
                item.name.to_lowercase().contains(&q)
                    || item.description.to_lowercase().contains(&q)
            })
            .take(50)
            .map(|item| self.item_to_candidate(item))
            .collect())
    }

    fn resolve(&self, name: &str, _version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        let feed = self.feed.read().unwrap();
        let found = feed
            .iter()
            .find(|item| item.name.to_lowercase() == name.to_lowercase());
        Ok(found.map(|item| self.item_to_candidate(item)))
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        if candidate.download_url.is_empty() {
            return Err(ZlError::Plugin {
                plugin: "appimage".into(),
                message: format!("No download URL for AppImage '{}'", candidate.name),
            });
        }

        let filename = candidate
            .download_url
            .rsplit('/')
            .next()
            .unwrap_or(&candidate.name);
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

            // Make executable
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))?;

            Ok(dest.clone())
        })
    }

    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage> {
        // AppImages are self-contained executables — place in usr/bin/
        let extract_dir = tempfile::tempdir()?;
        let bin_dir = extract_dir.path().join("usr").join("bin");
        std::fs::create_dir_all(&bin_dir)?;

        let fname = archive_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Clean name: strip version and extension
        let bin_name = fname.split('-').next().unwrap_or(&fname).to_lowercase();

        let dest_bin = bin_dir.join(&bin_name);
        std::fs::copy(archive_path, &dest_bin)?;

        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest_bin, std::fs::Permissions::from_mode(0o755))?;

        let metadata = PackageCandidate {
            name: bin_name,
            version: String::new(),
            description: String::new(),
            arch: std::env::consts::ARCH.to_string(),
            source: "appimage".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: std::fs::metadata(archive_path)
                .map(|m| m.len())
                .unwrap_or(0),
            download_url: String::new(),
            checksum: None,
        };

        Ok(ExtractedPackage {
            extract_dir,
            metadata,
            files: vec![dest_bin.clone()],
            elf_files: vec![dest_bin],
            script_files: vec![],
        })
    }

    fn sync(&self) -> ZlResult<()> {
        tracing::info!("AppImage: syncing feed from {}", APPIMAGE_FEED);

        let resp = self
            .client
            .get(APPIMAGE_FEED)
            .send()
            .map_err(|e| ZlError::DownloadFailed {
                url: APPIMAGE_FEED.into(),
                attempts: 1,
                message: e.to_string(),
            })?;

        if !resp.status().is_success() {
            tracing::warn!("AppImage: failed to sync feed: HTTP {}", resp.status());
            return Ok(());
        }

        let feed_data: AppImageFeed = resp.json().map_err(|e| ZlError::Plugin {
            plugin: "appimage".into(),
            message: format!("Failed to parse AppImage feed: {}", e),
        })?;

        let count = feed_data.items.len();
        let mut feed = self.feed.write().unwrap();
        *feed = feed_data.items;
        tracing::info!("AppImage: {} apps loaded", count);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_appimage_plugin_default() {
        let p = AppImagePlugin::new();
        assert_eq!(p.name(), "appimage");
        assert_eq!(p.display_name(), "AppImageHub");
    }
}
