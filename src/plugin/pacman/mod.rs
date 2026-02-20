pub mod database;
pub mod mirror;
pub mod package;

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

use self::database::DbEntry;
use self::mirror::Mirror;

/// The default Arch Linux repositories to sync
const DEFAULT_REPOS: &[&str] = &["core", "extra"];
const DEFAULT_ARCH: &str = "x86_64";

pub struct PacmanPlugin {
    mirrors: Vec<Mirror>,
    /// repo name -> list of database entries
    db_cache: RwLock<Vec<(String, Vec<DbEntry>)>>,
    cache_dir: PathBuf,
    arch: String,
    repos: Vec<String>,
}

impl PacmanPlugin {
    pub fn new() -> Self {
        Self {
            mirrors: Vec::new(),
            db_cache: RwLock::new(Vec::new()),
            cache_dir: PathBuf::new(),
            arch: DEFAULT_ARCH.to_string(),
            repos: DEFAULT_REPOS.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn primary_mirror(&self) -> ZlResult<&Mirror> {
        self.mirrors.first().ok_or_else(|| ZlError::Plugin {
            plugin: "pacman".into(),
            message: "No mirrors configured".into(),
        })
    }

    /// Search the cached database for entries matching a query
    fn search_db(&self, query: &str) -> Vec<(String, DbEntry)> {
        let cache = self.db_cache.read().unwrap();
        let query_lower = query.to_lowercase();

        let mut results = Vec::new();
        for (repo, entries) in cache.iter() {
            for entry in entries {
                if entry.name.to_lowercase().contains(&query_lower)
                    || entry.description.to_lowercase().contains(&query_lower)
                {
                    results.push((repo.clone(), entry.clone()));
                }
            }
        }
        results
    }

    /// Find an exact package by name (and optionally version) in the cached database
    fn find_in_db(&self, name: &str, version: Option<&str>) -> Option<(String, DbEntry)> {
        let cache = self.db_cache.read().unwrap();

        for (repo, entries) in cache.iter() {
            for entry in entries {
                if entry.name == name {
                    if let Some(v) = version {
                        if entry.version == v {
                            return Some((repo.clone(), entry.clone()));
                        }
                    } else {
                        return Some((repo.clone(), entry.clone()));
                    }
                }
            }
        }
        None
    }
}

impl SourcePlugin for PacmanPlugin {
    fn name(&self) -> &str {
        "pacman"
    }

    fn display_name(&self) -> &str {
        "Arch Linux (pacman)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        std::fs::create_dir_all(&self.cache_dir)?;

        // Load custom mirrorlist path from config, or use defaults
        let mirrorlist_path = config
            .extra
            .get("mirrorlist")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        self.mirrors = mirror::load_mirrors(mirrorlist_path.as_deref())?;

        // Custom arch if specified
        if let Some(arch) = config.extra.get("arch").and_then(|v| v.as_str()) {
            self.arch = arch.to_string();
        }

        // Custom repos if specified
        if let Some(repos) = config.extra.get("repos").and_then(|v| v.as_array()) {
            let custom: Vec<String> = repos
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
            if !custom.is_empty() {
                self.repos = custom;
            }
        }

        tracing::info!(
            "Pacman plugin initialized with {} mirrors, repos: {:?}",
            self.mirrors.len(),
            self.repos
        );
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let results = self.search_db(query);
        let mirror = self.primary_mirror()?;

        Ok(results
            .into_iter()
            .map(|(repo, entry)| database::entry_to_candidate(&entry, mirror, &repo))
            .collect())
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        match self.find_in_db(name, version) {
            Some((repo, entry)) => {
                let mirror = self.primary_mirror()?;
                Ok(Some(database::entry_to_candidate(&entry, mirror, &repo)))
            }
            None => Ok(None),
        }
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        package::download(candidate, dest_dir)
    }

    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage> {
        // Create a minimal candidate — it will be enriched by .PKGINFO parsing
        let filename = archive_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let placeholder = PackageCandidate {
            name: filename.to_string(),
            version: String::new(),
            description: String::new(),
            arch: self.arch.clone(),
            source: "pacman".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };
        package::extract(archive_path, placeholder)
    }

    fn sync(&self) -> ZlResult<()> {
        let mirror = self.primary_mirror()?;
        let mut all_entries = Vec::new();

        for repo in &self.repos {
            match database::sync_repo(mirror, repo, &self.arch, &self.cache_dir) {
                Ok(entries) => {
                    tracing::info!("Synced {}: {} packages", repo, entries.len());
                    all_entries.push((repo.clone(), entries));
                }
                Err(e) => {
                    tracing::warn!("Failed to sync {}: {}", repo, e);
                }
            }
        }

        let mut cache = self.db_cache.write().unwrap();
        *cache = all_entries;
        Ok(())
    }
}
