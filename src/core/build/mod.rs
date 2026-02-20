pub mod systems;

use std::path::{Path, PathBuf};

use crate::error::{ZlError, ZlResult};

/// Describes what build system a source package uses
#[derive(Debug, Clone, PartialEq)]
pub enum BuildSystem {
    /// ./configure && make && make install
    Autotools,
    /// cmake -B build && cmake --build build && cmake --install build
    CMake,
    /// meson setup build && ninja -C build && ninja -C build install
    Meson,
    /// cargo build --release
    Cargo,
    /// make (simple Makefile)
    Make,
    /// Custom build script
    Script { path: PathBuf },
}

/// Everything needed to build a package from source
#[derive(Debug, Clone)]
pub struct BuildSpec {
    pub name: String,
    pub version: String,
    pub source_dir: PathBuf,
    pub build_system: BuildSystem,
    pub configure_flags: Vec<String>,
    pub environment: Vec<(String, String)>,
}

/// Result of a successful build
pub struct BuildResult {
    /// Directory containing the installed files (the --prefix destination)
    pub install_dir: tempfile::TempDir,
    /// List of all files that were installed
    pub files: Vec<PathBuf>,
}

/// Detect which build system a source directory uses
pub fn detect_build_system(source_dir: &Path) -> Option<BuildSystem> {
    // Check in order of specificity
    if source_dir.join("CMakeLists.txt").exists() {
        return Some(BuildSystem::CMake);
    }
    if source_dir.join("meson.build").exists() {
        return Some(BuildSystem::Meson);
    }
    if source_dir.join("Cargo.toml").exists() {
        return Some(BuildSystem::Cargo);
    }
    if source_dir.join("configure").exists() || source_dir.join("configure.ac").exists() {
        return Some(BuildSystem::Autotools);
    }
    if source_dir.join("Makefile").exists() || source_dir.join("makefile").exists() {
        return Some(BuildSystem::Make);
    }
    if source_dir.join("build.sh").exists() {
        return Some(BuildSystem::Script {
            path: source_dir.join("build.sh"),
        });
    }

    None
}

/// Check if required build tools are available on the system
pub fn check_build_tools(build_system: &BuildSystem) -> ZlResult<()> {
    let required = match build_system {
        BuildSystem::Autotools => vec!["make", "gcc"],
        BuildSystem::CMake => vec!["cmake", "make", "gcc"],
        BuildSystem::Meson => vec!["meson", "ninja", "gcc"],
        BuildSystem::Cargo => vec!["cargo", "rustc"],
        BuildSystem::Make => vec!["make", "gcc"],
        BuildSystem::Script { .. } => vec!["bash"],
    };

    for tool in required {
        if !tool_exists(tool) {
            return Err(ZlError::BuildToolMissing {
                tool: tool.to_string(),
            });
        }
    }

    Ok(())
}

/// Build a package from source
pub fn build_package(spec: &BuildSpec, prefix: &Path) -> ZlResult<BuildResult> {
    check_build_tools(&spec.build_system)?;

    let install_dir = tempfile::tempdir()?;
    let dest_prefix = install_dir
        .path()
        .join(prefix.strip_prefix("/").unwrap_or(prefix));
    std::fs::create_dir_all(&dest_prefix)?;

    // Set up environment
    let mut env: Vec<(String, String)> = spec.environment.clone();
    env.push(("PREFIX".into(), prefix.to_string_lossy().into()));
    env.push((
        "DESTDIR".into(),
        install_dir.path().to_string_lossy().into(),
    ));

    // Run the build
    match &spec.build_system {
        BuildSystem::Autotools => {
            systems::build_autotools(&spec.source_dir, &dest_prefix, &spec.configure_flags, &env)?;
        }
        BuildSystem::CMake => {
            systems::build_cmake(&spec.source_dir, &dest_prefix, &spec.configure_flags, &env)?;
        }
        BuildSystem::Meson => {
            systems::build_meson(&spec.source_dir, &dest_prefix, &spec.configure_flags, &env)?;
        }
        BuildSystem::Cargo => {
            systems::build_cargo(&spec.source_dir, &dest_prefix, &env)?;
        }
        BuildSystem::Make => {
            systems::build_make(&spec.source_dir, &dest_prefix, &env)?;
        }
        BuildSystem::Script { path } => {
            systems::build_script(path, &spec.source_dir, &dest_prefix, &env)?;
        }
    }

    // Collect all installed files
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(install_dir.path())
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            files.push(entry.path().to_path_buf());
        }
    }

    Ok(BuildResult { install_dir, files })
}

/// Check if a command-line tool exists in PATH
fn tool_exists(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_build_system_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_build_system(dir.path()), None);
    }

    #[test]
    fn test_detect_build_system_cmake() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("CMakeLists.txt"), "").unwrap();
        assert_eq!(detect_build_system(dir.path()), Some(BuildSystem::CMake));
    }

    #[test]
    fn test_detect_build_system_autotools() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("configure"), "").unwrap();
        assert_eq!(
            detect_build_system(dir.path()),
            Some(BuildSystem::Autotools)
        );
    }

    #[test]
    fn test_detect_build_system_cargo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(detect_build_system(dir.path()), Some(BuildSystem::Cargo));
    }

    #[test]
    fn test_detect_build_system_meson() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("meson.build"), "").unwrap();
        assert_eq!(detect_build_system(dir.path()), Some(BuildSystem::Meson));
    }
}
