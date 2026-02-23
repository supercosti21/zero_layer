use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ZlError {
    // ── ELF ──
    #[error("ELF analysis failed for {path}: {source}")]
    ElfAnalysis {
        path: PathBuf,
        source: Box<goblin::error::Error>,
    },

    #[error("ELF patching failed for {path}: {message}")]
    ElfPatch { path: PathBuf, message: String },

    // ── Package resolution ──
    #[error("Package not found: {name}\n  hint: try `zl search {name}` to find available packages")]
    PackageNotFound { name: String },

    #[error("Conflict: {installed} conflicts with {requested}")]
    PackageConflict {
        installed: String,
        requested: String,
    },

    // ── Network ──
    #[error("Download failed for {url} after {attempts} attempts: {message}")]
    DownloadFailed {
        url: String,
        attempts: u32,
        message: String,
    },

    #[error("Request timed out after {timeout_secs}s: {url}")]
    Timeout { url: String, timeout_secs: u64 },

    #[error("Checksum mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    // ── Archive ──
    #[error("Archive extraction error: {0}")]
    Archive(String),

    // ── Plugin ──
    #[error("Plugin error ({plugin}): {message}")]
    Plugin { plugin: String, message: String },

    // ── Build/Source ──
    #[error("Build failed for {package}: {message}")]
    BuildFailed { package: String, message: String },

    #[error("Build tool not found: {tool}\n  hint: install it with your system package manager")]
    BuildToolMissing { tool: String },

    // ── IO ──
    #[error("IO error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    // ── Serialization ──
    #[error("Serialization error: {source}")]
    Serialization {
        #[from]
        source: serde_json::Error,
    },

    // ── Config ──
    #[error("Config error: {0}")]
    Config(String),

    // ── GPG/Signature ──
    #[error("GPG signature verification failed for {path}: {message}")]
    GpgVerification { path: PathBuf, message: String },

    // ── Self-update ──
    #[error("Self-update failed: {0}")]
    SelfUpdate(String),

    // ── Verification ──
    #[allow(dead_code)]
    #[error("Verification failed:\n{0}")]
    Verification(String),

    // ── Architecture ──
    #[allow(dead_code)]
    #[error(
        "Architecture mismatch: package is built for {pkg_arch} but your system is {host_arch}"
    )]
    ArchMismatch { pkg_arch: String, host_arch: String },

    // ── Environments ──
    #[error("Environment error: {0}")]
    Environment(String),
}

impl ZlError {
    /// User-friendly suggestion for how to fix or work around this error.
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            ZlError::PackageNotFound { .. } => {
                Some("Check the package name or try a different source with --from")
            }
            ZlError::DownloadFailed { url, .. } if url.contains("archlinux.org") => Some(
                "Mirror sync failed. Check /etc/pacman.d/mirrorlist or your internet connection",
            ),
            ZlError::DownloadFailed { url, .. }
                if url.contains("debian.org") || url.contains("ubuntu.com") =>
            {
                Some("Failed to fetch from APT repo. Check the repository URL in your config")
            }
            ZlError::DownloadFailed { url, .. }
                if url.contains("github.com") || url.contains("api.github.com") =>
            {
                Some(
                    "GitHub download failed. You may be rate-limited — set GITHUB_TOKEN env var or wait",
                )
            }
            ZlError::DownloadFailed { .. } => {
                Some("Check your internet connection or try again later")
            }
            ZlError::Timeout { .. } => {
                Some("The server may be slow — try again or use a different mirror")
            }
            ZlError::ChecksumMismatch { .. } => {
                Some("The downloaded file is corrupted — run `zl cache clean` and try again")
            }
            ZlError::BuildToolMissing { tool } if tool == "git" => {
                Some("Install git: sudo pacman -S git (Arch) or sudo apt install git (Debian)")
            }
            ZlError::BuildToolMissing { tool } if tool == "makepkg" => {
                Some("makepkg is part of pacman. On non-Arch systems, AUR builds are not supported")
            }
            ZlError::BuildToolMissing { .. } => {
                Some("Install the required build tool with your system package manager")
            }
            ZlError::BuildFailed { message, .. }
                if message.contains("base-devel") || message.contains("fakeroot") =>
            {
                Some("Install base-devel: sudo pacman -S --needed base-devel")
            }
            ZlError::BuildFailed { message, .. }
                if message.contains("PGP") || message.contains("signature") =>
            {
                Some("A PGP key is missing. Import it or rebuild with --skippgpcheck")
            }
            ZlError::BuildFailed { .. } => {
                Some("Check the PKGBUILD for errors or missing build dependencies")
            }
            ZlError::PackageConflict { .. } => {
                Some("Remove the conflicting package first with `zl remove`")
            }
            ZlError::Plugin { plugin, message } if plugin == "aur" && message.contains("HTTP") => {
                Some(
                    "AUR API returned an error. The package name may be incorrect or AUR may be down",
                )
            }
            ZlError::Plugin { plugin, message }
                if plugin == "github" && message.contains("rate") =>
            {
                Some(
                    "GitHub API rate limit reached. Set GITHUB_TOKEN env var to increase the limit",
                )
            }
            ZlError::Plugin { plugin, .. } if plugin == "github" => Some(
                "GitHub packages require owner/repo format (e.g., `zl install BurntSushi/ripgrep --from github`)",
            ),
            ZlError::Plugin { .. } => None,
            ZlError::GpgVerification { .. } => Some(
                "The package signature is invalid — this may indicate tampering. Use --skip-verify to bypass (not recommended)",
            ),
            ZlError::ArchMismatch { .. } => Some(
                "This package was built for a different CPU architecture and cannot run on your system",
            ),
            ZlError::SelfUpdate(msg)
                if msg.contains("Permission denied") || msg.contains("not writable") =>
            {
                Some("Run with elevated permissions: sudo zl self-update")
            }
            ZlError::SelfUpdate(msg) if msg.contains("No binary found") => Some(
                "No prebuilt binary for your architecture. Build from source: cargo install --git https://github.com/supercosti21/zero_layer",
            ),
            ZlError::SelfUpdate(_) => Some("Check your internet connection and try again"),
            ZlError::Archive(_) => {
                Some("The archive may be corrupted. Run `zl cache clean` and try again")
            }
            _ => None,
        }
    }
}

pub type ZlResult<T> = Result<T, ZlError>;

/// Retry a fallible operation with exponential backoff.
/// Calls `op` up to `max_attempts` times, sleeping between retries.
/// Returns the first success or the last error.
pub fn retry_with_backoff<T, F>(max_attempts: u32, base_delay_ms: u64, mut op: F) -> ZlResult<T>
where
    F: FnMut(u32) -> ZlResult<T>,
{
    let mut last_err = None;
    for attempt in 1..=max_attempts {
        match op(attempt) {
            Ok(val) => return Ok(val),
            Err(e) => {
                tracing::warn!("Attempt {}/{} failed: {}", attempt, max_attempts, e);
                last_err = Some(e);
                if attempt < max_attempts {
                    let delay = base_delay_ms * 2u64.pow(attempt - 1);
                    std::thread::sleep(std::time::Duration::from_millis(delay));
                }
            }
        }
    }
    Err(last_err.unwrap())
}
