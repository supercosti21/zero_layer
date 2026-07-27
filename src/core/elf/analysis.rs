use std::path::{Path, PathBuf};

use crate::error::ZlResult;

/// All interesting metadata extracted from an ELF binary
#[derive(Debug, Clone)]
pub struct ElfInfo {
    /// Path to the ELF file
    pub path: PathBuf,
    /// Whether this is a dynamically linked executable, shared library, or static
    #[allow(dead_code)]
    pub elf_type: ElfType,
    /// The PT_INTERP path (e.g., /lib64/ld-linux-x86-64.so.2)
    pub interpreter: Option<String>,
    /// DT_NEEDED entries — shared libraries this binary needs
    pub needed_libs: Vec<String>,
    /// Current RPATH (DT_RPATH)
    #[allow(dead_code)]
    pub rpath: Option<String>,
    /// Current RUNPATH (DT_RUNPATH)
    #[allow(dead_code)]
    pub runpath: Option<String>,
    /// SONAME if this is a shared library
    pub soname: Option<String>,
    /// Architecture (e.g., EM_X86_64)
    #[allow(dead_code)]
    pub machine: u16,
    /// Whether the binary is 64-bit
    #[allow(dead_code)]
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

/// Is DF_1_PIE set in DT_FLAGS_1? Marks an ET_DYN object as an executable
/// rather than a shared library.
fn is_pie(elf: &goblin::elf::Elf) -> bool {
    use goblin::elf::dynamic::DF_1_PIE;

    elf.dynamic
        .as_ref()
        .is_some_and(|dynamic| dynamic.info.flags_1 & DF_1_PIE != 0)
}

/// Analyze a single ELF file, extracting all relevant metadata
pub fn analyze(path: &Path) -> ZlResult<ElfInfo> {
    use goblin::elf::{Elf, header};

    let data = std::fs::read(path)?;
    let elf = Elf::parse(&data).map_err(|e| crate::error::ZlError::ElfAnalysis {
        path: path.to_path_buf(),
        source: Box::new(e),
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
            // PIE executables are also ET_DYN. A dynamically linked one has a
            // PT_INTERP; a static-pie has none and is otherwise shaped exactly
            // like a shared library, so it is told apart by DF_1_PIE — the same
            // flag file(1) uses to print "pie executable". musl release builds,
            // which pick_best_asset prefers, are typically static-pie.
            if elf.interpreter.is_some() || is_pie(&elf) {
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

/// Map ELF e_machine value to our Arch enum.
/// Returns None for unknown/unsupported machine types.
pub fn elf_machine_to_arch(machine: u16) -> Option<crate::system::arch::Arch> {
    use crate::system::arch::Arch;
    use goblin::elf::header;
    match machine {
        header::EM_X86_64 => Some(Arch::X86_64),
        header::EM_AARCH64 => Some(Arch::Aarch64),
        header::EM_ARM => Some(Arch::Armv7),
        header::EM_RISCV => Some(Arch::Riscv64), // could be 32-bit riscv but we assume 64
        header::EM_386 => Some(Arch::I686),
        header::EM_S390 => Some(Arch::S390x),
        header::EM_PPC64 => Some(Arch::Ppc64le),
        header::EM_MIPS => Some(Arch::Mips64),
        _ => None,
    }
}

/// Check if an ELF binary's architecture is compatible with the host system.
/// Returns Ok(()) if compatible, or a descriptive error message if not.
pub fn check_arch_compat(
    info: &ElfInfo,
    host_arch: &crate::system::arch::Arch,
) -> Result<(), String> {
    let elf_arch = match elf_machine_to_arch(info.machine) {
        Some(a) => a,
        None => return Ok(()), // Unknown machine type — skip check
    };

    if elf_arch == *host_arch {
        return Ok(());
    }

    // Allow i686 binaries on x86_64 (multilib compat)
    if *host_arch == crate::system::arch::Arch::X86_64
        && elf_arch == crate::system::arch::Arch::I686
    {
        return Ok(());
    }

    Err(format!(
        "Binary {} is built for {} but your system is {}",
        info.path.display(),
        elf_arch,
        host_arch
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_elf_file_on_bin_sh() {
        assert!(is_elf_file(Path::new("/bin/sh")));
    }

    #[test]
    fn test_is_elf_file_on_non_elf() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notelf.txt");
        std::fs::write(&file, "hello world").unwrap();
        assert!(!is_elf_file(&file));
    }

    #[test]
    fn test_is_elf_file_on_nonexistent() {
        assert!(!is_elf_file(Path::new("/nonexistent/file")));
    }

    #[test]
    fn test_analyze_bin_sh() {
        let info = analyze(Path::new("/bin/sh")).unwrap();
        assert_eq!(info.path, PathBuf::from("/bin/sh"));
        // /bin/sh is typically a PIE executable (ET_DYN with PT_INTERP)
        assert!(
            info.elf_type == ElfType::Executable,
            "Expected executable, got: {:?}",
            info.elf_type
        );
        assert!(
            info.interpreter.is_some(),
            "/bin/sh should have a PT_INTERP"
        );
        assert!(
            !info.needed_libs.is_empty(),
            "/bin/sh should have DT_NEEDED entries"
        );
    }

    /// The DF_1_PIE check that rescues static-pie executables must not drag
    /// ordinary shared libraries along with it: they are ET_DYN and have no
    /// PT_INTERP either, and misclassifying one would put it on PATH.
    #[test]
    fn test_analyze_shared_library_is_not_an_executable() {
        let libs: Vec<PathBuf> = ["/usr/lib", "/usr/lib/x86_64-linux-gnu", "/lib"]
            .iter()
            .filter_map(|dir| std::fs::read_dir(dir).ok())
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_file() && is_elf_file(p))
            .filter_map(|p| analyze(&p).ok())
            .filter(|i| i.soname.is_some())
            .take(20)
            .map(|i| i.path)
            .collect();

        assert!(
            !libs.is_empty(),
            "expected to find shared libraries in the system lib dirs"
        );
        for lib in libs {
            let info = analyze(&lib).unwrap();
            assert_eq!(
                info.elf_type,
                ElfType::SharedLibrary,
                "{} has a SONAME and should be a shared library",
                lib.display()
            );
        }
    }

    #[test]
    fn test_scan_directory_finds_elfs() {
        let results = scan_directory(Path::new("/bin")).unwrap();
        assert!(
            !results.is_empty(),
            "/bin should contain at least one ELF file"
        );
    }
}
