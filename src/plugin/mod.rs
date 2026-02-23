pub mod apk_alpine;
pub mod appimage;
pub mod apt;
pub mod aur;
pub mod dnf;
pub mod flatpak;
pub mod github;
pub mod nix;
pub mod pacman;
pub mod portage;
pub mod rpm;
pub mod snap;
pub mod xbps;
pub mod zypper;

use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::error::ZlResult;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PackageCandidate {
    pub name: String,
    pub version: String,
    pub description: String,
    pub arch: String,
    pub source: String,
    pub dependencies: Vec<String>,
    pub provides: Vec<String>,
    pub conflicts: Vec<String>,
    pub installed_size: u64,
    pub download_url: String,
    pub checksum: Option<String>,
}

pub struct ExtractedPackage {
    pub extract_dir: tempfile::TempDir,
    #[allow(dead_code)]
    pub metadata: PackageCandidate,
    pub files: Vec<PathBuf>,
    pub elf_files: Vec<PathBuf>,
    pub script_files: Vec<PathBuf>,
}

pub trait SourcePlugin: Send + Sync {
    fn name(&self) -> &str;
    fn display_name(&self) -> &str;
    fn init(&mut self, config: &PluginConfig) -> ZlResult<()>;
    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>>;
    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>>;
    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf>;
    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage>;
    fn sync(&self) -> ZlResult<()>;
}

#[derive(Default)]
pub struct PluginRegistry {
    plugins: Vec<Box<dyn SourcePlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Box<dyn SourcePlugin>) {
        self.plugins.push(plugin);
    }

    pub fn get(&self, name: &str) -> Option<&dyn SourcePlugin> {
        self.plugins
            .iter()
            .find(|p| p.name() == name)
            .map(|p| p.as_ref())
    }

    /// Get all registered plugins
    pub fn all(&self) -> Vec<&dyn SourcePlugin> {
        self.plugins.iter().map(|p| p.as_ref()).collect()
    }

    /// Get plugin by name, or fall back to first registered plugin
    pub fn get_or_default(&self, name: Option<&str>) -> Option<&dyn SourcePlugin> {
        match name {
            Some(n) => self.get(n),
            None => self.plugins.first().map(|p| p.as_ref()),
        }
    }

    /// Return the names of all registered plugins
    pub fn names(&self) -> Vec<&str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }

    /// Keep only plugins whose name is in the given list.
    /// Also respects per-plugin `enabled` flag via the config.
    pub fn retain_sources(&mut self, sources: &[String]) {
        self.plugins
            .retain(|p| sources.iter().any(|s| s == p.name()));
    }

    /// List all registered plugin names and their display names
    #[allow(dead_code)]
    pub fn list_info(&self) -> Vec<PluginInfo> {
        self.plugins
            .iter()
            .map(|p| PluginInfo {
                name: p.name().to_string(),
                display_name: p.display_name().to_string(),
                builtin: true,
            })
            .collect()
    }
}

/// Metadata about a plugin (for registry listing)
#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PluginInfo {
    pub name: String,
    pub display_name: String,
    pub builtin: bool,
}

/// Remote plugin registry: fetch available plugins from a URL.
/// Returns a list of PluginInfo for plugins available in the registry.
#[allow(dead_code)]
pub fn fetch_remote_registry(registry_url: &str) -> ZlResult<Vec<PluginInfo>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("zero-layer/0.1")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap_or_default();

    let resp = client
        .get(registry_url)
        .send()
        .map_err(|e| crate::error::ZlError::Plugin {
            plugin: "registry".into(),
            message: format!("Failed to fetch registry: {}", e),
        })?;

    if !resp.status().is_success() {
        return Err(crate::error::ZlError::Plugin {
            plugin: "registry".into(),
            message: format!("Registry returned HTTP {}", resp.status()),
        });
    }

    let plugins: Vec<PluginInfo> = resp.json().map_err(|e| crate::error::ZlError::Plugin {
        plugin: "registry".into(),
        message: format!("Failed to parse registry response: {}", e),
    })?;

    Ok(plugins)
}
