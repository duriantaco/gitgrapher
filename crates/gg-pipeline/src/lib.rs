mod community;
mod process;

use gg_core::config::Config;
use gg_core::error::GgResult;
use gg_core::types::*;
use gg_graph::GraphStore;
use gg_parse::scanner::{scan_repository, SourceFile};
use gg_parse::LanguageRegistry;
use gg_resolve::call_resolver::{build_implementor_map, CallResolver};
use gg_resolve::heritage_resolver::HeritageResolver;
use gg_resolve::import_resolver::ImportResolver;
use gg_resolve::resolution::ResolutionContext;
use gg_resolve::symbol_table::SymbolTable;
use rayon::prelude::*;
use smol_str::SmolStr;
use std::path::Path;
use tracing::{debug, info};

/// The standard directory name for gitgrapher data inside a repo.
pub const DATA_DIR: &str = ".gitgrapher";

/// Result of a full pipeline analysis.
#[derive(Debug)]
pub struct AnalyzeResult {
    pub files_scanned: usize,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub languages_detected: Vec<Language>,
    pub resolution_stats: ResolutionStats,
    pub store: GraphStore,
}

#[derive(Debug, Default)]
pub struct ResolutionStats {
    pub import_edges_resolved: usize,
    pub call_edges_resolved: usize,
    pub heritage_edges_resolved: usize,
    pub named_bindings: usize,
    pub communities: usize,
    pub processes: usize,
}

/// Analyze a repository: scan, parse, resolve, and build the knowledge graph.
/// Hash file contents with xxhash for change detection.
fn hash_file(path: &std::path::Path) -> Option<u64> {
    let data = std::fs::read(path).ok()?;
    Some(xxhash_rust::xxh3::xxh3_64(&data))
}

/// Changeset from comparing current files against stored hashes.
#[derive(Debug)]
struct ChangeSet {
    added: Vec<usize>,    // indices into `files`
    modified: Vec<usize>, // indices into `files`
    deleted: Vec<String>, // relative paths of removed files
    unchanged: Vec<usize>,
}

pub fn analyze(repo_path: &str) -> GgResult<AnalyzeResult> {
    let config = Config::from_env();
    let registry = LanguageRegistry::new();
    let root = Path::new(repo_path);

    info!("Scanning repository: {}", repo_path);

    // -----------------------------------------------------------------------
    // Phase 1: Scan filesystem
    // -----------------------------------------------------------------------
    let files = scan_repository(root, &config);
    info!("Found {} source files", files.len());

    if files.is_empty() {
        return Ok(AnalyzeResult {
            files_scanned: 0,
            total_nodes: 0,
            total_edges: 0,
            languages_detected: vec![],
            resolution_stats: ResolutionStats::default(),
            store: GraphStore::new(),
        });
    }

    let mut languages: Vec<Language> = files.iter().map(|f| f.language).collect();
    languages.sort_by_key(|l| l.as_str());
    languages.dedup();

    // -----------------------------------------------------------------------
    // Phase 1b: Check for existing index (incremental mode)
    // -----------------------------------------------------------------------
    let data_dir = root.join(DATA_DIR);
    let existing_store = if GraphStore::exists(&data_dir) {
        match GraphStore::load(&data_dir) {
            Ok(s) => Some(s),
            Err(e) => {
                info!("Could not load existing index ({}), doing full reindex", e);
                None
            }
        }
    } else {
        None
    };

    // Compute file hashes and detect changes
    let current_hashes: Vec<(usize, u64)> = files
        .iter()
        .enumerate()
        .filter_map(|(i, f)| hash_file(&f.path).map(|h| (i, h)))
        .collect();

    let changeset = if let Some(ref old_store) = existing_store {
        let mut added = Vec::new();
        let mut modified = Vec::new();
        let mut unchanged = Vec::new();
        let mut seen_files = std::collections::HashSet::new();

        for &(idx, hash) in &current_hashes {
            let rel = &files[idx].relative_path;
            seen_files.insert(rel.clone());

            match old_store.get_file_hash(rel) {
                Some(old_hash) if old_hash == hash => unchanged.push(idx),
                Some(_) => modified.push(idx),
                None => added.push(idx),
            }
        }

        // Find deleted files (in old store but not in current scan)
        let mut deleted = Vec::new();
        for node in old_store.nodes() {
            if node.label == NodeLabel::File {
                let fp = node.properties.file_path.as_str();
                if !seen_files.contains(fp) {
                    deleted.push(fp.to_string());
                }
            }
        }

        ChangeSet {
            added,
            modified,
            deleted,
            unchanged,
        }
    } else {
        // No existing store — everything is new
        ChangeSet {
            added: (0..files.len()).collect(),
            modified: vec![],
            deleted: vec![],
            unchanged: vec![],
        }
    };

    let is_incremental = existing_store.is_some()
        && !changeset.unchanged.is_empty()
        && (changeset.added.len() + changeset.modified.len() + changeset.deleted.len()) > 0;

    let files_to_parse: Vec<usize> = if is_incremental {
        info!(
            "Incremental: {} added, {} modified, {} deleted, {} unchanged",
            changeset.added.len(),
            changeset.modified.len(),
            changeset.deleted.len(),
            changeset.unchanged.len()
        );
        let mut to_parse = changeset.added.clone();
        to_parse.extend(&changeset.modified);
        to_parse
    } else {
        if existing_store.is_some()
            && changeset.added.is_empty()
            && changeset.modified.is_empty()
            && changeset.deleted.is_empty()
        {
            info!("No changes detected — index is up to date");
            let store = existing_store.unwrap();
            return Ok(AnalyzeResult {
                files_scanned: 0,
                total_nodes: store.node_count(),
                total_edges: store.edge_count(),
                languages_detected: languages,
                resolution_stats: ResolutionStats::default(),
                store,
            });
        }
        info!("Full index (no previous index or too many changes)");
        (0..files.len()).collect()
    };

    // -----------------------------------------------------------------------
    // Phase 2: Build structure nodes (File, Folder)
    // -----------------------------------------------------------------------
    let mut store = if is_incremental {
        let mut s = existing_store.unwrap();
        // Remove nodes for deleted and modified files
        for path in &changeset.deleted {
            s.remove_file(path);
        }
        for &idx in &changeset.modified {
            s.remove_file(&files[idx].relative_path);
        }
        // Add structure for new files only
        let new_files: Vec<_> = changeset
            .added
            .iter()
            .map(|&i| &files[i])
            .cloned()
            .collect();
        build_structure(&new_files, root, &mut s);
        s
    } else {
        let mut s = GraphStore::new();
        build_structure(&files, root, &mut s);
        s
    };

    // Store file hashes for next incremental run
    for &(idx, hash) in &current_hashes {
        store.set_file_hash(SmolStr::new(&files[idx].relative_path), hash);
    }

    // -----------------------------------------------------------------------
    // Phase 3: Parse files in parallel using Rayon
    // -----------------------------------------------------------------------
    let parse_count = files_to_parse.len();
    info!(
        "Parsing {} files across {} threads...",
        parse_count, config.worker_threads
    );

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.worker_threads)
        .build()
        .unwrap_or_else(|_| rayon::ThreadPoolBuilder::new().build().unwrap());

    let parse_results: Vec<(String, ParseResult)> = pool.install(|| {
        files_to_parse
            .par_iter()
            .filter_map(|&idx| {
                let file = &files[idx];
                let source = match std::fs::read(&file.path) {
                    Ok(s) => s,
                    Err(e) => {
                        debug!("Failed to read {}: {}", file.path.display(), e);
                        return None;
                    }
                };

                match registry.parse_file(&file.path, &source, &config) {
                    Ok(result) => Some((file.relative_path.clone(), result)),
                    Err(e) => {
                        debug!("Failed to parse {}: {}", file.path.display(), e);
                        None
                    }
                }
            })
            .collect()
    });

    let files_parsed = parse_results.len();
    info!("Parsed {} files successfully", files_parsed);

    // -----------------------------------------------------------------------
    // Phase 4: Populate symbol table + graph nodes
    // -----------------------------------------------------------------------
    let symbols = SymbolTable::new();

    for (file_path, result) in &parse_results {
        let file_node_id = SmolStr::new(format!("file::{}", file_path));

        for node in &result.nodes {
            // Add to graph store
            store.add_node(node.clone());

            // Add to symbol table for resolution
            symbols.insert(gg_resolve::symbol_table::SymbolDefinition {
                node_id: node.id.clone(),
                name: node.properties.name.clone(),
                label: node.label,
                file_path: SmolStr::new(file_path.as_str()),
                language: node.properties.language.unwrap_or(Language::TypeScript),
                is_exported: node.properties.is_exported,
                return_type: node.properties.return_type.clone(),
                start_line: node.properties.start_line,
                end_line: node.properties.end_line,
            });

            // File CONTAINS definition
            store.add_edge(GraphEdge::new(
                file_node_id.clone(),
                node.id.clone(),
                RelationType::Contains,
                1.0,
                "file contains definition",
            ));
        }
    }

    info!("Symbol table: {} symbols", symbols.total_symbols());

    // -----------------------------------------------------------------------
    // Phase 5: Import resolution (cross-file linking)
    // -----------------------------------------------------------------------
    info!("Resolving imports...");

    let all_file_paths: Vec<String> = parse_results.iter().map(|(f, _)| f.clone()).collect();
    let mut import_resolver = ImportResolver::new(all_file_paths.clone());
    let mut resolution_ctx = ResolutionContext::new();
    let mut stats = ResolutionStats::default();

    for (file_path, result) in &parse_results {
        for import in &result.imports {
            let lang = Language::from_extension(
                Path::new(file_path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("ts"),
            )
            .unwrap_or(Language::TypeScript);

            // Resolve the import path to a target file
            let target = import_resolver.resolve(&import.module_path, file_path, lang);

            if let Some(ref target_file) = target {
                // Record the import edge in the resolution context
                resolution_ctx.add_import_edge(
                    SmolStr::new(file_path.as_str()),
                    SmolStr::new(target_file.as_str()),
                );

                // Record named bindings for precise symbol resolution
                if !import.is_namespace {
                    let exported_name = if import.is_default {
                        import.imported_name.clone()
                    } else {
                        import.imported_name.clone()
                    };
                    let local_name = import
                        .alias
                        .as_ref()
                        .unwrap_or(&import.imported_name)
                        .clone();

                    resolution_ctx.add_named_binding(
                        SmolStr::new(file_path.as_str()),
                        local_name,
                        SmolStr::new(target_file.as_str()),
                        exported_name,
                    );
                    stats.named_bindings += 1;
                }

                // Add IMPORTS edge to the graph
                let file_node_id = SmolStr::new(format!("file::{}", file_path));
                let target_file_id = SmolStr::new(format!("file::{}", target_file));
                store.add_edge(GraphEdge::new(
                    file_node_id,
                    target_file_id,
                    RelationType::Imports,
                    0.9,
                    format!("imports {}", import.module_path),
                ));
                stats.import_edges_resolved += 1;
            }
        }
    }

    info!(
        "Import resolution: {} edges, {} named bindings",
        stats.import_edges_resolved, stats.named_bindings
    );

    // -----------------------------------------------------------------------
    // Phase 6: Call resolution (cross-file)
    // -----------------------------------------------------------------------
    info!("Resolving calls...");

    // Build implementor map for interface dispatch
    let all_heritage: Vec<(SmolStr, ExtractedHeritage)> = parse_results
        .iter()
        .flat_map(|(f, r)| {
            r.heritage
                .iter()
                .map(|h| (SmolStr::new(f.as_str()), h.clone()))
        })
        .collect();
    let implementor_map = build_implementor_map(&all_heritage);

    for (file_path, result) in &parse_results {
        // Resolve each call individually
        for call in &result.calls {
            let targets = CallResolver::resolve_calls(
                &[call.clone()],
                file_path,
                &mut resolution_ctx,
                &symbols,
                &implementor_map,
            );

            if !targets.is_empty() {
                for t in &targets {
                    store.add_edge(GraphEdge::new(
                        call.caller_id.clone(),
                        t.target_node_id.clone(),
                        RelationType::Calls,
                        t.confidence,
                        t.reason.clone(),
                    ));
                    stats.call_edges_resolved += 1;
                }
            }
        }
    }

    info!("Call resolution: {} edges", stats.call_edges_resolved);

    // -----------------------------------------------------------------------
    // Phase 7: Heritage resolution (extends/implements)
    // -----------------------------------------------------------------------
    info!("Resolving heritage...");

    let all_heritage_items: Vec<ExtractedHeritage> = parse_results
        .iter()
        .flat_map(|(_, r)| r.heritage.clone())
        .collect();

    let resolved_heritage =
        HeritageResolver::resolve_heritage(&all_heritage_items, &mut resolution_ctx, &symbols);

    for rh in &resolved_heritage {
        store.add_edge(GraphEdge::new(
            rh.child_id.clone(),
            rh.parent_id.clone(),
            rh.rel_type,
            rh.confidence,
            rh.reason.clone(),
        ));
        stats.heritage_edges_resolved += 1;
    }

    info!(
        "Heritage resolution: {} edges",
        stats.heritage_edges_resolved
    );

    // -----------------------------------------------------------------------
    // Phase 8: Community detection (Louvain clustering)
    // -----------------------------------------------------------------------
    info!("Detecting communities...");

    let community_result = community::detect_communities(&store, config.community_resolution);
    for node in &community_result.community_nodes {
        store.add_node(node.clone());
    }
    for edge in &community_result.membership_edges {
        store.add_edge(edge.clone());
    }
    stats.communities = community_result.stats.total_communities;

    info!(
        "Community detection: {} communities ({} nodes)",
        community_result.stats.total_communities, community_result.stats.nodes_processed
    );

    // -----------------------------------------------------------------------
    // Phase 9: Process detection (entry points + flow tracing)
    // -----------------------------------------------------------------------
    info!("Detecting processes...");

    let process_config = process::ProcessConfig {
        max_trace_depth: config.max_trace_depth,
        max_processes: config.max_processes,
        ..Default::default()
    };
    let process_result =
        process::detect_processes(&store, &community_result.membership_map, &process_config);
    for node in &process_result.process_nodes {
        store.add_node(node.clone());
    }
    for edge in &process_result.step_edges {
        store.add_edge(edge.clone());
    }
    stats.processes = process_result.stats.total_processes;

    info!(
        "Process detection: {} processes ({} entry points, avg {:.1} steps)",
        process_result.stats.total_processes,
        process_result.stats.entry_points_found,
        process_result.stats.avg_step_count
    );

    // -----------------------------------------------------------------------
    // Persist
    // -----------------------------------------------------------------------
    let total_nodes = store.node_count();
    let total_edges = store.edge_count();

    let data_dir = root.join(DATA_DIR);
    store
        .save(&data_dir)
        .map_err(|e| gg_core::error::GgError::Serialization(e))?;
    info!("Graph saved to {}", data_dir.display());

    info!(
        "Analysis complete: {} files, {} nodes, {} edges",
        files_parsed, total_nodes, total_edges
    );

    Ok(AnalyzeResult {
        files_scanned: files_parsed,
        total_nodes,
        total_edges,
        languages_detected: languages,
        resolution_stats: stats,
        store,
    })
}

/// Build File and Folder nodes from scanned source files.
fn build_structure(files: &[SourceFile], root: &Path, store: &mut GraphStore) {
    use std::collections::HashSet;

    let mut seen_folders = HashSet::new();

    let root_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    store.add_node(GraphNode {
        id: SmolStr::new("project::root"),
        label: NodeLabel::Project,
        properties: NodeProperties::file(root_name, root.to_string_lossy().as_ref()),
    });

    for file in files {
        let file_id = SmolStr::new(format!("file::{}", file.relative_path));
        let file_name = file.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let mut props = NodeProperties::file(file_name, &file.relative_path);
        props.language = Some(file.language);
        store.add_node(GraphNode {
            id: file_id.clone(),
            label: NodeLabel::File,
            properties: props,
        });

        let rel_path = Path::new(&file.relative_path);
        if let Some(parent) = rel_path.parent() {
            let parent_str = parent.to_string_lossy().to_string();
            if !parent_str.is_empty() && seen_folders.insert(parent_str.clone()) {
                let folder_id = SmolStr::new(format!("folder::{}", parent_str));
                let folder_name = parent
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&parent_str);
                store.add_node(GraphNode {
                    id: folder_id.clone(),
                    label: NodeLabel::Folder,
                    properties: NodeProperties::file(folder_name, &parent_str),
                });

                store.add_edge(GraphEdge::new(
                    folder_id,
                    file_id.clone(),
                    RelationType::Contains,
                    1.0,
                    "folder contains file",
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_analyze_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let result = analyze(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result.files_scanned, 0);
        assert_eq!(result.total_nodes, 0);
    }

    #[test]
    fn test_analyze_single_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("index.ts"),
            r#"
            export function greet(name: string): string {
                return `Hello, ${name}!`;
            }

            export class Greeter {
                greet(name: string): string {
                    return greet(name);
                }
            }
            "#,
        )
        .unwrap();

        let result = analyze(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result.files_scanned, 1);
        assert!(result.total_nodes > 0);
        assert!(result.total_edges > 0);
        assert!(result.languages_detected.contains(&Language::TypeScript));
    }

    #[test]
    fn test_cross_file_import_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();

        fs::write(
            src.join("user.ts"),
            r#"
            export interface User {
                name: string;
                email: string;
            }

            export class UserService {
                getUser(id: string): User {
                    return { name: "test", email: "test@test.com" };
                }
            }
            "#,
        )
        .unwrap();

        fs::write(
            src.join("main.ts"),
            r#"
            import { UserService } from './user';

            function main() {
                const service = new UserService();
                service.getUser("1");
            }
            "#,
        )
        .unwrap();

        let result = analyze(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result.files_scanned, 2);

        // Should have resolved the import
        assert!(
            result.resolution_stats.import_edges_resolved > 0,
            "Should resolve cross-file imports, got {} edges",
            result.resolution_stats.import_edges_resolved
        );

        // Should have named bindings
        assert!(
            result.resolution_stats.named_bindings > 0,
            "Should have named bindings"
        );
    }

    #[test]
    fn test_cross_file_heritage() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        fs::create_dir(&src).unwrap();

        fs::write(
            src.join("base.ts"),
            r#"
            export class BaseService {
                log(msg: string): void {
                    console.log(msg);
                }
            }
            "#,
        )
        .unwrap();

        fs::write(
            src.join("user-service.ts"),
            r#"
            import { BaseService } from './base';

            export class UserService extends BaseService {
                getUser(id: string): string {
                    this.log("Getting user");
                    return id;
                }
            }
            "#,
        )
        .unwrap();

        let result = analyze(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result.files_scanned, 2);
        assert!(
            result.resolution_stats.heritage_edges_resolved > 0,
            "Should resolve heritage, got {} edges",
            result.resolution_stats.heritage_edges_resolved
        );
    }

    #[test]
    fn test_barrel_export_resolution() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        let models = src.join("models");
        fs::create_dir_all(&models).unwrap();

        fs::write(
            models.join("user.ts"),
            r#"
            export class User {
                constructor(public name: string) {}
            }
            "#,
        )
        .unwrap();

        fs::write(
            models.join("index.ts"),
            r#"
            export { User } from './user';
            "#,
        )
        .unwrap();

        fs::write(
            src.join("app.ts"),
            r#"
            import { User } from './models';

            const user = new User("test");
            "#,
        )
        .unwrap();

        let result = analyze(dir.path().to_str().unwrap()).unwrap();
        assert_eq!(result.files_scanned, 3);
        assert!(
            result.resolution_stats.import_edges_resolved > 0,
            "Should resolve barrel exports"
        );
    }
}
