use std::path::Path;

use crate::core::path::PathMapping;
use crate::error::ZlResult;
use crate::system::SystemProfile;

/// Does this ELF object resolve anything through the dynamic loader?
///
/// A statically linked executable — including a static-pie, which musl release
/// builds commonly ship — has neither a PT_INTERP nor DT_NEEDED entries, so an
/// interpreter and a RUNPATH would both be inert. Worse than useless: rewriting
/// the dynamic section of such a file corrupts it. ripgrep's musl build came out
/// of `set_dynamic_tag(Runpath, ..)` with `DT_RUNPATH` pointing into the ELF
/// magic (readelf printed `Library runpath: [ELF]`) and segfaulted on startup.
fn needs_patching(info: &super::analysis::ElfInfo) -> bool {
    info.interpreter.is_some() || !info.needed_libs.is_empty()
}

/// Apply all necessary patches to an ELF file for the ZL environment
pub fn patch_for_zl(
    path: &Path,
    info: &super::analysis::ElfInfo,
    mapping: &PathMapping,
    profile: &SystemProfile,
) -> ZlResult<()> {
    use elb::{DynamicTag, Elf};
    use std::fs::OpenOptions;

    if !needs_patching(info) {
        tracing::debug!("Skipping patch for statically linked {}", path.display());
        return Ok(());
    }

    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let elf =
        Elf::read(&mut file, profile.page_size).map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    let mut patcher = elb::ElfPatcher::new(elf, file);

    // Patch interpreter if present and needs remapping
    if let Some(ref orig_interp) = info.interpreter
        && let Some(new_interp) = mapping.remap_interpreter(orig_interp)
    {
        let c_interp =
            std::ffi::CString::new(new_interp).map_err(|e| crate::error::ZlError::ElfPatch {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;
        patcher
            .set_interpreter(&c_interp)
            .map_err(|e| crate::error::ZlError::ElfPatch {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;
    }

    // Build and set RUNPATH so needed libs resolve correctly
    if let Some(runpath) = mapping.compute_runpath(path, &info.needed_libs) {
        let c_runpath =
            std::ffi::CString::new(runpath).map_err(|e| crate::error::ZlError::ElfPatch {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;
        patcher
            .set_dynamic_tag(DynamicTag::Runpath, &*c_runpath)
            .map_err(|e| crate::error::ZlError::ElfPatch {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;
    }

    patcher
        .finish()
        .map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

    Ok(())
}

/// Set only the interpreter of an ELF binary
#[allow(dead_code)]
pub fn set_interpreter(path: &Path, new_interp: &str, page_size: u64) -> ZlResult<()> {
    use std::fs::OpenOptions;

    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let elf =
        elb::Elf::read(&mut file, page_size).map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    let mut patcher = elb::ElfPatcher::new(elf, file);

    let c_interp =
        std::ffi::CString::new(new_interp).map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    patcher
        .set_interpreter(&c_interp)
        .map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    patcher
        .finish()
        .map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

    Ok(())
}

/// Set only the RUNPATH of an ELF binary
#[allow(dead_code)]
pub fn set_runpath(path: &Path, new_runpath: &str, page_size: u64) -> ZlResult<()> {
    use elb::DynamicTag;
    use std::fs::OpenOptions;

    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let elf =
        elb::Elf::read(&mut file, page_size).map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    let mut patcher = elb::ElfPatcher::new(elf, file);

    let c_runpath =
        std::ffi::CString::new(new_runpath).map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    patcher
        .set_dynamic_tag(DynamicTag::Runpath, &*c_runpath)
        .map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    patcher
        .finish()
        .map_err(|e| crate::error::ZlError::ElfPatch {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::elf::analysis::{ElfInfo, ElfType};
    use std::path::PathBuf;

    fn info(elf_type: ElfType, interpreter: Option<&str>, needed: &[&str]) -> ElfInfo {
        ElfInfo {
            path: PathBuf::from("/tmp/fixture"),
            elf_type,
            interpreter: interpreter.map(String::from),
            needed_libs: needed.iter().map(|s| s.to_string()).collect(),
            rpath: None,
            runpath: None,
            soname: None,
            machine: 0x3e,
            is_64bit: true,
        }
    }

    #[test]
    fn test_dynamic_executable_is_patched() {
        let dynamic = info(
            ElfType::Executable,
            Some("/lib64/ld-linux-x86-64.so.2"),
            &["libc.so.6"],
        );
        assert!(needs_patching(&dynamic));
    }

    #[test]
    fn test_shared_library_is_patched() {
        // No PT_INTERP, but it still resolves DT_NEEDED at load time
        let lib = info(ElfType::SharedLibrary, None, &["libc.so.6"]);
        assert!(needs_patching(&lib));
    }

    #[test]
    fn test_static_pie_is_not_patched() {
        // Regression: patching ripgrep's musl static-pie build wrote a bogus
        // DT_RUNPATH and the binary segfaulted on startup.
        let static_pie = info(ElfType::Executable, None, &[]);
        assert!(!needs_patching(&static_pie));
    }

    #[test]
    fn test_fully_static_binary_is_not_patched() {
        let static_bin = info(ElfType::StaticBinary, None, &[]);
        assert!(!needs_patching(&static_bin));
    }
}
