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
            ZlError::PackageConflict { .. } => {
                Some("Remove the conflicting package first with `zl remove`")
            }
            ZlError::GpgVerification { .. } => Some(
                "The package signature is invalid — this may indicate tampering. Use --skip-verify to bypass (not recommended)",
            ),
            ZlError::SelfUpdate(msg) if msg.contains("Permission denied") => {
                Some("Run with elevated permissions: sudo zl self-update")
            }
            ZlError::SelfUpdate(_) => Some("Check your internet connection and try again"),
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
