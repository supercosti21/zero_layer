use std::path::Path;

use redb::{Database, ReadableTable};

use super::schema::{DEPENDENCIES, FILE_OWNERS, LIB_INDEX, PACKAGES, PINNED, PLUGIN_META};
use crate::core::graph::model::PackageNode;
use crate::error::{ZlError, ZlResult};

pub struct ZlDatabase {
    db: Database,
}

impl ZlDatabase {
    /// Open (or create) the database at the given path
    pub fn open(path: &Path) -> ZlResult<Self> {
        let db = Database::create(path)
            .map_err(|e| ZlError::Config(format!("Failed to open database: {}", e)))?;

        // Ensure all tables exist by doing an initial write txn
        let txn = db
            .begin_write()
            .map_err(|e| ZlError::Config(format!("Failed to init database: {}", e)))?;
        {
            let _ = txn.open_table(PACKAGES);
            let _ = txn.open_table(FILE_OWNERS);
            let _ = txn.open_table(LIB_INDEX);
            let _ = txn.open_table(DEPENDENCIES);
            let _ = txn.open_table(PLUGIN_META);
            let _ = txn.open_table(PINNED);
        }
        txn.commit()
            .map_err(|e| ZlError::Config(format!("Failed to commit init: {}", e)))?;

        Ok(Self { db })
    }

    // ── Package CRUD ──

    /// Insert or update a package record
    pub fn put_package(&self, node: &PackageNode) -> ZlResult<()> {
        let key = format!("{}-{}", node.id.name, node.id.version);
        let value = serde_json::to_vec(node)?;

        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        {
            let mut table = txn
                .open_table(PACKAGES)
                .map_err(|e| ZlError::Config(e.to_string()))?;
            table
                .insert(key.as_str(), value.as_slice())
                .map_err(|e| ZlError::Config(e.to_string()))?;
        }
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(())
    }

    /// Get a package by name and version
    pub fn get_package(&self, name: &str, version: &str) -> ZlResult<Option<PackageNode>> {
        let key = format!("{}-{}", name, version);
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(PACKAGES)
            .map_err(|e| ZlError::Config(e.to_string()))?;

        match table
            .get(key.as_str())
            .map_err(|e| ZlError::Config(e.to_string()))?
        {
            Some(value) => {
                let node: PackageNode = serde_json::from_slice(value.value())?;
                Ok(Some(node))
            }
            None => Ok(None),
        }
    }

    /// Get all installed versions of a package by name
    pub fn get_all_versions(&self, name: &str) -> ZlResult<Vec<PackageNode>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(PACKAGES)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let prefix = format!("{}-", name);
        let mut versions = Vec::new();

        let iter = table
            .iter()
            .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
        for entry in iter {
            let (k, v) = entry.map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
            if k.value().starts_with(&prefix) {
                let node: PackageNode = serde_json::from_slice(v.value())?;
                if node.id.name == name {
                    versions.push(node);
                }
            }
        }
        Ok(versions)
    }

    /// Get a package by name only (returns first match)
    pub fn get_package_by_name(&self, name: &str) -> ZlResult<Option<PackageNode>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(PACKAGES)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let prefix = format!("{}-", name);

        let iter = table
            .iter()
            .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
        for entry in iter {
            let (k, v) = entry.map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
            if k.value().starts_with(&prefix) {
                let node: PackageNode = serde_json::from_slice(v.value())?;
                return Ok(Some(node));
            }
        }
        Ok(None)
    }

    /// Remove a package record
    pub fn remove_package(&self, name: &str, version: &str) -> ZlResult<bool> {
        let key = format!("{}-{}", name, version);
        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let removed = {
            let mut table = txn
                .open_table(PACKAGES)
                .map_err(|e| ZlError::Config(e.to_string()))?;
            table
                .remove(key.as_str())
                .map_err(|e| ZlError::Config(e.to_string()))?
                .is_some()
        };
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(removed)
    }

    /// List all installed packages
    pub fn list_packages(&self) -> ZlResult<Vec<PackageNode>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(PACKAGES)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let mut packages = Vec::new();

        let iter = table
            .iter()
            .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
        for entry in iter {
            let (_, v) = entry.map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
            let node: PackageNode = serde_json::from_slice(v.value())?;
            packages.push(node);
        }
        Ok(packages)
    }

    // ── File ownership ──

    /// Register that a file belongs to a package
    pub fn register_file(&self, file_path: &str, package_key: &str) -> ZlResult<()> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        {
            let mut table = txn
                .open_table(FILE_OWNERS)
                .map_err(|e| ZlError::Config(e.to_string()))?;
            table
                .insert(file_path, package_key)
                .map_err(|e| ZlError::Config(e.to_string()))?;
        }
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(())
    }

    /// Look up which package owns a file
    pub fn file_owner(&self, file_path: &str) -> ZlResult<Option<String>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(FILE_OWNERS)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        match table
            .get(file_path)
            .map_err(|e| ZlError::Config(e.to_string()))?
        {
            Some(v) => Ok(Some(v.value().to_string())),
            None => Ok(None),
        }
    }

    /// Remove all file ownership entries for a package
    pub fn remove_files_for_package(&self, package_key: &str) -> ZlResult<Vec<String>> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let mut removed = Vec::new();
        {
            let mut table = txn
                .open_table(FILE_OWNERS)
                .map_err(|e| ZlError::Config(e.to_string()))?;

            // Collect keys to remove
            let keys: Vec<String> = {
                let iter = table
                    .iter()
                    .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
                let mut keys = Vec::new();
                for entry in iter {
                    let (k, v) =
                        entry.map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
                    if v.value() == package_key {
                        keys.push(k.value().to_string());
                    }
                }
                keys
            };

            for key in &keys {
                table
                    .remove(key.as_str())
                    .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
                removed.push(key.clone());
            }
        }
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(removed)
    }

    // ── Library index ──

    /// Register a shared library as provided by a package
    pub fn register_lib(&self, lib_name: &str, package_key: &str) -> ZlResult<()> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        {
            let mut table = txn
                .open_table(LIB_INDEX)
                .map_err(|e| ZlError::Config(e.to_string()))?;
            table
                .insert(lib_name, package_key)
                .map_err(|e| ZlError::Config(e.to_string()))?;
        }
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(())
    }

    /// Look up which package provides a library
    pub fn lib_provider(&self, lib_name: &str) -> ZlResult<Option<String>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(LIB_INDEX)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        match table
            .get(lib_name)
            .map_err(|e| ZlError::Config(e.to_string()))?
        {
            Some(v) => Ok(Some(v.value().to_string())),
            None => Ok(None),
        }
    }

    // ── Dependencies ──

    /// Register a dependency relationship: package_key depends on dep_name
    pub fn register_dependency(&self, package_key: &str, dep_name: &str) -> ZlResult<()> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        {
            let mut table = txn
                .open_table(DEPENDENCIES)
                .map_err(|e| ZlError::Config(e.to_string()))?;
            // Key: "package_key:dep_name", Value: serialized as empty (just tracking the edge)
            let key = format!("{}:{}", package_key, dep_name);
            table
                .insert(key.as_str(), &[] as &[u8])
                .map_err(|e| ZlError::Config(e.to_string()))?;
        }
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(())
    }

    /// Get all dependencies for a package
    pub fn get_dependencies(&self, package_key: &str) -> ZlResult<Vec<String>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(DEPENDENCIES)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let prefix = format!("{}:", package_key);
        let mut deps = Vec::new();

        let iter = table
            .iter()
            .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
        for entry in iter {
            let (k, _) = entry.map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
            let key_str = k.value();
            if key_str.starts_with(&prefix) {
                deps.push(key_str[prefix.len()..].to_string());
            }
        }
        Ok(deps)
    }

    /// Get all packages that depend on the given dependency name
    pub fn reverse_dependencies(&self, dep_name: &str) -> ZlResult<Vec<String>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(DEPENDENCIES)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let suffix = format!(":{}", dep_name);
        let mut dependents = Vec::new();

        let iter = table
            .iter()
            .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
        for entry in iter {
            let (k, _) = entry.map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
            let key_str = k.value();
            if key_str.ends_with(&suffix) {
                let pkg = &key_str[..key_str.len() - suffix.len()];
                dependents.push(pkg.to_string());
            }
        }
        Ok(dependents)
    }

    /// Remove all dependency entries for a package
    pub fn remove_dependencies(&self, package_key: &str) -> ZlResult<()> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        {
            let mut table = txn
                .open_table(DEPENDENCIES)
                .map_err(|e| ZlError::Config(e.to_string()))?;
            let prefix = format!("{}:", package_key);

            let keys: Vec<String> = {
                let iter = table
                    .iter()
                    .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
                let mut keys = Vec::new();
                for entry in iter {
                    let (k, _) =
                        entry.map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
                    if k.value().starts_with(&prefix) {
                        keys.push(k.value().to_string());
                    }
                }
                keys
            };

            for key in &keys {
                table
                    .remove(key.as_str())
                    .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
            }
        }
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(())
    }

    // ── Plugin metadata ──

    /// Store arbitrary plugin metadata (e.g. last sync timestamp)
    pub fn put_plugin_meta(&self, plugin_name: &str, data: &[u8]) -> ZlResult<()> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        {
            let mut table = txn
                .open_table(PLUGIN_META)
                .map_err(|e| ZlError::Config(e.to_string()))?;
            table
                .insert(plugin_name, data)
                .map_err(|e| ZlError::Config(e.to_string()))?;
        }
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(())
    }

    /// Read plugin metadata
    #[allow(dead_code)]
    pub fn get_plugin_meta(&self, plugin_name: &str) -> ZlResult<Option<Vec<u8>>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(PLUGIN_META)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        match table
            .get(plugin_name)
            .map_err(|e| ZlError::Config(e.to_string()))?
        {
            Some(v) => Ok(Some(v.value().to_vec())),
            None => Ok(None),
        }
    }
    // ── Package pinning ──

    /// Pin a package at its current version (prevents updates)
    pub fn pin_package(&self, name: &str, version: &str) -> ZlResult<()> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        {
            let mut table = txn
                .open_table(PINNED)
                .map_err(|e| ZlError::Config(e.to_string()))?;
            table
                .insert(name, version)
                .map_err(|e| ZlError::Config(e.to_string()))?;
        }
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(())
    }

    /// Unpin a package (allow updates again)
    pub fn unpin_package(&self, name: &str) -> ZlResult<bool> {
        let txn = self
            .db
            .begin_write()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let removed = {
            let mut table = txn
                .open_table(PINNED)
                .map_err(|e| ZlError::Config(e.to_string()))?;
            table
                .remove(name)
                .map_err(|e| ZlError::Config(e.to_string()))?
                .is_some()
        };
        txn.commit().map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(removed)
    }

    /// Check if a package is pinned
    pub fn is_pinned(&self, name: &str) -> ZlResult<bool> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(PINNED)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        Ok(table
            .get(name)
            .map_err(|e| ZlError::Config(e.to_string()))?
            .is_some())
    }

    /// List all pinned packages: returns (name, pinned_version)
    pub fn list_pinned(&self) -> ZlResult<Vec<(String, String)>> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let table = txn
            .open_table(PINNED)
            .map_err(|e| ZlError::Config(e.to_string()))?;
        let mut pinned = Vec::new();

        let iter = table
            .iter()
            .map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
        for entry in iter {
            let (k, v) = entry.map_err(|e: redb::StorageError| ZlError::Config(e.to_string()))?;
            pinned.push((k.value().to_string(), v.value().to_string()));
        }
        Ok(pinned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::model::{PackageId, PackageNode};
    use std::collections::HashMap;

    fn test_db() -> ZlDatabase {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        ZlDatabase::open(tmp.path()).unwrap()
    }

    fn make_node(name: &str, version: &str) -> PackageNode {
        PackageNode {
            id: PackageId {
                name: name.into(),
                version: version.into(),
                source: "test".into(),
            },
            installed_files: vec![],
            provides_libs: HashMap::new(),
            needs_libs: vec![],
            installed_at: 0,
            explicit: true,
        }
    }

    #[test]
    fn test_package_crud() {
        let db = test_db();
        let node = make_node("firefox", "120.0");

        db.put_package(&node).unwrap();
        let got = db.get_package("firefox", "120.0").unwrap().unwrap();
        assert_eq!(got.id.name, "firefox");

        assert!(db.remove_package("firefox", "120.0").unwrap());
        assert!(db.get_package("firefox", "120.0").unwrap().is_none());
    }

    #[test]
    fn test_file_ownership() {
        let db = test_db();
        db.register_file("/zl/bin/firefox", "firefox-120.0")
            .unwrap();

        let owner = db.file_owner("/zl/bin/firefox").unwrap().unwrap();
        assert_eq!(owner, "firefox-120.0");

        let removed = db.remove_files_for_package("firefox-120.0").unwrap();
        assert_eq!(removed.len(), 1);
        assert!(db.file_owner("/zl/bin/firefox").unwrap().is_none());
    }

    #[test]
    fn test_lib_index() {
        let db = test_db();
        db.register_lib("libssl.so.3", "openssl-3.1").unwrap();

        let provider = db.lib_provider("libssl.so.3").unwrap().unwrap();
        assert_eq!(provider, "openssl-3.1");
    }

    #[test]
    fn test_list_packages() {
        let db = test_db();
        db.put_package(&make_node("a", "1.0")).unwrap();
        db.put_package(&make_node("b", "2.0")).unwrap();

        let list = db.list_packages().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_dependency_tracking() {
        let db = test_db();
        db.register_dependency("firefox-120.0", "gtk3").unwrap();
        db.register_dependency("firefox-120.0", "dbus-glib")
            .unwrap();
        db.register_dependency("chromium-119.0", "gtk3").unwrap();

        let deps = db.get_dependencies("firefox-120.0").unwrap();
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"dbus-glib".to_string()));
        assert!(deps.contains(&"gtk3".to_string()));

        let rdeps = db.reverse_dependencies("gtk3").unwrap();
        assert_eq!(rdeps.len(), 2);
        assert!(rdeps.contains(&"firefox-120.0".to_string()));
        assert!(rdeps.contains(&"chromium-119.0".to_string()));

        db.remove_dependencies("firefox-120.0").unwrap();
        let deps = db.get_dependencies("firefox-120.0").unwrap();
        assert!(deps.is_empty());

        // chromium's dep on gtk3 should still exist
        let rdeps = db.reverse_dependencies("gtk3").unwrap();
        assert_eq!(rdeps.len(), 1);
    }

    #[test]
    fn test_pin_unpin() {
        let db = test_db();
        db.pin_package("firefox", "120.0").unwrap();
        assert!(db.is_pinned("firefox").unwrap());
        assert!(!db.is_pinned("chrome").unwrap());

        let pinned = db.list_pinned().unwrap();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0], ("firefox".to_string(), "120.0".to_string()));

        assert!(db.unpin_package("firefox").unwrap());
        assert!(!db.is_pinned("firefox").unwrap());
        assert!(db.list_pinned().unwrap().is_empty());
    }

    #[test]
    fn test_get_all_versions() {
        let db = test_db();
        db.put_package(&make_node("firefox", "120.0")).unwrap();
        db.put_package(&make_node("firefox", "121.0")).unwrap();
        db.put_package(&make_node("chrome", "119.0")).unwrap();

        let versions = db.get_all_versions("firefox").unwrap();
        assert_eq!(versions.len(), 2);
        assert!(versions.iter().any(|v| v.id.version == "120.0"));
        assert!(versions.iter().any(|v| v.id.version == "121.0"));

        let chrome = db.get_all_versions("chrome").unwrap();
        assert_eq!(chrome.len(), 1);

        let none = db.get_all_versions("nonexistent").unwrap();
        assert!(none.is_empty());
    }
}
