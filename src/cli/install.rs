use std::collections::HashMap;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::core::conflicts;
use crate::core::db::ops::ZlDatabase;
use crate::core::elf::{analysis, patcher};
use crate::core::graph::model::{PackageId, PackageNode};
use crate::core::path::PathMapping;
use crate::core::path::remapper;
use crate::core::transaction::Transaction;
use crate::error::{ZlError, ZlResult};
use crate::paths::ZlPaths;
use crate::plugin::{PackageCandidate, PluginRegistry, SourcePlugin};
use crate::system::SystemProfile;

use super::InstallArgs;
use super::deps;

/// Maximum number of concurrent downloads
const MAX_PARALLEL_DOWNLOADS: usize = 4;

pub fn handle(
    args: InstallArgs,
    paths: &ZlPaths,
    db: &ZlDatabase,
    registry: &PluginRegistry,
    profile: &SystemProfile,
    auto_yes: bool,
) -> ZlResult<()> {
    // 1. Pick plugin and sync
    let plugin = registry
        .get_or_default(args.from.as_deref())
        .ok_or_else(|| ZlError::Plugin {
            plugin: args.from.as_deref().unwrap_or("default").into(),
            message: "No matching source plugin found".into(),
        })?;

    println!("Syncing package database from {}...", plugin.display_name());
    plugin.sync()?;

    // 2. Resolve package and all dependencies
    println!("Resolving dependencies...");
    let plan = deps::resolve_with_deps(
        &args.package,
        args.version.as_deref(),
        args.from.as_deref(),
        db,
        registry,
    )?;

    if plan.packages.is_empty() {
        println!("{} is already installed.", args.package);
        return Ok(());
    }

    // 3. Check for conflicts before proceeding
    println!("Checking for conflicts...");
    let candidates_refs: Vec<&PackageCandidate> =
        plan.packages.iter().map(|e| &e.candidate).collect();
    let conflict_report = conflicts::check_conflicts(&candidates_refs, db, paths)?;

    if conflict_report.has_conflicts() {
        conflict_report.display();
        if !auto_yes {
            eprintln!("\nConflicts must be resolved before installing.");
            eprintln!("  hint: remove conflicting packages with `zl remove` first");
            return Err(ZlError::PackageConflict {
                installed: "multiple".into(),
                requested: args.package,
            });
        }
        eprintln!("\nWarning: proceeding despite conflicts (--yes).");
    }

    // 4. Display install plan
    deps::display_plan(&plan);

    // 5. Confirm
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

    // 6. Download all packages with progress bars
    let total = plan.packages.len();
    let candidates: Vec<&PackageCandidate> = plan.packages.iter().map(|e| &e.candidate).collect();

    println!("\nDownloading {} package(s)...", total);
    let archives = download_parallel(&candidates, plugin, &paths.cache)?;

    // 7. Install each package with transaction support
    let mut txn = Transaction::new();
    let mut installed_count = 0;

    let install_pb = ProgressBar::new(total as u64);
    install_pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=> "),
    );

    for (i, (entry, archive_path)) in plan.packages.iter().zip(archives.iter()).enumerate() {
        install_pb.set_message(format!("Installing {}...", entry.candidate.name));
        install_pb.set_position(i as u64);

        match install_from_archive(
            &entry.candidate,
            archive_path,
            entry.explicit,
            paths,
            db,
            plugin,
            profile,
            &mut txn,
        ) {
            Ok(()) => {
                installed_count += 1;
            }
            Err(e) => {
                install_pb.finish_and_clear();
                eprintln!(
                    "\nFailed to install {}: {}",
                    entry.candidate.name, e
                );
                eprintln!("Rolling back {} installed package(s)...", installed_count);
                txn.rollback(db);
                return Err(e);
            }
        }
    }

    install_pb.set_position(total as u64);
    install_pb.finish_and_clear();

    // Commit the transaction — all installs succeeded
    txn.commit();

    // 8. Summary
    let dep_count = plan.dep_count();
    if dep_count > 0 {
        println!(
            "\nInstalled {} package(s) + {} dependency(ies).",
            total - dep_count,
            dep_count
        );
    } else {
        println!("\nInstalled {} package(s).", total);
    }

    if !plan.unresolvable.is_empty() {
        eprintln!(
            "\nWarning: {} dependency(ies) could not be resolved: {}",
            plan.unresolvable.len(),
            plan.unresolvable.join(", ")
        );
        eprintln!("  hint: install them manually or from a different source");
    }

    Ok(())
}

/// Download multiple packages in parallel using thread::scope with progress bars.
/// Returns archive paths in the same order as the input candidates.
fn download_parallel(
    candidates: &[&PackageCandidate],
    plugin: &dyn SourcePlugin,
    cache_dir: &Path,
) -> ZlResult<Vec<PathBuf>> {
    let total = candidates.len();

    if total <= 1 {
        let mut results = Vec::new();
        for candidate in candidates {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("  {spinner:.green} Downloading {msg}...")
                    .unwrap_or_else(|_| ProgressStyle::default_spinner()),
            );
            pb.set_message(candidate.name.clone());
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
            let result = plugin.download(candidate, cache_dir)?;
            pb.finish_with_message(format!("{} done", candidate.name));
            results.push(result);
        }
        return Ok(results);
    }

    let mp = MultiProgress::new();
    let completed = Mutex::new(0usize);

    let overall = mp.add(ProgressBar::new(total as u64));
    overall.set_style(
        ProgressStyle::default_bar()
            .template("  [{bar:40.cyan/blue}] {pos}/{len} downloads")
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=> "),
    );

    // Initialize with placeholder errors (will be replaced)
    let mut all_results: Vec<ZlResult<PathBuf>> = Vec::with_capacity(total);
    for _ in 0..total {
        all_results.push(Err(ZlError::DownloadFailed {
            url: String::new(),
            attempts: 0,
            message: "not started".into(),
        }));
    }
    let results_mutex = Mutex::new(all_results);

    std::thread::scope(|scope| {
        for chunk_start in (0..total).step_by(MAX_PARALLEL_DOWNLOADS) {
            let chunk_end = (chunk_start + MAX_PARALLEL_DOWNLOADS).min(total);
            let mut handles = Vec::new();

            for idx in chunk_start..chunk_end {
                let candidate = candidates[idx];
                let results_mutex = &results_mutex;
                let completed = &completed;
                let overall = &overall;
                let mp = &mp;

                handles.push(scope.spawn(move || {
                    let pb = mp.add(ProgressBar::new_spinner());
                    pb.set_style(
                        ProgressStyle::default_spinner()
                            .template("    {spinner:.green} {msg}")
                            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                    );
                    pb.set_message(format!("Downloading {}...", candidate.name));
                    pb.enable_steady_tick(std::time::Duration::from_millis(100));

                    let result = plugin.download(candidate, cache_dir);

                    let mut count = completed.lock().unwrap();
                    *count += 1;
                    overall.set_position(*count as u64);
                    drop(count);

                    let is_ok = result.is_ok();
                    if is_ok {
                        pb.finish_with_message(format!("{} downloaded", candidate.name));
                    } else {
                        pb.finish_with_message(format!("{} FAILED", candidate.name));
                    }

                    let mut results = results_mutex.lock().unwrap();
                    results[idx] = result;
                }));
            }

            for handle in handles {
                handle.join().unwrap();
            }
        }
    });

    overall.finish_and_clear();

    // Collect results, fail on first error
    let results = results_mutex.into_inner().unwrap();
    results.into_iter().collect()
}

/// Install a single package: download, extract, patch, place files, register in DB.
/// This is the core install logic, used by `update::handle()`.
pub fn install_single_package(
    candidate: &PackageCandidate,
    explicit: bool,
    paths: &ZlPaths,
    db: &ZlDatabase,
    plugin: &dyn SourcePlugin,
    profile: &SystemProfile,
) -> ZlResult<()> {
    // Check if already installed
    if db
        .get_package(&candidate.name, &candidate.version)?
        .is_some()
    {
        tracing::debug!(
            "{}-{} is already installed, skipping",
            candidate.name,
            candidate.version
        );
        return Ok(());
    }

    // Download
    println!("  Downloading {}...", candidate.name);
    let archive_path = plugin.download(candidate, &paths.cache)?;

    let mut txn = Transaction::new();
    match install_from_archive(
        candidate,
        &archive_path,
        explicit,
        paths,
        db,
        plugin,
        profile,
        &mut txn,
    ) {
        Ok(()) => {
            txn.commit();
            Ok(())
        }
        Err(e) => {
            txn.rollback(db);
            Err(e)
        }
    }
}

/// Install a package from an already-downloaded archive.
/// Extracts, patches ELF binaries, remaps scripts, places files, and registers in DB.
/// Tracks all changes in the transaction for rollback support.
pub fn install_from_archive(
    candidate: &PackageCandidate,
    archive_path: &Path,
    explicit: bool,
    paths: &ZlPaths,
    db: &ZlDatabase,
    plugin: &dyn SourcePlugin,
    profile: &SystemProfile,
    txn: &mut Transaction,
) -> ZlResult<()> {
    // Check if already installed
    if db
        .get_package(&candidate.name, &candidate.version)?
        .is_some()
    {
        tracing::debug!(
            "{}-{} is already installed, skipping",
            candidate.name,
            candidate.version
        );
        return Ok(());
    }

    // Extract
    let extracted = plugin.extract(archive_path)?;

    // Create path mapping (uses SystemProfile for dynamic path detection)
    let mapping =
        PathMapping::for_package(&paths.root, &candidate.name, &candidate.version, profile);

    // Patch ELF binaries
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

    // Remap scripts
    for script_path in &extracted.script_files {
        let _ = remapper::remap_shebang(script_path, &mapping);
        let _ = remapper::remap_text_file(script_path, &mapping);
    }

    // Install files to package directory
    let pkg_dir = paths
        .packages
        .join(format!("{}-{}", candidate.name, candidate.version));
    std::fs::create_dir_all(&pkg_dir)?;
    txn.track_dir(&pkg_dir);

    let mut installed_files = Vec::new();
    let mut provides_libs = HashMap::new();

    let extract_root = extracted.extract_dir.path();
    for file in &extracted.files {
        let rel_path = match file.strip_prefix(extract_root) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let dest = pkg_dir.join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(file, &dest)?;
        txn.track_file(&dest);

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

    // Create symlinks for binaries
    create_bin_symlinks(&pkg_dir, &paths.bin, txn)?;

    // Create symlinks for shared libraries
    for (soname, lib_path) in &provides_libs {
        let link_path = paths.lib.join(soname);
        if link_path.exists() || link_path.symlink_metadata().is_ok() {
            std::fs::remove_file(&link_path)?;
        }
        unix_fs::symlink(lib_path, &link_path)?;
        txn.track_symlink(&link_path);
        tracing::debug!("Linked lib {} -> {}", soname, lib_path.display());
    }

    // Build PackageNode and save to DB
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
        explicit,
    };

    db.put_package(&node)?;

    // Register file ownership and lib index
    let pkg_key = format!("{}-{}", candidate.name, candidate.version);
    txn.track_db_package(&pkg_key);

    for file in &installed_files {
        db.register_file(&file.to_string_lossy(), &pkg_key)?;
    }
    for (soname, _) in &provides_libs {
        db.register_lib(soname, &pkg_key)?;
    }

    // Register dependencies in the DB
    for dep in &candidate.dependencies {
        db.register_dependency(&pkg_key, dep)?;
    }

    // Verification (warn only)
    let lib_index_paths: HashMap<String, std::path::PathBuf> = provides_libs;
    let verification = crate::core::graph::verifier::verify_package(
        &pkg_dir,
        &candidate.name,
        paths,
        &lib_index_paths,
        profile,
    )?;
    if !verification.all_ok {
        let report = crate::core::graph::verifier::format_report(&verification);
        eprintln!("  Warning: {}", report);
    }

    tracing::info!(
        "Installed {}-{} ({} files)",
        candidate.name,
        candidate.version,
        installed_files.len()
    );

    Ok(())
}

/// Find executables inside a package directory and symlink them into bin/.
fn create_bin_symlinks(pkg_dir: &Path, bin_dir: &Path, txn: &mut Transaction) -> ZlResult<()> {
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
                txn.track_symlink(&link_path);
                tracing::debug!(
                    "Linked bin {} -> {}",
                    name.to_string_lossy(),
                    path.display()
                );
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
