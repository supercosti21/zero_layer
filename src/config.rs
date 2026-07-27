use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::ZlResult;

/// Top-level ZL configuration (~/.config/zl/config.toml)
#[derive(Debug, Deserialize, Serialize, Default)]
pub struct ZlConfig {
    /// Global settings
    #[serde(default)]
    pub general: GeneralConfig,
    /// System overrides (interpreter, extra paths, layout)
    #[serde(default)]
    pub system: SystemConfig,
    /// Per-plugin configuration sections
    #[serde(default)]
    pub plugins: HashMap<String, PluginConfig>,
}

/// User overrides for auto-detected system profile
#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct SystemConfig {
    /// Override the auto-detected dynamic linker path
    pub interpreter: Option<PathBuf>,
    /// Extra library search directories (prepended to auto-detected list)
    #[serde(default)]
    pub extra_lib_dirs: Vec<PathBuf>,
    /// Extra binary search directories (prepended to auto-detected list)
    #[serde(default)]
    pub extra_bin_dirs: Vec<PathBuf>,
    /// Override auto-detected layout (fhs, merged, nixos, guix, termux, custom)
    pub layout: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct GeneralConfig {
    /// Override ZL root directory
    pub root: Option<PathBuf>,
    /// Whether to auto-confirm prompts
    #[serde(default)]
    pub auto_confirm: bool,
    /// Enabled sources whitelist. If set, only these plugins are loaded.
    /// If None or empty, all plugins are loaded.
    #[serde(default)]
    pub sources: Option<Vec<String>>,
}

/// Configuration for a single plugin
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PluginConfig {
    /// Whether this plugin is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Cache directory for this plugin (set at runtime)
    #[serde(skip)]
    pub cache_dir: PathBuf,
    /// Plugin-specific extra settings
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

/// A plugin with no `[plugins.<name>]` table is enabled. `#[serde(default)]`
/// only covers a table that exists but omits the key, so the derived `Default`
/// (`enabled: false`) would silently disable every unconfigured plugin.
impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cache_dir: PathBuf::new(),
            extra: HashMap::new(),
        }
    }
}

fn default_true() -> bool {
    true
}

impl ZlConfig {
    /// Load config from the default location or return defaults
    pub fn load() -> ZlResult<Self> {
        let config_path = Self::default_path();
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            toml::from_str(&content).map_err(|e| crate::error::ZlError::Config(e.to_string()))
        } else {
            Ok(Self::default())
        }
    }

    /// Load config from a specific path
    #[allow(dead_code)]
    pub fn load_from(path: &Path) -> ZlResult<Self> {
        let content = std::fs::read_to_string(path)?;
        toml::from_str(&content).map_err(|e| crate::error::ZlError::Config(e.to_string()))
    }

    /// Return the default config file path
    pub fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("zl")
            .join("config.toml")
    }

    /// Get plugin config, returning default if not configured
    pub fn plugin_config(&self, name: &str) -> PluginConfig {
        self.plugins.get(name).cloned().unwrap_or_default()
    }

    /// Save config to the default path
    pub fn save(&self) -> ZlResult<()> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| crate::error::ZlError::Config(e.to_string()))?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Returns the list of enabled sources, or None if all should be used
    pub fn enabled_sources(&self) -> Option<&[String]> {
        self.general
            .sources
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(|s| s.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unconfigured_plugin_is_enabled() {
        // Regression: the derived Default gave `enabled: false`, which disabled
        // every plugin that had no explicit [plugins.<name>] table.
        let config = ZlConfig::default();
        assert!(config.plugin_config("github").enabled);
        assert!(config.plugin_config("pacman").enabled);
    }

    #[test]
    fn test_plugin_table_without_enabled_key_defaults_to_enabled() {
        let config: ZlConfig = toml::from_str("[plugins.github]\n").unwrap();
        assert!(config.plugin_config("github").enabled);
    }

    #[test]
    fn test_plugin_can_be_disabled_explicitly() {
        let config: ZlConfig = toml::from_str("[plugins.github]\nenabled = false\n").unwrap();
        assert!(!config.plugin_config("github").enabled);
        // Other plugins are unaffected
        assert!(config.plugin_config("apt").enabled);
    }

    #[test]
    fn test_enabled_sources_none_when_unset_or_empty() {
        assert!(ZlConfig::default().enabled_sources().is_none());

        let empty: ZlConfig = toml::from_str("[general]\nsources = []\n").unwrap();
        assert!(empty.enabled_sources().is_none());

        let filtered: ZlConfig = toml::from_str("[general]\nsources = [\"github\"]\n").unwrap();
        assert_eq!(
            filtered.enabled_sources(),
            Some(&["github".to_string()][..])
        );
    }
}
