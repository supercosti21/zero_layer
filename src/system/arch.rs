use std::fmt;

/// CPU architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Arch {
    X86_64,
    Aarch64,
    Armv7,
    Riscv64,
    I686,
    S390x,
    Ppc64le,
    Mips64,
    Unknown,
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Armv7 => "armv7",
            Arch::Riscv64 => "riscv64",
            Arch::I686 => "i686",
            Arch::S390x => "s390x",
            Arch::Ppc64le => "ppc64le",
            Arch::Mips64 => "mips64",
            Arch::Unknown => "unknown",
        };
        write!(f, "{}", s)
    }
}

impl Arch {
    /// Detect the current system architecture.
    /// Uses `std::env::consts::ARCH` (compile-time) as primary, which is always correct
    /// for the running binary. Falls back to parsing uname-style strings.
    pub fn detect() -> Self {
        Self::from_str(std::env::consts::ARCH)
    }

    /// Parse an architecture string (as returned by `uname -m` or similar).
    pub fn from_str(s: &str) -> Self {
        match s {
            "x86_64" | "amd64" => Arch::X86_64,
            "aarch64" | "arm64" => Arch::Aarch64,
            "armv7l" | "armv7" | "armhf" | "arm" => Arch::Armv7,
            "riscv64" | "riscv64gc" => Arch::Riscv64,
            "i686" | "i386" | "i586" | "x86" => Arch::I686,
            "s390x" => Arch::S390x,
            "ppc64le" | "powerpc64le" => Arch::Ppc64le,
            "mips64" | "mips64el" => Arch::Mips64,
            _ => Arch::Unknown,
        }
    }

    /// Whether this architecture is 64-bit.
    pub fn is_64bit(&self) -> bool {
        matches!(
            self,
            Arch::X86_64 | Arch::Aarch64 | Arch::Riscv64 | Arch::S390x | Arch::Ppc64le | Arch::Mips64
        )
    }

    /// The Pacman repo name for this architecture.
    pub fn pacman_name(&self) -> &str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Armv7 => "armv7h",
            Arch::I686 => "i686",
            _ => "any",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arch_detect() {
        let arch = Arch::detect();
        // We should always detect *something* on a real system
        assert_ne!(arch, Arch::Unknown);
    }

    #[test]
    fn test_arch_from_str() {
        assert_eq!(Arch::from_str("x86_64"), Arch::X86_64);
        assert_eq!(Arch::from_str("amd64"), Arch::X86_64);
        assert_eq!(Arch::from_str("aarch64"), Arch::Aarch64);
        assert_eq!(Arch::from_str("arm64"), Arch::Aarch64);
        assert_eq!(Arch::from_str("armv7l"), Arch::Armv7);
        assert_eq!(Arch::from_str("riscv64"), Arch::Riscv64);
        assert_eq!(Arch::from_str("i686"), Arch::I686);
        assert_eq!(Arch::from_str("s390x"), Arch::S390x);
    }

    #[test]
    fn test_arch_64bit() {
        assert!(Arch::X86_64.is_64bit());
        assert!(Arch::Aarch64.is_64bit());
        assert!(!Arch::Armv7.is_64bit());
        assert!(!Arch::I686.is_64bit());
    }
}
