pub mod remapper;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::system::SystemProfile;

/// A complete mapping from source FHS paths to ZL-managed paths
#[derive(Debug, Clone)]
pub struct PathMapping {
    /// ZL root directory
    #[allow(dead_code)]
    pub zl_root: PathBuf,
    /// Package-specific install prefix
    #[allow(dead_code)]
    pub pkg_prefix: PathBuf,
    /// Shared library directory
    pub shared_lib_dir: PathBuf,
    /// Shared binary directory
    #[allow(dead_code)]
    pub shared_bin_dir: PathBuf,
    /// The system's actual ld-linux path
    pub system_interpreter: String,
    /// Map of FHS prefix -> ZL prefix for text replacement
    pub prefix_map: HashMap<String, String>,
}

impl PathMapping {
    /// Create a mapping for a specific package, using the detected system profile.
    pub fn for_package(
        zl_root: &Path,
        pkg_name: &str,
        pkg_version: &str,
        profile: &SystemProfile,
    ) -> Self {
        let pkg_prefix = zl_root
            .join("packages")
            .join(format!("{}-{}", pkg_name, pkg_version));

        let shared_lib_dir = zl_root.join("lib");
        let shared_bin_dir = zl_root.join("bin");

        let system_interpreter = profile.interpreter_str();

        // Build prefix_map dynamically from FHS source prefixes.
        // This maps all standard FHS paths that package contents might reference
        // to their ZL-managed equivalents.
        let mut prefix_map = HashMap::new();

        for (fhs_path, category) in crate::system::paths::fhs_source_prefixes() {
            let zl_target = match category {
                "lib" => shared_lib_dir.to_string_lossy().into_owned(),
                "bin" => shared_bin_dir.to_string_lossy().into_owned(),
                "share" => zl_root.join("share").to_string_lossy().into_owned(),
                "etc" => zl_root.join("etc").to_string_lossy().into_owned(),
                _ => continue,
            };
            prefix_map.insert(fhs_path, zl_target);
        }

        // If the system uses multiarch paths (Debian), add those too
        if let Some(ref tuple) = profile.multiarch_tuple {
            let multiarch_lib = format!("/usr/lib/{}", tuple);
            prefix_map.insert(multiarch_lib, shared_lib_dir.to_string_lossy().into_owned());
        }

        Self {
            zl_root: zl_root.to_path_buf(),
            pkg_prefix,
            shared_lib_dir,
            shared_bin_dir,
            system_interpreter,
            prefix_map,
        }
    }

    /// Remap an interpreter path to the system's actual interpreter
    pub fn remap_interpreter(&self, original: &str) -> Option<String> {
        if !Path::new(original).exists() {
            Some(self.system_interpreter.clone())
        } else {
            None
        }
    }

    /// Compute the RUNPATH string for an ELF binary
    pub fn compute_runpath(&self, _binary_path: &Path, _needed_libs: &[String]) -> Option<String> {
        Some(format!("$ORIGIN:{}", self.shared_lib_dir.to_string_lossy()))
    }

    /// Remap an arbitrary FHS path to its ZL equivalent
    pub fn remap_path(&self, original: &str) -> String {
        let mut prefixes: Vec<_> = self.prefix_map.iter().collect();
        prefixes.sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));

        for (from, to) in &prefixes {
            if original.starts_with(from.as_str()) {
                return original.replacen(from.as_str(), to.as_str(), 1);
            }
        }
        original.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile() -> SystemProfile {
        SystemProfile::detect()
    }

    #[test]
    fn test_path_mapping_for_package() {
        let profile = test_profile();
        let zl_root = Path::new("/tmp/test-zl");
        let mapping = PathMapping::for_package(zl_root, "firefox", "120.0", &profile);

        assert_eq!(mapping.zl_root, PathBuf::from("/tmp/test-zl"));
        assert_eq!(
            mapping.pkg_prefix,
            PathBuf::from("/tmp/test-zl/packages/firefox-120.0")
        );
        assert_eq!(mapping.shared_lib_dir, PathBuf::from("/tmp/test-zl/lib"));
        assert_eq!(mapping.shared_bin_dir, PathBuf::from("/tmp/test-zl/bin"));
        assert!(!mapping.prefix_map.is_empty());
    }

    #[test]
    fn test_remap_path_usr_lib() {
        let profile = test_profile();
        let zl_root = Path::new("/tmp/test-zl");
        let mapping = PathMapping::for_package(zl_root, "test", "1.0", &profile);

        let remapped = mapping.remap_path("/usr/lib/libfoo.so");
        assert!(
            remapped.starts_with("/tmp/test-zl/lib"),
            "Expected /usr/lib to remap to ZL lib dir, got: {}",
            remapped
        );
    }

    #[test]
    fn test_remap_path_usr_bin() {
        let profile = test_profile();
        let zl_root = Path::new("/tmp/test-zl");
        let mapping = PathMapping::for_package(zl_root, "test", "1.0", &profile);

        let remapped = mapping.remap_path("/usr/bin/myprog");
        assert!(
            remapped.starts_with("/tmp/test-zl/bin"),
            "Expected /usr/bin to remap to ZL bin dir, got: {}",
            remapped
        );
    }

    #[test]
    fn test_remap_path_unknown_stays_unchanged() {
        let profile = test_profile();
        let zl_root = Path::new("/tmp/test-zl");
        let mapping = PathMapping::for_package(zl_root, "test", "1.0", &profile);

        assert_eq!(mapping.remap_path("/opt/custom/path"), "/opt/custom/path");
    }

    #[test]
    fn test_remap_interpreter_nonexistent() {
        let profile = test_profile();
        let zl_root = Path::new("/tmp/test-zl");
        let mapping = PathMapping::for_package(zl_root, "test", "1.0", &profile);

        let result = mapping.remap_interpreter("/nonexistent/ld-linux.so.2");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), profile.interpreter_str());
    }

    #[test]
    fn test_remap_interpreter_existing() {
        let profile = test_profile();
        let zl_root = Path::new("/tmp/test-zl");
        let mapping = PathMapping::for_package(zl_root, "test", "1.0", &profile);

        let result = mapping.remap_interpreter(&profile.interpreter_str());
        assert!(result.is_none());
    }

    #[test]
    fn test_compute_runpath() {
        let profile = test_profile();
        let zl_root = Path::new("/tmp/test-zl");
        let mapping = PathMapping::for_package(zl_root, "test", "1.0", &profile);

        let runpath = mapping
            .compute_runpath(Path::new("/tmp/binary"), &[])
            .unwrap();
        assert!(runpath.contains("$ORIGIN"));
        assert!(runpath.contains("/tmp/test-zl/lib"));
    }
}
