//! DNF plugin — installs packages from Fedora/RHEL/CentOS RPM repositories.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.dnf]
//! mirror = "https://mirrors.fedoraproject.org/metalink"
//! repos = ["fedora", "updates"]
//! arch = "x86_64"
//! ```
//!
//! Usage:  zl install bash --from dnf
//!         zl search vim --from dnf

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::rpm::repodata::RpmEntry;
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const DEFAULT_MIRROR: &str = "https://dl.fedoraproject.org/pub/fedora/linux";
// Fedora 40 is EOL and no longer on the mirrors; 43 is the current stable.
// Overridable via `[plugins.dnf] release = "44"`.
const DEFAULT_RELEASE: &str = "43";

pub struct DnfPlugin {
    mirror: String,
    release: String,
    repos: Vec<String>,
    arch: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
    packages: RwLock<Vec<RpmEntry>>,
}

impl Default for DnfPlugin {
    fn default() -> Self {
        Self {
            mirror: DEFAULT_MIRROR.to_string(),
            release: DEFAULT_RELEASE.to_string(),
            repos: vec!["fedora".into(), "updates".into()],
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

impl DnfPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    /// Repository root for a repo, without the trailing `/repodata/...`.
    /// Primary hrefs from repomd.xml are relative to this.
    fn repo_base_url(&self, repo: &str) -> String {
        if repo == "updates" {
            format!(
                "{}/updates/{}/Everything/{}",
                self.mirror, self.release, self.arch
            )
        } else {
            format!(
                "{}/releases/{}/Everything/{}/os",
                self.mirror, self.release, self.arch
            )
        }
    }

    fn repomd_url(&self, repo: &str) -> String {
        format!("{}/repodata/repomd.xml", self.repo_base_url(repo))
    }

    fn entry_to_candidate(&self, entry: &RpmEntry, repo: &str) -> PackageCandidate {
        let base_url = self.repo_base_url(repo);

        PackageCandidate {
            name: entry.name.clone(),
            version: entry.evr(),
            description: entry.summary.clone(),
            arch: entry.arch.clone(),
            source: format!("dnf/{}", repo),
            dependencies: entry.requires.clone(),
            provides: entry.provides.clone(),
            conflicts: entry.conflicts.clone(),
            installed_size: entry.installed_size,
            download_url: format!("{}/{}", base_url, entry.location_href),
            checksum: entry.checksum.clone(),
        }
    }
}

impl SourcePlugin for DnfPlugin {
    fn name(&self) -> &str {
        "dnf"
    }

    fn display_name(&self) -> &str {
        "Fedora/RHEL (DNF)"
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

        tracing::info!("DNF plugin initialized (mirror: {})", self.mirror);
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let packages = self.packages.read().unwrap();
        let q = query.to_lowercase();
        Ok(packages
            .iter()
            .filter(|e| e.name.to_lowercase().contains(&q) || e.summary.to_lowercase().contains(&q))
            .take(50)
            .map(|e| self.entry_to_candidate(e, "fedora"))
            .collect())
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        let packages = self.packages.read().unwrap();
        let found = packages
            .iter()
            .find(|e| e.name == name && version.is_none_or(|v| e.version == v || e.evr() == v));
        Ok(found.map(|e| self.entry_to_candidate(e, "fedora")))
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
        classify_extracted_rpm(extract_dir, archive_path, "dnf")
    }

    fn sync(&self) -> ZlResult<()> {
        let mut all_entries = Vec::new();

        for repo in &self.repos {
            match self.sync_repo(repo) {
                Ok(entries) => all_entries.extend(entries),
                Err(e) => tracing::warn!("DNF: failed to sync {}: {}", repo, e),
            }
        }

        let mut packages = self.packages.write().unwrap();
        *packages = all_entries;

        tracing::info!("DNF: {} packages loaded", packages.len());
        Ok(())
    }
}

impl DnfPlugin {
    /// Fetch and parse a single repo's primary metadata, discovering the
    /// primary file through repomd.xml.
    fn sync_repo(&self, repo: &str) -> ZlResult<Vec<RpmEntry>> {
        use crate::plugin::rpm::repomd;

        let repomd_url = self.repomd_url(repo);
        tracing::info!("DNF: syncing {} from {}", repo, repomd_url);

        let repomd_bytes = self.get_bytes(&repomd_url)?;
        let data = repomd::parse_repomd(std::io::Cursor::new(repomd_bytes))?;
        let href = repomd::primary_href(&data).ok_or_else(|| ZlError::Plugin {
            plugin: "dnf".into(),
            message: format!("no primary metadata listed in repomd.xml for {}", repo),
        })?;

        let primary_url = format!("{}/{}", self.repo_base_url(repo), href);
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

/// Classify extracted RPM files (shared by dnf and zypper plugins).
pub fn classify_extracted_rpm(
    extract_dir: tempfile::TempDir,
    archive_path: &Path,
    source: &str,
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
        source: source.into(),
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
    fn test_dnf_plugin_default() {
        let p = DnfPlugin::new();
        assert_eq!(p.name(), "dnf");
        assert_eq!(p.display_name(), "Fedora/RHEL (DNF)");
        assert_eq!(p.repos, vec!["fedora", "updates"]);
    }

    #[test]
    fn test_dnf_repomd_url() {
        let p = DnfPlugin::new();
        let url = p.repomd_url("fedora");
        assert!(url.contains("repodata/repomd.xml"));
        assert!(url.contains("releases"));
        assert!(url.contains("/43/")); // current release, not EOL 40
    }

    #[test]
    fn test_dnf_updates_repo_base() {
        let p = DnfPlugin::new();
        let url = p.repomd_url("updates");
        assert!(url.contains("/updates/"));
        assert!(url.ends_with("repodata/repomd.xml"));
    }

    #[test]
    fn test_dnf_entry_to_candidate() {
        let p = DnfPlugin::new();
        let entry = RpmEntry {
            name: "bash".into(),
            version: "5.2.26".into(),
            release: "3.fc40".into(),
            arch: "x86_64".into(),
            summary: "The GNU Bourne Again shell".into(),
            description: "Bash is a sh-compatible shell.".into(),
            installed_size: 8000000,
            location_href: "Packages/b/bash-5.2.26-3.fc40.x86_64.rpm".into(),
            checksum: Some("abc123".into()),
            requires: vec!["glibc".into()],
            provides: vec!["bash".into()],
            conflicts: vec![],
        };
        let c = p.entry_to_candidate(&entry, "fedora");
        assert_eq!(c.name, "bash");
        assert_eq!(c.version, "5.2.26-3.fc40");
        assert_eq!(c.source, "dnf/fedora");
        assert!(c.download_url.contains("bash-5.2.26"));
    }
}
