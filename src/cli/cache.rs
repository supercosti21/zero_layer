use crate::error::ZlResult;
use crate::paths::ZlPaths;

use super::CacheCommand;

pub fn handle(cmd: CacheCommand, paths: &ZlPaths) -> ZlResult<()> {
    match cmd {
        CacheCommand::List => handle_list(paths),
        CacheCommand::Clean => handle_clean(paths),
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
