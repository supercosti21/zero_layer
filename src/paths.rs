use std::path::{Path, PathBuf};

/// All ZL-managed paths
pub struct ZlPaths {
    /// Root of all ZL data (default: ~/.local/share/zl)
    pub root: PathBuf,
    /// Symlinked binaries (added to PATH)
    pub bin: PathBuf,
    /// Shared libraries (all packages' libs symlinked here)
    pub lib: PathBuf,
    /// Shared data files
    pub share: PathBuf,
    /// Config files
    pub etc: PathBuf,
    /// Individual package directories
    pub packages: PathBuf,
    /// Download cache
    pub cache: PathBuf,
    /// The redb database file
    pub db_file: PathBuf,
    /// Ephemeral/named environment roots
    pub envs: PathBuf,
}

impl ZlPaths {
    pub fn new(root: Option<&Path>) -> Self {
        let root = root.map(PathBuf::from).unwrap_or_else(|| {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("~/.local/share"))
                .join("zl")
        });

        Self {
            bin: root.join("bin"),
            lib: root.join("lib"),
            share: root.join("share"),
            etc: root.join("etc"),
            packages: root.join("packages"),
            cache: root.join("cache"),
            db_file: root.join("zl.redb"),
            envs: root.join("envs"),
            root,
        }
    }

    /// Create all directories if they don't exist
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [
            &self.root,
            &self.bin,
            &self.lib,
            &self.share,
            &self.etc,
            &self.packages,
            &self.cache,
            &self.envs,
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
