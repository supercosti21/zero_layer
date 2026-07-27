use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{ZlError, ZlResult};

/// Result of package verification
#[derive(Debug)]
pub struct VerifyResult {
    /// SHA256 checksum matched
    #[allow(dead_code)]
    pub checksum_ok: bool,
    /// GPG signature verification result (None if no signature available)
    #[allow(dead_code)]
    pub gpg_ok: Option<bool>,
    /// Human-readable verification message
    pub message: String,
}

impl VerifyResult {
    #[allow(dead_code)]
    pub fn passed(&self) -> bool {
        self.checksum_ok && self.gpg_ok.unwrap_or(true)
    }
}

/// SHA256 of a byte slice as a lowercase hex string.
///
/// sha2 0.11 returns a `hybrid_array::Array`, which no longer implements
/// `LowerHex`, so the hex encoding is done here once for the whole crate.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    Sha256::digest(bytes).iter().fold(
        String::with_capacity(Sha256::output_size() * 2),
        |mut hex, byte| {
            let _ = write!(hex, "{:02x}", byte);
            hex
        },
    )
}

/// Verify SHA256 checksum of a file
pub fn verify_sha256(path: &Path, expected: &str) -> ZlResult<bool> {
    let bytes = std::fs::read(path)?;
    Ok(sha256_hex(&bytes) == expected)
}

/// Compute SHA256 of a file
pub fn compute_sha256(path: &Path) -> ZlResult<String> {
    let bytes = std::fs::read(path)?;
    Ok(sha256_hex(&bytes))
}

/// Check if the system gpg binary is available
fn gpg_available() -> bool {
    std::process::Command::new("gpg")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Outcome of checking a detached GPG signature.
#[derive(Debug, PartialEq)]
pub enum GpgOutcome {
    /// The signature matches the file.
    Valid,
    /// The signature does not match — the file may have been tampered with.
    Invalid,
    /// The signature could not be checked at all.
    Unverified(String),
}

/// Verify a GPG detached signature against a file.
///
/// A non-zero exit from `gpg --verify` does **not** mean the signature is bad:
/// it also covers "the signing key is not in the keyring", which is the normal
/// case for distro packages, whose keys live in the package manager's own
/// keyring (e.g. /etc/pacman.d/gnupg) rather than the user's. The two are told
/// apart through the machine-readable status output, because treating a missing
/// key as tampering blocks every install it touches.
pub fn verify_gpg_signature(file_path: &Path, sig_path: &Path) -> ZlResult<GpgOutcome> {
    if !gpg_available() {
        return Ok(GpgOutcome::Unverified(
            "gpg is not installed on this system".into(),
        ));
    }

    let output = std::process::Command::new("gpg")
        .arg("--status-fd")
        .arg("1")
        .arg("--verify")
        .arg(sig_path)
        .arg(file_path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()?;

    Ok(classify_gpg_status(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

/// Map gpg's `--status-fd` output onto a [`GpgOutcome`].
fn classify_gpg_status(status: &str) -> GpgOutcome {
    let has = |token: &str| {
        status
            .lines()
            .any(|line| line.starts_with(&format!("[GNUPG:] {}", token)))
    };

    // A bad signature is conclusive and takes precedence over everything else
    if has("BADSIG") {
        return GpgOutcome::Invalid;
    }
    if has("GOODSIG") || has("VALIDSIG") {
        return GpgOutcome::Valid;
    }
    // Signed by a key we do have, but one that is no longer current. The
    // signature itself is intact, so this is not tampering.
    if has("EXPKEYSIG") {
        return GpgOutcome::Unverified("the signing key has expired".into());
    }
    if has("REVKEYSIG") {
        return GpgOutcome::Unverified("the signing key was revoked".into());
    }
    if has("NO_PUBKEY") || has("ERRSIG") {
        return GpgOutcome::Unverified("the signing key is not in the local keyring".into());
    }

    GpgOutcome::Unverified("gpg returned no usable verification status".into())
}

/// Try to download a .sig file for a given package URL.
/// Returns None if no signature is available (404, timeout, etc.)
pub fn download_signature(download_url: &str) -> ZlResult<Option<Vec<u8>>> {
    let sig_url = format!("{}.sig", download_url);

    let response = reqwest::blocking::Client::new()
        .get(&sig_url)
        .timeout(std::time::Duration::from_secs(30))
        .send();

    match response {
        Ok(resp) if resp.status().is_success() => {
            let bytes = resp.bytes().map_err(|e| ZlError::DownloadFailed {
                url: sig_url,
                attempts: 1,
                message: e.to_string(),
            })?;
            Ok(Some(bytes.to_vec()))
        }
        _ => Ok(None),
    }
}

/// Full verification pipeline: checksum + optional GPG signature.
///
/// If `skip_verify` is true, all checks are bypassed.
/// If checksum is available, it MUST match (unless skipped).
/// GPG signatures are best-effort: downloaded if available, verified if gpg is installed.
pub fn verify_package(
    file_path: &Path,
    checksum: Option<&str>,
    download_url: &str,
    skip_verify: bool,
) -> ZlResult<VerifyResult> {
    if skip_verify {
        return Ok(VerifyResult {
            checksum_ok: true,
            gpg_ok: None,
            message: "Verification skipped (--skip-verify)".into(),
        });
    }

    // 1. SHA256 checksum
    let checksum_ok = match checksum {
        Some(expected) => {
            let ok = verify_sha256(file_path, expected)?;
            if !ok {
                let actual = compute_sha256(file_path)?;
                return Err(ZlError::ChecksumMismatch {
                    path: file_path.to_path_buf(),
                    expected: expected.to_string(),
                    actual,
                });
            }
            true
        }
        None => {
            tracing::warn!(
                "No checksum available for {}, integrity cannot be verified",
                file_path.display()
            );
            true
        }
    };

    // 2. GPG signature (best-effort)
    let (gpg_ok, gpg_msg) = match download_signature(download_url) {
        Ok(Some(sig_bytes)) => {
            let sig_path = file_path.with_extension(
                file_path
                    .extension()
                    .map(|e| format!("{}.sig", e.to_string_lossy()))
                    .unwrap_or_else(|| "sig".into()),
            );
            std::fs::write(&sig_path, &sig_bytes)?;
            let outcome = verify_gpg_signature(file_path, &sig_path)?;
            let _ = std::fs::remove_file(&sig_path);
            match outcome {
                GpgOutcome::Valid => (Some(true), "GPG signature verified"),
                GpgOutcome::Invalid => {
                    return Err(ZlError::GpgVerification {
                        path: file_path.to_path_buf(),
                        message: "Detached signature does not match the downloaded file".into(),
                    });
                }
                GpgOutcome::Unverified(reason) => {
                    tracing::warn!(
                        "Could not verify the GPG signature of {}: {}",
                        file_path.display(),
                        reason
                    );
                    (None, "GPG signature present but not verified")
                }
            }
        }
        Ok(None) => (None, "No GPG signature available"),
        Err(e) => {
            tracing::debug!("Could not download signature: {}", e);
            (None, "GPG signature not checked (download failed)")
        }
    };

    let message = if checksum.is_some() {
        format!("SHA256 OK. {}", gpg_msg)
    } else {
        format!("No checksum. {}", gpg_msg)
    };

    Ok(VerifyResult {
        checksum_ok,
        gpg_ok,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_sha256_hex_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // Contains a 0x01 byte, so this also covers zero-padded nibbles
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_sha256_hex_is_lowercase_and_64_chars() {
        let hex = sha256_hex(b"zero layer");
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
    }

    #[test]
    fn test_gpg_status_good_signature_is_valid() {
        let status = "[GNUPG:] NEWSIG\n\
             [GNUPG:] GOODSIG 1234ABCD Some Packager <p@example.com>\n\
             [GNUPG:] VALIDSIG 05C7775A 2026-01-01\n";
        assert_eq!(classify_gpg_status(status), GpgOutcome::Valid);
    }

    #[test]
    fn test_gpg_status_bad_signature_is_invalid() {
        // The one case that must stay a hard error
        let status = "[GNUPG:] NEWSIG\n[GNUPG:] BADSIG 1234ABCD Some Packager\n";
        assert_eq!(classify_gpg_status(status), GpgOutcome::Invalid);
    }

    #[test]
    fn test_gpg_status_missing_key_is_unverified_not_invalid() {
        // Regression: this is what every Arch package produces, because the
        // developer keys live in pacman's keyring rather than the user's.
        // Treating it as tampering made `zl install --from pacman` impossible.
        let status = "[GNUPG:] NEWSIG\n\
             [GNUPG:] ERRSIG 9D4C5AA15426DA0A 22 10 00 1781511300 9\n\
             [GNUPG:] NO_PUBKEY 9D4C5AA15426DA0A\n";
        assert!(matches!(
            classify_gpg_status(status),
            GpgOutcome::Unverified(_)
        ));
    }

    #[test]
    fn test_gpg_status_expired_key_is_unverified() {
        let status = "[GNUPG:] EXPKEYSIG 1234ABCD Some Packager\n";
        assert!(matches!(
            classify_gpg_status(status),
            GpgOutcome::Unverified(_)
        ));
    }

    #[test]
    fn test_gpg_status_bad_signature_wins_over_other_tokens() {
        // A forged file signed with an also-unknown key must not be downgraded
        // to "unverified" just because NO_PUBKEY is present too
        let status = "[GNUPG:] NO_PUBKEY 9D4C5AA1\n[GNUPG:] BADSIG 1234ABCD Someone\n";
        assert_eq!(classify_gpg_status(status), GpgOutcome::Invalid);
    }

    #[test]
    fn test_gpg_status_empty_output_is_unverified() {
        assert!(matches!(classify_gpg_status(""), GpgOutcome::Unverified(_)));
    }

    #[test]
    fn test_verify_sha256_correct() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world").unwrap();
        tmp.flush().unwrap();

        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(verify_sha256(tmp.path(), expected).unwrap());
    }

    #[test]
    fn test_verify_sha256_wrong() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world").unwrap();
        tmp.flush().unwrap();

        assert!(!verify_sha256(tmp.path(), "0000000000").unwrap());
    }

    #[test]
    fn test_compute_sha256() {
        let mut tmp = tempfile::NamedTempFile::new().unwrap();
        tmp.write_all(b"hello world").unwrap();
        tmp.flush().unwrap();

        let hash = compute_sha256(tmp.path()).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_verify_result_passed() {
        let r = VerifyResult {
            checksum_ok: true,
            gpg_ok: Some(true),
            message: "all good".into(),
        };
        assert!(r.passed());

        let r2 = VerifyResult {
            checksum_ok: true,
            gpg_ok: None,
            message: "no sig".into(),
        };
        assert!(r2.passed());

        let r3 = VerifyResult {
            checksum_ok: false,
            gpg_ok: Some(true),
            message: "bad checksum".into(),
        };
        assert!(!r3.passed());
    }

    #[test]
    fn test_skip_verify() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let result =
            verify_package(tmp.path(), Some("wrong"), "http://example.com/pkg", true).unwrap();
        assert!(result.passed());
        assert!(result.message.contains("skipped"));
    }
}
