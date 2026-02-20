use std::fmt;
use std::path::Path;

/// The C library used by the host system.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LibC {
    Glibc { version: Option<String> },
    Musl { version: Option<String> },
    Bionic,
    Unknown,
}

impl fmt::Display for LibC {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibC::Glibc { version: Some(v) } => write!(f, "glibc {}", v),
            LibC::Glibc { version: None } => write!(f, "glibc"),
            LibC::Musl { version: Some(v) } => write!(f, "musl {}", v),
            LibC::Musl { version: None } => write!(f, "musl"),
            LibC::Bionic => write!(f, "bionic"),
            LibC::Unknown => write!(f, "unknown"),
        }
    }
}

/// Detect the C library from the interpreter path.
///
/// The interpreter name encodes the libc:
/// - `ld-linux-*` or `ld64.so.*` → glibc
/// - `ld-musl-*` → musl
/// - `linker64` or `ld-android*` → bionic (Android/Termux)
pub fn detect_libc(interpreter: &Path) -> LibC {
    let interp_str = interpreter.to_string_lossy();
    let filename = interpreter
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    if filename.contains("ld-musl") {
        let version = detect_musl_version(&interp_str);
        LibC::Musl { version }
    } else if filename.contains("ld-linux")
        || filename.starts_with("ld64.so")
        || filename.starts_with("ld.so")
    {
        let version = detect_glibc_version();
        LibC::Glibc { version }
    } else if filename.contains("linker") || interp_str.contains("android") {
        LibC::Bionic
    } else {
        LibC::Unknown
    }
}

/// Try to get glibc version by running the interpreter itself.
/// The dynamic linker prints version info when invoked with no args or --version.
fn detect_glibc_version() -> Option<String> {
    // Try `ldd --version` which is the most portable way
    let output = std::process::Command::new("ldd")
        .arg("--version")
        .output()
        .ok()?;

    let text = String::from_utf8_lossy(&output.stdout);
    // Also check stderr — some implementations print there
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", text, stderr);

    // Look for version pattern like "2.38" or "2.39"
    for line in combined.lines() {
        if let Some(ver) = extract_glibc_version(line) {
            return Some(ver);
        }
    }
    None
}

fn extract_glibc_version(line: &str) -> Option<String> {
    // Patterns: "GLIBC 2.38", "GNU C Library ... 2.38", "ldd (GNU libc) 2.38"
    let lower = line.to_lowercase();
    if lower.contains("glibc") || lower.contains("gnu") {
        // Find version-like pattern: digits.digits
        for word in line.split_whitespace().rev() {
            let trimmed = word.trim_end_matches(&['.', ',', ')', ';'] as &[char]);
            if trimmed.contains('.')
                && trimmed.chars().next().map(|c| c.is_ascii_digit()) == Some(true)
            {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Try to get musl version. Musl's linker prints version info when run directly.
fn detect_musl_version(interp_path: &str) -> Option<String> {
    let output = std::process::Command::new(interp_path)
        .output()
        .ok()?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    for line in stderr.lines() {
        let lower = line.to_lowercase();
        if lower.contains("musl") && lower.contains("version") {
            // Extract version like "1.2.4"
            for word in line.split_whitespace() {
                if word.contains('.')
                    && word.chars().next().map(|c| c.is_ascii_digit()) == Some(true)
                {
                    return Some(word.to_string());
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_detect_libc_from_interpreter_name() {
        assert!(matches!(
            detect_libc(&PathBuf::from("/lib64/ld-linux-x86-64.so.2")),
            LibC::Glibc { .. }
        ));
        assert!(matches!(
            detect_libc(&PathBuf::from("/lib/ld-musl-x86_64.so.1")),
            LibC::Musl { .. }
        ));
        assert!(matches!(
            detect_libc(&PathBuf::from("/lib/ld-linux-aarch64.so.1")),
            LibC::Glibc { .. }
        ));
        assert!(matches!(
            detect_libc(&PathBuf::from("/lib/ld-musl-aarch64.so.1")),
            LibC::Musl { .. }
        ));
    }

    #[test]
    fn test_extract_glibc_version() {
        assert_eq!(
            extract_glibc_version("ldd (GNU libc) 2.38"),
            Some("2.38".to_string())
        );
        assert_eq!(
            extract_glibc_version("GNU C Library (GNU libc) stable release version 2.39."),
            Some("2.39".to_string())
        );
        assert_eq!(extract_glibc_version("random line"), None);
    }
}
