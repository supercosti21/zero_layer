use crate::core::db::ops::ZlDatabase;
use crate::error::{ZlError, ZlResult};

use super::{ExportArgs, ImportArgs};

/// A lockfile entry representing one installed package
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct LockEntry {
    name: String,
    version: String,
    source: String,
    explicit: bool,
}

/// The lockfile format
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Lockfile {
    /// Format version
    version: u32,
    /// All installed packages
    packages: Vec<LockEntry>,
}

pub fn handle_export(args: ExportArgs, db: &ZlDatabase) -> ZlResult<()> {
    let packages = db.list_packages()?;

    let lockfile = Lockfile {
        version: 1,
        packages: packages
            .iter()
            .map(|pkg| LockEntry {
                name: pkg.id.name.clone(),
                version: pkg.id.version.clone(),
                source: pkg.id.source.clone(),
                explicit: pkg.explicit,
            })
            .collect(),
    };

    let json = serde_json::to_string_pretty(&lockfile)?;

    let output_path = args
        .file
        .unwrap_or_else(|| std::path::PathBuf::from("zl-lock.json"));

    std::fs::write(&output_path, json)?;
    println!(
        "Exported {} package(s) to {}",
        lockfile.packages.len(),
        output_path.display()
    );

    Ok(())
}

pub fn handle_import(args: ImportArgs, db: &ZlDatabase) -> ZlResult<()> {
    let input_path = &args.file;

    if !input_path.exists() {
        return Err(ZlError::Config(format!(
            "Lockfile not found: {}",
            input_path.display()
        )));
    }

    let content = std::fs::read_to_string(input_path)?;
    let lockfile: Lockfile = serde_json::from_str(&content)?;

    if lockfile.version != 1 {
        return Err(ZlError::Config(format!(
            "Unsupported lockfile version: {}",
            lockfile.version
        )));
    }

    let installed = db.list_packages()?;
    let installed_names: std::collections::HashSet<String> =
        installed.iter().map(|p| p.id.name.clone()).collect();

    let mut to_install = Vec::new();
    let mut already = Vec::new();

    for entry in &lockfile.packages {
        if installed_names.contains(&entry.name) {
            already.push(&entry.name);
        } else {
            to_install.push(entry);
        }
    }

    if to_install.is_empty() {
        println!("All packages from lockfile are already installed.");
        if !already.is_empty() {
            println!(
                "Already installed: {}",
                already
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        return Ok(());
    }

    println!("Packages to install from lockfile:");
    for entry in &to_install {
        println!(
            "  {} {} (from {}){}",
            entry.name,
            entry.version,
            entry.source,
            if entry.explicit { "" } else { " [dep]" }
        );
    }

    println!(
        "\nTo install these packages, run:\n  {}",
        to_install
            .iter()
            .filter(|e| e.explicit)
            .map(|e| format!(
                "zl install {} --from {} --version {}",
                e.name,
                source_plugin_name(&e.source),
                e.version
            ))
            .collect::<Vec<_>>()
            .join("\n  ")
    );

    Ok(())
}

/// Extract the plugin name from a source string like "pacman/extra" -> "pacman"
fn source_plugin_name(source: &str) -> &str {
    source.split('/').next().unwrap_or(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_plugin_name() {
        assert_eq!(source_plugin_name("pacman/extra"), "pacman");
        assert_eq!(source_plugin_name("pacman/core"), "pacman");
        assert_eq!(source_plugin_name("apt"), "apt");
    }

    #[test]
    fn test_lockfile_serde() {
        let lockfile = Lockfile {
            version: 1,
            packages: vec![
                LockEntry {
                    name: "firefox".into(),
                    version: "120.0".into(),
                    source: "pacman/extra".into(),
                    explicit: true,
                },
                LockEntry {
                    name: "gtk3".into(),
                    version: "3.24".into(),
                    source: "pacman/extra".into(),
                    explicit: false,
                },
            ],
        };

        let json = serde_json::to_string_pretty(&lockfile).unwrap();
        let parsed: Lockfile = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.packages.len(), 2);
        assert_eq!(parsed.packages[0].name, "firefox");
        assert!(parsed.packages[0].explicit);
        assert!(!parsed.packages[1].explicit);
    }
}
