//! AUR (Arch User Repository) plugin.
//!
//! Searches the AUR via its JSON RPC API v5, then builds packages locally
//! using `git` + `makepkg` (requires `base-devel` group installed).
//!
//! When searching, also discovers `-bin` variants (precompiled binaries)
//! so users can choose between building from source or using a binary.
//!
//! Usage:  zl install yay --from aur
//!         zl search rofi-wayland --from aur

use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const AUR_RPC: &str = "https://aur.archlinux.org/rpc/v5";
const AUR_GIT: &str = "https://aur.archlinux.org";

/// Common suffixes for AUR binary/precompiled variants
const BIN_SUFFIXES: &[&str] = &["-bin", "-appimage", "-prebuilt"];

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

impl Default for AurPlugin {
    fn default() -> Self {
        Self {
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1 (https://github.com/supercosti21/zero_layer)")
                .build()
                .unwrap_or_default(),
        }
    }
}

impl AurPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn to_candidate(pkg: &AurPackage) -> PackageCandidate {
        // Tag binary variants in the description so the user can tell them apart
        let is_bin_variant = BIN_SUFFIXES.iter().any(|s| pkg.name.ends_with(s));
        let description = if is_bin_variant {
            format!("[binary] {}", pkg.description.clone().unwrap_or_default())
        } else {
            pkg.description.clone().unwrap_or_default()
        };

        PackageCandidate {
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            description,
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
            .send()
            .map_err(|e| ZlError::Plugin {
                plugin: "aur".into(),
                message: format!("AUR RPC request failed: {}", e),
            })?;

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

    /// Fetch info for specific package names via the AUR multiinfo endpoint.
    /// Returns candidates for all names that exist on AUR.
    fn fetch_info_multi(&self, names: &[String]) -> ZlResult<Vec<PackageCandidate>> {
        if names.is_empty() {
            return Ok(vec![]);
        }
        // AUR RPC v5 info endpoint accepts multiple args: /info/{name1},{name2},...
        // But the standard way is multiple &arg[]= params
        let mut url = format!("{}/info?", AUR_RPC);
        for (i, name) in names.iter().enumerate() {
            if i > 0 {
                url.push('&');
            }
            url.push_str(&format!("arg[]={}", name));
        }
        let resp = self.fetch_rpc(&url)?;
        Ok(resp.results.iter().map(Self::to_candidate).collect())
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
        let mut results: Vec<PackageCandidate> =
            resp.results.iter().map(Self::to_candidate).collect();

        // If query doesn't already end with a binary suffix, also look up -bin variants
        let already_has_bin_suffix = BIN_SUFFIXES.iter().any(|s| query.ends_with(s));
        if !already_has_bin_suffix {
            let bin_names: Vec<String> = BIN_SUFFIXES
                .iter()
                .map(|s| format!("{}{}", query, s))
                .collect();

            // Only fetch variants that aren't already in the results
            let existing_names: std::collections::HashSet<&str> =
                results.iter().map(|r| r.name.as_str()).collect();
            let missing: Vec<String> = bin_names
                .into_iter()
                .filter(|n| !existing_names.contains(n.as_str()))
                .collect();

            if let Ok(bin_results) = self.fetch_info_multi(&missing) {
                results.extend(bin_results);
            }
        }

        Ok(results)
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
        Ok(candidate.filter(|c| version.is_none_or(|v| c.version == v)))
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

        let clone_output = std::process::Command::new("git")
            .args(["clone", "--depth=1"])
            .arg(&candidate.download_url)
            .arg(&clone_dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()?;

        if !clone_output.status.success() {
            let stderr = String::from_utf8_lossy(&clone_output.stderr);
            return Err(ZlError::Plugin {
                plugin: "aur".into(),
                message: format!(
                    "git clone failed for {}:\n  {}",
                    candidate.download_url,
                    stderr.trim()
                ),
            });
        }

        tracing::info!("Building {} with makepkg...", candidate.name);

        let build_output = std::process::Command::new("makepkg")
            .args([
                "--syncdeps", // install build deps via pacman
                "--force",    // overwrite existing pkg file
                "--noconfirm",
                "--noprogressbar",
            ])
            .current_dir(&clone_dir)
            .output()?;

        if !build_output.status.success() {
            let stderr = String::from_utf8_lossy(&build_output.stderr);
            let hint = if stderr.contains("base-devel") || stderr.contains("fakeroot") {
                "\n  hint: install base-devel: sudo pacman -S --needed base-devel"
            } else if stderr.contains("PGP") || stderr.contains("signature") {
                "\n  hint: import the PGP key or use makepkg with --skippgpcheck"
            } else if stderr.contains("dependency") {
                "\n  hint: a build dependency could not be installed — check the PKGBUILD"
            } else {
                ""
            };
            return Err(ZlError::BuildFailed {
                package: candidate.name.clone(),
                message: format!(
                    "makepkg failed:\n  {}{}",
                    stderr.lines().take(5).collect::<Vec<_>>().join("\n  "),
                    hint
                ),
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

    #[test]
    fn test_to_candidate_bin_tagged() {
        let pkg = AurPackage {
            name: "yay-bin".into(),
            package_base: "yay-bin".into(),
            version: "12.3.5-1".into(),
            description: Some("AUR helper (prebuilt)".into()),
            depends: vec![],
            conflicts: vec![],
            provides: vec!["yay".into()],
        };
        let c = AurPlugin::to_candidate(&pkg);
        assert_eq!(c.name, "yay-bin");
        assert!(c.description.starts_with("[binary]"));
    }

    #[test]
    fn test_to_candidate_source_not_tagged() {
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
        assert!(!c.description.contains("[binary]"));
    }
}
