use std::io::Read;
use std::path::{Path, PathBuf};

use crate::core::elf::analysis;
use crate::error::{ZlError, ZlResult, retry_with_backoff};
use crate::plugin::{ExtractedPackage, PackageCandidate};

/// Download a pacman package file to the cache directory with retry
pub fn download(candidate: &PackageCandidate, dest_dir: &Path) -> ZlResult<PathBuf> {
    let filename = candidate
        .download_url
        .rsplit('/')
        .next()
        .unwrap_or("package.pkg.tar.zst");
    let dest_path = dest_dir.join(filename);

    // Skip download if already cached and checksum matches
    if dest_path.exists() {
        if let Some(ref expected_sha256) = candidate.checksum {
            if verify_checksum(&dest_path, expected_sha256) {
                tracing::debug!("Using cached {} (checksum OK)", dest_path.display());
                return Ok(dest_path);
            }
            tracing::debug!("Cached file has wrong checksum, re-downloading");
            std::fs::remove_file(&dest_path)?;
        } else {
            tracing::debug!("Using cached {}", dest_path.display());
            return Ok(dest_path);
        }
    }

    tracing::info!("Downloading {}", candidate.download_url);

    let url = candidate.download_url.clone();
    let expected_checksum = candidate.checksum.clone();

    let bytes = retry_with_backoff(3, 1000, |attempt| {
        if attempt > 1 {
            tracing::info!("Download attempt {}/3 for {}", attempt, filename);
        }

        let response = reqwest::blocking::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(300))
            .send()
            .map_err(|e| {
                if e.is_timeout() {
                    ZlError::Timeout {
                        url: url.clone(),
                        timeout_secs: 300,
                    }
                } else {
                    ZlError::DownloadFailed {
                        url: url.clone(),
                        attempts: attempt,
                        message: e.to_string(),
                    }
                }
            })?;

        if !response.status().is_success() {
            return Err(ZlError::DownloadFailed {
                url: url.clone(),
                attempts: attempt,
                message: format!("HTTP {}", response.status()),
            });
        }

        let bytes = response.bytes().map_err(|e| ZlError::DownloadFailed {
            url: url.clone(),
            attempts: attempt,
            message: format!("Failed to read response body: {}", e),
        })?;

        // Verify checksum if available
        if let Some(ref expected_sha256) = expected_checksum {
            let actual = crate::core::verify::sha256_hex(&bytes);
            if actual != *expected_sha256 {
                return Err(ZlError::ChecksumMismatch {
                    path: dest_path.clone(),
                    expected: expected_sha256.clone(),
                    actual,
                });
            }
        }

        Ok(bytes)
    })?;

    std::fs::write(&dest_path, &bytes)?;
    Ok(dest_path)
}

/// Verify SHA256 checksum of an existing file
fn verify_checksum(path: &Path, expected: &str) -> bool {
    match std::fs::read(path) {
        Ok(bytes) => crate::core::verify::sha256_hex(&bytes) == expected,
        Err(_) => false,
    }
}

/// Extract a .pkg.tar.zst archive and classify its contents
pub fn extract(archive_path: &Path, metadata: PackageCandidate) -> ZlResult<ExtractedPackage> {
    let extract_dir = tempfile::tempdir()?;

    let file = std::fs::File::open(archive_path)?;
    let decompressor = zstd::stream::Decoder::new(file)?;
    let mut archive = tar::Archive::new(decompressor);

    archive.unpack(extract_dir.path()).map_err(|e| {
        ZlError::Archive(format!(
            "Failed to extract {}: {}",
            archive_path.display(),
            e
        ))
    })?;

    // Parse .PKGINFO if present
    let pkginfo_path = extract_dir.path().join(".PKGINFO");
    let metadata = if pkginfo_path.exists() {
        let content = std::fs::read_to_string(&pkginfo_path)?;
        merge_pkginfo(metadata, &content)
    } else {
        metadata
    };

    // Classify files
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

        // Skip pacman metadata files
        let name = entry.file_name().to_string_lossy();
        if name.starts_with('.')
            && (name == ".PKGINFO"
                || name == ".MTREE"
                || name == ".BUILDINFO"
                || name == ".INSTALL"
                || name == ".CHANGELOG")
        {
            continue;
        }

        if analysis::is_elf_file(&path) {
            elf_files.push(path.clone());
        } else if is_script_file(&path) {
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

/// Parse .PKGINFO and merge any extra info into the candidate
fn merge_pkginfo(mut candidate: PackageCandidate, content: &str) -> PackageCandidate {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        if let Some((key, value)) = trimmed.split_once(" = ") {
            match key.trim() {
                "pkgname" => candidate.name = value.trim().to_string(),
                "pkgver" => candidate.version = value.trim().to_string(),
                "pkgdesc" => candidate.description = value.trim().to_string(),
                "arch" => candidate.arch = value.trim().to_string(),
                "size" => {
                    candidate.installed_size =
                        value.trim().parse().unwrap_or(candidate.installed_size);
                }
                "depend" => {
                    let dep = value.trim().to_string();
                    if !candidate.dependencies.contains(&dep) {
                        candidate.dependencies.push(dep);
                    }
                }
                "provides" => {
                    let prov = value.trim().to_string();
                    if !candidate.provides.contains(&prov) {
                        candidate.provides.push(prov);
                    }
                }
                "conflict" => {
                    let conflict = value.trim().to_string();
                    if !candidate.conflicts.contains(&conflict) {
                        candidate.conflicts.push(conflict);
                    }
                }
                _ => {}
            }
        }
    }
    candidate
}

/// Check if a file looks like a script (shebang or known extension)
fn is_script_file(path: &Path) -> bool {
    // Check extension
    if let Some(ext) = path.extension() {
        let ext = ext.to_string_lossy();
        if matches!(
            ext.as_ref(),
            "sh" | "bash" | "py" | "pl" | "rb" | "lua" | "fish"
        ) {
            return true;
        }
    }

    // Check for shebang
    if let Ok(mut file) = std::fs::File::open(path) {
        let mut buf = [0u8; 2];
        if file.read_exact(&mut buf).is_ok() && buf == *b"#!" {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_pkginfo() {
        let candidate = PackageCandidate {
            name: String::new(),
            version: String::new(),
            description: String::new(),
            arch: String::new(),
            source: "pacman/extra".into(),
            dependencies: vec![],
            provides: vec![],
            conflicts: vec![],
            installed_size: 0,
            download_url: String::new(),
            checksum: None,
        };

        let pkginfo = r#"
# Generated by makepkg
pkgname = firefox
pkgver = 120.0-1
pkgdesc = Fast, Private & Safe Web Browser
arch = x86_64
size = 238000000
depend = dbus-glib
depend = gtk3
provides = www-browser
"#;
        let result = merge_pkginfo(candidate, pkginfo);
        assert_eq!(result.name, "firefox");
        assert_eq!(result.version, "120.0-1");
        assert_eq!(result.dependencies.len(), 2);
        assert_eq!(result.provides, vec!["www-browser"]);
        assert_eq!(result.installed_size, 238000000);
    }
}
