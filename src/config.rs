use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::ZlResult;

/// Top-level ZL configuration (~/.config/zl/config.toml)
#[derive(Debug, Deserialize, Default)]
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
#[derive(Debug, Deserialize, Default, Clone)]
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

#[derive(Debug, Deserialize, Default)]
pub struct GeneralConfig {
    /// Override ZL root directory
    pub root: Option<PathBuf>,
    /// Whether to auto-confirm prompts
    #[serde(default)]
    pub auto_confirm: bool,
}

/// Configuration for a single plugin
#[derive(Debug, Deserialize, Default, Clone)]
pub struct PluginConfig {
    /// Whether this plugin is enabled
    #[serde(default = "default_true")]
    #[allow(dead_code)]
    pub enabled: bool,
    /// Cache directory for this plugin (set at runtime)
    #[serde(skip)]
    pub cache_dir: PathBuf,
    /// Plugin-specific extra settings
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
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

    fn default_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("zl")
            .join("config.toml")
    }

    /// Get plugin config, returning default if not configured
    pub fn plugin_config(&self, name: &str) -> PluginConfig {
        self.plugins.get(name).cloned().unwrap_or_default()
    }
}
