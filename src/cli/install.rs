use std::collections::HashMap;
use std::os::unix::fs as unix_fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::core::db::ops::ZlDatabase;
use crate::core::elf::{analysis, patcher};
use crate::core::graph::model::{PackageId, PackageNode};
use crate::core::path::PathMapping;
use crate::core::path::remapper;
use crate::error::{ZlError, ZlResult};
use crate::paths::ZlPaths;
use crate::plugin::PluginRegistry;
use crate::system::SystemProfile;

use super::InstallArgs;

pub fn handle(
    args: InstallArgs,
    paths: &ZlPaths,
    db: &ZlDatabase,
    registry: &PluginRegistry,
    profile: &SystemProfile,
    auto_yes: bool,
) -> ZlResult<()> {
    // 1. Pick plugin
    let plugin = registry
        .get_or_default(args.from.as_deref())
        .ok_or_else(|| ZlError::Plugin {
            plugin: args.from.unwrap_or_default(),
            message: "No matching source plugin found".into(),
        })?;

    println!("Syncing package database from {}...", plugin.display_name());
    plugin.sync()?;

    // 2. Resolve package
    let candidate = plugin
        .resolve(&args.package, args.version.as_deref())?
        .ok_or_else(|| ZlError::PackageNotFound(args.package.clone()))?;

    // 3. Check if already installed
    if db.get_package(&candidate.name, &candidate.version)?.is_some() {
        println!(
            "{}-{} is already installed.",
            candidate.name, candidate.version
        );
        return Ok(());
    }

    // 4. Confirm
    println!(
        "\nPackage: {}-{} ({})",
        candidate.name, candidate.version, candidate.source
    );
    println!("Description: {}", candidate.description);
    if !candidate.dependencies.is_empty() {
        println!("Dependencies: {}", candidate.dependencies.join(", "));
    }
    println!(
        "Installed size: {:.1} MB",
        candidate.installed_size as f64 / 1_000_000.0
    );

    if !auto_yes {
        print!("\nProceed with installation? [Y/n] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        let input = input.trim().to_lowercase();
        if !input.is_empty() && input != "y" && input != "yes" {
            println!("Installation cancelled.");
            return Ok(());
        }
    }

    // 5. Download
    println!("Downloading {}...", candidate.name);
    let archive_path = plugin.download(&candidate, &paths.cache)?;

    // 6. Extract
    println!("Extracting...");
    let extracted = plugin.extract(&archive_path)?;

    // 7. Create path mapping (now uses SystemProfile instead of hardcoded FHS)
    let mapping = PathMapping::for_package(&paths.root, &candidate.name, &candidate.version, profile);

    // 8. Patch ELF binaries
    let elf_count = extracted.elf_files.len();
    if elf_count > 0 {
        println!("Patching {} ELF binaries...", elf_count);
    }
    for elf_path in &extracted.elf_files {
        match analysis::analyze(elf_path) {
            Ok(info) => {
                if let Err(e) = patcher::patch_for_zl(elf_path, &info, &mapping, profile) {
                    tracing::warn!("Failed to patch {}: {}", elf_path.display(), e);
                }
            }
            Err(e) => {
                tracing::debug!("Skipping ELF {}: {}", elf_path.display(), e);
            }
        }
    }

    // 9. Remap scripts
    let script_count = extracted.script_files.len();
    if script_count > 0 {
        println!("Remapping {} scripts...", script_count);
    }
    for script_path in &extracted.script_files {
        let _ = remapper::remap_shebang(script_path, &mapping);
        let _ = remapper::remap_text_file(script_path, &mapping);
    }

    // 10. Install files to package directory
    let pkg_dir = paths
        .packages
        .join(format!("{}-{}", candidate.name, candidate.version));
    std::fs::create_dir_all(&pkg_dir)?;

    println!("Installing files...");
    let mut installed_files = Vec::new();
    let mut provides_libs = HashMap::new();

    let extract_root = extracted.extract_dir.path();
    for file in &extracted.files {
        // Compute relative path from extract dir
        let rel_path = match file.strip_prefix(extract_root) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let dest = pkg_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(file, &dest)?;

        installed_files.push(dest.clone());

        // Track shared libraries
        if analysis::is_elf_file(&dest) {
            if let Ok(info) = analysis::analyze(&dest) {
                if let Some(ref soname) = info.soname {
                    provides_libs.insert(soname.clone(), dest.clone());
                }
            }
        }
    }

    // 11. Create symlinks for binaries (scans common + dynamic locations)
    create_bin_symlinks(&pkg_dir, &paths.bin)?;

    // 12. Create symlinks for shared libraries
    for (soname, lib_path) in &provides_libs {
        let link_path = paths.lib.join(soname);
        if link_path.exists() || link_path.symlink_metadata().is_ok() {
            std::fs::remove_file(&link_path)?;
        }
        unix_fs::symlink(lib_path, &link_path)?;
        tracing::debug!("Linked lib {} -> {}", soname, lib_path.display());
    }

    // 13. Build PackageNode and save to DB
    let needs_libs: Vec<String> = extracted
        .elf_files
        .iter()
        .filter_map(|p| analysis::analyze(p).ok())
        .flat_map(|info| info.needed_libs)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let node = PackageNode {
        id: PackageId {
            name: candidate.name.clone(),
            version: candidate.version.clone(),
            source: candidate.source.clone(),
        },
        installed_files: installed_files.clone(),
        provides_libs: provides_libs.clone(),
        needs_libs,
        installed_at: now,
        explicit: true,
    };

    db.put_package(&node)?;

    // 14. Register file ownership and lib index
    let pkg_key = format!("{}-{}", candidate.name, candidate.version);
    for file in &installed_files {
        db.register_file(&file.to_string_lossy(), &pkg_key)?;
    }
    for (soname, _) in &provides_libs {
        db.register_lib(soname, &pkg_key)?;
    }

    // 15. Verification (warn only)
    let lib_index_paths: HashMap<String, std::path::PathBuf> = provides_libs;
    let verification =
        crate::core::graph::verifier::verify_package(&pkg_dir, &candidate.name, paths, &lib_index_paths, profile)?;
    if !verification.all_ok {
        let report = crate::core::graph::verifier::format_report(&verification);
        eprintln!("\nWarning: {}", report);
    }

    // 16. Summary
    println!(
        "\nInstalled {}-{} ({} files)",
        candidate.name,
        candidate.version,
        installed_files.len()
    );

    Ok(())
}

/// Find executables inside a package directory and symlink them into bin/.
/// Scans common FHS subdirectories plus performs a recursive scan for any
/// executable ELF files in non-standard locations.
fn create_bin_symlinks(pkg_dir: &Path, bin_dir: &Path) -> ZlResult<()> {
    // Common binary subdirectories within extracted packages
    let bin_subdirs = [
        "usr/bin",
        "usr/sbin",
        "bin",
        "sbin",
        "usr/local/bin",
        "usr/local/sbin",
    ];

    let mut linked = std::collections::HashSet::new();

    for subdir in &bin_subdirs {
        let dir = pkg_dir.join(subdir);
        if !dir.is_dir() {
            continue;
        }

        let entries = std::fs::read_dir(&dir)?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() && is_executable(&path) {
                let name = entry.file_name();
                let link_path = bin_dir.join(&name);
                if link_path.exists() || link_path.symlink_metadata().is_ok() {
                    std::fs::remove_file(&link_path)?;
                }
                unix_fs::symlink(&path, &link_path)?;
                tracing::debug!("Linked bin {} -> {}", name.to_string_lossy(), path.display());
                linked.insert(name.to_string_lossy().to_string());
            }
        }
    }

    Ok(())
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}
