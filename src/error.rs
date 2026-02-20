use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ZlError {
    #[error("ELF analysis failed for {path}: {source}")]
    ElfAnalysis {
        path: PathBuf,
        source: goblin::error::Error,
    },

    #[error("ELF patching failed for {path}: {message}")]
    ElfPatch { path: PathBuf, message: String },

    #[error("Path remapping failed: {0}")]
    PathRemap(String),

    #[error("Package not found: {0}")]
    PackageNotFound(String),

    #[error("Dependency resolution failed: {0}")]
    DependencyResolution(String),

    #[error("Database error: {source}")]
    Database {
        #[from]
        source: redb::Error,
    },

    #[error("Network error: {source}")]
    Network {
        #[from]
        source: reqwest::Error,
    },

    #[error("Archive extraction error: {0}")]
    Archive(String),

    #[error("Plugin error ({plugin}): {message}")]
    Plugin { plugin: String, message: String },

    #[error("IO error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    #[error("Verification failed: {0}")]
    Verification(String),

    #[error("Serialization error: {source}")]
    Serialization {
        #[from]
        source: serde_json::Error,
    },

    #[error("Config error: {0}")]
    Config(String),
}

pub type ZlResult<T> = Result<T, ZlError>;
