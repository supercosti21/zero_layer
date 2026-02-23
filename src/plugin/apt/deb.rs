//! .deb package extraction.
//!
//! A .deb file is an `ar` archive containing three members:
//!   - `debian-binary`    — version string ("2.0\n")
//!   - `control.tar.*`   — package metadata
//!   - `data.tar.*`      — actual file contents (gz, xz, zst)
//!
//! We only extract `data.tar.*` into the staging directory.

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::core::elf::analysis;
use crate::error::{ZlError, ZlResult};
use crate::plugin::{ExtractedPackage, PackageCandidate};

/// Extract a .deb file and classify its contents
pub fn extract(deb_path: &Path, metadata: PackageCandidate) -> ZlResult<ExtractedPackage> {
    let extract_dir = tempfile::tempdir()?;

    let deb_file = std::fs::File::open(deb_path)?;
    let mut ar_archive = ar::Archive::new(deb_file);

    let mut found_data = false;

    while let Some(entry) = ar_archive.next_entry() {
        let entry = entry.map_err(|e| ZlError::Archive(format!("ar error: {}", e)))?;
        let name = String::from_utf8_lossy(entry.header().identifier())
            .trim()
            .to_string();

        if !name.starts_with("data.tar") {
            continue;
        }

        found_data = true;
        tracing::debug!("Extracting {} from {}", name, deb_path.display());

        if name.ends_with(".gz") || name == "data.tar" {
            let decoder = flate2::read::GzDecoder::new(entry);
            unpack_tar(decoder, extract_dir.path())?;
        } else if name.ends_with(".zst") {
            let decoder = zstd::stream::Decoder::new(entry)
                .map_err(|e| ZlError::Archive(format!("zstd error: {}", e)))?;
            unpack_tar(decoder, extract_dir.path())?;
        } else if name.ends_with(".xz") {
            let decoder = xz2::read::XzDecoder::new(entry);
            unpack_tar(decoder, extract_dir.path())?;
        } else if name.ends_with(".bz2") {
            let decoder = bzip2::read::BzDecoder::new(entry);
            unpack_tar(decoder, extract_dir.path())?;
        } else {
            return Err(ZlError::Archive(format!(
                "Unsupported data.tar compression in {}: {}",
                deb_path.display(),
                name
            )));
        }

        break;
    }

    if !found_data {
        return Err(ZlError::Archive(format!(
            "No data.tar.* found in {}",
            deb_path.display()
        )));
    }

    // Classify extracted files
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

    Ok(ExtractedPackage {
        extract_dir,
        metadata,
        files,
        elf_files,
        script_files,
    })
}

fn unpack_tar<R: Read>(reader: R, dest: &Path) -> ZlResult<()> {
    let mut archive = tar::Archive::new(reader);
    archive.set_preserve_permissions(false);
    archive
        .unpack(dest)
        .map_err(|e| ZlError::Archive(format!("tar extraction failed: {}", e)))
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
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut buf = [0u8; 2];
        if file.read_exact(&mut buf).is_ok() && buf == *b"#!" {
            return true;
        }
    }
    false
}

/// Download a .deb file from a URL into dest_dir, verify checksum
pub fn download_deb(
    url: &str,
    expected_sha256: Option<&str>,
    dest_dir: &Path,
) -> ZlResult<PathBuf> {
    let filename = url.rsplit('/').next().unwrap_or("package.deb");
    let dest_path = dest_dir.join(filename);

    // Use cached file if checksum matches
    if dest_path.exists() {
        if let Some(expected) = expected_sha256 {
            if verify_sha256(&dest_path, expected) {
                tracing::debug!("Using cached {} (checksum OK)", dest_path.display());
                return Ok(dest_path);
            }
            std::fs::remove_file(&dest_path)?;
        } else {
            return Ok(dest_path);
        }
    }

    tracing::info!("Downloading {}", url);
    let bytes = crate::error::retry_with_backoff(3, 1000, |attempt| {
        if attempt > 1 {
            tracing::info!("Retry {}/3 for {}", attempt, filename);
        }
        let resp = reqwest::blocking::Client::new()
            .get(url)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .map_err(|e| ZlError::DownloadFailed {
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

        let bytes = resp.bytes().map_err(|e| ZlError::DownloadFailed {
            url: url.to_string(),
            attempts: attempt,
            message: e.to_string(),
        })?;

        if let Some(expected) = expected_sha256 {
            use sha2::{Digest, Sha256};
            let actual = format!("{:x}", Sha256::digest(&bytes));
            if actual != expected {
                return Err(ZlError::ChecksumMismatch {
                    path: dest_path.clone(),
                    expected: expected.to_string(),
                    actual,
                });
            }
        }

        Ok(bytes)
    })?;

    std::fs::write(&dest_path, &bytes)?;
    Ok(dest_path)
}

fn verify_sha256(path: &Path, expected: &str) -> bool {
    use sha2::{Digest, Sha256};
    std::fs::read(path)
        .map(|b| format!("{:x}", Sha256::digest(&b)) == expected)
        .unwrap_or(false)
}
