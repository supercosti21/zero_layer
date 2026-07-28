//! Nix plugin — installs packages from the Nix binary cache (cache.nixos.org).
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.nix]
//! channel = "nixos-unstable"
//! cache_url = "https://cache.nixos.org"
//! ```
//!
//! Usage:  zl install firefox --from nix
//!         zl search ripgrep --from nix
//!
//! Search uses the search.nixos.org API (ElasticSearch).
//! Downloads use the Nix binary cache (NAR archives).

pub mod nar;

use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const CACHE_URL: &str = "https://cache.nixos.org";

/// ElasticSearch mapping-schema version baked into the search.nixos.org index
/// name (`latest-<version>-<channel>`). It is bumped whenever the backend
/// re-indexes with a new schema; overridable via `[plugins.nix] index_version`.
const DEFAULT_INDEX_VERSION: u32 = 50;

/// Public read-only credentials the search.nixos.org web UI ships in its
/// frontend bundle. The ElasticSearch backend rejects anonymous queries with
/// 401, so these must be sent on every search.
const SEARCH_USERNAME: &str = "aWVSALXpZv";
const SEARCH_PASSWORD: &str = "X8gPHnzL52wFEekuxsfQ9cSh";

#[derive(serde::Deserialize)]
struct NixSearchResponse {
    hits: NixSearchHits,
}

#[derive(serde::Deserialize)]
struct NixSearchHits {
    hits: Vec<NixSearchHit>,
}

#[derive(serde::Deserialize)]
struct NixSearchHit {
    #[serde(rename = "_source")]
    source: NixPackageSource,
}

#[derive(serde::Deserialize)]
struct NixPackageSource {
    package_pname: String,
    package_pversion: String,
    package_description: Option<String>,
    package_attr_name: String,
}

pub struct NixPlugin {
    channel: String,
    index_version: u32,
    cache_url: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
}

impl Default for NixPlugin {
    fn default() -> Self {
        Self {
            channel: "nixos-unstable".to_string(),
            index_version: DEFAULT_INDEX_VERSION,
            cache_url: CACHE_URL.to_string(),
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
        }
    }
}

impl NixPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn search_api_url(&self) -> String {
        // The index name is `latest-<mapping-schema-version>-<channel>`.
        format!(
            "https://search.nixos.org/backend/latest-{version}-{channel}/_search",
            version = self.index_version,
            channel = self.channel
        )
    }
}

impl SourcePlugin for NixPlugin {
    fn name(&self) -> &str {
        "nix"
    }

    fn display_name(&self) -> &str {
        "Nix Packages (nixpkgs)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        if let Some(channel) = config.extra.get("channel").and_then(|v| v.as_str()) {
            self.channel = channel.to_string();
        }
        if let Some(version) = config
            .extra
            .get("index_version")
            .and_then(|v| v.as_integer())
        {
            self.index_version = version as u32;
        }
        if let Some(url) = config.extra.get("cache_url").and_then(|v| v.as_str()) {
            self.cache_url = url.to_string();
        }

        tracing::info!("Nix plugin initialized (channel: {})", self.channel);
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        let url = self.search_api_url();
        let body = serde_json::json!({
            "from": 0,
            "size": 50,
            "query": {
                "multi_match": {
                    "query": query,
                    "fields": ["package_pname^3", "package_attr_name^2", "package_description"],
                    "type": "best_fields"
                }
            }
        });

        let resp = self
            .client
            .post(&url)
            .basic_auth(SEARCH_USERNAME, Some(SEARCH_PASSWORD))
            .json(&body)
            .send()
            .map_err(|e| ZlError::Plugin {
                plugin: "nix".into(),
                message: format!("Nix search API failed: {}", e),
            })?;

        if !resp.status().is_success() {
            return Err(ZlError::Plugin {
                plugin: "nix".into(),
                message: format!("Nix search API returned HTTP {}", resp.status()),
            });
        }

        let search_resp: NixSearchResponse = resp.json().map_err(|e| ZlError::Plugin {
            plugin: "nix".into(),
            message: format!("Failed to parse Nix search response: {}", e),
        })?;

        let candidates = search_resp
            .hits
            .hits
            .into_iter()
            .map(|hit| PackageCandidate {
                name: hit.source.package_pname,
                version: hit.source.package_pversion,
                description: hit.source.package_description.unwrap_or_default(),
                arch: std::env::consts::ARCH.to_string(),
                source: format!("nix/{}", hit.source.package_attr_name),
                dependencies: vec![],
                provides: vec![],
                conflicts: vec![],
                installed_size: 0,
                download_url: String::new(), // Resolved at download time via cache
                checksum: None,
            })
            .collect();

        Ok(candidates)
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        // Search for exact name match
        let results = self.search(name)?;
        let found = results
            .into_iter()
            .find(|c| c.name == name && version.is_none_or(|v| c.version == v));
        Ok(found)
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        // For Nix packages, we'd normally need to resolve the store path
        // and download the NAR from the binary cache. This is a simplified version.
        let filename = format!("{}-{}.nar.xz", candidate.name, candidate.version);
        let dest = dest_dir.join(&filename);

        if dest.exists() {
            return Ok(dest);
        }

        // In a full implementation, we'd:
        // 1. Query cache.nixos.org/<store-hash>.narinfo
        // 2. Get the NAR URL from narinfo
        // 3. Download the NAR
        // For now, return an error indicating the package needs nix-store
        Err(ZlError::Plugin {
            plugin: "nix".into(),
            message: format!(
                "Direct NAR download not yet supported for '{}'. \
                 Use `nix profile install nixpkgs#{}` as a workaround.",
                candidate.name, candidate.name
            ),
        })
    }

    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage> {
        // NAR extraction
        nar::extract_nar(archive_path)
    }

    fn sync(&self) -> ZlResult<()> {
        tracing::info!("Nix: nothing to sync (search queries are live)");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nix_plugin_default() {
        let p = NixPlugin::new();
        assert_eq!(p.name(), "nix");
        assert_eq!(p.display_name(), "Nix Packages (nixpkgs)");
        assert_eq!(p.channel, "nixos-unstable");
    }

    #[test]
    fn test_nix_search_api_url() {
        let p = NixPlugin::new();
        let url = p.search_api_url();
        assert!(url.contains("nixos-unstable"));
        assert!(url.contains("latest-50-"));
        assert!(url.contains("_search"));
    }

    #[test]
    fn test_nix_index_version_override() {
        let mut p = NixPlugin::new();
        p.index_version = 51;
        assert!(p.search_api_url().contains("latest-51-nixos-unstable"));
    }
}
