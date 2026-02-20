use std::fmt;
use std::path::Path;

/// The filesystem layout of the host system.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SystemLayout {
    /// Standard FHS with separate /bin, /lib, /usr/bin, /usr/lib
    FHS,
    /// Merged /usr: /bin → /usr/bin, /lib → /usr/lib (most modern distros)
    MergedUsr,
    /// NixOS: everything in /nix/store, declarative system
    NixOS,
    /// GNU Guix: everything in /gnu/store
    Guix,
    /// Termux on Android: prefix at /data/data/com.termux/files/usr
    Termux,
    /// GoboLinux: /Programs/Name/Version/
    GoboLinux,
    /// Unknown or custom layout
    Custom,
}

impl fmt::Display for SystemLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SystemLayout::FHS => "FHS",
            SystemLayout::MergedUsr => "Merged /usr",
            SystemLayout::NixOS => "NixOS",
            SystemLayout::Guix => "GNU Guix",
            SystemLayout::Termux => "Termux",
            SystemLayout::GoboLinux => "GoboLinux",
            SystemLayout::Custom => "Custom",
        };
        write!(f, "{}", s)
    }
}

impl SystemLayout {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "fhs" => SystemLayout::FHS,
            "merged" | "merged_usr" | "mergedusr" => SystemLayout::MergedUsr,
            "nixos" | "nix" => SystemLayout::NixOS,
            "guix" => SystemLayout::Guix,
            "termux" => SystemLayout::Termux,
            "gobo" | "gobolinux" => SystemLayout::GoboLinux,
            "custom" => SystemLayout::Custom,
            _ => SystemLayout::Custom,
        }
    }
}

/// Detect the filesystem layout of the current system.
pub fn detect_layout() -> SystemLayout {
    // NixOS: /nix/store exists and /etc/NIXOS marker
    if Path::new("/nix/store").is_dir() && Path::new("/etc/NIXOS").exists() {
        return SystemLayout::NixOS;
    }

    // GNU Guix: /gnu/store exists
    if Path::new("/gnu/store").is_dir() {
        return SystemLayout::Guix;
    }

    // Termux: PREFIX env or the characteristic path
    if std::env::var("PREFIX")
        .map(|p| p.contains("com.termux"))
        .unwrap_or(false)
        || Path::new("/data/data/com.termux").is_dir()
    {
        return SystemLayout::Termux;
    }

    // GoboLinux: /Programs directory
    if Path::new("/Programs").is_dir() && Path::new("/System").is_dir() {
        return SystemLayout::GoboLinux;
    }

    // Merged /usr: /bin is a symlink to /usr/bin
    if is_symlink_to("/bin", "/usr/bin") || is_symlink_to("/lib", "/usr/lib") {
        return SystemLayout::MergedUsr;
    }

    // Default: standard FHS
    SystemLayout::FHS
}

/// Check if `link` is a symlink pointing to `target` (directly or via canonical path).
fn is_symlink_to(link: &str, target: &str) -> bool {
    let link_path = Path::new(link);
    if !link_path.is_symlink() {
        return false;
    }
    match std::fs::read_link(link_path) {
        Ok(dest) => {
            let dest_str = dest.to_string_lossy();
            dest_str == target
                || dest_str.ends_with(target)
                || std::fs::canonicalize(link_path)
                    .ok()
                    .and_then(|c| std::fs::canonicalize(target).ok().map(|t| c == t))
                    .unwrap_or(false)
        }
        Err(_) => false,
    }
}

/// Detect the kernel page size via libc sysconf.
pub fn detect_page_size() -> u64 {
    // SAFETY: sysconf(_SC_PAGESIZE) is always safe and returns the page size.
    #[cfg(target_os = "linux")]
    {
        let ps = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if ps > 0 {
            return ps as u64;
        }
    }

    // Fallback for non-Linux or failed sysconf (shouldn't happen)
    4096
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_layout() {
        let layout = detect_layout();
        // Should always return a valid layout
        println!("Detected layout: {}", layout);
        // Just ensure it doesn't panic
    }

    #[test]
    fn test_detect_page_size() {
        let ps = detect_page_size();
        // Page size should be a power of 2, at least 4096
        assert!(ps >= 4096, "Page size should be >= 4096, got {}", ps);
        assert!(ps.is_power_of_two(), "Page size should be power of 2, got {}", ps);
    }

    #[test]
    fn test_layout_from_str() {
        assert_eq!(SystemLayout::from_str("fhs"), SystemLayout::FHS);
        assert_eq!(SystemLayout::from_str("nixos"), SystemLayout::NixOS);
        assert_eq!(SystemLayout::from_str("merged"), SystemLayout::MergedUsr);
        assert_eq!(SystemLayout::from_str("termux"), SystemLayout::Termux);
    }
}
