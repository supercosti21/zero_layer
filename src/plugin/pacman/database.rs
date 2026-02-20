use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use crate::error::{ZlError, ZlResult, retry_with_backoff};
use crate::plugin::PackageCandidate;

use super::mirror::{self, Mirror};

/// A parsed entry from the pacman sync database
#[derive(Debug, Clone, Default)]
pub struct DbEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub arch: String,
    pub url: String,
    pub depends: Vec<String>,
    pub provides: Vec<String>,
    pub conflicts: Vec<String>,
    pub installed_size: u64,
    pub compressed_size: u64,
    pub filename: String,
    pub md5sum: String,
    pub sha256sum: String,
}

/// Download and parse a pacman repository database
pub fn sync_repo(
    mirror: &Mirror,
    repo: &str,
    arch: &str,
    cache_dir: &Path,
) -> ZlResult<Vec<DbEntry>> {
    let url = mirror::repo_db_url(mirror, repo, arch);
    let db_path = cache_dir.join(format!("{}.db", repo));

    tracing::info!("Syncing {} from {}", repo, url);

    let bytes = retry_with_backoff(3, 1000, |attempt| {
        if attempt > 1 {
            tracing::info!("Sync attempt {}/3 for {}", attempt, repo);
        }

        let response = reqwest::blocking::Client::new()
            .get(&url)
            .timeout(std::time::Duration::from_secs(120))
            .send()
            .map_err(|e| ZlError::DownloadFailed {
                url: url.clone(),
                attempts: attempt,
                message: format!("Failed to download {}: {}", repo, e),
            })?;

        if !response.status().is_success() {
            return Err(ZlError::DownloadFailed {
                url: url.clone(),
                attempts: attempt,
                message: format!("HTTP {}", response.status()),
            });
        }

        response.bytes().map_err(|e| ZlError::DownloadFailed {
            url: url.clone(),
            attempts: attempt,
            message: format!("Failed to read response: {}", e),
        })
    })?;

    std::fs::write(&db_path, &bytes)?;
    parse_db(&db_path)
}

/// Parse a pacman .db file (tar.gz archive containing desc files)
pub fn parse_db(db_path: &Path) -> ZlResult<Vec<DbEntry>> {
    let file = std::fs::File::open(db_path)?;
    let decompressed = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decompressed);

    let mut entries = Vec::new();
    let mut current_map: HashMap<String, DbEntry> = HashMap::new();

    for entry in archive
        .entries()
        .map_err(|e| ZlError::Archive(e.to_string()))?
    {
        let mut entry = entry.map_err(|e| ZlError::Archive(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| ZlError::Archive(e.to_string()))?
            .to_path_buf();

        let path_str = path.to_string_lossy().to_string();

        // Each package dir looks like: "firefox-120.0-1/desc"
        let parts: Vec<&str> = path_str.split('/').collect();
        if parts.len() == 2 && parts[1] == "desc" {
            let pkg_dir = parts[0].to_string();
            let mut content = String::new();
            entry
                .read_to_string(&mut content)
                .map_err(|e| ZlError::Archive(e.to_string()))?;

            let db_entry = parse_desc(&content);
            current_map.insert(pkg_dir, db_entry);
        }
    }

    entries.extend(current_map.into_values());
    Ok(entries)
}

/// Parse a single "desc" file from the pacman database
/// Format: sections delimited by %NAME%, %VERSION%, etc.
fn parse_desc(content: &str) -> DbEntry {
    let mut entry = DbEntry::default();
    let mut current_section = String::new();
    let mut values: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with('%') && trimmed.ends_with('%') {
            // Flush previous section
            if !current_section.is_empty() {
                apply_section(&mut entry, &current_section, &values);
            }
            current_section = trimmed.to_string();
            values.clear();
        } else if !trimmed.is_empty() {
            values.push(trimmed.to_string());
        }
    }

    // Flush last section
    if !current_section.is_empty() {
        apply_section(&mut entry, &current_section, &values);
    }

    entry
}

fn apply_section(entry: &mut DbEntry, section: &str, values: &[String]) {
    match section {
        "%NAME%" => {
            if let Some(v) = values.first() {
                entry.name = v.clone();
            }
        }
        "%VERSION%" => {
            if let Some(v) = values.first() {
                entry.version = v.clone();
            }
        }
        "%DESC%" => {
            if let Some(v) = values.first() {
                entry.description = v.clone();
            }
        }
        "%ARCH%" => {
            if let Some(v) = values.first() {
                entry.arch = v.clone();
            }
        }
        "%URL%" => {
            if let Some(v) = values.first() {
                entry.url = v.clone();
            }
        }
        "%DEPENDS%" => {
            entry.depends = values.to_vec();
        }
        "%PROVIDES%" => {
            entry.provides = values.to_vec();
        }
        "%CONFLICTS%" => {
            entry.conflicts = values.to_vec();
        }
        "%ISIZE%" => {
            if let Some(v) = values.first() {
                entry.installed_size = v.parse().unwrap_or(0);
            }
        }
        "%CSIZE%" => {
            if let Some(v) = values.first() {
                entry.compressed_size = v.parse().unwrap_or(0);
            }
        }
        "%FILENAME%" => {
            if let Some(v) = values.first() {
                entry.filename = v.clone();
            }
        }
        "%MD5SUM%" => {
            if let Some(v) = values.first() {
                entry.md5sum = v.clone();
            }
        }
        "%SHA256SUM%" => {
            if let Some(v) = values.first() {
                entry.sha256sum = v.clone();
            }
        }
        _ => {}
    }
}

/// Convert a DbEntry to a PackageCandidate
pub fn entry_to_candidate(entry: &DbEntry, mirror: &Mirror, repo: &str) -> PackageCandidate {
    let download_url = mirror::package_url(mirror, repo, &entry.arch, &entry.filename);

    PackageCandidate {
        name: entry.name.clone(),
        version: entry.version.clone(),
        description: entry.description.clone(),
        arch: entry.arch.clone(),
        source: format!("pacman/{}", repo),
        dependencies: entry.depends.clone(),
        provides: entry.provides.clone(),
        conflicts: entry.conflicts.clone(),
        installed_size: entry.installed_size,
        download_url,
        checksum: if entry.sha256sum.is_empty() {
            None
        } else {
            Some(entry.sha256sum.clone())
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_desc() {
        let desc = r#"
%NAME%
firefox

%VERSION%
120.0-1

%DESC%
Fast, Private & Safe Web Browser

%ARCH%
x86_64

%DEPENDS%
dbus-glib
gtk3
libxt

%ISIZE%
238000000

%FILENAME%
firefox-120.0-1-x86_64.pkg.tar.zst

%SHA256SUM%
abc123def456
"#;
        let entry = parse_desc(desc);
        assert_eq!(entry.name, "firefox");
        assert_eq!(entry.version, "120.0-1");
        assert_eq!(entry.description, "Fast, Private & Safe Web Browser");
        assert_eq!(entry.arch, "x86_64");
        assert_eq!(entry.depends.len(), 3);
        assert_eq!(entry.depends[0], "dbus-glib");
        assert_eq!(entry.installed_size, 238000000);
        assert_eq!(entry.sha256sum, "abc123def456");
    }
}
