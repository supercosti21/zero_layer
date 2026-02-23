//! `zl run` — run a package without installing it.
//!
//! Downloads to a temp directory, extracts, patches ELF binaries, executes
//! the requested binary, then cleans up automatically.

use std::path::Path;

use crate::core::elf::{analysis, patcher};
use crate::core::path::PathMapping;
use crate::error::{ZlError, ZlResult};

use super::{AppContext, RunArgs};

pub fn handle(args: RunArgs, ctx: &AppContext) -> ZlResult<()> {
    let from: String = match args.from.as_deref() {
        Some(f) => f.to_string(),
        None => super::install::pick_source_for_run(
            &args.package,
            args.version.as_deref(),
            ctx.registry,
            ctx.auto_yes,
        )?,
    };

    let plugin = ctx.registry.get(&from).ok_or_else(|| ZlError::Plugin {
        plugin: from.clone(),
        message: "No matching source plugin found".into(),
    })?;

    // Resolve the package
    let candidate = plugin
        .resolve(&args.package, args.version.as_deref())?
        .ok_or_else(|| ZlError::PackageNotFound {
            name: args.package.clone(),
        })?;

    println!(
        "Fetching {}-{} from {} for temporary execution...",
        candidate.name, candidate.version, from
    );

    // Download to temp dir
    let tmp_dir = tempfile::tempdir()?;
    let archive_path = plugin.download(&candidate, tmp_dir.path())?;

    // Extract
    let extracted = plugin.extract(&archive_path)?;

    // Patch ELF binaries
    let mapping = PathMapping::for_package(
        extracted.extract_dir.path(),
        &candidate.name,
        &candidate.version,
        ctx.profile,
    );

    for elf_path in &extracted.elf_files {
        if let Ok(info) = analysis::analyze(elf_path)
            && let Err(e) = patcher::patch_for_zl(elf_path, &info, &mapping, ctx.profile)
        {
            tracing::warn!("Failed to patch {}: {}", elf_path.display(), e);
        }
    }

    // Find the main executable
    let binary = find_main_binary(extracted.extract_dir.path(), &args.package)?;

    println!("Running {}...\n", binary.display());

    // Execute
    let status = std::process::Command::new(&binary)
        .args(&args.args)
        .env(
            "LD_LIBRARY_PATH",
            build_ld_path(extracted.extract_dir.path(), ctx),
        )
        .status()
        .map_err(|e| ZlError::Plugin {
            plugin: "run".into(),
            message: format!("Failed to execute {}: {}", binary.display(), e),
        })?;

    // tmp_dir drops automatically, cleaning up everything

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

/// Find the main binary in the extracted package directory.
/// Looks in standard bin subdirectories for a binary matching the package name,
/// or falls back to the first executable found.
fn find_main_binary(extract_dir: &Path, package_name: &str) -> ZlResult<std::path::PathBuf> {
    let bin_subdirs = [
        "usr/bin",
        "usr/sbin",
        "bin",
        "sbin",
        "usr/local/bin",
        "usr/local/sbin",
    ];

    let mut first_executable = None;

    for subdir in &bin_subdirs {
        let dir = extract_dir.join(subdir);
        if !dir.is_dir() {
            continue;
        }

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && is_executable(&path) {
                    // Exact match on package name
                    if entry.file_name().to_string_lossy() == package_name {
                        return Ok(path);
                    }
                    if first_executable.is_none() {
                        first_executable = Some(path);
                    }
                }
            }
        }
    }

    first_executable.ok_or_else(|| ZlError::Plugin {
        plugin: "run".into(),
        message: format!(
            "No executable found in {} — the package may not contain binaries",
            extract_dir.display()
        ),
    })
}

/// Build LD_LIBRARY_PATH from the extracted package + system + ZL lib dirs
fn build_ld_path(extract_dir: &Path, ctx: &AppContext) -> String {
    let mut paths = Vec::new();

    // Add lib dirs from the extracted package
    for subdir in &["usr/lib", "lib", "usr/lib64", "lib64"] {
        let dir = extract_dir.join(subdir);
        if dir.is_dir() {
            paths.push(dir.to_string_lossy().into_owned());
        }
    }

    // Add ZL lib dir
    paths.push(ctx.paths.lib.to_string_lossy().into_owned());

    // Add system lib dirs
    for dir in &ctx.profile.lib_dirs {
        paths.push(dir.to_string_lossy().into_owned());
    }

    paths.join(":")
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_main_binary() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("usr/bin");
        std::fs::create_dir_all(&bin_dir).unwrap();

        // Create a mock executable
        let exe = bin_dir.join("testpkg");
        std::fs::write(&exe, "#!/bin/sh\necho hi").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let found = find_main_binary(tmp.path(), "testpkg").unwrap();
        assert_eq!(found, exe);
    }

    #[test]
    fn test_find_main_binary_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let bin_dir = tmp.path().join("usr/bin");
        std::fs::create_dir_all(&bin_dir).unwrap();

        let exe = bin_dir.join("other-binary");
        std::fs::write(&exe, "#!/bin/sh\necho hi").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let found = find_main_binary(tmp.path(), "nonexistent-name").unwrap();
        assert_eq!(found, exe);
    }

    #[test]
    fn test_find_main_binary_none() {
        let tmp = tempfile::tempdir().unwrap();
        let result = find_main_binary(tmp.path(), "pkg");
        assert!(result.is_err());
    }
}
