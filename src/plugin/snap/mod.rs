//! Snap plugin — installs packages from the Snapcraft Store.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.snap]
//! channel = "stable"
//! ```
//!
//! Usage:  zl install firefox --from snap
//!         zl search vlc --from snap
//!
//! Uses the Snapcraft Store API for searching and metadata.
//! Requires `snap` CLI tool for actual installation.

use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const SNAP_API: &str = "https://api.snapcraft.io/v2";

#[derive(serde::Deserialize)]
struct SnapSearchResponse {
    results: Vec<SnapSearchResult>,
}

#[derive(serde::Deserialize)]
struct SnapSearchResult {
    name: String,
    snap: SnapInfo,
}

#[derive(serde::Deserialize)]
struct SnapInfo {
    #[serde(default)]
    summary: String,
}

#[derive(serde::Deserialize)]
struct SnapDetailResponse {
    #[serde(rename = "channel-map")]
    channel_map: Vec<SnapChannel>,
}

#[derive(serde::Deserialize)]
struct SnapChannel {
    channel: SnapChannelInfo,
    version: String,
    download: SnapDownload,
}

#[derive(serde::Deserialize)]
struct SnapChannelInfo {
    name: String,
    architecture: String,
}

#[derive(serde::Deserialize)]
struct SnapDownload {
    url: String,
    size: u64,
    sha3_384: Option<String>,
}

pub struct SnapPlugin {
    channel: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
}

impl Default for SnapPlugin {
    fn default() -> Self {
        Self {
            channel: "stable".to_string(),
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl SnapPlugin {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SourcePlugin for SnapPlugin {
    fn name(&self) -> &str {
        "snap"
    }

    fn display_name(&self) -> &str {
        "Snapcraft Store"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        if let Some(channel) = config.extra.get("channel").and_then(|v| v.as_str()) {
            self.channel = channel.to_string();
        }

        tracing::info!("Snap plugin initialized (channel: {})", self.channel);
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let url = format!(
            "{}/snaps/find?q={}&fields=title,summary,publisher",
            SNAP_API, query
        );

        let resp = self
            .client
            .get(&url)
            .header("Snap-Device-Series", "16")
            .header("Snap-Device-Architecture", snap_arch())
            .send()
            .map_err(|e| ZlError::Plugin {
                plugin: "snap".into(),
                message: format!("Snap search failed: {}", e),
            })?;

        if !resp.status().is_success() {
            return Err(ZlError::Plugin {
                plugin: "snap".into(),
                message: format!("Snap API returned HTTP {}", resp.status()),
            });
        }

        let search_resp: SnapSearchResponse = resp.json().map_err(|e| ZlError::Plugin {
            plugin: "snap".into(),
            message: format!("Failed to parse Snap response: {}", e),
        })?;

        let candidates = search_resp
            .results
            .into_iter()
            .map(|r| PackageCandidate {
                name: r.name,
                version: String::new(), // Version requires a detail call
                description: r.snap.summary,
                arch: snap_arch().to_string(),
                source: "snap".into(),
                dependencies: vec![],
                provides: vec![],
                conflicts: vec![],
                installed_size: 0,
                download_url: String::new(),
                checksum: None,
            })
            .collect();

        Ok(candidates)
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        let url = format!("{}/snaps/info/{}", SNAP_API, name);

        let resp = self
            .client
            .get(&url)
            .header("Snap-Device-Series", "16")
            .header("Snap-Device-Architecture", snap_arch())
            .send()
            .map_err(|e| ZlError::Plugin {
                plugin: "snap".into(),
                message: format!("Snap info failed: {}", e),
            })?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !resp.status().is_success() {
            return Err(ZlError::Plugin {
                plugin: "snap".into(),
                message: format!("Snap API returned HTTP {}", resp.status()),
            });
        }

        let detail: SnapDetailResponse = resp.json().map_err(|e| ZlError::Plugin {
            plugin: "snap".into(),
            message: format!("Failed to parse Snap detail: {}", e),
        })?;

        // Find the channel matching our preference and arch
        let arch = snap_arch();
        let channel = detail
            .channel_map
            .iter()
            .find(|c| {
                c.channel.name == self.channel
                    && c.channel.architecture == arch
                    && version.is_none_or(|v| c.version == v)
            })
            .or_else(|| {
                detail
                    .channel_map
                    .iter()
                    .find(|c| c.channel.architecture == arch)
            });

        Ok(channel.map(|c| PackageCandidate {
            name: name.to_string(),
            version: c.version.clone(),
            description: String::new(),
            arch: c.channel.architecture.clone(),
            source: format!("snap/{}", c.channel.name),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: c.download.size,
            download_url: c.download.url.clone(),
            checksum: c.download.sha3_384.clone(),
        }))
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        let filename = format!("{}-{}.snap", candidate.name, candidate.version);
        let dest = dest_dir.join(&filename);
        if dest.exists() {
            return Ok(dest);
        }

        if candidate.download_url.is_empty() {
            return Err(ZlError::Plugin {
                plugin: "snap".into(),
                message: format!("No download URL for snap '{}'", candidate.name),
            });
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
        // .snap files are SquashFS images
        // We need unsquashfs to extract them
        let extract_dir = tempfile::tempdir()?;

        let output = std::process::Command::new("unsquashfs")
            .args([
                "-f",
                "-d",
                &extract_dir.path().to_string_lossy(),
                &archive_path.to_string_lossy(),
            ])
            .output()
            .map_err(|_| ZlError::BuildToolMissing {
                tool: "unsquashfs (squashfs-tools)".into(),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ZlError::Archive(format!("unsquashfs failed: {}", stderr)));
        }

        classify_extracted(extract_dir, archive_path)
    }

    fn sync(&self) -> ZlResult<()> {
        tracing::info!("Snap: nothing to sync (Snapcraft API queries are live)");
        Ok(())
    }
}

fn snap_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "arm" => "armhf",
        "i686" => "i386",
        other => other,
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
        arch: snap_arch().to_string(),
        source: "snap".into(),
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
    fn test_snap_plugin_default() {
        let p = SnapPlugin::new();
        assert_eq!(p.name(), "snap");
        assert_eq!(p.display_name(), "Snapcraft Store");
        assert_eq!(p.channel, "stable");
    }

    #[test]
    fn test_snap_arch() {
        let arch = snap_arch();
        // Should map x86_64 to amd64
        if std::env::consts::ARCH == "x86_64" {
            assert_eq!(arch, "amd64");
        }
    }
}
