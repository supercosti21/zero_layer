use std::path::{Path, PathBuf};

use tracing::{error, warn};

use crate::core::db::ops::ZlDatabase;

/// Tracks all filesystem and database changes made during an install operation.
/// If the install fails, `rollback()` undoes everything in reverse order.
/// On success, call `commit()` to consume the transaction without rolling back.
///
/// If a `Transaction` is dropped without calling `commit()`, the `Drop` impl
/// will log a warning. It does **not** auto-rollback because it doesn't have a
/// reference to the database — call `rollback()` explicitly when you have one.
#[derive(Default)]
pub struct Transaction {
    created_files: Vec<PathBuf>,
    created_dirs: Vec<PathBuf>,
    created_symlinks: Vec<PathBuf>,
    db_package_keys: Vec<String>,
    committed: bool,
}

impl Transaction {
    /// Create a new, empty transaction.
    pub fn new() -> Self {
        Self::default()
    }

    // ── Tracking methods ──

    /// Record that a regular file was created during this transaction.
    pub fn track_file(&mut self, path: &Path) {
        self.created_files.push(path.to_path_buf());
    }

    /// Record that a directory was created during this transaction.
    pub fn track_dir(&mut self, path: &Path) {
        self.created_dirs.push(path.to_path_buf());
    }

    /// Record that a symlink was created during this transaction.
    pub fn track_symlink(&mut self, path: &Path) {
        self.created_symlinks.push(path.to_path_buf());
    }

    /// Record that a package was inserted into the database (key = "name-version").
    pub fn track_db_package(&mut self, key: &str) {
        self.db_package_keys.push(key.to_string());
    }

    // ── Completion methods ──

    /// Mark the transaction as successfully committed.
    /// This is a no-op in terms of side effects — all filesystem/DB writes
    /// already happened — it simply prevents the destructor warning and blocks
    /// any future rollback.
    pub fn commit(mut self) {
        self.committed = true;
    }

    /// Undo every tracked change in reverse order:
    /// 1. Remove DB package entries (and their associated file-ownership + dep records)
    /// 2. Remove symlinks
    /// 3. Remove files
    /// 4. Remove directories (only empty ones, deepest first)
    pub fn rollback(mut self, db: &ZlDatabase) {
        self.committed = true; // prevent Drop warning — we are handling it

        // 1. Remove database entries in reverse
        for key in self.db_package_keys.iter().rev() {
            // key is "name-version"; we need name and version separately
            if let Some((name, version)) = split_package_key(key) {
                if let Err(e) = db.remove_package(name, version) {
                    error!("rollback: failed to remove package {key} from DB: {e}");
                }
                if let Err(e) = db.remove_files_for_package(key) {
                    error!("rollback: failed to remove file-ownership entries for {key}: {e}");
                }
                if let Err(e) = db.remove_dependencies(key) {
                    error!("rollback: failed to remove dependency entries for {key}: {e}");
                }
            } else {
                warn!("rollback: could not parse package key '{key}' into name-version");
            }
        }

        // 2. Remove symlinks in reverse
        for path in self.created_symlinks.iter().rev() {
            if path.symlink_metadata().is_ok()
                && let Err(e) = std::fs::remove_file(path)
            {
                error!("rollback: failed to remove symlink {}: {e}", path.display());
            }
        }

        // 3. Remove files in reverse
        for path in self.created_files.iter().rev() {
            if path.exists()
                && let Err(e) = std::fs::remove_file(path)
            {
                error!("rollback: failed to remove file {}: {e}", path.display());
            }
        }

        // 4. Remove directories in reverse (deepest first, only if empty)
        for path in self.created_dirs.iter().rev() {
            if path.is_dir() {
                // std::fs::remove_dir only removes empty directories
                if let Err(e) = std::fs::remove_dir(path) {
                    warn!(
                        "rollback: could not remove dir {} (may not be empty): {e}",
                        path.display()
                    );
                }
            }
        }
    }

    // ── Accessors (useful for tests and diagnostics) ──

    #[cfg(test)]
    pub fn created_files(&self) -> &[PathBuf] {
        &self.created_files
    }

    #[cfg(test)]
    pub fn created_dirs(&self) -> &[PathBuf] {
        &self.created_dirs
    }

    #[cfg(test)]
    pub fn created_symlinks(&self) -> &[PathBuf] {
        &self.created_symlinks
    }

    #[cfg(test)]
    pub fn db_package_keys(&self) -> &[String] {
        &self.db_package_keys
    }

    #[cfg(test)]
    pub fn is_committed(&self) -> bool {
        self.committed
    }
}

impl Drop for Transaction {
    fn drop(&mut self) {
        if !self.committed {
            warn!(
                "Transaction dropped without commit! \
                 {} files, {} dirs, {} symlinks, {} DB entries were NOT rolled back. \
                 Call rollback() explicitly before dropping.",
                self.created_files.len(),
                self.created_dirs.len(),
                self.created_symlinks.len(),
                self.db_package_keys.len(),
            );
        }
    }
}

/// Split a package key like "firefox-120.0" into ("firefox", "120.0").
/// Splits on the *last* hyphen so names with hyphens (e.g. "dbus-glib-0.3") work correctly.
fn split_package_key(key: &str) -> Option<(&str, &str)> {
    let pos = key.rfind('-')?;
    if pos == 0 || pos == key.len() - 1 {
        return None;
    }
    Some((&key[..pos], &key[pos + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_commit_prevents_drop_warning() {
        let txn = Transaction::new();
        assert!(!txn.is_committed());
        txn.commit();
        // No warning emitted — committed flag is true
    }

    #[test]
    fn test_track_file() {
        let mut txn = Transaction::new();
        txn.track_file(Path::new("/tmp/test_file.txt"));
        assert_eq!(txn.created_files().len(), 1);
        assert_eq!(txn.created_files()[0], PathBuf::from("/tmp/test_file.txt"));
        txn.commit();
    }

    #[test]
    fn test_track_dir() {
        let mut txn = Transaction::new();
        txn.track_dir(Path::new("/tmp/test_dir"));
        assert_eq!(txn.created_dirs().len(), 1);
        txn.commit();
    }

    #[test]
    fn test_track_symlink() {
        let mut txn = Transaction::new();
        txn.track_symlink(Path::new("/tmp/test_link"));
        assert_eq!(txn.created_symlinks().len(), 1);
        txn.commit();
    }

    #[test]
    fn test_track_db_package() {
        let mut txn = Transaction::new();
        txn.track_db_package("firefox-120.0");
        txn.track_db_package("gtk3-3.24");
        assert_eq!(txn.db_package_keys().len(), 2);
        assert_eq!(txn.db_package_keys()[0], "firefox-120.0");
        txn.commit();
    }

    #[test]
    fn test_split_package_key() {
        assert_eq!(
            split_package_key("firefox-120.0"),
            Some(("firefox", "120.0"))
        );
        assert_eq!(
            split_package_key("dbus-glib-0.3"),
            Some(("dbus-glib", "0.3"))
        );
        assert_eq!(split_package_key("noversion"), None);
        assert_eq!(split_package_key("-bad"), None);
        assert_eq!(split_package_key("bad-"), None);
    }

    #[test]
    fn test_rollback_cleans_filesystem() {
        let tmp = TempDir::new().unwrap();

        // Create a directory inside the temp dir
        let dir_path = tmp.path().join("pkg_dir");
        std::fs::create_dir(&dir_path).unwrap();

        // Create a file inside the directory
        let file_path = dir_path.join("data.txt");
        std::fs::write(&file_path, "hello").unwrap();

        // Create a symlink
        let link_path = tmp.path().join("link");
        std::os::unix::fs::symlink(&file_path, &link_path).unwrap();

        // Build a transaction tracking these
        let mut txn = Transaction::new();
        txn.track_dir(&dir_path);
        txn.track_file(&file_path);
        txn.track_symlink(&link_path);

        // We need a real DB for rollback, create a temp one
        let db_file = tempfile::NamedTempFile::new().unwrap();
        let db = ZlDatabase::open(db_file.path()).unwrap();

        txn.rollback(&db);

        // Symlink and file should be gone
        assert!(!link_path.exists());
        assert!(!file_path.exists());
        // Directory should be removed (it is now empty)
        assert!(!dir_path.exists());
    }

    #[test]
    fn test_rollback_removes_db_entries() {
        use crate::core::graph::model::{PackageId, PackageNode};
        use std::collections::HashMap;

        let db_file = tempfile::NamedTempFile::new().unwrap();
        let db = ZlDatabase::open(db_file.path()).unwrap();

        let node = PackageNode {
            id: PackageId {
                name: "testpkg".into(),
                version: "1.0".into(),
                source: "test".into(),
            },
            installed_files: vec![],
            provides_libs: HashMap::new(),
            needs_libs: vec![],
            installed_at: 0,
            explicit: true,
        };
        db.put_package(&node).unwrap();
        db.register_file("/zl/bin/testpkg", "testpkg-1.0").unwrap();
        db.register_dependency("testpkg-1.0", "glibc").unwrap();

        // Verify the entries exist
        assert!(db.get_package("testpkg", "1.0").unwrap().is_some());
        assert!(db.file_owner("/zl/bin/testpkg").unwrap().is_some());

        let mut txn = Transaction::new();
        txn.track_db_package("testpkg-1.0");
        txn.rollback(&db);

        // Everything should be cleaned up
        assert!(db.get_package("testpkg", "1.0").unwrap().is_none());
        assert!(db.file_owner("/zl/bin/testpkg").unwrap().is_none());
        assert!(db.get_dependencies("testpkg-1.0").unwrap().is_empty());
    }
}
