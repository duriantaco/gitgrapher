//! Process detection: traces execution flows from entry points through CALLS edges.
//!
//! Processes answer "what happens when X runs?" — they trace the call chain
//! from entry points (handlers, main functions, exported APIs) through the graph.

use gg_core::types::*;
use gg_graph::GraphStore;
use smol_str::SmolStr;
use std::collections::{HashMap, HashSet, VecDeque};
use tracing::info;

/// Result of process detection.
#[derive(Debug)]
pub struct ProcessResult {
    pub process_nodes: Vec<GraphNode>,
    pub step_edges: Vec<GraphEdge>,
    pub stats: ProcessStats,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct ProcessStats {
    pub total_processes: usize,
    pub entry_points_found: usize,
    pub cross_community: usize,
    pub avg_step_count: f64,
}

/// Configuration for process detection.
pub struct ProcessConfig {
    pub max_trace_depth: usize,
    pub max_branching: usize,
    pub max_processes: usize,
    pub min_steps: usize,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            max_trace_depth: 10,
            max_branching: 4,
            max_processes: 75,
            min_steps: 3,
        }
    }
}

/// Entry point naming patterns that boost score.
const ENTRY_PATTERNS: &[&str] = &[
    "handle",
    "on",
    "main",
    "init",
    "start",
    "run",
    "execute",
    "process",
    "serve",
    "listen",
    "route",
    "controller",
    "endpoint",
    "handler",
    "middleware",
    "dispatch",
    "trigger",
    "invoke",
];

/// Test file patterns to exclude.
const TEST_PATTERNS: &[&str] = &[
    "test",
    "spec",
    "__test__",
    "__tests__",
    ".test.",
    ".spec.",
    "_test.",
    "_spec.",
    "fixture",
    "mock",
];

/// Minimum confidence for edges used in tracing.
const MIN_TRACE_CONFIDENCE: f64 = 0.5;

/// Detect execution flow processes in the graph.
pub fn detect_processes(
    store: &GraphStore,
    membership_map: &HashMap<SmolStr, SmolStr>,
    config: &ProcessConfig,
) -> ProcessResult {
    // Build CALLS adjacency lists
    let mut forward: HashMap<SmolStr, Vec<SmolStr>> = HashMap::new();
    let mut reverse: HashMap<SmolStr, Vec<SmolStr>> = HashMap::new();

    for edge in store.edges() {
        if edge.rel_type != RelationType::Calls || edge.confidence < MIN_TRACE_CONFIDENCE {
            continue;
        }
        forward
            .entry(edge.source_id.clone())
            .or_default()
            .push(edge.target_id.clone());
        reverse
            .entry(edge.target_id.clone())
            .or_default()
            .push(edge.source_id.clone());
    }

    // Find entry points
    let entry_points = find_entry_points(store, &forward, &reverse);
    info!("Found {} entry point candidates", entry_points.len());

    // Trace from each entry point
    let mut all_traces: Vec<Vec<SmolStr>> = Vec::new();
    for ep in &entry_points {
        if all_traces.len() >= config.max_processes * 2 {
            break;
        }
        let traces = trace_from_entry(ep, &forward, config);
        for trace in traces {
            if trace.len() >= config.min_steps {
                all_traces.push(trace);
            }
        }
    }

    // Deduplicate: remove traces that are subsets of longer traces
    let unique_traces = deduplicate_traces(all_traces);

    // Deduplicate by entry+terminal pair (keep longest)
    let deduped = deduplicate_by_endpoints(unique_traces);

    // Limit and sort by length (longer = more interesting)
    let mut limited: Vec<Vec<SmolStr>> = deduped;
    limited.sort_by_key(|trace| std::cmp::Reverse(trace.len()));
    limited.truncate(config.max_processes);

    // Create process nodes and step edges
    let mut process_nodes = Vec::new();
    let mut step_edges = Vec::new();
    let mut cross_community = 0usize;

    for (idx, trace) in limited.iter().enumerate() {
        let entry_id = &trace[0];
        let terminal_id = &trace[trace.len() - 1];

        let entry_name = store
            .get_node(entry_id)
            .map(|n| n.properties.name.as_str())
            .unwrap_or("?");
        let terminal_name = store
            .get_node(terminal_id)
            .map(|n| n.properties.name.as_str())
            .unwrap_or("?");

        // Determine communities touched
        let communities: Vec<SmolStr> = trace
            .iter()
            .filter_map(|id| membership_map.get(id))
            .cloned()
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let process_type = if communities.len() > 1 {
            "cross_community"
        } else {
            "intra_community"
        };
        if communities.len() > 1 {
            cross_community += 1;
        }

        let process_id = SmolStr::new(format!("proc_{}_{}", idx, sanitize(entry_name)));
        let label = format!(
            "{} -> {}",
            capitalize(entry_name),
            capitalize(terminal_name)
        );

        let mut props = NodeProperties::file(&label, "");
        props.process_type = Some(SmolStr::new(process_type));
        props.step_count = Some(trace.len() as u32);
        props.communities = Some(communities);
        props.entry_point_id = Some(entry_id.clone());
        props.terminal_id = Some(terminal_id.clone());

        process_nodes.push(GraphNode {
            id: process_id.clone(),
            label: NodeLabel::Process,
            properties: props,
        });

        // Create STEP_IN_PROCESS edges
        for (step_idx, node_id) in trace.iter().enumerate() {
            let mut edge = GraphEdge::new(
                node_id.clone(),
                process_id.clone(),
                RelationType::StepInProcess,
                1.0,
                format!("step {} of {}", step_idx + 1, trace.len()),
            );
            edge.step = Some((step_idx + 1) as i32);
            step_edges.push(edge);
        }
    }

    let total = process_nodes.len();
    let avg_steps = if total > 0 {
        process_nodes
            .iter()
            .filter_map(|p| p.properties.step_count)
            .sum::<u32>() as f64
            / total as f64
    } else {
        0.0
    };

    info!(
        "Detected {} processes ({} cross-community)",
        total, cross_community
    );

    ProcessResult {
        process_nodes,
        step_edges,
        stats: ProcessStats {
            total_processes: total,
            entry_points_found: entry_points.len(),
            cross_community,
            avg_step_count: (avg_steps * 10.0).round() / 10.0,
        },
    }
}

/// Score and rank entry point candidates.
fn find_entry_points(
    store: &GraphStore,
    forward: &HashMap<SmolStr, Vec<SmolStr>>,
    reverse: &HashMap<SmolStr, Vec<SmolStr>>,
) -> Vec<SmolStr> {
    let mut candidates: Vec<(SmolStr, f64)> = Vec::new();

    for node in store.nodes() {
        if !matches!(node.label, NodeLabel::Function | NodeLabel::Method) {
            continue;
        }

        let file = node.properties.file_path.as_str();
        if TEST_PATTERNS
            .iter()
            .any(|p| file.to_lowercase().contains(p))
        {
            continue;
        }

        let out_degree = forward.get(&node.id).map(|v| v.len()).unwrap_or(0);
        let in_degree = reverse.get(&node.id).map(|v| v.len()).unwrap_or(0);

        // Must call at least one other function
        if out_degree == 0 {
            continue;
        }

        let mut score = 0.0;

        // Call ratio: more outgoing than incoming = more likely entry point
        if in_degree == 0 {
            score += 3.0; // No callers = strong entry point signal
        } else {
            score += (out_degree as f64 / (in_degree as f64 + 1.0)).min(3.0);
        }

        // Export bonus
        if node.properties.is_exported {
            score += 1.5;
        }

        // Name pattern bonus
        let name_lower = node.properties.name.to_lowercase();
        for pattern in ENTRY_PATTERNS {
            if name_lower.contains(pattern) {
                score += 2.0;
                break;
            }
        }

        candidates.push((node.id.clone(), score));
    }

    // Sort by score descending
    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    candidates.into_iter().map(|(id, _)| id).collect()
}

/// Trace execution paths from an entry point using BFS with branching limit.
fn trace_from_entry(
    entry: &SmolStr,
    forward: &HashMap<SmolStr, Vec<SmolStr>>,
    config: &ProcessConfig,
) -> Vec<Vec<SmolStr>> {
    let mut traces: Vec<Vec<SmolStr>> = Vec::new();
    let mut queue: VecDeque<Vec<SmolStr>> = VecDeque::new();
    queue.push_back(vec![entry.clone()]);

    while let Some(path) = queue.pop_front() {
        if path.len() > config.max_trace_depth {
            traces.push(path);
            continue;
        }

        let current = path.last().unwrap();
        let callees = match forward.get(current) {
            Some(c) => c,
            None => {
                traces.push(path);
                continue;
            }
        };

        // Filter out already-visited nodes (prevent cycles)
        let visited: HashSet<&SmolStr> = path.iter().collect();
        let next: Vec<&SmolStr> = callees
            .iter()
            .filter(|c| !visited.contains(c))
            .take(config.max_branching)
            .collect();

        if next.is_empty() {
            traces.push(path);
        } else {
            for callee in next {
                let mut new_path = path.clone();
                new_path.push(callee.clone());
                queue.push_back(new_path);
            }
        }

        // Safety: limit total traces
        if traces.len() + queue.len() > 500 {
            break;
        }
    }

    traces
}

/// Remove traces that are subsets of longer traces.
fn deduplicate_traces(mut traces: Vec<Vec<SmolStr>>) -> Vec<Vec<SmolStr>> {
    traces.sort_by_key(|trace| std::cmp::Reverse(trace.len()));

    let mut result: Vec<Vec<SmolStr>> = Vec::new();
    for trace in traces {
        let trace_set: HashSet<&SmolStr> = trace.iter().collect();
        let is_subset = result.iter().any(|existing| {
            let existing_set: HashSet<&SmolStr> = existing.iter().collect();
            trace_set.is_subset(&existing_set)
        });
        if !is_subset {
            result.push(trace);
        }
    }
    result
}

/// Deduplicate by entry+terminal pair (keep the longest path per pair).
fn deduplicate_by_endpoints(traces: Vec<Vec<SmolStr>>) -> Vec<Vec<SmolStr>> {
    let mut best: HashMap<(SmolStr, SmolStr), Vec<SmolStr>> = HashMap::new();
    for trace in traces {
        if trace.is_empty() {
            continue;
        }
        let key = (trace[0].clone(), trace[trace.len() - 1].clone());
        let existing_len = best.get(&key).map(|t| t.len()).unwrap_or(0);
        if trace.len() > existing_len {
            best.insert(key, trace);
        }
    }
    best.into_values().collect()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fn(id: &str, name: &str, file: &str, exported: bool) -> GraphNode {
        let mut props = NodeProperties::symbol(name, file, Language::TypeScript, 1, 10);
        props.is_exported = exported;
        GraphNode {
            id: SmolStr::new(id),
            label: NodeLabel::Function,
            properties: props,
        }
    }

    #[test]
    fn test_detects_linear_process() {
        let mut store = GraphStore::new();
        store.add_node(make_fn("a", "handleRequest", "src/server.ts", true));
        store.add_node(make_fn("b", "authenticate", "src/auth.ts", true));
        store.add_node(make_fn("c", "getUser", "src/user.ts", true));
        store.add_node(make_fn("d", "formatResponse", "src/response.ts", false));
        store.add_edge(GraphEdge::new("a", "b", RelationType::Calls, 0.9, ""));
        store.add_edge(GraphEdge::new("b", "c", RelationType::Calls, 0.9, ""));
        store.add_edge(GraphEdge::new("c", "d", RelationType::Calls, 0.9, ""));

        let config = ProcessConfig {
            min_steps: 3,
            ..Default::default()
        };
        let result = detect_processes(&store, &HashMap::new(), &config);

        assert!(
            result.stats.total_processes >= 1,
            "Expected >=1 process, got {}",
            result.stats.total_processes
        );
        // The process should trace: handleRequest → authenticate → getUser → formatResponse
        let longest = result
            .process_nodes
            .iter()
            .max_by_key(|p| p.properties.step_count.unwrap_or(0));
        assert!(longest.is_some());
        assert!(longest.unwrap().properties.step_count.unwrap_or(0) >= 3);
    }

    #[test]
    fn test_skips_test_files() {
        let mut store = GraphStore::new();
        store.add_node(make_fn(
            "t1",
            "testLogin",
            "src/__tests__/auth.test.ts",
            true,
        ));
        store.add_node(make_fn("t2", "helper", "src/__tests__/helpers.ts", true));
        store.add_edge(GraphEdge::new("t1", "t2", RelationType::Calls, 0.9, ""));

        let result = detect_processes(&store, &HashMap::new(), &ProcessConfig::default());
        assert_eq!(result.stats.total_processes, 0);
    }

    #[test]
    fn test_empty_graph() {
        let store = GraphStore::new();
        let result = detect_processes(&store, &HashMap::new(), &ProcessConfig::default());
        assert_eq!(result.stats.total_processes, 0);
    }
}
