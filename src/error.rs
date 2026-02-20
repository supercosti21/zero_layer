use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ZlError {
    // ── ELF ──
    #[error("ELF analysis failed for {path}: {source}")]
    ElfAnalysis {
        path: PathBuf,
        source: goblin::error::Error,
    },

    #[error("ELF patching failed for {path}: {message}")]
    ElfPatch { path: PathBuf, message: String },

    // ── Path remapping ──
    #[error("Path remapping failed: {0}")]
    PathRemap(String),

    // ── Package resolution ──
    #[error("Package not found: {name}\n  hint: try `zl search {name}` to find available packages")]
    PackageNotFound { name: String },

    #[error("Package already installed: {name}-{version}")]
    AlreadyInstalled { name: String, version: String },

    // ── Dependencies ──
    #[error("Dependency resolution failed for {package}: {message}")]
    DependencyResolution { package: String, message: String },

    #[error("Unresolvable dependencies for {package}:\n{}", format_missing_deps(.missing))]
    UnresolvableDeps {
        package: String,
        missing: Vec<String>,
    },

    #[error("Dependency cycle detected: {}", .chain.join(" → "))]
    DependencyCycle { chain: Vec<String> },

    #[error("Conflict: {installed} conflicts with {requested}")]
    PackageConflict {
        installed: String,
        requested: String,
    },

    // ── Database ──
    #[error("Database error: {source}")]
    Database {
        #[from]
        source: redb::Error,
    },

    // ── Network ──
    #[error("Network error: {source}")]
    Network {
        #[from]
        source: reqwest::Error,
    },

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

    // ── Verification ──
    #[error("Verification failed:\n{0}")]
    Verification(String),

    // ── Serialization ──
    #[error("Serialization error: {source}")]
    Serialization {
        #[from]
        source: serde_json::Error,
    },

    // ── Config ──
    #[error("Config error: {0}")]
    Config(String),
}

fn format_missing_deps(deps: &[String]) -> String {
    deps.iter()
        .map(|d| format!("  - {}", d))
        .collect::<Vec<_>>()
        .join("\n")
}

impl ZlError {
    /// User-friendly suggestion for how to fix or work around this error.
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            ZlError::PackageNotFound { .. } => {
                Some("Check the package name or try a different source with --from")
            }
            ZlError::UnresolvableDeps { .. } => {
                Some("Try installing the missing dependencies manually first")
            }
            ZlError::DownloadFailed { .. } => {
                Some("Check your internet connection or try again later")
            }
            ZlError::Timeout { .. } => {
                Some("The server may be slow — try again or use a different mirror")
            }
            ZlError::ChecksumMismatch { .. } => {
                Some("The downloaded file is corrupted — delete the cache and try again")
            }
            ZlError::BuildToolMissing { .. } => {
                Some("Install the required build tool with your system package manager")
            }
            ZlError::DependencyCycle { .. } => Some("This is a packaging bug — report it upstream"),
            ZlError::PackageConflict { .. } => {
                Some("Remove the conflicting package first with `zl remove`")
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
