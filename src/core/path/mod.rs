pub mod fhs;
pub mod remapper;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A complete mapping from source FHS paths to ZL-managed paths
#[derive(Debug, Clone)]
pub struct PathMapping {
    /// ZL root directory
    pub zl_root: PathBuf,
    /// Package-specific install prefix
    pub pkg_prefix: PathBuf,
    /// Shared library directory
    pub shared_lib_dir: PathBuf,
    /// Shared binary directory
    pub shared_bin_dir: PathBuf,
    /// The system's actual ld-linux path
    pub system_interpreter: String,
    /// Map of FHS prefix -> ZL prefix for text replacement
    pub prefix_map: HashMap<String, String>,
}

impl PathMapping {
    /// Create a mapping for a specific package
    pub fn for_package(zl_root: &Path, pkg_name: &str, pkg_version: &str) -> Self {
        let pkg_prefix = zl_root
            .join("packages")
            .join(format!("{}-{}", pkg_name, pkg_version));

        let shared_lib_dir = zl_root.join("lib");
        let shared_bin_dir = zl_root.join("bin");

        let system_interpreter = fhs::detect_system_interpreter();

        let mut prefix_map = HashMap::new();
        prefix_map.insert(
            "/usr/lib".into(),
            shared_lib_dir.to_string_lossy().into_owned(),
        );
        prefix_map.insert(
            "/usr/lib64".into(),
            shared_lib_dir.to_string_lossy().into_owned(),
        );
        prefix_map.insert(
            "/usr/bin".into(),
            shared_bin_dir.to_string_lossy().into_owned(),
        );
        prefix_map.insert(
            "/usr/sbin".into(),
            shared_bin_dir.to_string_lossy().into_owned(),
        );
        prefix_map.insert(
            "/usr/share".into(),
            zl_root.join("share").to_string_lossy().into_owned(),
        );
        prefix_map.insert(
            "/etc".into(),
            zl_root.join("etc").to_string_lossy().into_owned(),
        );

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
        let mut paths = Vec::new();
        paths.push("$ORIGIN".to_string());
        paths.push(self.shared_lib_dir.to_string_lossy().to_string());
        Some(paths.join(":"))
    }

    /// Remap an arbitrary FHS path to its ZL equivalent
    pub fn remap_path(&self, original: &str) -> String {
        let mut prefixes: Vec<_> = self.prefix_map.iter().collect();
        prefixes.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        for (from, to) in &prefixes {
            if original.starts_with(from.as_str()) {
                return original.replacen(from.as_str(), to.as_str(), 1);
            }
        }
        original.to_string()
    }
}
