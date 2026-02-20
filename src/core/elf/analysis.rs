use std::path::{Path, PathBuf};

use crate::error::ZlResult;

/// All interesting metadata extracted from an ELF binary
#[derive(Debug, Clone)]
pub struct ElfInfo {
    /// Path to the ELF file
    pub path: PathBuf,
    /// Whether this is a dynamically linked executable, shared library, or static
    pub elf_type: ElfType,
    /// The PT_INTERP path (e.g., /lib64/ld-linux-x86-64.so.2)
    pub interpreter: Option<String>,
    /// DT_NEEDED entries — shared libraries this binary needs
    pub needed_libs: Vec<String>,
    /// Current RPATH (DT_RPATH)
    pub rpath: Option<String>,
    /// Current RUNPATH (DT_RUNPATH)
    pub runpath: Option<String>,
    /// SONAME if this is a shared library
    pub soname: Option<String>,
    /// Architecture (e.g., EM_X86_64)
    pub machine: u16,
    /// Whether the binary is 64-bit
    pub is_64bit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ElfType {
    Executable,
    SharedLibrary,
    StaticBinary,
    Other,
}

/// Quick check: does this file start with ELF magic bytes?
pub fn is_elf_file(path: &Path) -> bool {
    std::fs::read(path)
        .map(|data| data.len() >= 4 && data[..4] == *b"\x7fELF")
        .unwrap_or(false)
}

/// Analyze a single ELF file, extracting all relevant metadata
pub fn analyze(path: &Path) -> ZlResult<ElfInfo> {
    use goblin::elf::{header, Elf};

    let data = std::fs::read(path)?;
    let elf = Elf::parse(&data).map_err(|e| crate::error::ZlError::ElfAnalysis {
        path: path.to_path_buf(),
        source: e,
    })?;

    let elf_type = match elf.header.e_type {
        header::ET_EXEC => {
            if elf.dynamic.is_some() {
                ElfType::Executable
            } else {
                ElfType::StaticBinary
            }
        }
        header::ET_DYN => {
            // PIE executables are also ET_DYN; distinguish by presence of PT_INTERP
            if elf.interpreter.is_some() {
                ElfType::Executable
            } else {
                ElfType::SharedLibrary
            }
        }
        _ => ElfType::Other,
    };

    let interpreter = elf.interpreter.map(String::from);
    let needed_libs = elf.libraries.iter().map(|s| s.to_string()).collect();
    let rpath = if elf.rpaths.is_empty() {
        None
    } else {
        Some(elf.rpaths.join(":"))
    };
    let runpath = if elf.runpaths.is_empty() {
        None
    } else {
        Some(elf.runpaths.join(":"))
    };
    let soname = elf.soname.map(String::from);

    Ok(ElfInfo {
        path: path.to_path_buf(),
        elf_type,
        interpreter,
        needed_libs,
        rpath,
        runpath,
        soname,
        machine: elf.header.e_machine,
        is_64bit: elf.is_64,
    })
}

/// Scan a directory tree and return ElfInfo for every ELF file found
pub fn scan_directory(dir: &Path) -> ZlResult<Vec<ElfInfo>> {
    let mut results = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() && is_elf_file(entry.path()) {
            match analyze(entry.path()) {
                Ok(info) => results.push(info),
                Err(e) => {
                    tracing::debug!("Skipping {}: {}", entry.path().display(), e);
                }
            }
        }
    }
    Ok(results)
}
