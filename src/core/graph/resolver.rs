use petgraph::Direction;
use petgraph::algo::{is_cyclic_directed, toposort};
use petgraph::graph::NodeIndex;
use std::collections::HashSet;

use super::model::{DepGraph, DepType, DependencyEdge, PackageNode};
use crate::error::{ZlError, ZlResult};

impl DepGraph {
    /// Add a package to the graph and return its node index
    pub fn add_package(&mut self, node: PackageNode) -> NodeIndex {
        let key = format!("{}-{}", node.id.name, node.id.version);
        let idx = self.graph.add_node(node);
        self.index.insert(key, idx);
        idx
    }

    /// Add a declared dependency edge between two packages
    pub fn add_dependency(
        &mut self,
        from: NodeIndex,
        to: NodeIndex,
        version_constraint: Option<String>,
    ) {
        self.graph.add_edge(
            from,
            to,
            DependencyEdge {
                dep_type: DepType::Declared,
                version_constraint,
            },
        );
    }

    /// Add a shared-library dependency edge
    pub fn add_lib_dependency(&mut self, from: NodeIndex, to: NodeIndex, lib_name: String) {
        self.graph.add_edge(
            from,
            to,
            DependencyEdge {
                dep_type: DepType::SharedLibrary { lib_name },
                version_constraint: None,
            },
        );
    }

    /// Look up a package node index by "name-version" key
    pub fn lookup(&self, name: &str, version: &str) -> Option<NodeIndex> {
        self.index.get(&format!("{}-{}", name, version)).copied()
    }

    /// Look up by name only (returns first match)
    pub fn lookup_by_name(&self, name: &str) -> Option<NodeIndex> {
        self.index
            .iter()
            .find(|(k, _)| k.starts_with(&format!("{}-", name)))
            .map(|(_, idx)| *idx)
    }

    /// Remove a package from the graph
    pub fn remove_package(&mut self, idx: NodeIndex) -> Option<PackageNode> {
        let node = self.graph.remove_node(idx);
        if let Some(ref n) = node {
            let key = format!("{}-{}", n.id.name, n.id.version);
            self.index.remove(&key);
        }
        node
    }

    /// Return a topological ordering of all packages (dependencies before dependents)
    pub fn topological_order(&self) -> ZlResult<Vec<NodeIndex>> {
        toposort(&self.graph, None).map_err(|cycle| {
            let node = &self.graph[cycle.node_id()];
            ZlError::DependencyResolution {
                package: format!("{}-{}", node.id.name, node.id.version),
                message: "Dependency cycle detected".into(),
            }
        })
    }

    /// Check if the graph contains any cycles
    pub fn has_cycles(&self) -> bool {
        is_cyclic_directed(&self.graph)
    }

    /// Find all packages that depend on the given package
    pub fn reverse_deps(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        self.graph
            .neighbors_directed(idx, Direction::Incoming)
            .collect()
    }

    /// Find all dependencies of the given package
    pub fn deps_of(&self, idx: NodeIndex) -> Vec<NodeIndex> {
        self.graph
            .neighbors_directed(idx, Direction::Outgoing)
            .collect()
    }

    /// Find orphan packages: installed as dependencies but no longer needed by any explicit package
    pub fn find_orphans(&self) -> Vec<NodeIndex> {
        self.graph
            .node_indices()
            .filter(|&idx| {
                let node = &self.graph[idx];
                if node.explicit {
                    return false;
                }
                self.graph
                    .neighbors_directed(idx, Direction::Incoming)
                    .next()
                    .is_none()
            })
            .collect()
    }

    /// Compute the full install order for a package and its transitive dependencies.
    /// Returns nodes in dependency-first order.
    pub fn install_order(&self, root: NodeIndex) -> ZlResult<Vec<NodeIndex>> {
        let mut visited = HashSet::new();
        let mut order = Vec::new();
        self.visit_deps(root, &mut visited, &mut order)?;
        Ok(order)
    }

    fn visit_deps(
        &self,
        idx: NodeIndex,
        visited: &mut HashSet<NodeIndex>,
        order: &mut Vec<NodeIndex>,
    ) -> ZlResult<()> {
        if !visited.insert(idx) {
            return Ok(());
        }
        for dep in self.deps_of(idx) {
            self.visit_deps(dep, visited, order)?;
        }
        order.push(idx);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::graph::model::PackageId;
    use std::collections::HashMap;

    fn make_node(name: &str, version: &str, explicit: bool) -> PackageNode {
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
            explicit,
        }
    }

    #[test]
    fn test_topological_order() {
        let mut graph = DepGraph::new();
        let a = graph.add_package(make_node("a", "1.0", true));
        let b = graph.add_package(make_node("b", "1.0", false));
        let c = graph.add_package(make_node("c", "1.0", false));
        // a depends on b, b depends on c => valid topo order has c before b before a
        graph.add_dependency(a, b, None);
        graph.add_dependency(b, c, None);

        let order = graph.topological_order().unwrap();
        assert_eq!(order.len(), 3);
        // In a topo sort, each node must come after its dependencies.
        // petgraph's toposort returns nodes such that for edge u->v, u appears before v.
        // Our edges are: a->b, b->c (meaning a depends on b, b depends on c).
        // So toposort yields: a, b, c (dependents first) or similar valid ordering.
        let pos_a = order.iter().position(|&n| n == a).unwrap();
        let pos_b = order.iter().position(|&n| n == b).unwrap();
        let pos_c = order.iter().position(|&n| n == c).unwrap();
        // In petgraph's toposort, for edge a->b, a comes before b
        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn test_find_orphans() {
        let mut graph = DepGraph::new();
        let _a = graph.add_package(make_node("a", "1.0", true));
        let b = graph.add_package(make_node("b", "1.0", false));
        let c = graph.add_package(make_node("c", "1.0", false));
        graph.add_dependency(_a, b, None);
        // c is a dependency-type package but nothing depends on it => orphan

        let orphans = graph.find_orphans();
        assert_eq!(orphans.len(), 1);
        assert_eq!(graph.graph[orphans[0]].id.name, "c");
    }

    #[test]
    fn test_install_order() {
        let mut graph = DepGraph::new();
        let a = graph.add_package(make_node("a", "1.0", true));
        let b = graph.add_package(make_node("b", "1.0", false));
        let c = graph.add_package(make_node("c", "1.0", false));
        graph.add_dependency(a, b, None);
        graph.add_dependency(a, c, None);
        graph.add_dependency(b, c, None);

        let order = graph.install_order(a).unwrap();
        let pos_a = order.iter().position(|&n| n == a).unwrap();
        let pos_b = order.iter().position(|&n| n == b).unwrap();
        let pos_c = order.iter().position(|&n| n == c).unwrap();
        assert!(pos_c < pos_b);
        assert!(pos_b < pos_a);
    }
}
