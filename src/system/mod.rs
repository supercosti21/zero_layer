pub mod arch;
pub mod detect;
pub mod interpreter;
pub mod libc;
pub mod paths;

use std::fmt;
use std::path::PathBuf;

use crate::config::SystemConfig;

pub use arch::Arch;
pub use detect::SystemLayout;
pub use libc::LibC;

/// A complete profile of the host system, auto-detected at startup.
///
/// Replaces all hardcoded FHS assumptions. Every path, every interpreter,
/// every library directory is discovered dynamically from the running system.
#[derive(Debug, Clone)]
pub struct SystemProfile {
    /// CPU architecture
    pub arch: Arch,
    /// Whether the system is 64-bit
    pub is_64bit: bool,
    /// Kernel page size (for ELF patching)
    pub page_size: u64,

    /// C library (glibc, musl, bionic)
    pub libc: LibC,
    /// Path to the dynamic linker/interpreter
    pub interpreter: PathBuf,

    /// All directories where the system searches for shared libraries
    pub lib_dirs: Vec<PathBuf>,
    /// All directories where system binaries live
    pub bin_dirs: Vec<PathBuf>,
    /// Debian-style multiarch tuple (e.g., "x86_64-linux-gnu"), if applicable
    pub multiarch_tuple: Option<String>,

    /// Filesystem layout type
    pub layout: SystemLayout,
}

impl SystemProfile {
    /// Auto-detect everything about the current system.
    /// This is the main entry point — call this once at startup.
    pub fn detect() -> Self {
        let arch = Arch::detect();
        let layout = detect::detect_layout();
        let page_size = detect::detect_page_size();

        let interpreter = interpreter::detect_interpreter()
            .unwrap_or_else(|| PathBuf::from("/lib64/ld-linux-x86-64.so.2"));

        let libc_type = libc::detect_libc(&interpreter);
        let lib_dirs = paths::discover_lib_dirs(&layout);
        let bin_dirs = paths::discover_bin_dirs(&layout);
        let multiarch_tuple = paths::detect_multiarch_tuple();

        tracing::info!("System profile: arch={}, layout={}, libc={}", arch, layout, libc_type);
        tracing::debug!("Interpreter: {}", interpreter.display());
        tracing::debug!("Page size: {}", page_size);
        tracing::debug!("Lib dirs: {} found", lib_dirs.len());
        tracing::debug!("Bin dirs: {} found", bin_dirs.len());
        if let Some(ref tuple) = multiarch_tuple {
            tracing::debug!("Multiarch tuple: {}", tuple);
        }

        SystemProfile {
            arch,
            is_64bit: arch.is_64bit(),
            page_size,
            libc: libc_type,
            interpreter,
            lib_dirs,
            bin_dirs,
            multiarch_tuple,
            layout,
        }
    }

    /// Apply user config overrides to the detected profile.
    pub fn apply_overrides(&mut self, config: &SystemConfig) {
        if let Some(ref interp) = config.interpreter {
            self.interpreter = PathBuf::from(interp);
            tracing::info!("Interpreter overridden to: {}", interp.display());
        }

        if let Some(ref layout_str) = config.layout {
            self.layout = SystemLayout::from_str(layout_str);
            tracing::info!("Layout overridden to: {}", self.layout);
        }

        // Prepend extra dirs (they take priority over auto-detected ones)
        for dir in config.extra_lib_dirs.iter().rev() {
            self.lib_dirs.insert(0, dir.clone());
        }
        for dir in config.extra_bin_dirs.iter().rev() {
            self.bin_dirs.insert(0, dir.clone());
        }
    }

    /// Check if a shared library exists anywhere on the system.
    pub fn system_lib_exists(&self, lib_name: &str) -> bool {
        self.lib_dirs.iter().any(|dir| dir.join(lib_name).exists())
    }

    /// Find the full path of a system library, or None.
    pub fn find_system_lib(&self, lib_name: &str) -> Option<PathBuf> {
        for dir in &self.lib_dirs {
            let path = dir.join(lib_name);
            if path.exists() {
                return Some(path);
            }
        }
        None
    }

    /// Get the interpreter path as a string.
    pub fn interpreter_str(&self) -> String {
        self.interpreter.to_string_lossy().into_owned()
    }
}

impl fmt::Display for SystemProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} ({}, page_size={}, {} lib dirs)",
            self.arch,
            self.libc,
            self.layout,
            self.page_size,
            self.lib_dirs.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_profile_detect() {
        let profile = SystemProfile::detect();

        // Basic sanity checks
        assert_ne!(profile.arch, Arch::Unknown);
        assert!(profile.page_size >= 4096);
        assert!(profile.interpreter.exists(), "Interpreter should exist: {:?}", profile.interpreter);
        assert!(!profile.lib_dirs.is_empty(), "Should find at least one lib dir");
        assert!(!profile.bin_dirs.is_empty(), "Should find at least one bin dir");

        println!("Profile: {}", profile);
        println!("Interpreter: {:?}", profile.interpreter);
        println!("Layout: {:?}", profile.layout);
        println!("LibC: {:?}", profile.libc);
        println!("Lib dirs ({}):", profile.lib_dirs.len());
        for d in &profile.lib_dirs {
            println!("  {}", d.display());
        }
    }

    #[test]
    fn test_system_lib_exists() {
        let profile = SystemProfile::detect();
        // libc.so.6 or libc.so should exist on any Linux system
        let has_libc = profile.system_lib_exists("libc.so.6")
            || profile.system_lib_exists("libc.so")
            || profile.system_lib_exists("ld-musl-x86_64.so.1")
            || profile.system_lib_exists("ld-musl-aarch64.so.1");
        assert!(has_libc, "Should find libc on the system");
    }
}
