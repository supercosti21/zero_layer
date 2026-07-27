use crate::error::ZlResult;
use crate::paths::ZlPaths;

use super::CacheCommand;

pub fn handle(cmd: CacheCommand, paths: &ZlPaths) -> ZlResult<()> {
    match cmd {
        CacheCommand::List => handle_list(paths),
        CacheCommand::Clean => handle_clean(paths),
        CacheCommand::Dedup => handle_dedup(paths),
    }
}

fn handle_list(paths: &ZlPaths) -> ZlResult<()> {
    if !paths.cache.is_dir() {
        println!("Cache directory does not exist.");
        return Ok(());
    }

    let mut total_size: u64 = 0;
    let mut file_count: u64 = 0;

    for entry in walkdir::WalkDir::new(&paths.cache)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            total_size += size;
            file_count += 1;

            let rel_path = entry
                .path()
                .strip_prefix(&paths.cache)
                .unwrap_or(entry.path());
            println!(
                "  {:<60} {:>8.1} MB",
                rel_path.display(),
                size as f64 / 1_000_000.0
            );
        }
    }

    if file_count == 0 {
        println!("Cache is empty.");
    } else {
        println!(
            "\n{} file(s), {:.1} MB total",
            file_count,
            total_size as f64 / 1_000_000.0
        );
    }

    Ok(())
}

fn handle_clean(paths: &ZlPaths) -> ZlResult<()> {
    if !paths.cache.is_dir() {
        println!("Cache directory does not exist.");
        return Ok(());
    }

    let mut total_size: u64 = 0;
    let mut file_count: u64 = 0;

    // Count before removing
    for entry in walkdir::WalkDir::new(&paths.cache)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            total_size += entry.metadata().map(|m| m.len()).unwrap_or(0);
            file_count += 1;
        }
    }

    if file_count == 0 {
        println!("Cache is already empty.");
        return Ok(());
    }

    // Remove all contents but keep the cache directory itself
    for entry in std::fs::read_dir(&paths.cache)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
    }

    println!(
        "Cleaned {} file(s), freed {:.1} MB.",
        file_count,
        total_size as f64 / 1_000_000.0
    );

    Ok(())
}

/// Deduplicate identical shared libraries across packages using hardlinks.
/// Libraries with the same SHA256 hash are hardlinked to save disk space.
fn handle_dedup(paths: &ZlPaths) -> ZlResult<()> {
    use std::collections::HashMap;

    if !paths.packages.is_dir() {
        println!("No packages installed.");
        return Ok(());
    }

    println!("Scanning packages for duplicate libraries...");

    // Map: SHA256 hash -> (canonical_path, size)
    let mut seen: HashMap<String, (std::path::PathBuf, u64)> = HashMap::new();
    let mut dedup_count = 0u64;
    let mut saved_bytes = 0u64;

    for entry in walkdir::WalkDir::new(&paths.packages)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();

        // Only process shared library files
        if !fname.contains(".so") {
            continue;
        }

        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Skip symlinks and already-hardlinked files (nlink > 1 is fine, but check hash)
        let size = metadata.len();
        if size == 0 {
            continue;
        }

        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let hash = crate::core::verify::sha256_hex(&data);

        if let Some((canonical, _)) = seen.get(&hash) {
            // Same content — replace with hardlink
            if canonical == path {
                continue;
            }

            // Check if already hardlinked (same inode)
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if let Ok(cm) = std::fs::metadata(canonical)
                    && cm.ino() == metadata.ino()
                    && cm.dev() == metadata.dev()
                {
                    continue; // already hardlinked
                }
            }

            if let Err(e) = std::fs::remove_file(path) {
                tracing::warn!("Failed to remove {} for dedup: {}", path.display(), e);
                continue;
            }
            if let Err(e) = std::fs::hard_link(canonical, path) {
                tracing::warn!(
                    "Failed to hardlink {} -> {}: {}",
                    path.display(),
                    canonical.display(),
                    e
                );
                // Restore by writing the data back
                let _ = std::fs::write(path, &data);
                continue;
            }

            dedup_count += 1;
            saved_bytes += size;
            tracing::debug!(
                "Deduped: {} -> {} ({:.1} KB)",
                path.display(),
                canonical.display(),
                size as f64 / 1000.0
            );
        } else {
            seen.insert(hash, (path.to_path_buf(), size));
        }
    }

    if dedup_count == 0 {
        println!("No duplicate libraries found.");
    } else {
        println!(
            "Deduplicated {} file(s), saved {:.1} MB.",
            dedup_count,
            saved_bytes as f64 / 1_000_000.0
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_hardlinks_identical_files() {
        let tmp = tempfile::tempdir().unwrap();
        let packages_dir = tmp.path().join("packages");
        let pkg1 = packages_dir.join("a-1.0");
        let pkg2 = packages_dir.join("b-1.0");
        std::fs::create_dir_all(&pkg1).unwrap();
        std::fs::create_dir_all(&pkg2).unwrap();

        // Create identical .so files
        let content = b"fake shared library content for testing";
        std::fs::write(pkg1.join("libfoo.so.1"), content).unwrap();
        std::fs::write(pkg2.join("libfoo.so.1"), content).unwrap();

        let paths = ZlPaths {
            root: tmp.path().to_path_buf(),
            bin: tmp.path().join("bin"),
            lib: tmp.path().join("lib"),
            share: tmp.path().join("share"),
            etc: tmp.path().join("etc"),
            packages: packages_dir,
            cache: tmp.path().join("cache"),
            db_file: tmp.path().join("zl.redb"),
            envs: tmp.path().join("envs"),
        };

        handle_dedup(&paths).unwrap();

        // After dedup, both files should have the same inode
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let m1 = std::fs::metadata(pkg1.join("libfoo.so.1")).unwrap();
            let m2 = std::fs::metadata(pkg2.join("libfoo.so.1")).unwrap();
            assert_eq!(m1.ino(), m2.ino());
        }
    }
}
