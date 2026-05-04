use anyhow::{anyhow, bail, Context, Result};
use gg_core::types::{GraphNode, NodeLabel, RelationType};
use gg_graph::store::GraphStore;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;

const WORKTREE_REF: &str = "WORKTREE";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffStatus {
    Added,
    Removed,
    Modified,
    Moved,
    Unchanged,
}

impl DiffStatus {
    fn is_changed(self) -> bool {
        !matches!(self, Self::Unchanged)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchKind {
    Location,
    Symbol,
    Body,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffSummary {
    pub base_ref: String,
    pub head_ref: String,
    pub files_added: usize,
    pub files_removed: usize,
    pub files_modified: usize,
    pub files_unchanged: usize,
    pub symbols_added: usize,
    pub symbols_removed: usize,
    pub symbols_modified: usize,
    pub symbols_moved: usize,
    pub symbols_unchanged: usize,
    pub edges_added: usize,
    pub edges_removed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDiffRecord {
    pub path: String,
    pub status: DiffStatus,
    pub base_hash: Option<u64>,
    pub head_hash: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffNodeRecord {
    pub id: String,
    pub name: String,
    pub label: String,
    pub file: String,
    pub file_name: String,
    pub folder: String,
    pub line: u32,
    pub base_file: Option<String>,
    pub base_line: Option<u32>,
    pub head_file: Option<String>,
    pub head_line: Option<u32>,
    pub status: DiffStatus,
    pub layer: String,
    pub group: u8,
    pub degree: usize,
    pub in_deg: usize,
    pub out_deg: usize,
    pub return_type: String,
    pub is_async: bool,
    pub container_name: String,
    pub qualified_name: String,
    pub body_changed: bool,
    pub file_changed: bool,
    pub match_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLinkRecord {
    pub source: String,
    pub target: String,
    pub r#type: String,
    pub status: DiffStatus,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDiffData {
    pub mode: String,
    pub base_ref: String,
    pub head_ref: String,
    pub summary: DiffSummary,
    pub files: Vec<FileDiffRecord>,
    pub nodes: Vec<DiffNodeRecord>,
    pub links: Vec<DiffLinkRecord>,
}

#[derive(Debug, Clone)]
struct SymbolRecord {
    canonical_id: String,
    status: DiffStatus,
    match_kind: Option<MatchKind>,
    body_changed: bool,
    base: Option<GraphNode>,
    head: Option<GraphNode>,
    degree: usize,
    in_deg: usize,
    out_deg: usize,
}

#[derive(Debug, Clone)]
struct EdgeRecord {
    source: String,
    target: String,
    rel_type: RelationType,
    confidence: f64,
}

pub fn cmd_diff(
    path: &Path,
    base_ref: &str,
    head_ref: &str,
    format: &str,
    output: Option<&Path>,
    limit: Option<usize>,
) -> Result<()> {
    let repo_root = git_root(path)?;
    let base_store = load_store_for_revision(&repo_root, base_ref)?;
    let head_store = load_store_for_revision(&repo_root, head_ref)?;
    let diff = build_diff_data(base_ref, &base_store, head_ref, &head_store, limit);

    match format {
        "text" => print_diff_text(&repo_root, &diff),
        "json" => {
            let out = output.unwrap_or_else(|| Path::new("gitgrapher-diff.json"));
            std::fs::write(out, serde_json::to_string_pretty(&diff)?)?;
            println!("  Exported diff JSON to {}", out.display());
        }
        "html" => {
            let out = output.unwrap_or_else(|| Path::new("gitgrapher-diff.html"));
            export_diff_html(&diff, out)?;
            println!("  Exported diff HTML to {}", out.display());
        }
        other => bail!("Unknown format: {other}. Use text, json, or html."),
    }

    Ok(())
}

pub fn build_diff_data(
    base_ref: &str,
    base_store: &GraphStore,
    head_ref: &str,
    head_store: &GraphStore,
    limit: Option<usize>,
) -> GraphDiffData {
    let base_file_nodes = file_nodes(base_store);
    let head_file_nodes = file_nodes(head_store);
    let file_changes = diff_files(base_store, &base_file_nodes, head_store, &head_file_nodes);
    let file_status_by_path: HashMap<String, DiffStatus> = file_changes
        .iter()
        .map(|file| (file.path.clone(), file.status))
        .collect();

    let base_symbols: Vec<GraphNode> = base_store
        .nodes()
        .filter(|node| node.label.is_symbol())
        .cloned()
        .collect();
    let head_symbols: Vec<GraphNode> = head_store
        .nodes()
        .filter(|node| node.label.is_symbol())
        .cloned()
        .collect();

    let symbol_records = diff_symbols(&base_symbols, base_store, &head_symbols, head_store);
    let max_symbols = limit.unwrap_or(2000);
    let selected_symbol_ids = select_symbol_ids(&symbol_records, max_symbols);
    let selected_file_paths =
        select_file_paths(&selected_symbol_ids, &symbol_records, &file_changes);

    let mut nodes = build_file_nodes(
        &selected_file_paths,
        &file_changes,
        &base_file_nodes,
        base_store,
        &head_file_nodes,
        head_store,
    );
    nodes.extend(build_symbol_nodes(
        &selected_symbol_ids,
        &symbol_records,
        &file_status_by_path,
    ));
    nodes.sort_by(|a, b| a.label.cmp(&b.label).then_with(|| a.name.cmp(&b.name)));

    let selected_node_ids: HashSet<String> = nodes.iter().map(|node| node.id.clone()).collect();
    let (base_canonical, head_canonical) =
        build_canonical_maps(&symbol_records, &selected_file_paths);
    let (mut links, edges_added, edges_removed) = build_links(
        base_store,
        head_store,
        &base_canonical,
        &head_canonical,
        &selected_node_ids,
    );
    links.sort_by(|a, b| {
        a.source
            .cmp(&b.source)
            .then_with(|| a.target.cmp(&b.target))
            .then_with(|| a.r#type.cmp(&b.r#type))
    });

    let summary = DiffSummary {
        base_ref: base_ref.to_string(),
        head_ref: head_ref.to_string(),
        files_added: file_changes
            .iter()
            .filter(|file| file.status == DiffStatus::Added)
            .count(),
        files_removed: file_changes
            .iter()
            .filter(|file| file.status == DiffStatus::Removed)
            .count(),
        files_modified: file_changes
            .iter()
            .filter(|file| file.status == DiffStatus::Modified)
            .count(),
        files_unchanged: file_changes
            .iter()
            .filter(|file| file.status == DiffStatus::Unchanged)
            .count(),
        symbols_added: symbol_records
            .iter()
            .filter(|record| record.status == DiffStatus::Added)
            .count(),
        symbols_removed: symbol_records
            .iter()
            .filter(|record| record.status == DiffStatus::Removed)
            .count(),
        symbols_modified: symbol_records
            .iter()
            .filter(|record| record.status == DiffStatus::Modified)
            .count(),
        symbols_moved: symbol_records
            .iter()
            .filter(|record| record.status == DiffStatus::Moved)
            .count(),
        symbols_unchanged: symbol_records
            .iter()
            .filter(|record| record.status == DiffStatus::Unchanged)
            .count(),
        edges_added,
        edges_removed,
    };

    GraphDiffData {
        mode: "diff".to_string(),
        base_ref: base_ref.to_string(),
        head_ref: head_ref.to_string(),
        summary,
        files: file_changes,
        nodes,
        links,
    }
}

fn build_links(
    base_store: &GraphStore,
    head_store: &GraphStore,
    base_canonical: &HashMap<String, String>,
    head_canonical: &HashMap<String, String>,
    selected_node_ids: &HashSet<String>,
) -> (Vec<DiffLinkRecord>, usize, usize) {
    let base_edges = collect_edges(base_store, base_canonical);
    let head_edges = collect_edges(head_store, head_canonical);

    let base_keys: HashSet<String> = base_edges.keys().cloned().collect();
    let head_keys: HashSet<String> = head_edges.keys().cloned().collect();

    let mut links = Vec::new();
    let mut added = 0;
    let mut removed = 0;

    for (key, edge) in &head_edges {
        if !selected_node_ids.contains(&edge.source) || !selected_node_ids.contains(&edge.target) {
            continue;
        }
        let status = if base_keys.contains(key) {
            DiffStatus::Unchanged
        } else {
            added += 1;
            DiffStatus::Added
        };
        links.push(DiffLinkRecord {
            source: edge.source.clone(),
            target: edge.target.clone(),
            r#type: edge.rel_type.to_string(),
            status,
            confidence: edge.confidence,
        });
    }

    for (key, edge) in &base_edges {
        if head_keys.contains(key) {
            continue;
        }
        if !selected_node_ids.contains(&edge.source) || !selected_node_ids.contains(&edge.target) {
            continue;
        }
        removed += 1;
        links.push(DiffLinkRecord {
            source: edge.source.clone(),
            target: edge.target.clone(),
            r#type: edge.rel_type.to_string(),
            status: DiffStatus::Removed,
            confidence: edge.confidence,
        });
    }

    (links, added, removed)
}

fn collect_edges(
    store: &GraphStore,
    canonical: &HashMap<String, String>,
) -> HashMap<String, EdgeRecord> {
    let mut edges = HashMap::new();
    for edge in store.edges() {
        let Some(source) = canonical.get(edge.source_id.as_str()) else {
            continue;
        };
        let Some(target) = canonical.get(edge.target_id.as_str()) else {
            continue;
        };

        let key = canonical_edge_key(source, edge.rel_type, target);
        edges.insert(
            key,
            EdgeRecord {
                source: source.clone(),
                target: target.clone(),
                rel_type: edge.rel_type,
                confidence: edge.confidence,
            },
        );
    }
    edges
}

fn build_canonical_maps(
    symbol_records: &[SymbolRecord],
    selected_file_paths: &HashSet<String>,
) -> (HashMap<String, String>, HashMap<String, String>) {
    let mut base = HashMap::new();
    let mut head = HashMap::new();

    for record in symbol_records {
        if let Some(base_node) = &record.base {
            base.insert(base_node.id.to_string(), record.canonical_id.clone());
        }
        if let Some(head_node) = &record.head {
            head.insert(head_node.id.to_string(), record.canonical_id.clone());
        }
    }

    for path in selected_file_paths {
        let file_id = format!("file::{path}");
        base.insert(file_id.clone(), file_id.clone());
        head.insert(file_id.clone(), file_id);
    }

    (base, head)
}

fn build_symbol_nodes(
    selected_symbol_ids: &HashSet<String>,
    symbol_records: &[SymbolRecord],
    file_status_by_path: &HashMap<String, DiffStatus>,
) -> Vec<DiffNodeRecord> {
    let mut nodes = Vec::new();

    for record in symbol_records {
        if !selected_symbol_ids.contains(&record.canonical_id) {
            continue;
        }

        let primary = record.head.as_ref().or(record.base.as_ref()).unwrap();
        let file_path = primary.properties.file_path.to_string();
        let folder = Path::new(primary.properties.file_path.as_str())
            .parent()
            .and_then(|parent| parent.to_str())
            .unwrap_or("")
            .to_string();
        let file_name = Path::new(primary.properties.file_path.as_str())
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        let line = primary.properties.start_line.unwrap_or(0);
        let file_changed = file_status_by_path
            .get(primary.properties.file_path.as_str())
            .copied()
            .unwrap_or(DiffStatus::Unchanged)
            .is_changed();

        nodes.push(DiffNodeRecord {
            id: record.canonical_id.clone(),
            name: primary.properties.name.to_string(),
            label: primary.label.to_string(),
            file: file_path,
            file_name,
            folder,
            line,
            base_file: record
                .base
                .as_ref()
                .map(|node| node.properties.file_path.to_string()),
            base_line: record
                .base
                .as_ref()
                .and_then(|node| node.properties.start_line),
            head_file: record
                .head
                .as_ref()
                .map(|node| node.properties.file_path.to_string()),
            head_line: record
                .head
                .as_ref()
                .and_then(|node| node.properties.start_line),
            status: record.status,
            layer: if record.head.is_some() {
                "head".to_string()
            } else {
                "base".to_string()
            },
            group: group_for_label(primary.label),
            degree: record.degree,
            in_deg: record.in_deg,
            out_deg: record.out_deg,
            return_type: primary
                .properties
                .return_type
                .as_deref()
                .unwrap_or("")
                .to_string(),
            is_async: primary.properties.is_async,
            container_name: primary
                .properties
                .container_name
                .as_deref()
                .unwrap_or("")
                .to_string(),
            qualified_name: primary.qualified_name().to_string(),
            body_changed: record.body_changed,
            file_changed,
            match_kind: record.match_kind.map(match_kind_name).map(str::to_string),
        });
    }

    nodes
}

fn build_file_nodes(
    selected_file_paths: &HashSet<String>,
    file_changes: &[FileDiffRecord],
    base_file_nodes: &HashMap<String, GraphNode>,
    base_store: &GraphStore,
    head_file_nodes: &HashMap<String, GraphNode>,
    head_store: &GraphStore,
) -> Vec<DiffNodeRecord> {
    let mut nodes = Vec::new();

    for file in file_changes {
        if !selected_file_paths.contains(&file.path) {
            continue;
        }

        let primary = head_file_nodes
            .get(&file.path)
            .or_else(|| base_file_nodes.get(&file.path))
            .unwrap();
        let folder = Path::new(file.path.as_str())
            .parent()
            .and_then(|parent| parent.to_str())
            .unwrap_or("")
            .to_string();
        let file_name = Path::new(file.path.as_str())
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("")
            .to_string();
        let (degree, in_deg, out_deg) = if head_file_nodes.contains_key(&file.path) {
            let file_id = format!("file::{}", file.path);
            (
                head_store.degree(&file_id),
                head_store.in_degree(&file_id),
                head_store.out_degree(&file_id),
            )
        } else {
            let file_id = format!("file::{}", file.path);
            (
                base_store.degree(&file_id),
                base_store.in_degree(&file_id),
                base_store.out_degree(&file_id),
            )
        };

        nodes.push(DiffNodeRecord {
            id: format!("file::{}", file.path),
            name: file_name.clone(),
            label: primary.label.to_string(),
            file: file.path.clone(),
            file_name,
            folder,
            line: 0,
            base_file: Some(file.path.clone()),
            base_line: None,
            head_file: if head_file_nodes.contains_key(&file.path) {
                Some(file.path.clone())
            } else {
                None
            },
            head_line: None,
            status: file.status,
            layer: if head_file_nodes.contains_key(&file.path) {
                "head".to_string()
            } else {
                "base".to_string()
            },
            group: group_for_label(NodeLabel::File),
            degree,
            in_deg,
            out_deg,
            return_type: String::new(),
            is_async: false,
            container_name: String::new(),
            qualified_name: file.path.clone(),
            body_changed: file.status == DiffStatus::Modified,
            file_changed: file.status.is_changed(),
            match_kind: None,
        });
    }

    nodes
}

fn select_file_paths(
    selected_symbol_ids: &HashSet<String>,
    symbol_records: &[SymbolRecord],
    file_changes: &[FileDiffRecord],
) -> HashSet<String> {
    let mut paths = HashSet::new();
    for file in file_changes {
        if file.status.is_changed() {
            paths.insert(file.path.clone());
        }
    }

    for record in symbol_records {
        if !selected_symbol_ids.contains(&record.canonical_id) {
            continue;
        }
        if let Some(head) = &record.head {
            paths.insert(head.properties.file_path.to_string());
        }
        if let Some(base) = &record.base {
            paths.insert(base.properties.file_path.to_string());
        }
    }

    paths
}

fn select_symbol_ids(symbol_records: &[SymbolRecord], max_symbols: usize) -> HashSet<String> {
    let mut changed: Vec<&SymbolRecord> = symbol_records
        .iter()
        .filter(|record| record.status.is_changed())
        .collect();
    changed.sort_by(|a, b| {
        status_priority(a.status)
            .cmp(&status_priority(b.status))
            .then_with(|| b.degree.cmp(&a.degree))
            .then_with(|| a.canonical_id.cmp(&b.canonical_id))
    });

    let mut unchanged: Vec<&SymbolRecord> = symbol_records
        .iter()
        .filter(|record| record.status == DiffStatus::Unchanged && record.head.is_some())
        .collect();
    unchanged.sort_by(|a, b| {
        b.degree
            .cmp(&a.degree)
            .then_with(|| a.canonical_id.cmp(&b.canonical_id))
    });

    let mut selected = HashSet::new();

    for record in changed.into_iter().take(max_symbols) {
        selected.insert(record.canonical_id.clone());
    }

    if selected.len() < max_symbols {
        for record in unchanged {
            if selected.len() >= max_symbols {
                break;
            }
            selected.insert(record.canonical_id.clone());
        }
    }

    selected
}

fn status_priority(status: DiffStatus) -> usize {
    match status {
        DiffStatus::Modified => 0,
        DiffStatus::Moved => 1,
        DiffStatus::Added => 2,
        DiffStatus::Removed => 3,
        DiffStatus::Unchanged => 4,
    }
}

fn match_kind_name(kind: MatchKind) -> &'static str {
    match kind {
        MatchKind::Location => "location",
        MatchKind::Symbol => "symbol",
        MatchKind::Body => "body",
    }
}

fn diff_symbols(
    base_symbols: &[GraphNode],
    base_store: &GraphStore,
    head_symbols: &[GraphNode],
    head_store: &GraphStore,
) -> Vec<SymbolRecord> {
    let (base_to_head, head_to_base, match_kinds) = match_symbol_nodes(base_symbols, head_symbols);
    let mut records = Vec::new();

    let base_by_id: HashMap<&str, &GraphNode> = base_symbols
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();

    for head in head_symbols {
        if let Some(base_id) = head_to_base.get(head.id.as_str()) {
            let base = base_by_id
                .get(base_id.as_str())
                .expect("matched base symbol must exist");
            let match_kind = match_kinds.get(head.id.as_str()).copied();
            let moved = match_kind != Some(MatchKind::Location)
                || base.properties.file_path != head.properties.file_path
                || base.qualified_name() != head.qualified_name();
            let body_changed = symbol_body_changed(base, head);
            let status = if moved {
                DiffStatus::Moved
            } else if body_changed {
                DiffStatus::Modified
            } else {
                DiffStatus::Unchanged
            };

            records.push(SymbolRecord {
                canonical_id: head.id.to_string(),
                status,
                match_kind,
                body_changed,
                base: Some((*base).clone()),
                head: Some(head.clone()),
                degree: head_store.degree(&head.id),
                in_deg: head_store.in_degree(&head.id),
                out_deg: head_store.out_degree(&head.id),
            });
        } else {
            records.push(SymbolRecord {
                canonical_id: head.id.to_string(),
                status: DiffStatus::Added,
                match_kind: None,
                body_changed: false,
                base: None,
                head: Some(head.clone()),
                degree: head_store.degree(&head.id),
                in_deg: head_store.in_degree(&head.id),
                out_deg: head_store.out_degree(&head.id),
            });
        }
    }

    for base in base_symbols {
        if base_to_head.contains_key(base.id.as_str()) {
            continue;
        }
        records.push(SymbolRecord {
            canonical_id: format!("removed::{}", base.id),
            status: DiffStatus::Removed,
            match_kind: None,
            body_changed: false,
            base: Some(base.clone()),
            head: None,
            degree: base_store.degree(&base.id),
            in_deg: base_store.in_degree(&base.id),
            out_deg: base_store.out_degree(&base.id),
        });
    }

    records
}

fn symbol_body_changed(base: &GraphNode, head: &GraphNode) -> bool {
    base.properties.body_hash != head.properties.body_hash
        || base.properties.parameter_count != head.properties.parameter_count
        || base.properties.return_type != head.properties.return_type
        || base.properties.visibility != head.properties.visibility
        || base.properties.is_async != head.properties.is_async
        || base.properties.is_static != head.properties.is_static
        || base.properties.annotations != head.properties.annotations
}

fn match_symbol_nodes(
    base_symbols: &[GraphNode],
    head_symbols: &[GraphNode],
) -> (
    HashMap<String, String>,
    HashMap<String, String>,
    HashMap<String, MatchKind>,
) {
    let base_by_id: HashMap<String, &GraphNode> = base_symbols
        .iter()
        .map(|node| (node.id.to_string(), node))
        .collect();
    let head_by_id: HashMap<String, &GraphNode> = head_symbols
        .iter()
        .map(|node| (node.id.to_string(), node))
        .collect();

    let mut base_remaining: HashSet<String> = base_by_id.keys().cloned().collect();
    let mut head_remaining: HashSet<String> = head_by_id.keys().cloned().collect();
    let mut base_to_head = HashMap::new();
    let mut head_to_base = HashMap::new();
    let mut match_kinds = HashMap::new();

    match_unique_stage(
        &base_by_id,
        &head_by_id,
        &mut base_remaining,
        &mut head_remaining,
        |node| Some(node.location_key()),
        MatchKind::Location,
        &mut base_to_head,
        &mut head_to_base,
        &mut match_kinds,
    );

    match_unique_stage(
        &base_by_id,
        &head_by_id,
        &mut base_remaining,
        &mut head_remaining,
        |node| Some(node.symbol_key()),
        MatchKind::Symbol,
        &mut base_to_head,
        &mut head_to_base,
        &mut match_kinds,
    );

    match_unique_stage(
        &base_by_id,
        &head_by_id,
        &mut base_remaining,
        &mut head_remaining,
        body_key,
        MatchKind::Body,
        &mut base_to_head,
        &mut head_to_base,
        &mut match_kinds,
    );

    (base_to_head, head_to_base, match_kinds)
}

fn body_key(node: &GraphNode) -> Option<String> {
    node.properties
        .body_hash
        .map(|hash| format!("{}::{hash}", node.label))
}

#[allow(clippy::too_many_arguments)]
fn match_unique_stage<F>(
    base_by_id: &HashMap<String, &GraphNode>,
    head_by_id: &HashMap<String, &GraphNode>,
    base_remaining: &mut HashSet<String>,
    head_remaining: &mut HashSet<String>,
    key_fn: F,
    kind: MatchKind,
    base_to_head: &mut HashMap<String, String>,
    head_to_base: &mut HashMap<String, String>,
    match_kinds: &mut HashMap<String, MatchKind>,
) where
    F: Fn(&GraphNode) -> Option<String>,
{
    let base_keys = collect_unique_keys(base_remaining, base_by_id, &key_fn);
    let head_keys = collect_unique_keys(head_remaining, head_by_id, &key_fn);
    let mut pairs = Vec::new();

    for (key, base_ids) in &base_keys {
        let Some(head_ids) = head_keys.get(key) else {
            continue;
        };
        if base_ids.len() == 1 && head_ids.len() == 1 {
            pairs.push((base_ids[0].clone(), head_ids[0].clone()));
        }
    }

    for (base_id, head_id) in pairs {
        if !base_remaining.remove(&base_id) || !head_remaining.remove(&head_id) {
            continue;
        }
        base_to_head.insert(base_id.clone(), head_id.clone());
        head_to_base.insert(head_id.clone(), base_id);
        match_kinds.insert(head_id, kind);
    }
}

fn collect_unique_keys<F>(
    ids: &HashSet<String>,
    nodes_by_id: &HashMap<String, &GraphNode>,
    key_fn: &F,
) -> HashMap<String, Vec<String>>
where
    F: Fn(&GraphNode) -> Option<String>,
{
    let mut keys = HashMap::new();
    for id in ids {
        let Some(node) = nodes_by_id.get(id) else {
            continue;
        };
        let Some(key) = key_fn(node) else {
            continue;
        };
        keys.entry(key).or_insert_with(Vec::new).push(id.clone());
    }
    keys
}

fn diff_files(
    base_store: &GraphStore,
    base_file_nodes: &HashMap<String, GraphNode>,
    head_store: &GraphStore,
    head_file_nodes: &HashMap<String, GraphNode>,
) -> Vec<FileDiffRecord> {
    let mut paths: HashSet<String> = base_file_nodes.keys().cloned().collect();
    paths.extend(head_file_nodes.keys().cloned());

    let mut files = Vec::new();
    for path in paths {
        let base_hash = base_store.get_file_hash(&path);
        let head_hash = head_store.get_file_hash(&path);
        let status = match (
            base_file_nodes.contains_key(&path),
            head_file_nodes.contains_key(&path),
        ) {
            (false, true) => DiffStatus::Added,
            (true, false) => DiffStatus::Removed,
            (true, true) if base_hash != head_hash => DiffStatus::Modified,
            _ => DiffStatus::Unchanged,
        };
        files.push(FileDiffRecord {
            path,
            status,
            base_hash,
            head_hash,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn file_nodes(store: &GraphStore) -> HashMap<String, GraphNode> {
    store
        .nodes()
        .filter(|node| node.label == NodeLabel::File)
        .map(|node| (node.properties.file_path.to_string(), node.clone()))
        .collect()
}

fn group_for_label(label: NodeLabel) -> u8 {
    match label {
        NodeLabel::File => 0,
        NodeLabel::Class => 1,
        NodeLabel::Interface => 2,
        NodeLabel::Function => 3,
        NodeLabel::Method => 4,
        NodeLabel::Enum => 5,
        NodeLabel::Property => 6,
        NodeLabel::TypeAlias => 7,
        _ => 8,
    }
}

fn canonical_edge_key(source: &str, rel_type: RelationType, target: &str) -> String {
    format!("{source}::{}::{target}", rel_type)
}

fn export_diff_html(diff: &GraphDiffData, out: &Path) -> Result<()> {
    let data_str = serde_json::to_string(diff)?;
    let stats = format!(
        "{} added | {} removed | {} modified | {} moved",
        diff.summary.symbols_added,
        diff.summary.symbols_removed,
        diff.summary.symbols_modified,
        diff.summary.symbols_moved,
    );
    let title = format!("GitGrapher Diff: {} -> {}", diff.base_ref, diff.head_ref);

    let template = include_str!("../diff_template.html");
    let lib_js = include_str!("../3d-force-graph.min.js");
    let html = template
        .replace("/*3D_FORCE_GRAPH_LIB*/", lib_js)
        .replace("/*DATA_JSON*/null", &data_str)
        .replace("<!--TITLE-->", &title)
        .replace("<!--STATS-->", &stats);

    std::fs::write(out, html)?;
    Ok(())
}

fn print_diff_text(repo_root: &Path, diff: &GraphDiffData) {
    println!();
    println!("  GitGrapher Diff");
    println!("  Repo: {}", repo_root.display());
    println!("  Base: {}", diff.base_ref);
    println!("  Head: {}", diff.head_ref);
    println!();
    println!(
        "  Files: {} added | {} removed | {} modified | {} unchanged",
        diff.summary.files_added,
        diff.summary.files_removed,
        diff.summary.files_modified,
        diff.summary.files_unchanged
    );
    println!(
        "  Symbols: {} added | {} removed | {} modified | {} moved | {} unchanged",
        diff.summary.symbols_added,
        diff.summary.symbols_removed,
        diff.summary.symbols_modified,
        diff.summary.symbols_moved,
        diff.summary.symbols_unchanged
    );
    println!(
        "  Edges: {} added | {} removed",
        diff.summary.edges_added, diff.summary.edges_removed
    );

    let changed_nodes: Vec<_> = diff
        .nodes
        .iter()
        .filter(|node| node.status.is_changed() && node.label != "File")
        .collect();
    if !changed_nodes.is_empty() {
        println!();
        println!("  Changed symbols:");
        for node in changed_nodes.iter().take(20) {
            println!(
                "    {:>8}  {} — {}:{}",
                format!("{:?}", node.status).to_lowercase(),
                node.qualified_name,
                node.file,
                node.line
            );
        }
        if changed_nodes.len() > 20 {
            println!("    ... and {} more", changed_nodes.len() - 20);
        }
    }

    println!();
}

fn load_store_for_revision(repo_root: &Path, revision: &str) -> Result<GraphStore> {
    if revision.eq_ignore_ascii_case(WORKTREE_REF) {
        return Ok(gg_pipeline::analyze(
            repo_root
                .to_str()
                .ok_or_else(|| anyhow!("Repository path is not valid UTF-8"))?,
        )?
        .store);
    }

    let resolved = run_git_capture(
        repo_root,
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )
    .with_context(|| format!("Could not resolve git revision `{revision}`"))?;

    let tempdir = tempfile::tempdir().context("Failed to create temporary directory")?;
    let archive = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["archive", "--format=tar", resolved.trim()])
        .output()
        .with_context(|| format!("Failed to archive revision `{revision}`"))?;

    if !archive.status.success() {
        bail!(
            "git archive failed for `{revision}`: {}",
            String::from_utf8_lossy(&archive.stderr).trim()
        );
    }

    let cursor = Cursor::new(archive.stdout);
    let mut tar = tar::Archive::new(cursor);
    tar.unpack(tempdir.path())
        .with_context(|| format!("Failed to unpack revision `{revision}`"))?;

    Ok(gg_pipeline::analyze(
        tempdir
            .path()
            .to_str()
            .ok_or_else(|| anyhow!("Temporary path is not valid UTF-8"))?,
    )?
    .store)
}

fn git_root(path: &Path) -> Result<PathBuf> {
    let abs = std::fs::canonicalize(path)
        .with_context(|| format!("Could not resolve {}", path.display()))?;
    let output = run_git_capture(&abs, &["rev-parse", "--show-toplevel"])
        .with_context(|| format!("{} is not inside a git repository", abs.display()))?;
    Ok(PathBuf::from(output.trim()))
}

fn run_git_capture(cwd: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;

    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }

    Ok(String::from_utf8(output.stdout)
        .context("git output was not valid UTF-8")?
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gg_core::types::{GraphEdge, Language, NodeProperties};

    fn file_node(path: &str) -> GraphNode {
        GraphNode {
            id: format!("file::{path}").into(),
            label: NodeLabel::File,
            properties: NodeProperties::file(
                Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(path),
                path,
            ),
        }
    }

    fn symbol_node(
        id: &str,
        name: &str,
        label: NodeLabel,
        file: &str,
        line: u32,
        container: Option<&str>,
        body_hash: u64,
    ) -> GraphNode {
        let mut props = NodeProperties::symbol(name, file, Language::TypeScript, line, line + 2);
        props.parameter_count = Some(1);
        props.set_diff_metadata(label, container.map(str::to_string), Some(body_hash));
        GraphNode {
            id: id.into(),
            label,
            properties: props,
        }
    }

    #[test]
    fn test_build_diff_detects_symbol_changes() {
        let mut base = GraphStore::new();
        base.add_node(file_node("src/a.ts"));
        base.add_node(file_node("src/old.ts"));
        base.set_file_hash("src/a.ts".into(), 11);
        base.set_file_hash("src/old.ts".into(), 22);

        let foo_base = symbol_node(
            "src/a.ts::foo::1",
            "foo",
            NodeLabel::Function,
            "src/a.ts",
            1,
            None,
            100,
        );
        let moved_base = symbol_node(
            "src/old.ts::bar::1",
            "bar",
            NodeLabel::Function,
            "src/old.ts",
            1,
            None,
            200,
        );
        let removed_base = symbol_node(
            "src/old.ts::gone::10",
            "gone",
            NodeLabel::Function,
            "src/old.ts",
            10,
            None,
            300,
        );
        base.add_node(foo_base.clone());
        base.add_node(moved_base.clone());
        base.add_node(removed_base.clone());
        base.add_edge(GraphEdge::new(
            "file::src/a.ts",
            foo_base.id.clone(),
            RelationType::Contains,
            1.0,
            "",
        ));
        base.add_edge(GraphEdge::new(
            "file::src/old.ts",
            moved_base.id.clone(),
            RelationType::Contains,
            1.0,
            "",
        ));

        let mut head = GraphStore::new();
        head.add_node(file_node("src/a.ts"));
        head.add_node(file_node("src/new.ts"));
        head.set_file_hash("src/a.ts".into(), 33);
        head.set_file_hash("src/new.ts".into(), 44);

        let foo_head = symbol_node(
            "src/a.ts::foo::1",
            "foo",
            NodeLabel::Function,
            "src/a.ts",
            1,
            None,
            101,
        );
        let moved_head = symbol_node(
            "src/new.ts::bar::1",
            "bar",
            NodeLabel::Function,
            "src/new.ts",
            1,
            None,
            200,
        );
        let added_head = symbol_node(
            "src/new.ts::baz::9",
            "baz",
            NodeLabel::Function,
            "src/new.ts",
            9,
            None,
            500,
        );
        head.add_node(foo_head.clone());
        head.add_node(moved_head.clone());
        head.add_node(added_head.clone());
        head.add_edge(GraphEdge::new(
            "file::src/a.ts",
            foo_head.id.clone(),
            RelationType::Contains,
            1.0,
            "",
        ));
        head.add_edge(GraphEdge::new(
            "file::src/new.ts",
            moved_head.id.clone(),
            RelationType::Contains,
            1.0,
            "",
        ));
        head.add_edge(GraphEdge::new(
            "file::src/new.ts",
            added_head.id.clone(),
            RelationType::Contains,
            1.0,
            "",
        ));

        let diff = build_diff_data("HEAD~1", &base, "HEAD", &head, Some(20));

        assert_eq!(diff.summary.files_added, 1);
        assert_eq!(diff.summary.files_removed, 1);
        assert_eq!(diff.summary.files_modified, 1);
        assert_eq!(diff.summary.symbols_added, 1);
        assert_eq!(diff.summary.symbols_removed, 1);
        assert_eq!(diff.summary.symbols_modified, 1);
        assert_eq!(diff.summary.symbols_moved, 1);

        let statuses: HashMap<_, _> = diff
            .nodes
            .iter()
            .filter(|node| node.label != "File")
            .map(|node| (node.name.as_str(), node.status))
            .collect();
        assert_eq!(statuses.get("foo"), Some(&DiffStatus::Modified));
        assert_eq!(statuses.get("bar"), Some(&DiffStatus::Moved));
        assert_eq!(statuses.get("baz"), Some(&DiffStatus::Added));
        assert_eq!(statuses.get("gone"), Some(&DiffStatus::Removed));
    }
}
