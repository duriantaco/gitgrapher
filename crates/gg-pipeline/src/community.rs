//! Community detection using a simplified Leiden/Louvain algorithm.
//!
//! Groups related symbols into communities based on CALLS, EXTENDS, IMPLEMENTS edges.
//! Communities represent functional areas of the codebase.

use gg_core::types::*;
use gg_graph::GraphStore;
use smol_str::SmolStr;
use std::collections::{HashMap, HashSet};
use tracing::info;

/// Result of community detection.
#[derive(Debug)]
pub struct CommunityResult {
    /// Community nodes to add to the graph.
    pub community_nodes: Vec<GraphNode>,
    /// Membership edges (symbol MEMBER_OF community).
    pub membership_edges: Vec<GraphEdge>,
    /// Map of node_id → community_id.
    pub membership_map: HashMap<SmolStr, SmolStr>,
    pub stats: CommunityStats,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct CommunityStats {
    pub total_communities: usize,
    pub nodes_processed: usize,
    pub modularity: f64,
}

/// Edge types used for clustering.
const CLUSTERING_TYPES: &[RelationType] = &[
    RelationType::Calls,
    RelationType::Extends,
    RelationType::Implements,
];

/// Detect communities in the graph using Louvain modularity optimization.
///
/// The algorithm:
/// 1. Build undirected adjacency from CALLS/EXTENDS/IMPLEMENTS edges
/// 2. Initialize each node in its own community
/// 3. Iteratively move nodes to the community that maximizes modularity gain
/// 4. Repeat until no more moves improve modularity
pub fn detect_communities(store: &GraphStore, resolution: f64) -> CommunityResult {
    // Step 1: Build undirected adjacency list of symbol nodes
    let mut adj: HashMap<SmolStr, Vec<SmolStr>> = HashMap::new();
    let mut edge_count = 0usize;

    let clustering_set: HashSet<RelationType> = CLUSTERING_TYPES.iter().copied().collect();

    for edge in store.edges() {
        if !clustering_set.contains(&edge.rel_type) {
            continue;
        }
        if edge.confidence < 0.5 {
            continue;
        }
        // Only include symbol nodes
        let src_ok = store
            .get_node(&edge.source_id)
            .is_some_and(|n| n.label.is_symbol());
        let tgt_ok = store
            .get_node(&edge.target_id)
            .is_some_and(|n| n.label.is_symbol());
        if !src_ok || !tgt_ok {
            continue;
        }
        if edge.source_id == edge.target_id {
            continue;
        }

        adj.entry(edge.source_id.clone())
            .or_default()
            .push(edge.target_id.clone());
        adj.entry(edge.target_id.clone())
            .or_default()
            .push(edge.source_id.clone());
        edge_count += 1;
    }

    let node_ids: Vec<SmolStr> = adj.keys().cloned().collect();
    let n = node_ids.len();

    if n == 0 || edge_count == 0 {
        return CommunityResult {
            community_nodes: vec![],
            membership_edges: vec![],
            membership_map: HashMap::new(),
            stats: CommunityStats {
                total_communities: 0,
                nodes_processed: 0,
                modularity: 0.0,
            },
        };
    }

    info!("Community detection: {} nodes, {} edges", n, edge_count);

    // Step 2: Initialize each node in its own community
    let mut community: HashMap<SmolStr, usize> = HashMap::new();
    for (i, id) in node_ids.iter().enumerate() {
        community.insert(id.clone(), i);
    }

    // Precompute degree for each node
    let degree: HashMap<&SmolStr, usize> = adj.iter().map(|(k, v)| (k, v.len())).collect();
    let m2 = (edge_count * 2) as f64; // 2 * total edges (undirected counted once)

    // Step 3: Louvain local moving phase
    let max_iterations = 20;
    for iteration in 0..max_iterations {
        let mut moved = 0;

        for node_id in &node_ids {
            let current_comm = community[node_id];
            let ki = *degree.get(node_id).unwrap_or(&0) as f64;
            let neighbors = match adj.get(node_id) {
                Some(n) => n,
                None => continue,
            };

            // Count edges to each neighboring community
            let mut comm_edges: HashMap<usize, f64> = HashMap::new();
            for neighbor in neighbors {
                let nc = community[neighbor];
                *comm_edges.entry(nc).or_insert(0.0) += 1.0;
            }

            // Compute sum of degrees in each community
            let mut comm_degree_sum: HashMap<usize, f64> = HashMap::new();
            for (nid, &comm) in &community {
                let d = *degree.get(nid).unwrap_or(&0) as f64;
                *comm_degree_sum.entry(comm).or_insert(0.0) += d;
            }

            // Find the community that gives the best modularity gain
            let mut best_comm = current_comm;
            let mut best_gain = 0.0;

            // Remove node from current community for calculation
            let sigma_current = comm_degree_sum.get(&current_comm).copied().unwrap_or(0.0) - ki;
            let ki_in_current = comm_edges.get(&current_comm).copied().unwrap_or(0.0);

            for (&target_comm, &ki_in_target) in &comm_edges {
                if target_comm == current_comm {
                    continue;
                }
                let sigma_target = comm_degree_sum.get(&target_comm).copied().unwrap_or(0.0);

                // Modularity gain formula (with resolution parameter)
                let gain = (ki_in_target - ki_in_current) / m2
                    - resolution * ki * (sigma_target - sigma_current) / (m2 * m2);

                if gain > best_gain {
                    best_gain = gain;
                    best_comm = target_comm;
                }
            }

            if best_comm != current_comm {
                community.insert(node_id.clone(), best_comm);
                moved += 1;
            }
        }

        if moved == 0 {
            info!(
                "Community detection converged after {} iterations",
                iteration + 1
            );
            break;
        }
    }

    // Step 4: Renumber communities consecutively and skip singletons
    let mut comm_members: HashMap<usize, Vec<SmolStr>> = HashMap::new();
    for (node_id, &comm) in &community {
        comm_members.entry(comm).or_default().push(node_id.clone());
    }

    // Filter out singletons and renumber
    let mut final_comms: Vec<(usize, Vec<SmolStr>)> = comm_members
        .into_iter()
        .filter(|(_, members)| members.len() >= 2)
        .collect();
    final_comms.sort_by_key(|community| std::cmp::Reverse(community.1.len()));

    // Step 5: Create community nodes with heuristic labels
    let mut community_nodes = Vec::new();
    let mut membership_edges = Vec::new();
    let mut membership_map = HashMap::new();

    for (idx, (_old_comm, members)) in final_comms.iter().enumerate() {
        let comm_id = SmolStr::new(format!("comm_{}", idx));

        // Generate heuristic label from folder names
        let label = generate_community_label(members, store, idx);
        let cohesion = calculate_cohesion(members, &adj);

        let mut props = NodeProperties::file(&label, "");
        props.heuristic_label = Some(SmolStr::new(&label));
        props.cohesion = Some(cohesion);
        props.symbol_count = Some(members.len() as u32);

        community_nodes.push(GraphNode {
            id: comm_id.clone(),
            label: NodeLabel::Community,
            properties: props,
        });

        for member_id in members {
            membership_edges.push(GraphEdge::new(
                member_id.clone(),
                comm_id.clone(),
                RelationType::MemberOf,
                1.0,
                "community membership",
            ));
            membership_map.insert(member_id.clone(), comm_id.clone());
        }
    }

    let total = community_nodes.len();
    info!("Found {} communities ({} nodes processed)", total, n);

    CommunityResult {
        community_nodes,
        membership_edges,
        membership_map,
        stats: CommunityStats {
            total_communities: total,
            nodes_processed: n,
            modularity: 0.0, // TODO: compute actual modularity
        },
    }
}

/// Generate a label from the most common folder name in the community.
fn generate_community_label(members: &[SmolStr], store: &GraphStore, idx: usize) -> String {
    let mut folder_counts: HashMap<String, usize> = HashMap::new();
    let skip_folders: HashSet<&str> = [
        "src", "lib", "core", "utils", "common", "shared", "helpers", "test", "tests",
    ]
    .iter()
    .copied()
    .collect();

    for member_id in members {
        if let Some(node) = store.get_node(member_id) {
            let path = node.properties.file_path.as_str();
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 2 {
                let folder = parts[parts.len() - 2];
                if !skip_folders.contains(folder.to_lowercase().as_str()) {
                    *folder_counts.entry(folder.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    folder_counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(folder, _)| {
            let mut label = folder;
            if let Some(first) = label.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            label
        })
        .unwrap_or_else(|| format!("Cluster_{}", idx))
}

/// Calculate cohesion: fraction of edges that stay within the community.
fn calculate_cohesion(members: &[SmolStr], adj: &HashMap<SmolStr, Vec<SmolStr>>) -> f64 {
    if members.len() <= 1 {
        return 1.0;
    }
    let member_set: HashSet<&SmolStr> = members.iter().collect();
    let mut internal = 0usize;
    let mut total = 0usize;

    for member in members.iter().take(50) {
        if let Some(neighbors) = adj.get(member) {
            for neighbor in neighbors {
                total += 1;
                if member_set.contains(neighbor) {
                    internal += 1;
                }
            }
        }
    }

    if total == 0 {
        1.0
    } else {
        internal as f64 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, name: &str, file: &str) -> GraphNode {
        GraphNode {
            id: SmolStr::new(id),
            label: NodeLabel::Function,
            properties: NodeProperties::symbol(name, file, Language::TypeScript, 1, 10),
        }
    }

    #[test]
    fn test_detects_two_clusters() {
        let mut store = GraphStore::new();

        // Cluster A: auth functions that call each other
        store.add_node(make_node("a1", "login", "src/auth/login.ts"));
        store.add_node(make_node("a2", "validateToken", "src/auth/token.ts"));
        store.add_node(make_node("a3", "refreshToken", "src/auth/token.ts"));
        store.add_edge(GraphEdge::new("a1", "a2", RelationType::Calls, 0.9, ""));
        store.add_edge(GraphEdge::new("a2", "a3", RelationType::Calls, 0.9, ""));
        store.add_edge(GraphEdge::new("a3", "a1", RelationType::Calls, 0.9, ""));

        // Cluster B: user functions that call each other
        store.add_node(make_node("b1", "getUser", "src/user/service.ts"));
        store.add_node(make_node("b2", "updateUser", "src/user/service.ts"));
        store.add_node(make_node("b3", "deleteUser", "src/user/service.ts"));
        store.add_edge(GraphEdge::new("b1", "b2", RelationType::Calls, 0.9, ""));
        store.add_edge(GraphEdge::new("b2", "b3", RelationType::Calls, 0.9, ""));
        store.add_edge(GraphEdge::new("b3", "b1", RelationType::Calls, 0.9, ""));

        let result = detect_communities(&store, 1.0);
        assert!(
            result.stats.total_communities >= 2,
            "Expected >=2 communities, got {}",
            result.stats.total_communities
        );
        assert_eq!(result.stats.nodes_processed, 6);
    }

    #[test]
    fn test_empty_graph() {
        let store = GraphStore::new();
        let result = detect_communities(&store, 1.0);
        assert_eq!(result.stats.total_communities, 0);
    }

    #[test]
    fn test_singletons_excluded() {
        let mut store = GraphStore::new();
        store.add_node(make_node("a", "lone", "src/lone.ts"));
        // No edges — should not form a community
        let result = detect_communities(&store, 1.0);
        assert_eq!(result.stats.total_communities, 0);
    }
}
