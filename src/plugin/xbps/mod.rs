//! XBPS plugin — installs packages from Void Linux repositories.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.xbps]
//! mirror = "https://repo-default.voidlinux.org"
//! arch = "x86_64"
//! repos = ["current", "current/nonfree"]
//! ```
//!
//! Usage:  zl install curl --from xbps
//!         zl search nginx --from xbps
//!
//! XBPS repodata is a plist (property list) file compressed with zstd.
//! For simplicity we parse the repodata index as a simple key-value format.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const DEFAULT_MIRROR: &str = "https://repo-default.voidlinux.org";

#[derive(Debug, Clone)]
struct XbpsEntry {
    name: String,
    version: String,
    arch: String,
    description: String,
    installed_size: u64,
    depends: Vec<String>,
    provides: Vec<String>,
    filename: String,
    repo: String,
}

pub struct XbpsPlugin {
    mirror: String,
    repos: Vec<String>,
    arch: String,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
    packages: RwLock<Vec<XbpsEntry>>,
}

impl Default for XbpsPlugin {
    fn default() -> Self {
        Self {
            mirror: DEFAULT_MIRROR.to_string(),
            repos: vec!["current".into()],
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

impl XbpsPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn repodata_url(&self, repo: &str) -> String {
        format!("{}/{}/{}-repodata", self.mirror, repo, self.arch)
    }

    fn entry_to_candidate(&self, entry: &XbpsEntry) -> PackageCandidate {
        PackageCandidate {
            name: entry.name.clone(),
            version: entry.version.clone(),
            description: entry.description.clone(),
            arch: entry.arch.clone(),
            source: format!("xbps/{}", entry.repo),
            dependencies: entry.depends.clone(),
            provides: entry.provides.clone(),
            conflicts: vec![],
            installed_size: entry.installed_size,
            download_url: format!("{}/{}/{}", self.mirror, entry.repo, entry.filename),
            checksum: None,
        }
    }
}

/// Parse XBPS repodata: a zstd-compressed tar holding `index.plist`.
///
/// Despite the name, the plist is the XML flavour rather than Apple's binary
/// one, so quick-xml is enough and no plist crate is needed.
fn parse_repodata(data: &[u8], repo: &str, arch: &str) -> Vec<XbpsEntry> {
    let Some(plist) = read_index_plist(data) else {
        tracing::warn!("XBPS: repodata for {} has no index.plist", repo);
        return Vec::new();
    };

    match parse_index_plist(&plist, repo, arch) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("XBPS: could not parse index.plist for {}: {}", repo, e);
            Vec::new()
        }
    }
}

/// Pull `index.plist` out of the zstd-compressed repodata tarball.
fn read_index_plist(data: &[u8]) -> Option<Vec<u8>> {
    let decoded = zstd::stream::decode_all(std::io::Cursor::new(data)).ok()?;
    let mut tar = tar::Archive::new(std::io::Cursor::new(decoded));

    for entry in tar.entries().ok()? {
        let mut entry = entry.ok()?;
        let is_index = entry
            .path()
            .map(|p| p.as_os_str() == "index.plist")
            .unwrap_or(false);
        if is_index {
            let mut content = Vec::new();
            entry.read_to_end(&mut content).ok()?;
            return Some(content);
        }
    }
    None
}

/// The plist is one top-level `<dict>` mapping a package name to a `<dict>` of
/// its metadata, so parsing tracks the nesting depth to tell the package names
/// (depth 1) apart from the metadata keys inside them (depth 2).
fn parse_index_plist(plist: &[u8], repo: &str, arch: &str) -> Result<Vec<XbpsEntry>, String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut xml = Reader::from_reader(std::io::BufReader::new(plist));
    let mut buf = Vec::new();

    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut current: Option<XbpsEntry> = None;
    let mut key = String::new();
    let mut text = String::new();
    // The package file is named after pkgver *and* architecture, which arrive
    // as separate keys, so the name is assembled once the dict closes rather
    // than relying on the order the two happen to appear in.
    let mut pkgver = String::new();
    // Values inside <array> belong to the array's key, not to a new one
    let mut in_array = false;

    loop {
        match xml.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                match e.name().as_ref() {
                    b"dict" => {
                        depth += 1;
                        if depth == 2 {
                            // Entering a package's metadata; `key` holds its name
                            current = Some(new_entry(&key, repo, arch));
                            pkgver.clear();
                        }
                    }
                    b"array" => in_array = true,
                    b"key" => text.clear(),
                    b"string" | b"integer" => text.clear(),
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                text.push_str(&e.xml10_content().unwrap_or_default());
            }
            Ok(Event::GeneralRef(e)) => {
                // Maintainer names carry &lt;mail&gt;, dependencies carry &gt;=
                if let Ok(Some(c)) = e.resolve_char_ref() {
                    text.push(c);
                } else if let Ok(name) = e.decode() {
                    match name.as_ref() {
                        "amp" => text.push('&'),
                        "lt" => text.push('<'),
                        "gt" => text.push('>'),
                        "quot" => text.push('"'),
                        "apos" => text.push('\''),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"key" => key = text.trim().to_string(),
                b"array" => in_array = false,
                b"string" | b"integer" => {
                    if key == "pkgver" {
                        pkgver = text.trim().to_string();
                    }
                    if let Some(entry) = current.as_mut() {
                        apply_field(entry, &key, text.trim(), in_array);
                    }
                    text.clear();
                }
                b"dict" => {
                    if depth == 2
                        && let Some(mut entry) = current.take()
                        && !entry.name.is_empty()
                    {
                        if !pkgver.is_empty() {
                            entry.filename = format!("{}.{}.xbps", pkgver, entry.arch);
                        }
                        entries.push(entry);
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            },
            Err(e) => return Err(e.to_string()),
            _ => {}
        }
        buf.clear();
    }

    Ok(entries)
}

fn new_entry(name: &str, repo: &str, arch: &str) -> XbpsEntry {
    XbpsEntry {
        name: name.to_string(),
        version: String::new(),
        arch: arch.to_string(),
        description: String::new(),
        installed_size: 0,
        depends: Vec::new(),
        provides: Vec::new(),
        filename: String::new(),
        repo: repo.to_string(),
    }
}

/// Assign one plist value to the entry it belongs to.
fn apply_field(entry: &mut XbpsEntry, key: &str, value: &str, in_array: bool) {
    match key {
        "architecture" => entry.arch = value.to_string(),
        "installed_size" => entry.installed_size = value.parse().unwrap_or(0),
        "short_desc" => entry.description = value.to_string(),
        // pkgver is "<name>-<version>", e.g. "jq-1.8.2_1". The package's own
        // name may contain hyphens, so split from the right.
        "pkgver" => {
            entry.version = value
                .rsplit_once('-')
                .map(|(_, version)| version.to_string())
                .unwrap_or_else(|| value.to_string());
        }
        "run_depends" if in_array => entry.depends.push(value.to_string()),
        "provides" if in_array => entry.provides.push(value.to_string()),
        _ => {}
    }
}

impl SourcePlugin for XbpsPlugin {
    fn name(&self) -> &str {
        "xbps"
    }

    fn display_name(&self) -> &str {
        "Void Linux (XBPS)"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        if !self.cache_dir.as_os_str().is_empty() {
            std::fs::create_dir_all(&self.cache_dir)?;
        }

        if let Some(mirror) = config.extra.get("mirror").and_then(|v| v.as_str()) {
            self.mirror = mirror.to_string();
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

        tracing::info!("XBPS plugin initialized (mirror: {})", self.mirror);
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
            .unwrap_or("package.xbps");
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
        // .xbps files are tar.zst archives
        let extract_dir = tempfile::tempdir()?;
        let file = std::fs::File::open(archive_path)?;
        let zst = zstd::stream::Decoder::new(file)
            .map_err(|e| ZlError::Archive(format!("zstd error: {}", e)))?;
        let mut tar = tar::Archive::new(zst);
        tar.set_preserve_permissions(false);
        tar.unpack(extract_dir.path())
            .map_err(|e| ZlError::Archive(format!("XBPS extraction failed: {}", e)))?;

        classify_extracted(extract_dir, archive_path)
    }

    fn sync(&self) -> ZlResult<()> {
        let mut all_entries = Vec::new();

        for repo in &self.repos {
            let url = self.repodata_url(repo);
            let cache_path = self
                .cache_dir
                .join(format!("{}-repodata", repo.replace('/', "_")));

            tracing::info!("XBPS: syncing {} from {}", repo, url);

            match self.client.get(&url).send() {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(bytes) = resp.bytes() {
                        if !self.cache_dir.as_os_str().is_empty() {
                            let _ = std::fs::write(&cache_path, &bytes);
                        }
                        let entries = parse_repodata(&bytes, repo, &self.arch);
                        all_entries.extend(entries);
                    }
                }
                _ => {
                    tracing::warn!("XBPS: failed to sync {}", repo);
                    if cache_path.exists()
                        && let Ok(bytes) = std::fs::read(&cache_path)
                    {
                        let entries = parse_repodata(&bytes, repo, &self.arch);
                        all_entries.extend(entries);
                    }
                }
            }
        }

        let mut packages = self.packages.write().unwrap();
        *packages = all_entries;
        tracing::info!("XBPS: {} packages loaded", packages.len());
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
        source: "xbps".into(),
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
    fn test_xbps_plugin_default() {
        let p = XbpsPlugin::new();
        assert_eq!(p.name(), "xbps");
        assert_eq!(p.display_name(), "Void Linux (XBPS)");
    }

    #[test]
    fn test_xbps_repodata_url() {
        let p = XbpsPlugin::new();
        let url = p.repodata_url("current");
        assert!(url.contains("current"));
        assert!(url.ends_with("-repodata"));
    }

    const SAMPLE_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>jq</key>
	<dict>
		<key>architecture</key>
		<string>x86_64</string>
		<key>installed_size</key>
		<integer>566717</integer>
		<key>maintainer</key>
		<string>Leah &lt;leah@vuxu.org&gt;</string>
		<key>pkgver</key>
		<string>jq-1.8.2_1</string>
		<key>provides</key>
		<array>
			<string>cmd:jq-1.8.2_1</string>
		</array>
		<key>run_depends</key>
		<array>
			<string>glibc&gt;=2.41_1</string>
			<string>oniguruma&gt;=6.8.1_1</string>
		</array>
		<key>short_desc</key>
		<string>Command-line JSON processor</string>
	</dict>
	<key>python3-foo-bar</key>
	<dict>
		<key>architecture</key>
		<string>noarch</string>
		<key>pkgver</key>
		<string>python3-foo-bar-2.1_3</string>
		<key>short_desc</key>
		<string>Hyphenated name</string>
	</dict>
</dict>
</plist>"#;

    #[test]
    fn test_parse_index_plist_reads_packages() {
        let entries = parse_index_plist(SAMPLE_PLIST.as_bytes(), "current", "x86_64").unwrap();
        assert_eq!(entries.len(), 2);

        let jq = &entries[0];
        assert_eq!(jq.name, "jq");
        assert_eq!(jq.version, "1.8.2_1");
        assert_eq!(jq.arch, "x86_64");
        assert_eq!(jq.description, "Command-line JSON processor");
        assert_eq!(jq.installed_size, 566717);
        assert_eq!(jq.repo, "current");
    }

    #[test]
    fn test_parse_index_plist_builds_package_filename() {
        let entries = parse_index_plist(SAMPLE_PLIST.as_bytes(), "current", "x86_64").unwrap();
        // <pkgver>.<arch>.xbps, using the package's own architecture
        assert_eq!(entries[0].filename, "jq-1.8.2_1.x86_64.xbps");
        assert_eq!(entries[1].filename, "python3-foo-bar-2.1_3.noarch.xbps");
    }

    #[test]
    fn test_parse_index_plist_splits_version_from_the_right() {
        let entries = parse_index_plist(SAMPLE_PLIST.as_bytes(), "current", "x86_64").unwrap();
        // A hyphenated package name must not be mistaken for the version
        assert_eq!(entries[1].name, "python3-foo-bar");
        assert_eq!(entries[1].version, "2.1_3");
    }

    #[test]
    fn test_parse_index_plist_collects_arrays() {
        let entries = parse_index_plist(SAMPLE_PLIST.as_bytes(), "current", "x86_64").unwrap();
        // Entities inside array values must survive parsing
        assert_eq!(
            entries[0].depends,
            vec!["glibc>=2.41_1", "oniguruma>=6.8.1_1"]
        );
        assert_eq!(entries[0].provides, vec!["cmd:jq-1.8.2_1"]);
        // A package with no arrays gets empty vectors, not the previous one's
        assert!(entries[1].depends.is_empty());
        assert!(entries[1].provides.is_empty());
    }

    #[test]
    fn test_parse_index_plist_ignores_metadata_keys_as_packages() {
        let entries = parse_index_plist(SAMPLE_PLIST.as_bytes(), "current", "x86_64").unwrap();
        // "architecture", "pkgver" etc. are keys inside a package, not packages
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["jq", "python3-foo-bar"]);
    }
}
