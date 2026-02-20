pub mod pacman;

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

pub struct PluginRegistry {
    plugins: Vec<Box<dyn SourcePlugin>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
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
}
