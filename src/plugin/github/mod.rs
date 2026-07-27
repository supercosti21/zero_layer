//! GitHub Releases plugin — installs pre-built binaries from GitHub releases.
//!
//! Package name format: `owner/repo` (e.g., `BurntSushi/ripgrep`).
//! Picks the best release asset for the current OS+arch automatically.
//!
//! Config (~/.config/zl/config.toml):
//! ```toml
//! [plugins.github]
//! token = "ghp_..."   # optional, avoids rate limiting (60 req/h unauth vs 5000 auth)
//! ```
//!
//! Usage:  zl install BurntSushi/ripgrep --from github
//!         zl search ripgrep --from github
//!         zl install sharkdp/fd --from github

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::config::PluginConfig;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate, SourcePlugin};

const GITHUB_API: &str = "https://api.github.com";

// ── GitHub API response types ─────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct GhRelease {
    tag_name: String,
    name: String,
    assets: Vec<GhAsset>,
}

#[derive(serde::Deserialize, Clone)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(serde::Deserialize)]
struct GhSearchResponse {
    items: Vec<GhRepo>,
}

#[derive(serde::Deserialize)]
struct GhRepo {
    full_name: String,
}

// ── Plugin struct ─────────────────────────────────────────────────────────────

pub struct GithubPlugin {
    token: Option<String>,
    cache_dir: PathBuf,
    client: reqwest::blocking::Client,
}

impl Default for GithubPlugin {
    fn default() -> Self {
        Self {
            token: None,
            cache_dir: PathBuf::new(),
            client: reqwest::blocking::Client::builder()
                .user_agent("zero-layer/0.1 (https://github.com/supercosti21/zero_layer)")
                .build()
                .unwrap_or_default(),
        }
    }
}

impl GithubPlugin {
    pub fn new() -> Self {
        Self::default()
    }

    fn get<T: serde::de::DeserializeOwned>(&self, url: &str) -> ZlResult<T> {
        let mut req = self
            .client
            .get(url)
            .timeout(std::time::Duration::from_secs(30));

        if let Some(ref token) = self.token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        let resp = req.send().map_err(|e| ZlError::Plugin {
            plugin: "github".into(),
            message: format!("GitHub API request failed: {}", e),
        })?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(ZlError::PackageNotFound {
                name: url.to_string(),
            });
        }

        if resp.status() == reqwest::StatusCode::FORBIDDEN
            || resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            return Err(ZlError::Plugin {
                plugin: "github".into(),
                message: "GitHub API rate limit exceeded — add a token in config: [plugins.github] token = \"ghp_...\"".into(),
            });
        }

        if !resp.status().is_success() {
            return Err(ZlError::Plugin {
                plugin: "github".into(),
                message: format!("GitHub API returned {}", resp.status()),
            });
        }

        resp.json::<T>().map_err(|e| ZlError::Plugin {
            plugin: "github".into(),
            message: format!("Failed to parse GitHub response: {}", e),
        })
    }

    fn release_to_candidate(
        &self,
        owner_repo: &str,
        release: &GhRelease,
    ) -> ZlResult<PackageCandidate> {
        let asset = pick_best_asset(&release.assets).ok_or_else(|| ZlError::Plugin {
            plugin: "github".into(),
            message: format!(
                "No compatible asset found for {} {} (arch: {}). Available: {}",
                owner_repo,
                release.tag_name,
                std::env::consts::ARCH,
                release
                    .assets
                    .iter()
                    .map(|a| a.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })?;

        // Strip leading "v" from tag for the version field
        let version = release.tag_name.trim_start_matches('v').to_string();
        let name = owner_repo
            .rsplit('/')
            .next()
            .unwrap_or(owner_repo)
            .to_string();

        Ok(PackageCandidate {
            name,
            version,
            description: release.name.clone(),
            arch: std::env::consts::ARCH.to_string(),
            source: format!("github/{}", owner_repo),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: asset.size,
            download_url: asset.browser_download_url.clone(),
            checksum: None, // GitHub doesn't provide checksums in the API directly
        })
    }
}

// ── SourcePlugin implementation ───────────────────────────────────────────────

impl SourcePlugin for GithubPlugin {
    fn name(&self) -> &str {
        "github"
    }

    fn display_name(&self) -> &str {
        "GitHub Releases"
    }

    fn init(&mut self, config: &PluginConfig) -> ZlResult<()> {
        self.cache_dir = config.cache_dir.clone();
        std::fs::create_dir_all(&self.cache_dir)?;

        if let Some(token) = config.extra.get("token").and_then(|v| v.as_str()) {
            self.token = Some(token.to_string());
        }

        tracing::info!("GitHub plugin initialized (live API queries)");
        Ok(())
    }

    fn search(&self, query: &str) -> ZlResult<Vec<PackageCandidate>> {
        // If query looks like "owner/repo", resolve directly instead of searching
        if query.contains('/') {
            if let Ok(Some(c)) = self.resolve(query, None) {
                return Ok(vec![c]);
            }
            return Ok(vec![]);
        }

        let url = format!(
            "{}/search/repositories?q={}+language:any&sort=stars&per_page=10",
            GITHUB_API, query
        );

        let resp: GhSearchResponse = self.get(&url)?;
        let mut candidates = Vec::new();

        for repo in &resp.items {
            // Try to get the latest release for each result
            let rel_url = format!("{}/repos/{}/releases/latest", GITHUB_API, repo.full_name);
            if let Ok(release) = self.get::<GhRelease>(&rel_url)
                && let Ok(candidate) = self.release_to_candidate(&repo.full_name, &release)
            {
                candidates.push(candidate);
            }
        }

        Ok(candidates)
    }

    fn resolve(&self, name: &str, version: Option<&str>) -> ZlResult<Option<PackageCandidate>> {
        // Normalize: accept "owner/repo" or just "repo" (if found via search)
        // For "repo" without owner, we can't resolve directly — need owner/repo format
        if !name.contains('/') {
            return Err(ZlError::Plugin {
                plugin: "github".into(),
                message: format!(
                    "GitHub packages require owner/repo format (e.g., 'BurntSushi/{}').\n  Use `zl search {} --from github` to find the full name.",
                    name, name
                ),
            });
        }

        let release: GhRelease = if let Some(v) = version {
            // Specific version requested — try tag with and without "v" prefix
            let tag_v = format!("v{}", v);
            let url_v = format!("{}/repos/{}/releases/tags/{}", GITHUB_API, name, tag_v);
            let url_bare = format!("{}/repos/{}/releases/tags/{}", GITHUB_API, name, v);

            self.get::<GhRelease>(&url_v)
                .or_else(|_| self.get::<GhRelease>(&url_bare))?
        } else {
            let url = format!("{}/repos/{}/releases/latest", GITHUB_API, name);
            self.get::<GhRelease>(&url)?
        };

        Ok(Some(self.release_to_candidate(name, &release)?))
    }

    fn download(&self, candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
        let url = &candidate.download_url;
        let filename = url.rsplit('/').next().unwrap_or("asset");
        let dest_path = dest_dir.join(filename);

        if dest_path.exists() {
            tracing::debug!("Using cached {}", dest_path.display());
            return Ok(dest_path);
        }

        tracing::info!("Downloading {}", url);

        let bytes = crate::error::retry_with_backoff(3, 1000, |attempt| {
            if attempt > 1 {
                tracing::info!("Retry {}/3 for {}", attempt, filename);
            }
            let mut req = self
                .client
                .get(url)
                .timeout(std::time::Duration::from_secs(600));

            if let Some(ref token) = self.token {
                req = req.header("Authorization", format!("Bearer {}", token));
            }

            let resp = req.send().map_err(|e| ZlError::DownloadFailed {
                url: url.to_string(),
                attempts: attempt,
                message: e.to_string(),
            })?;

            if !resp.status().is_success() {
                return Err(ZlError::DownloadFailed {
                    url: url.to_string(),
                    attempts: attempt,
                    message: format!("HTTP {}", resp.status()),
                });
            }

            resp.bytes().map_err(|e| ZlError::DownloadFailed {
                url: url.to_string(),
                attempts: attempt,
                message: e.to_string(),
            })
        })?;

        std::fs::write(&dest_path, &bytes)?;
        Ok(dest_path)
    }

    fn extract(&self, archive_path: &Path) -> ZlResult<ExtractedPackage> {
        extract_asset(archive_path)
    }

    fn sync(&self) -> ZlResult<()> {
        tracing::info!("GitHub: nothing to sync (releases are queried live)");
        Ok(())
    }
}

// ── Asset selection ───────────────────────────────────────────────────────────

/// Score and pick the best asset for the current platform.
/// Lower score = better match. Returns None if no suitable asset found.
fn pick_best_asset(assets: &[GhAsset]) -> Option<&GhAsset> {
    let arch = std::env::consts::ARCH; // "x86_64", "aarch64", etc.

    // Synonyms for our arch in release filenames
    let arch_patterns: &[&str] = match arch {
        "x86_64" => &["x86_64", "x86-64", "amd64", "x64"],
        "aarch64" => &["aarch64", "arm64"],
        "arm" => &["armv7", "armhf", "arm"],
        "i686" => &["i686", "i386", "x86"],
        "riscv64" => &["riscv64"],
        _ => &[arch],
    };

    let mut best: Option<(&GhAsset, i32)> = None;

    for asset in assets {
        let name_lower = asset.name.to_lowercase();

        // Skip Windows/macOS assets
        if name_lower.contains("windows")
            || name_lower.contains(".exe")
            || name_lower.contains("darwin")
            || name_lower.contains("macos")
            || name_lower.contains("apple")
        {
            continue;
        }

        // Skip package formats handled by dedicated plugins
        if name_lower.ends_with(".deb")
            || name_lower.ends_with(".rpm")
            || name_lower.ends_with(".apk")
        {
            continue;
        }

        // Must match current architecture (or be "any"/"all")
        let arch_match = arch_patterns.iter().any(|p| name_lower.contains(p))
            || name_lower.contains("linux-unknown")
            || (!name_lower.contains("x86")
                && !name_lower.contains("aarch")
                && !name_lower.contains("arm"));

        if !arch_match {
            continue;
        }

        let mut score = 100i32;

        // Prefer linux explicitly mentioned
        if name_lower.contains("linux") {
            score -= 10;
        }

        // Prefer musl (static, more portable)
        if name_lower.contains("musl") {
            score -= 5;
        }

        // Prefer compressed archives over bare binaries
        if name_lower.ends_with(".tar.gz") || name_lower.ends_with(".tgz") {
            score -= 4;
        } else if name_lower.ends_with(".tar.xz") || name_lower.ends_with(".tar.zst") {
            score -= 3;
        } else if name_lower.ends_with(".zip") {
            score -= 2;
        } else if name_lower.ends_with(".appimage") {
            score -= 1;
        }
        // Bare binary stays at 0 bonus

        if best.is_none_or(|(_, best_score)| score < best_score) {
            best = Some((asset, score));
        }
    }

    best.map(|(a, _)| a)
}

// ── Extraction ────────────────────────────────────────────────────────────────

fn extract_asset(archive_path: &Path) -> ZlResult<ExtractedPackage> {
    let name = archive_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();

    let extract_dir = tempfile::tempdir()?;

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar_gz(archive_path, extract_dir.path())?;
    } else if name.ends_with(".tar.xz") {
        extract_tar_xz(archive_path, extract_dir.path())?;
    } else if name.ends_with(".tar.zst") {
        extract_tar_zst(archive_path, extract_dir.path())?;
    } else if name.ends_with(".zip") {
        extract_zip(archive_path, extract_dir.path())?;
    } else if name.ends_with(".appimage") {
        install_appimage(archive_path, extract_dir.path())?;
    } else {
        // Treat as a bare binary
        install_bare_binary(archive_path, extract_dir.path())?;
    }

    normalize_archive_layout(extract_dir.path())?;

    classify_extracted(extract_dir, archive_path)
}

/// Rearrange an extracted archive into the FHS layout the installer expects.
///
/// Release archives rarely ship an FHS tree: the usual shape is a single
/// versioned wrapper directory with the executable at its root, e.g.
/// `ripgrep-15.2.0-x86_64-unknown-linux-musl/rg`. `create_bin_symlinks` only
/// looks in `core::path::FHS_BIN_DIRS`, so such a binary would be installed and tracked
/// but never linked onto PATH. Normalizing here mirrors what
/// `install_appimage` and `install_bare_binary` already do for the
/// non-archive assets.
fn normalize_archive_layout(root: &Path) -> ZlResult<()> {
    use crate::core::path::has_fhs_bin_dir;

    if has_fhs_bin_dir(root) {
        return Ok(());
    }

    // Unwrap the single top-level directory, if that is all there is
    if let Some(wrapper) = single_subdir(root)? {
        for entry in std::fs::read_dir(&wrapper)?.collect::<Result<Vec<_>, _>>()? {
            std::fs::rename(entry.path(), root.join(entry.file_name()))?;
        }
        std::fs::remove_dir(&wrapper)?;
    }

    // The wrapper may itself have held the FHS tree
    if has_fhs_bin_dir(root) {
        return Ok(());
    }

    promote_programs_to_bin(root)
}

/// The single directory inside `root`, or None if `root` holds anything else.
fn single_subdir(root: &Path) -> ZlResult<Option<PathBuf>> {
    let mut entries = std::fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    match entries.pop() {
        Some(entry) if entries.is_empty() && entry.path().is_dir() => Ok(Some(entry.path())),
        _ => Ok(None),
    }
}

/// Move the ELF programs sitting directly in `root` into `root/usr/bin`.
///
/// Only programs move: shared libraries stay put so RUNPATH `$ORIGIN` keeps
/// resolving, and non-ELF files (completions, man pages, licenses) are left
/// alone even when they carry the executable bit.
fn promote_programs_to_bin(root: &Path) -> ZlResult<()> {
    use crate::core::elf::analysis::{self, ElfType};

    let programs: Vec<PathBuf> = std::fs::read_dir(root)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            matches!(
                analysis::analyze(p).map(|info| info.elf_type),
                Ok(ElfType::Executable | ElfType::StaticBinary)
            )
        })
        .collect();

    if programs.is_empty() {
        return Ok(());
    }

    let bin_dir = root.join("usr").join("bin");
    std::fs::create_dir_all(&bin_dir)?;

    for program in programs {
        let name = program.file_name().unwrap_or_default().to_owned();
        std::fs::rename(&program, bin_dir.join(&name))?;
        tracing::debug!("Promoted {} to usr/bin", name.to_string_lossy());
    }

    Ok(())
}

fn extract_tar_gz(archive: &Path, dest: &Path) -> ZlResult<()> {
    let file = std::fs::File::open(archive)?;
    let gz = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(gz);
    tar.set_preserve_permissions(false);
    tar.unpack(dest)
        .map_err(|e| ZlError::Archive(format!("tar.gz extraction failed: {}", e)))
}

fn extract_tar_xz(archive: &Path, dest: &Path) -> ZlResult<()> {
    let file = std::fs::File::open(archive)?;
    let xz = xz2::read::XzDecoder::new(file);
    let mut tar = tar::Archive::new(xz);
    tar.set_preserve_permissions(false);
    tar.unpack(dest)
        .map_err(|e| ZlError::Archive(format!("tar.xz extraction failed: {}", e)))
}

fn extract_tar_zst(archive: &Path, dest: &Path) -> ZlResult<()> {
    let file = std::fs::File::open(archive)?;
    let zst = zstd::stream::Decoder::new(file)
        .map_err(|e| ZlError::Archive(format!("zstd error: {}", e)))?;
    let mut tar = tar::Archive::new(zst);
    tar.set_preserve_permissions(false);
    tar.unpack(dest)
        .map_err(|e| ZlError::Archive(format!("tar.zst extraction failed: {}", e)))
}

fn extract_zip(archive: &Path, dest: &Path) -> ZlResult<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| ZlError::Archive(format!("zip open failed: {}", e)))?;

    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| ZlError::Archive(format!("zip entry error: {}", e)))?;
        let outpath = dest.join(entry.name());

        if entry.is_dir() {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&outpath)?;
            std::io::copy(&mut entry, &mut out)?;
        }
    }

    Ok(())
}

/// AppImages are self-contained executables — just place them in bin/
fn install_appimage(archive: &Path, dest: &Path) -> ZlResult<()> {
    let fname = archive
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    // Strip version suffix to get clean binary name, e.g. "MyApp-1.2.3.AppImage" → "MyApp"
    let bin_name = fname
        .split('-')
        .next()
        .or_else(|| fname.strip_suffix(".AppImage"))
        .or_else(|| fname.strip_suffix(".appimage"))
        .unwrap_or(&fname)
        .to_lowercase();

    let bin_dir = dest.join("usr").join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let dest_bin = bin_dir.join(&bin_name);
    std::fs::copy(archive, &dest_bin)?;

    // Make executable
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dest_bin, std::fs::Permissions::from_mode(0o755))?;

    Ok(())
}

/// Bare binary — place it in usr/bin/ with the archive filename (stripped)
fn install_bare_binary(archive: &Path, dest: &Path) -> ZlResult<()> {
    let fname = archive
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let bin_dir = dest.join("usr").join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let dest_bin = bin_dir.join(&fname);
    std::fs::copy(archive, &dest_bin)?;

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dest_bin, std::fs::Permissions::from_mode(0o755))?;

    Ok(())
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

    // Build a minimal metadata placeholder from the archive filename
    let fname = archive_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let metadata = PackageCandidate {
        name: fname.clone(),
        version: String::new(),
        description: String::new(),
        arch: std::env::consts::ARCH.to_string(),
        source: "github".into(),
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
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy();
        if matches!(
            ext.as_ref(),
            "sh" | "bash" | "py" | "pl" | "rb" | "lua" | "fish"
        ) {
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_asset(name: &str, size: u64) -> GhAsset {
        GhAsset {
            name: name.to_string(),
            browser_download_url: format!("https://github.com/example/releases/{}", name),
            size,
        }
    }

    #[test]
    fn test_pick_best_asset_prefers_musl_tar_gz() {
        let assets = vec![
            make_asset("rg-14.0-x86_64-unknown-linux-gnu.tar.gz", 3_000_000),
            make_asset("rg-14.0-x86_64-unknown-linux-musl.tar.gz", 3_100_000),
            make_asset("rg-14.0-aarch64-unknown-linux-musl.tar.gz", 3_100_000),
            make_asset("rg-14.0-x86_64-pc-windows-msvc.zip", 2_000_000),
        ];

        // Assuming test runs on x86_64
        if std::env::consts::ARCH == "x86_64" {
            let best = pick_best_asset(&assets).unwrap();
            assert!(best.name.contains("musl"), "Should prefer musl over gnu");
            assert!(!best.name.contains("windows"), "Should not pick windows");
        }
    }

    #[test]
    fn test_pick_best_asset_skips_deb_rpm() {
        let assets = vec![
            make_asset("tool_1.0_amd64.deb", 1_000_000),
            make_asset("tool-1.0-1.x86_64.rpm", 1_000_000),
            make_asset("tool-1.0-linux-x86_64.tar.gz", 1_000_000),
        ];

        if std::env::consts::ARCH == "x86_64" {
            let best = pick_best_asset(&assets).unwrap();
            assert!(best.name.ends_with(".tar.gz"));
        }
    }

    #[test]
    fn test_pick_best_asset_no_match() {
        let assets = vec![
            make_asset("tool-1.0-darwin-amd64.tar.gz", 1_000_000),
            make_asset("tool-1.0-windows-amd64.zip", 1_000_000),
        ];
        assert!(pick_best_asset(&assets).is_none());
    }

    // ── Archive layout normalization ─────────────────────────────────────────

    /// Copy a real ELF program into `dest`. The repo's ELF tests already rely
    /// on /bin/sh existing, and a hand-written fake would not survive
    /// `analysis::analyze`.
    fn put_program(dest: &Path) {
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::copy("/bin/sh", dest).unwrap();
    }

    fn put_text(dest: &Path, contents: &str) {
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, contents).unwrap();
    }

    #[test]
    fn test_normalize_promotes_binary_out_of_wrapper_dir() {
        // The common release-tarball shape: one versioned wrapper directory
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let wrapper = root.join("ripgrep-15.2.0-x86_64-unknown-linux-musl");
        put_program(&wrapper.join("rg"));
        put_text(&wrapper.join("README.md"), "# ripgrep");
        put_text(&wrapper.join("complete/rg.bash"), "# completions");

        normalize_archive_layout(root).unwrap();

        assert!(root.join("usr/bin/rg").is_file(), "rg should be in usr/bin");
        // The wrapper is unwrapped, and non-programs keep their relative layout
        assert!(!wrapper.exists(), "wrapper dir should be gone");
        assert!(root.join("README.md").is_file());
        assert!(root.join("complete/rg.bash").is_file());
    }

    #[test]
    fn test_normalize_promotes_binary_at_archive_root() {
        // Some archives have no wrapper directory at all
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put_program(&root.join("tool"));
        put_text(&root.join("LICENSE"), "MIT");

        normalize_archive_layout(root).unwrap();

        assert!(root.join("usr/bin/tool").is_file());
        assert!(root.join("LICENSE").is_file());
    }

    #[test]
    fn test_normalize_leaves_fhs_archive_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        put_program(&root.join("usr/bin/tool"));
        put_text(&root.join("usr/share/man/tool.1"), ".TH TOOL 1");

        normalize_archive_layout(root).unwrap();

        assert!(root.join("usr/bin/tool").is_file());
        assert!(root.join("usr/share/man/tool.1").is_file());
    }

    #[test]
    fn test_normalize_lifts_fhs_tree_out_of_wrapper_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let wrapper = root.join("tool-1.0");
        put_program(&wrapper.join("bin/tool"));
        put_text(&wrapper.join("share/doc/README"), "docs");

        normalize_archive_layout(root).unwrap();

        // bin/ is an FHS dir, so the tree is lifted rather than rewritten
        assert!(root.join("bin/tool").is_file());
        assert!(root.join("share/doc/README").is_file());
        assert!(!root.join("usr/bin/tool").exists());
    }

    #[test]
    fn test_normalize_ignores_non_elf_executables() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let wrapper = root.join("tool-1.0");
        put_program(&wrapper.join("tool"));
        // A completion script carrying the executable bit must not be linked
        let script = wrapper.join("tool.bash");
        put_text(&script, "# completions");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        normalize_archive_layout(root).unwrap();

        assert!(root.join("usr/bin/tool").is_file());
        assert!(
            !root.join("usr/bin/tool.bash").exists(),
            "non-ELF files must not be promoted"
        );
        assert!(root.join("tool.bash").is_file());
    }

    #[test]
    fn test_normalize_keeps_shared_libraries_in_place() {
        let Some(libc) = find_shared_library() else {
            panic!("no shared library found in the system lib dirs");
        };

        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let wrapper = root.join("tool-1.0");
        put_program(&wrapper.join("tool"));
        std::fs::copy(&libc, wrapper.join("libfoo.so.1")).unwrap();

        normalize_archive_layout(root).unwrap();

        assert!(root.join("usr/bin/tool").is_file());
        // Libraries stay next to nothing in particular, but never in bin/:
        // moving them would break RUNPATH $ORIGIN resolution.
        assert!(!root.join("usr/bin/libfoo.so.1").exists());
        assert!(root.join("libfoo.so.1").is_file());
    }

    /// First real shared library found in the system's lib directories.
    fn find_shared_library() -> Option<PathBuf> {
        use crate::core::elf::analysis::{self, ElfType};

        ["/usr/lib", "/usr/lib/x86_64-linux-gnu", "/lib"]
            .iter()
            .filter_map(|dir| std::fs::read_dir(dir).ok())
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .find(|p| {
                matches!(
                    analysis::analyze(p).map(|i| i.elf_type),
                    Ok(ElfType::SharedLibrary)
                )
            })
    }
}
