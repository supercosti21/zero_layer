use petgraph::graph::{DiGraph, NodeIndex};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Hash, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PackageId {
    pub name: String,
    pub version: String,
    pub source: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PackageNode {
    pub id: PackageId,
    pub installed_files: Vec<PathBuf>,
    pub provides_libs: HashMap<String, PathBuf>,
    pub needs_libs: Vec<String>,
    pub installed_at: u64,
    pub explicit: bool,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DependencyEdge {
    pub dep_type: DepType,
    pub version_constraint: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DepType {
    Declared,
    SharedLibrary { lib_name: String },
}

#[derive(Default)]
#[allow(dead_code)]
pub struct DepGraph {
    pub graph: DiGraph<PackageNode, DependencyEdge>,
    pub index: HashMap<String, NodeIndex>,
}

#[allow(dead_code)]
impl DepGraph {
    pub fn new() -> Self {
        Self::default()
    }
}
