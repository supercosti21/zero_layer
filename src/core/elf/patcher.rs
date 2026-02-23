use std::path::Path;

use crate::core::path::PathMapping;
use crate::error::ZlResult;
use crate::system::SystemProfile;

/// Apply all necessary patches to an ELF file for the ZL environment
pub fn patch_for_zl(
    path: &Path,
    info: &super::analysis::ElfInfo,
    mapping: &PathMapping,
    profile: &SystemProfile,
) -> ZlResult<()> {
    use elb::{DynamicTag, Elf};
    use std::fs::OpenOptions;

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
