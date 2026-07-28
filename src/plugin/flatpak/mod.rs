//! Flatpak plugin — installs applications from Flathub.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.flatpak]
//! remote = "flathub"
//! ```
//!
//! Usage:  zl install firefox --from flatpak
//!         zl search gimp --from flatpak
//!
//! Uses the Flathub API for searching and listing applications.
//! Requires `flatpak` CLI tool to be installed for actual downloads.

use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const FLATHUB_API: &str = "https://flathub.org/api/v2";

#[derive(serde::Deserialize)]
struct FlathubSearchResponse {
    #[serde(default)]
    hits: Vec<FlathubApp>,
}

#[derive(serde::Deserialize)]
struct FlathubApp {
    app_id: String,
    name: String,
    summary: Option<String>,
}

#[derive(serde::Deserialize)]
struct FlathubAppDetail {
    #[serde(default)]
    releases: Vec<FlathubRelease>,
}

#[derive(serde::Deserialize)]
struct FlathubRelease {
    version: Option<String>,
}

pub struct FlatpakPlugin {
    remote: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
}

impl Default for FlatpakPlugin {
    fn default() -> Self {
        Self {
            remote: "flathub".to_string(),
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl FlatpakPlugin {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SourcePlugin for FlatpakPlugin {
    fn name(&self) -> &str {
        "flatpak"
    }

    fn display_name(&self) -> &str {
        "Flathub (Flatpak)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        if let Some(remote) = config.extra.get("remote").and_then(|v| v.as_str()) {
            self.remote = remote.to_string();
        }

        tracing::info!("Flatpak plugin initialized (remote: {})", self.remote);
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        // Flathub API v2 search is POST with a JSON body — a GET on this
        // endpoint answers 405 Method Not Allowed.
        let url = format!("{}/search", FLATHUB_API);
        let body = serde_json::json!({ "query": query });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .map_err(|e| ZlError::Plugin {
                plugin: "flatpak".into(),
                message: format!("Flathub search failed: {}", e),
            })?;

        if !resp.status().is_success() {
            return Err(ZlError::Plugin {
                plugin: "flatpak".into(),
                message: format!("Flathub API returned HTTP {}", resp.status()),
            });
        }

        let search_resp: FlathubSearchResponse = resp.json().map_err(|e| ZlError::Plugin {
            plugin: "flatpak".into(),
            message: format!("Failed to parse Flathub response: {}", e),
        })?;

        let candidates = search_resp
            .hits
            .into_iter()
            .map(|app| {
                // Try to get version from app detail (best-effort)
                let version = self.get_app_version(&app.app_id).unwrap_or_default();

                PackageCandidate {
                    name: app.name.clone(),
                    version,
                    description: app.summary.unwrap_or_default(),
                    arch: std::env::consts::ARCH.to_string(),
                    source: "flatpak".into(),
                    dependencies: vec![],
                    provides: vec![],
                    conflicts: vec![],
                    installed_size: 0,
                    download_url: app.app_id,
                    checksum: None,
                }
            })
            .collect();

        Ok(candidates)
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        // Search by name and find exact match
        let results = self.search(name)?;
        let found = results.into_iter().find(|c| {
            c.name.to_lowercase() == name.to_lowercase() && version.is_none_or(|v| c.version == v)
        });
        Ok(found)
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        // Flatpak uses ostree, so we need the `flatpak` CLI
        let app_id = &candidate.download_url; // We stored app_id in download_url
        let dest = dest_dir.join(format!("{}.flatpak", app_id));

        if dest.exists() {
            return Ok(dest);
        }

        // Check if flatpak CLI is available
        let output = std::process::Command::new("flatpak")
            .args(["--version"])
            .output()
            .map_err(|_| ZlError::BuildToolMissing {
                tool: "flatpak".into(),
            })?;

        if !output.status.success() {
            return Err(ZlError::BuildToolMissing {
                tool: "flatpak".into(),
            });
        }

        // Install to a temporary location
        let output = std::process::Command::new("flatpak")
            .args([
                "install",
                "--noninteractive",
                "--no-deploy",
                &self.remote,
                app_id,
            ])
            .output()
            .map_err(|e| ZlError::Plugin {
                plugin: "flatpak".into(),
                message: format!("flatpak install failed: {}", e),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ZlError::Plugin {
                plugin: "flatpak".into(),
                message: format!("flatpak install failed: {}", stderr),
            });
        }

        // Mark the destination file
        std::fs::write(&dest, format!("flatpak:{}", app_id))?;
        Ok(dest)
    }

    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage> {
        // Flatpak apps are managed by the flatpak runtime
        let extract_dir = tempfile::tempdir()?;
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
            source: "flatpak".into(),
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
            files: vec![],
            elf_files: vec![],
            script_files: vec![],
        })
    }

    fn sync(&self) -> ZlResult<()> {
        tracing::info!("Flatpak: nothing to sync (Flathub API queries are live)");
        Ok(())
    }
}

impl FlatpakPlugin {
    fn get_app_version(&self, app_id: &str) -> Option<String> {
        let url = format!("{}/appstream/{}", FLATHUB_API, app_id);
        let resp = self.client.get(&url).send().ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let detail: FlathubAppDetail = resp.json().ok()?;
        detail.releases.first().and_then(|r| r.version.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flatpak_plugin_default() {
        let p = FlatpakPlugin::new();
        assert_eq!(p.name(), "flatpak");
        assert_eq!(p.display_name(), "Flathub (Flatpak)");
        assert_eq!(p.remote, "flathub");
    }
}
