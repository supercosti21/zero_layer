use std::path::Path;

/// Standard FHS paths that packages commonly assume
pub const FHS_LIB_DIRS: &[&str] = &["/usr/lib", "/usr/lib64", "/lib", "/lib64"];
pub const FHS_BIN_DIRS: &[&str] = &["/usr/bin", "/usr/sbin", "/bin", "/sbin"];
pub const FHS_SHARE_DIR: &str = "/usr/share";
pub const FHS_ETC_DIR: &str = "/etc";

/// Common interpreter paths across distributions
const INTERPRETER_CANDIDATES: &[&str] = &[
    "/lib64/ld-linux-x86-64.so.2",
    "/lib/ld-linux-x86-64.so.2",
    "/usr/lib64/ld-linux-x86-64.so.2",
    "/usr/lib/ld-linux-x86-64.so.2",
];

/// Detect the system's actual dynamic linker path
pub fn detect_system_interpreter() -> String {
    for path in INTERPRETER_CANDIDATES {
        if Path::new(path).exists() {
            return path.to_string();
        }
    }
    // Fallback
    "/lib64/ld-linux-x86-64.so.2".to_string()
}
