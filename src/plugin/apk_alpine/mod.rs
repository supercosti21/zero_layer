//! Alpine APK plugin — installs packages from Alpine Linux repositories.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.apk]
//! mirror = "https://dl-cdn.alpinelinux.org/alpine"
//! branch = "v3.20"
//! repos = ["main", "community"]
//! arch = "x86_64"
//! ```
//!
//! Usage:  zl install curl --from apk
//!         zl search nginx --from apk

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const DEFAULT_MIRROR: &str = "https://dl-cdn.alpinelinux.org/alpine";
const DEFAULT_BRANCH: &str = "v3.20";

/// A package entry parsed from APKINDEX.
#[derive(Debug, Clone)]
struct ApkEntry {
    name: String,
    version: String,
    arch: String,
    description: String,
    installed_size: u64,
    depends: Vec<String>,
    provides: Vec<String>,
    repo: String,
}

pub struct ApkAlpinePlugin {
    mirror: String,
    branch: String,
    repos: Vec<String>,
    arch: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
    packages: RwLock<Vec<ApkEntry>>,
}

impl Default for ApkAlpinePlugin {
    fn default() -> Self {
        Self {
            mirror: DEFAULT_MIRROR.to_string(),
            branch: DEFAULT_BRANCH.to_string(),
            repos: vec!["main".into(), "community".into()],
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

impl ApkAlpinePlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn index_url(&self, repo: &str) -> String {
        format!(
            "{}/{}/{}/{}/APKINDEX.tar.gz",
            self.mirror, self.branch, repo, self.arch
        )
    }

    fn package_url(&self, repo: &str, name: &str, version: &str, arch: &str) -> String {
        format!(
            "{}/{}/{}/{}/{}-{}.apk",
            self.mirror, self.branch, repo, arch, name, version
        )
    }

    fn entry_to_candidate(&self, entry: &ApkEntry) -> PackageCandidate {
        PackageCandidate {
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone(),
            arch: entry.arch.clone(),
            source: format!("apk/{}", entry.repo),
            dependencies: entry.depends.clone(),
            provides: entry.provides.clone(),
            conflicts: vec![],
            installed_size: entry.installed_size,
            download_url: self.package_url(&entry.repo, &entry.name, &entry.version, &entry.arch),
            checksum: None,
        }
    }
}

/// Parse APKINDEX content (key=value blocks separated by blank lines)
fn parse_apkindex<R: Read>(reader: R, repo: &str) -> ZlResult<Vec<ApkEntry>> {
    let buf = BufReader::new(reader);
    let mut entries = Vec::new();
    let mut current = ApkEntry {
        name: String::new(),
        version: String::new(),
        arch: String::new(),
        description: String::new(),
        installed_size: 0,
        depends: Vec::new(),
        provides: Vec::new(),
        repo: repo.to_string(),
    };

    for line in buf.lines() {
        let line = line.map_err(|e| ZlError::Plugin {
            plugin: "apk".into(),
            message: format!("APKINDEX read error: {}", e),
        })?;

        if line.is_empty() {
            // End of block
            if !current.name.is_empty() {
                entries.push(current.clone());
            }
            current = ApkEntry {
                name: String::new(),
                version: String::new(),
                arch: String::new(),
                description: String::new(),
                installed_size: 0,
                depends: Vec::new(),
                provides: Vec::new(),
                repo: repo.to_string(),
            };
            continue;
        }

        if let Some((key, value)) = line.split_once(':') {
            match key {
                "P" => current.name = value.to_string(),
                "V" => current.version = value.to_string(),
                "A" => current.arch = value.to_string(),
                "T" => current.description = value.to_string(),
                "I" => current.installed_size = value.parse().unwrap_or(0),
                "D" => {
                    current.depends = value.split_whitespace().map(|s| s.to_string()).collect();
                }
                "p" => {
                    current.provides = value.split_whitespace().map(|s| s.to_string()).collect();
                }
                _ => {}
            }
        }
    }

    // Don't forget the last entry
    if !current.name.is_empty() {
        entries.push(current);
    }

    Ok(entries)
}

impl SourcePlugin for ApkAlpinePlugin {
    fn name(&self) -> &str {
        "apk"
    }

    fn display_name(&self) -> &str {
        "Alpine Linux (APK)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        if let Some(mirror) = config.extra.get("mirror").and_then(|v| v.as_str()) {
            self.mirror = mirror.to_string();
        }
        if let Some(branch) = config.extra.get("branch").and_then(|v| v.as_str()) {
            self.branch = branch.to_string();
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

        tracing::info!("Alpine APK plugin initialized (branch: {})", self.branch);
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
            .unwrap_or("package.apk");
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
        // APK files are tar.gz archives with a signature section, control section, and data section
        // The data section is another tar.gz inside
        let extract_dir = tempfile::tempdir()?;

        let file = std::fs::File::open(archive_path)?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut tar = tar::Archive::new(gz);
        tar.set_preserve_permissions(false);
        tar.unpack(extract_dir.path())
            .map_err(|e| ZlError::Archive(format!("APK extraction failed: {}", e)))?;

        classify_extracted(extract_dir, archive_path)
    }

    fn sync(&self) -> ZlResult<()> {
        let mut all_entries = Vec::new();

        for repo in &self.repos {
            let url = self.index_url(repo);
            let cache_path = self.cache_dir.join(format!("{}-APKINDEX.tar.gz", repo));

            tracing::info!("Alpine APK: syncing {} from {}", repo, url);

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
                tracing::warn!(
                    "Alpine APK: failed to sync {}: HTTP {}",
                    repo,
                    resp.status()
                );
                continue;
            }

            let bytes = resp.bytes().map_err(|e| ZlError::DownloadFailed {
                url: url.clone(),
                attempts: 1,
                message: e.to_string(),
            })?;

            if !self.cache_dir.as_os_str().is_empty() {
                let _ = std::fs::write(&cache_path, &bytes);
            }

            // APKINDEX.tar.gz contains an APKINDEX file inside
            let gz = flate2::read::GzDecoder::new(std::io::Cursor::new(&bytes));
            let mut tar = tar::Archive::new(gz);
            for entry in tar.entries().map_err(|e| ZlError::Archive(e.to_string()))? {
                let mut entry = entry.map_err(|e| ZlError::Archive(e.to_string()))?;
                let path = entry
                    .path()
                    .map_err(|e| ZlError::Archive(e.to_string()))?
                    .to_string_lossy()
                    .to_string();
                if path == "APKINDEX" {
                    let mut content = Vec::new();
                    entry
                        .read_to_end(&mut content)
                        .map_err(|e| ZlError::Archive(e.to_string()))?;
                    let entries = parse_apkindex(std::io::Cursor::new(content), repo)?;
                    all_entries.extend(entries);
                    break;
                }
            }
        }

        let mut packages = self.packages.write().unwrap();
        *packages = all_entries;
        tracing::info!("Alpine APK: {} packages loaded", packages.len());
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
        source: "apk".into(),
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
    fn test_apk_plugin_default() {
        let p = ApkAlpinePlugin::new();
        assert_eq!(p.name(), "apk");
        assert_eq!(p.display_name(), "Alpine Linux (APK)");
        assert_eq!(p.branch, "v3.20");
    }

    #[test]
    fn test_apk_index_url() {
        let p = ApkAlpinePlugin::new();
        let url = p.index_url("main");
        assert!(url.contains("v3.20/main"));
        assert!(url.ends_with("APKINDEX.tar.gz"));
    }

    #[test]
    fn test_parse_apkindex() {
        let index = "P:curl\nV:8.5.0-r0\nA:x86_64\nT:URL retrieval utility\nI:262144\nD:ca-certificates libcurl\np:curl=8.5.0-r0\n\nP:wget\nV:1.21.4-r0\nA:x86_64\nT:Network utility to download files\nI:524288\n\n";

        let entries = parse_apkindex(index.as_bytes(), "main").unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "curl");
        assert_eq!(entries[0].version, "8.5.0-r0");
        assert_eq!(entries[0].depends, vec!["ca-certificates", "libcurl"]);
        assert_eq!(entries[1].name, "wget");
    }
}
