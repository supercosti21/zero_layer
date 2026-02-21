//! AUR (Arch User Repository) plugin.
//!
//! Searches the AUR via its JSON RPC API v5, then builds packages locally
//! using `git` + `makepkg` (requires `base-devel` group installed).
//!
//! Usage:  zl install yay --from aur
//!         zl search rofi-wayland --from aur

use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const AUR_RPC: &str = "https://aur.archlinux.org/rpc/v5";
const AUR_GIT: &str = "https://aur.archlinux.org";

// ── AUR RPC response types ────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct AurResponse {
    results: Vec<AurPackage>,
}

#[derive(serde::Deserialize, Clone)]
struct AurPackage {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "PackageBase")]
    package_base: String,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "Depends", default)]
    depends: Vec<String>,
    #[serde(rename = "Conflicts", default)]
    conflicts: Vec<String>,
    #[serde(rename = "Provides", default)]
    provides: Vec<String>,
}

// ── Plugin struct ─────────────────────────────────────────────────────────────

pub struct AurPlugin {
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
}

impl AurPlugin {
    pub fn new() -> Self {
        Self {
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1 (https://github.com/supercosti21/zero_layer)")
                .build()
                .unwrap_or_default(),
        }
    }

    fn to_candidate(pkg: &AurPackage) -> PackageCandidate {
        PackageCandidate {
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            description: pkg.description.clone().unwrap_or_default(),
            arch: "any".to_string(), // set correctly after build by .PKGINFO
            source: "aur".to_string(),
            dependencies: pkg.depends.clone(),
            provides: pkg.provides.clone(),
            conflicts: pkg.conflicts.clone(),
            installed_size: 0,
            // Store the git clone URL here; `download()` will use it
            download_url: format!("{}/{}.git", AUR_GIT, pkg.package_base),
            checksum: None,
        }
    }

    fn fetch_rpc(&self, url: &str) -> ZlResult<AurResponse> {
        let resp = self
            .client
            .get(url)
            .timeout(std::time::Duration::from_secs(30))
            .send()?;

        if !resp.status().is_success() {
            return Err(ZlError::Plugin {
                plugin: "aur".into(),
                message: format!("AUR RPC returned HTTP {}", resp.status()),
            });
        }

        resp.json::<AurResponse>().map_err(|e| ZlError::Plugin {
            plugin: "aur".into(),
            message: format!("Failed to parse AUR response: {}", e),
        })
    }
}

// ── SourcePlugin implementation ───────────────────────────────────────────────

impl SourcePlugin for AurPlugin {
    fn name(&self) -> &str {
        "aur"
    }

    fn display_name(&self) -> &str {
        "Arch User Repository (AUR)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        std::fs::create_dir_all(&self.cache_dir)?;
        tracing::info!("AUR plugin initialized (live queries, no local DB)");
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let url = format!("{}/search/{}?by=name-desc", AUR_RPC, query);
        let resp = self.fetch_rpc(&url)?;
        Ok(resp.results.iter().map(Self::to_candidate).collect())
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        let url = format!("{}/info/{}", AUR_RPC, name);
        let resp = self.fetch_rpc(&url)?;

        let candidate = resp
            .results
            .iter()
            .find(|p| p.name == name)
            .map(Self::to_candidate);

        // If a version was requested, check it matches
        Ok(candidate.filter(|c| version.map_or(true, |v| c.version == v)))
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        // Verify required tools are present
        check_tool("git")?;
        check_tool("makepkg")?;

        let build_dir = tempfile::tempdir()?;
        let clone_dir = build_dir.path().join("pkg");

        tracing::info!(
            "Cloning AUR package {} from {}",
            candidate.name,
            candidate.download_url
        );

        let clone_status = std::process::Command::new("git")
            .args(["clone", "--depth=1"])
            .arg(&candidate.download_url)
            .arg(&clone_dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .status()?;

        if !clone_status.success() {
            return Err(ZlError::Plugin {
                plugin: "aur".into(),
                message: format!("git clone failed for {}", candidate.download_url),
            });
        }

        tracing::info!("Building {} with makepkg…", candidate.name);

        let build_status = std::process::Command::new("makepkg")
            .args([
                "--syncdeps", // install build deps via pacman
                "--force",    // overwrite existing pkg file
                "--noconfirm",
                "--noprogressbar",
            ])
            .current_dir(&clone_dir)
            .status()?;

        if !build_status.success() {
            return Err(ZlError::BuildFailed {
                package: candidate.name.clone(),
                message: "makepkg failed — check PKGBUILD or install base-devel".into(),
            });
        }

        // Find the built .pkg.tar.* in the clone dir
        for entry in std::fs::read_dir(&clone_dir)? {
            let entry = entry?;
            let path = entry.path();
            let fname = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if fname.ends_with(".pkg.tar.zst") || fname.ends_with(".pkg.tar.xz") {
                let dest = dest_dir.join(fname);
                std::fs::copy(&path, &dest)?;
                tracing::info!("AUR build complete: {}", dest.display());
                return Ok(dest);
            }
        }

        Err(ZlError::BuildFailed {
            package: candidate.name.clone(),
            message: "No .pkg.tar.* found after makepkg — unexpected build output".into(),
        })
    }

    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage> {
        // AUR builds produce standard pacman .pkg.tar.zst archives
        crate::plugin::pacman::package::extract(
            archive_path,
            PackageCandidate {
                name: archive_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                version: String::new(),
                description: String::new(),
                arch: String::new(),
                source: "aur".into(),
                dependencies: vec![],
                provides: vec![],
                conflicts: vec![],
                installed_size: 0,
                download_url: String::new(),
                checksum: None,
            },
        )
    }

    fn sync(&self) -> ZlResult<()> {
        // AUR is queried live — no local database to sync
        tracing::info!("AUR: nothing to sync (queries are made live to aur.archlinux.org)");
        Ok(())
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn check_tool(name: &str) -> ZlResult<()> {
    std::process::Command::new(name)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| ZlError::BuildToolMissing { tool: name.into() })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_candidate() {
        let pkg = AurPackage {
            name: "yay".into(),
            package_base: "yay".into(),
            version: "12.3.5-1".into(),
            description: Some("AUR helper".into()),
            depends: vec!["go".into()],
            conflicts: vec![],
            provides: vec![],
        };
        let c = AurPlugin::to_candidate(&pkg);
        assert_eq!(c.name, "yay");
        assert_eq!(c.source, "aur");
        assert!(c.download_url.contains("aur.archlinux.org"));
    }
}
