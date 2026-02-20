use std::path::{Path, PathBuf};

/// Detect the system's dynamic linker by reading the PT_INTERP of an existing ELF binary.
///
/// This is the most reliable method: we analyze an ELF that *already works* on this system
/// (like /bin/sh). Whatever interpreter it uses is the correct one, regardless of distro,
/// architecture, or libc.
pub fn detect_interpreter() -> Option<PathBuf> {
    // Try well-known ELFs that exist on virtually every Linux system.
    // We only need ONE that works — its interpreter is the system interpreter.
    let probe_targets = [
        "/bin/sh",
        "/usr/bin/env",
        "/bin/ls",
        "/usr/bin/cat",
        "/bin/cat",
    ];

    for target in &probe_targets {
        let path = Path::new(target);
        if !path.exists() {
            continue;
        }
        if let Some(interp) = read_elf_interpreter(path) {
            let interp_path = PathBuf::from(&interp);
            if interp_path.exists() {
                tracing::debug!("Detected interpreter from {}: {}", target, interp);
                return Some(interp_path);
            }
        }
    }

    // Last resort: scan common interpreter locations
    tracing::debug!("ELF probe failed, falling back to filesystem scan");
    scan_for_interpreter()
}

/// Read the PT_INTERP field from an ELF binary using goblin.
fn read_elf_interpreter(path: &Path) -> Option<String> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 4 || &data[..4] != b"\x7fELF" {
        return None;
    }
    let elf = goblin::elf::Elf::parse(&data).ok()?;
    elf.interpreter.map(String::from)
}

/// Fallback: scan common locations for any existing dynamic linker.
fn scan_for_interpreter() -> Option<PathBuf> {
    // Covers glibc and musl on all major architectures
    let candidates = [
        // glibc — x86_64
        "/lib64/ld-linux-x86-64.so.2",
        "/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2", // Debian multiarch
        "/usr/lib64/ld-linux-x86-64.so.2",
        "/usr/lib/ld-linux-x86-64.so.2",
        // glibc — aarch64
        "/lib/ld-linux-aarch64.so.1",
        "/lib64/ld-linux-aarch64.so.1",
        "/usr/lib/ld-linux-aarch64.so.1",
        "/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1", // Debian multiarch
        // glibc — armv7
        "/lib/ld-linux-armhf.so.3",
        "/lib/arm-linux-gnueabihf/ld-linux-armhf.so.3",
        // glibc — i686
        "/lib/ld-linux.so.2",
        "/lib32/ld-linux.so.2",
        // glibc — riscv64
        "/lib/ld-linux-riscv64-lp64d.so.1",
        // glibc — s390x
        "/lib/ld64.so.1",
        // glibc — ppc64le
        "/lib64/ld64.so.2",
        // musl — various architectures
        "/lib/ld-musl-x86_64.so.1",
        "/lib/ld-musl-aarch64.so.1",
        "/lib/ld-musl-armhf.so.1",
        "/lib/ld-musl-i386.so.1",
        "/lib/ld-musl-riscv64.so.1",
        "/lib/ld-musl-s390x.so.1",
    ];

    for candidate in &candidates {
        let path = Path::new(candidate);
        if path.exists() {
            tracing::debug!("Found interpreter via scan: {}", candidate);
            return Some(path.to_path_buf());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_interpreter() {
        // On any Linux system, we should detect an interpreter
        let interp = detect_interpreter();
        assert!(interp.is_some(), "Should detect a dynamic linker on Linux");
        let interp = interp.unwrap();
        assert!(interp.exists(), "Detected interpreter should exist");
        let interp_str = interp.to_string_lossy();
        assert!(
            interp_str.contains("ld-linux") || interp_str.contains("ld-musl"),
            "Interpreter should be ld-linux or ld-musl, got: {}",
            interp_str
        );
    }

    #[test]
    fn test_read_elf_interpreter_on_sh() {
        if Path::new("/bin/sh").exists() {
            let interp = read_elf_interpreter(Path::new("/bin/sh"));
            assert!(interp.is_some(), "/bin/sh should have a PT_INTERP");
        }
    }
}
