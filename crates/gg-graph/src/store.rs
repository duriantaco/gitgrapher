use gg_core::types::{GraphEdge, GraphNode, NodeId, NodeLabel, RelationType};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::HashMap;
use std::path::Path;

/// Direction for graph traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Outgoing,
    Incoming,
    Both,
}

/// A hit from a BFS/DFS traversal.
#[derive(Debug, Clone)]
pub struct TraversalHit {
    pub node_id: NodeId,
    pub depth: usize,
    pub path: Vec<NodeId>,
}

/// In-memory graph store with adjacency indexes.
///
/// this is a pure Rust data structure that lives entirely in-process.
#[derive(Debug, Default)]
pub struct GraphStore {
    /// All nodes by ID.
    nodes: HashMap<NodeId, GraphNode>,
    /// All edges by ID.
    edges: HashMap<NodeId, GraphEdge>,

    // Adjacency indexes — rebuilt on load, O(1) neighbor lookup
    /// source_id -> [edge_ids]
    outgoing: HashMap<NodeId, Vec<NodeId>>,
    /// target_id -> [edge_ids]
    incoming: HashMap<NodeId, Vec<NodeId>>,

    // Secondary indexes
    /// file_path -> [node_ids]
    by_file: HashMap<SmolStr, Vec<NodeId>>,
    /// label -> [node_ids]
    by_label: HashMap<NodeLabel, Vec<NodeId>>,
    /// name -> [node_ids]
    by_name: HashMap<SmolStr, Vec<NodeId>>,

    /// File hashes for incremental indexing (file_path -> xxhash).
    file_hashes: HashMap<SmolStr, u64>,
}

impl GraphStore {
    pub fn new() -> Self {
        Self::default()
    }

    // -----------------------------------------------------------------------
    // Mutation
    // -----------------------------------------------------------------------

    /// Add a node to the graph.
    pub fn add_node(&mut self, node: GraphNode) {
        let id = node.id.clone();

        // Update secondary indexes
        self.by_file
            .entry(node.properties.file_path.clone())
            .or_default()
            .push(id.clone());
        self.by_label
            .entry(node.label)
            .or_default()
            .push(id.clone());
        self.by_name
            .entry(node.properties.name.clone())
            .or_default()
            .push(id.clone());

        self.nodes.insert(id, node);
    }

    /// Add an edge to the graph.
    pub fn add_edge(&mut self, edge: GraphEdge) {
        let id = edge.id.clone();

        self.outgoing
            .entry(edge.source_id.clone())
            .or_default()
            .push(id.clone());
        self.incoming
            .entry(edge.target_id.clone())
            .or_default()
            .push(id.clone());

        self.edges.insert(id, edge);
    }

    /// Remove all nodes and edges associated with a file.
    /// Returns the number of nodes removed.
    /// Used for incremental re-indexing.
    pub fn remove_file(&mut self, file_path: &str) -> usize {
        let node_ids = match self.by_file.remove(file_path) {
            Some(ids) => ids,
            None => return 0,
        };

        let count = node_ids.len();

        for node_id in &node_ids {
            // Remove edges involving this node
            if let Some(out_edges) = self.outgoing.remove(node_id) {
                for eid in &out_edges {
                    if let Some(edge) = self.edges.remove(eid) {
                        if let Some(inc) = self.incoming.get_mut(&edge.target_id) {
                            inc.retain(|e| e != eid);
                        }
                    }
                }
            }
            if let Some(in_edges) = self.incoming.remove(node_id) {
                for eid in &in_edges {
                    if let Some(edge) = self.edges.remove(eid) {
                        if let Some(out) = self.outgoing.get_mut(&edge.source_id) {
                            out.retain(|e| e != eid);
                        }
                    }
                }
            }

            // Remove from secondary indexes
            if let Some(node) = self.nodes.remove(node_id) {
                if let Some(v) = self.by_label.get_mut(&node.label) {
                    v.retain(|id| id != node_id);
                }
                if let Some(v) = self.by_name.get_mut(&node.properties.name) {
                    v.retain(|id| id != node_id);
                }
            }
        }

        count
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    pub fn get_node(&self, id: &str) -> Option<&GraphNode> {
        self.nodes.get(id)
    }

    pub fn get_edge(&self, id: &str) -> Option<&GraphEdge> {
        self.edges.get(id)
    }

    /// Get all outgoing edges from a node, optionally filtered by type.
    pub fn outgoing_edges(&self, node_id: &str, rel_type: Option<RelationType>) -> Vec<&GraphEdge> {
        self.outgoing
            .get(node_id)
            .map(|edge_ids| {
                edge_ids
                    .iter()
                    .filter_map(|eid| self.edges.get(eid.as_str()))
                    .filter(|e| rel_type.map_or(true, |rt| e.rel_type == rt))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all incoming edges to a node, optionally filtered by type.
    pub fn incoming_edges(&self, node_id: &str, rel_type: Option<RelationType>) -> Vec<&GraphEdge> {
        self.incoming
            .get(node_id)
            .map(|edge_ids| {
                edge_ids
                    .iter()
                    .filter_map(|eid| self.edges.get(eid.as_str()))
                    .filter(|e| rel_type.map_or(true, |rt| e.rel_type == rt))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get nodes by label.
    pub fn nodes_by_label(&self, label: NodeLabel) -> Vec<&GraphNode> {
        self.by_label
            .get(&label)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.nodes.get(id.as_str()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get nodes by name.
    pub fn nodes_by_name(&self, name: &str) -> Vec<&GraphNode> {
        self.by_name
            .get(name)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.nodes.get(id.as_str()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get all nodes in a file.
    pub fn nodes_in_file(&self, file_path: &str) -> Vec<&GraphNode> {
        self.by_file
            .get(file_path)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.nodes.get(id.as_str()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// BFS traversal from a start node.
    pub fn bfs(
        &self,
        start: &str,
        direction: Direction,
        max_depth: usize,
        edge_filter: Option<&dyn Fn(&GraphEdge) -> bool>,
    ) -> Vec<TraversalHit> {
        use std::collections::{HashSet, VecDeque};

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut results = Vec::new();

        visited.insert(SmolStr::new(start));
        queue.push_back(TraversalHit {
            node_id: SmolStr::new(start),
            depth: 0,
            path: vec![SmolStr::new(start)],
        });

        while let Some(hit) = queue.pop_front() {
            if hit.depth > 0 {
                results.push(hit.clone());
            }
            if hit.depth >= max_depth {
                continue;
            }

            let neighbors = match direction {
                Direction::Outgoing => self.outgoing_edges(&hit.node_id, None),
                Direction::Incoming => self.incoming_edges(&hit.node_id, None),
                Direction::Both => {
                    let mut out = self.outgoing_edges(&hit.node_id, None);
                    out.extend(self.incoming_edges(&hit.node_id, None));
                    out
                }
            };

            for edge in neighbors {
                if let Some(filter) = &edge_filter {
                    if !filter(edge) {
                        continue;
                    }
                }

                let next_id = match direction {
                    Direction::Incoming => &edge.source_id,
                    _ => &edge.target_id,
                };

                if visited.insert(next_id.clone()) {
                    let mut path = hit.path.clone();
                    path.push(next_id.clone());
                    queue.push_back(TraversalHit {
                        node_id: next_id.clone(),
                        depth: hit.depth + 1,
                        path,
                    });
                }
            }
        }

        results
    }

    // -----------------------------------------------------------------------
    // Analytics
    // -----------------------------------------------------------------------

    /// Get the degree (in + out edges) for a node.
    pub fn degree(&self, node_id: &str) -> usize {
        let out = self.outgoing.get(node_id).map(|v| v.len()).unwrap_or(0);
        let inc = self.incoming.get(node_id).map(|v| v.len()).unwrap_or(0);
        out + inc
    }

    /// Get the in-degree for a node.
    pub fn in_degree(&self, node_id: &str) -> usize {
        self.incoming.get(node_id).map(|v| v.len()).unwrap_or(0)
    }

    /// Get the out-degree for a node.
    pub fn out_degree(&self, node_id: &str) -> usize {
        self.outgoing.get(node_id).map(|v| v.len()).unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Stats
    // -----------------------------------------------------------------------

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn file_count(&self) -> usize {
        self.by_file.len()
    }

    /// Set the hash for a file (for incremental indexing).
    pub fn set_file_hash(&mut self, file_path: SmolStr, hash: u64) {
        self.file_hashes.insert(file_path, hash);
    }

    /// Get the stored hash for a file.
    pub fn get_file_hash(&self, file_path: &str) -> Option<u64> {
        self.file_hashes.get(file_path).copied()
    }

    /// Get all nodes (iterator).
    pub fn nodes(&self) -> impl Iterator<Item = &GraphNode> {
        self.nodes.values()
    }

    /// Get all edges (iterator).
    pub fn edges(&self) -> impl Iterator<Item = &GraphEdge> {
        self.edges.values()
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Save graph to a directory. Creates `graph.json` inside it.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("Failed to create dir: {e}"))?;

        let data = SerializedGraph {
            version: 1,
            nodes: self.nodes.values().cloned().collect(),
            edges: self.edges.values().cloned().collect(),
            file_hashes: self.file_hashes.clone(),
        };

        let json = serde_json::to_vec(&data).map_err(|e| format!("Serialization failed: {e}"))?;

        let path = dir.join("graph.json");
        std::fs::write(&path, &json)
            .map_err(|e| format!("Failed to write {}: {e}", path.display()))?;

        Ok(())
    }

    /// Load graph from a directory. Reads `graph.json` and rebuilds indexes.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let path = dir.join("graph.json");
        let bytes =
            std::fs::read(&path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;

        let data: SerializedGraph =
            serde_json::from_slice(&bytes).map_err(|e| format!("Deserialization failed: {e}"))?;

        if data.version != 1 {
            return Err(format!("Unsupported graph version: {}", data.version));
        }

        let mut store = Self::new();
        store.file_hashes = data.file_hashes;

        for node in data.nodes {
            store.add_node(node);
        }
        for edge in data.edges {
            store.add_edge(edge);
        }

        Ok(store)
    }

    /// Check if a saved graph exists in a directory.
    pub fn exists(dir: &Path) -> bool {
        dir.join("graph.json").exists()
    }

    /// Search nodes by name substring (case-insensitive).
    pub fn search_nodes(&self, query: &str, limit: usize) -> Vec<&GraphNode> {
        let query_lower = query.to_lowercase();
        self.nodes
            .values()
            .filter(|n| {
                n.properties.name.to_lowercase().contains(&query_lower) && n.label.is_symbol()
            })
            .take(limit)
            .collect()
    }
}

/// Serializable representation of the graph (no indexes).
/// Indexes are rebuilt on load for forward compatibility.
#[derive(Serialize, Deserialize)]
struct SerializedGraph {
    version: u32,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    file_hashes: HashMap<SmolStr, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gg_core::types::NodeProperties;

    fn make_node(id: &str, name: &str, label: NodeLabel, file: &str) -> GraphNode {
        GraphNode {
            id: SmolStr::new(id),
            label,
            properties: NodeProperties::file(name, file),
        }
    }

    #[test]
    fn test_add_and_query() {
        let mut store = GraphStore::new();
        store.add_node(make_node("f1", "index.ts", NodeLabel::File, "src/index.ts"));
        store.add_node(make_node(
            "fn1",
            "main",
            NodeLabel::Function,
            "src/index.ts",
        ));
        store.add_edge(GraphEdge::new(
            "f1",
            "fn1",
            RelationType::Contains,
            1.0,
            "file contains",
        ));

        assert_eq!(store.node_count(), 2);
        assert_eq!(store.edge_count(), 1);
        assert_eq!(store.nodes_in_file("src/index.ts").len(), 2);
        assert_eq!(store.nodes_by_label(NodeLabel::Function).len(), 1);
        assert_eq!(
            store
                .outgoing_edges("f1", Some(RelationType::Contains))
                .len(),
            1
        );
    }

    #[test]
    fn test_remove_file() {
        let mut store = GraphStore::new();
        store.add_node(make_node("f1", "a.ts", NodeLabel::File, "src/a.ts"));
        store.add_node(make_node("fn1", "foo", NodeLabel::Function, "src/a.ts"));
        store.add_node(make_node("f2", "b.ts", NodeLabel::File, "src/b.ts"));
        store.add_edge(GraphEdge::new(
            "fn1",
            "f2",
            RelationType::Imports,
            0.9,
            "imports",
        ));

        let removed = store.remove_file("src/a.ts");
        assert_eq!(removed, 2);
        assert_eq!(store.node_count(), 1);
        assert_eq!(store.edge_count(), 0);
    }

    #[test]
    fn test_bfs() {
        let mut store = GraphStore::new();
        store.add_node(make_node("a", "a", NodeLabel::Function, "f.ts"));
        store.add_node(make_node("b", "b", NodeLabel::Function, "f.ts"));
        store.add_node(make_node("c", "c", NodeLabel::Function, "f.ts"));
        store.add_edge(GraphEdge::new("a", "b", RelationType::Calls, 0.9, ""));
        store.add_edge(GraphEdge::new("b", "c", RelationType::Calls, 0.9, ""));

        let hits = store.bfs("a", Direction::Outgoing, 3, None);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].node_id.as_str(), "b");
        assert_eq!(hits[1].node_id.as_str(), "c");
        assert_eq!(hits[1].depth, 2);
    }
}
