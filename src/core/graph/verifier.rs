use std::collections::HashMap;
use std::path::Path;

use crate::core::elf::analysis;
use crate::error::{ZlError, ZlResult};
use crate::paths::ZlPaths;
use crate::system::SystemProfile;

/// Result of verifying a single ELF binary
#[derive(Debug)]
pub struct ElfVerification {
    pub path: std::path::PathBuf,
    pub missing_libs: Vec<String>,
    pub interpreter_ok: bool,
}

/// Result of verifying all ELF binaries in a package
#[derive(Debug)]
pub struct PackageVerification {
    pub package_name: String,
    pub elf_results: Vec<ElfVerification>,
    pub all_ok: bool,
}

/// Verify that all ELF files in a package directory can find their dependencies
pub fn verify_package(
    package_dir: &Path,
    package_name: &str,
    paths: &ZlPaths,
    lib_index: &HashMap<String, std::path::PathBuf>,
    profile: &SystemProfile,
) -> ZlResult<PackageVerification> {
    let elf_files = analysis::scan_directory(package_dir)?;
    let mut elf_results = Vec::new();
    let mut all_ok = true;

    for elf_info in &elf_files {
        let mut missing_libs = Vec::new();

        for needed in &elf_info.needed_libs {
            // Check if the lib exists in ZL's shared lib dir
            let in_shared = paths.lib.join(needed).exists();
            // Check if it's in our lib_index
            let in_index = lib_index.contains_key(needed.as_str());
            // Check if it's available on the system (using dynamic profile, not hardcoded dirs)
            let on_system = profile.system_lib_exists(needed);

            if !in_shared && !in_index && !on_system {
                missing_libs.push(needed.clone());
                all_ok = false;
            }
        }

        let interpreter_ok = match &elf_info.interpreter {
            Some(interp) => Path::new(interp).exists(),
            None => true, // No interpreter needed (shared lib or static)
        };

        if !interpreter_ok {
            all_ok = false;
        }

        elf_results.push(ElfVerification {
            path: elf_info.path.clone(),
            missing_libs,
            interpreter_ok,
        });
    }

    Ok(PackageVerification {
        package_name: package_name.to_string(),
        elf_results,
        all_ok,
    })
}

/// Summarize verification results into a human-readable report
pub fn format_report(verification: &PackageVerification) -> String {
    if verification.all_ok {
        return format!(
            "Package '{}': all {} ELF files verified OK",
            verification.package_name,
            verification.elf_results.len()
        );
    }

    let mut lines = vec![format!(
        "Package '{}': verification FAILED",
        verification.package_name
    )];

    for result in &verification.elf_results {
        if !result.missing_libs.is_empty() {
            lines.push(format!(
                "  {}: missing libs: {}",
                result.path.display(),
                result.missing_libs.join(", ")
            ));
        }
        if !result.interpreter_ok {
            lines.push(format!(
                "  {}: interpreter not found",
                result.path.display()
            ));
        }
    }

    lines.join("\n")
}

/// Verify and return an error if anything is broken
#[allow(dead_code)]
pub fn verify_or_fail(
    package_dir: &Path,
    package_name: &str,
    paths: &ZlPaths,
    lib_index: &HashMap<String, std::path::PathBuf>,
    profile: &SystemProfile,
) -> ZlResult<()> {
    let result = verify_package(package_dir, package_name, paths, lib_index, profile)?;
    if result.all_ok {
        Ok(())
    } else {
        Err(ZlError::Verification(format_report(&result)))
    }
}
