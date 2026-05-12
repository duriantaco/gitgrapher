mod benchmark;
mod diff;
mod mcp;

use clap::{Parser, Subcommand};
use gg_core::types::{NodeLabel, RelationType};
use gg_graph::store::{Direction, GraphStore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const DATA_DIR: &str = ".gitgrapher";
const REGISTRY_DIR: &str = ".gitgrapher";
const REGISTRY_FILE: &str = "registry.json";

#[derive(Parser)]
#[command(name = "gitgrapher")]
#[command(about = "Rust-powered code intelligence for AI agents")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyze a repository and build its knowledge graph
    Analyze {
        /// Path to the repository to analyze
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Enable verbose output
        #[arg(short, long)]
        verbose: bool,
    },
    /// Search the knowledge graph
    Query {
        /// Search query (symbol name or keyword)
        query: String,
        /// Path to the repository
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Maximum results
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
    /// Show a symbol's connections (callers, callees, heritage)
    Context {
        /// Symbol name to look up
        name: String,
        /// Path to the repository
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Show blast radius of a symbol (what depends on it / what it depends on)
    Impact {
        /// Symbol name to analyze
        name: String,
        /// Direction: "up" (what depends on this) or "down" (what this depends on)
        #[arg(short, long, default_value = "up")]
        direction: String,
        /// Max traversal depth
        #[arg(short = 'D', long, default_value = "3")]
        depth: usize,
        /// Path to the repository
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// Show index status for a repository
    Status {
        /// Path to the repository
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Benchmark cold, no-change, and one-file incremental indexing
    Benchmark {
        /// Path to the repository to benchmark
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Output format: text or json
        #[arg(short, long, default_value = "text")]
        format: String,
        /// Source file to mutate for the one-file incremental run
        #[arg(long)]
        sample_file: Option<PathBuf>,
    },
    /// Show AI-agent integration status
    Setup,
    /// Start a stdio MCP server for AI coding agents
    Mcp,
    /// List all indexed repositories
    List,
    /// Remove index for a repository
    Clean {
        /// Path to the repository
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Export graph for visualization (HTML, JSON, or DOT format)
    Export {
        /// Output format: html (interactive), json, dot (Graphviz)
        #[arg(short, long, default_value = "html")]
        format: String,
        /// Output file path (default: gitgrapher-graph.<ext>)
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Path to the repository
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Max nodes to include (default: all)
        #[arg(short = 'n', long)]
        limit: Option<usize>,
    },
    /// Start local web server to explore the graph
    Serve {
        /// Port to listen on
        #[arg(short = 'P', long, default_value = "4747")]
        port: u16,
        /// Path to the repository
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
    },
    /// List all symbols in the graph
    Symbols {
        /// Filter by label (function, class, method, interface, etc.)
        #[arg(short, long)]
        label: Option<String>,
        /// Path to the repository
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Maximum results
        #[arg(short = 'n', long, default_value = "20")]
        limit: usize,
    },
    /// Compare two git revisions and export a diff graph
    Diff {
        /// Git revision to diff from. Use WORKTREE for the current checkout.
        #[arg(short = 'b', long, default_value = "HEAD")]
        base: String,
        /// Git revision to diff to. Use WORKTREE for the current checkout.
        #[arg(short = 'H', long, default_value = "WORKTREE")]
        head: String,
        /// Output format: text, json, html
        #[arg(short, long, default_value = "text")]
        format: String,
        /// Output file path for json/html exports
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Path inside the target repository
        #[arg(short, long, default_value = ".")]
        path: PathBuf,
        /// Maximum number of symbol nodes to include in graph exports
        #[arg(short = 'n', long)]
        limit: Option<usize>,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    match cli.command {
        Commands::Analyze { path, verbose } => cmd_analyze(&path, verbose)?,
        Commands::Query { query, path, limit } => cmd_query(&path, &query, limit)?,
        Commands::Context { name, path } => cmd_context(&path, &name)?,
        Commands::Impact {
            name,
            direction,
            depth,
            path,
        } => cmd_impact(&path, &name, &direction, depth)?,
        Commands::Status { path } => cmd_status(&path)?,
        Commands::Benchmark {
            path,
            format,
            sample_file,
        } => benchmark::cmd_benchmark(&path, &format, sample_file.as_deref())?,
        Commands::Setup => cmd_setup()?,
        Commands::Mcp => mcp::run_stdio()?,
        Commands::List => cmd_list()?,
        Commands::Clean { path } => cmd_clean(&path)?,
        Commands::Export {
            format,
            output,
            path,
            limit,
        } => cmd_export(&path, &format, output.as_deref(), limit)?,
        Commands::Serve { port, path } => cmd_serve(&path, port)?,
        Commands::Symbols { label, path, limit } => cmd_symbols(&path, label.as_deref(), limit)?,
        Commands::Diff {
            base,
            head,
            format,
            output,
            path,
            limit,
        } => diff::cmd_diff(&path, &base, &head, &format, output.as_deref(), limit)?,
    }

    Ok(())
}

fn cmd_analyze(path: &Path, _verbose: bool) -> anyhow::Result<()> {
    let abs_path = std::fs::canonicalize(path)?;
    let result = gg_pipeline::analyze(abs_path.to_str().unwrap_or("."))?;

    // Register in global registry
    let repo_name = abs_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    let mut registry = Registry::load();
    registry.repos.insert(
        repo_name.clone(),
        RepoEntry {
            path: abs_path.to_string_lossy().to_string(),
            nodes: result.total_nodes,
            edges: result.total_edges,
            files: result.files_scanned,
            indexed_at: chrono_now(),
        },
    );
    registry.save();

    println!();
    println!("  GitGrapher Analysis Complete");
    println!();
    println!(
        "  {} nodes | {} edges | {} files",
        result.total_nodes, result.total_edges, result.files_scanned
    );
    println!(
        "  {} imports | {} calls | {} heritage | {} communities | {} processes",
        result.resolution_stats.import_edges_resolved,
        result.resolution_stats.call_edges_resolved,
        result.resolution_stats.heritage_edges_resolved,
        result.resolution_stats.communities,
        result.resolution_stats.processes
    );
    println!("  Languages: {:?}", result.languages_detected);
    println!();
    println!("  Saved to {}/.gitgrapher/", abs_path.display());
    println!("  Registered as \"{}\" in global registry", repo_name);
    println!();
    println!("  Try:  gitgrapher query <name> -p {}", abs_path.display());
    println!(
        "        gitgrapher context <symbol> -p {}",
        abs_path.display()
    );
    println!(
        "        gitgrapher impact <symbol> -p {}",
        abs_path.display()
    );
    println!();

    Ok(())
}

fn load_graph(path: &Path) -> anyhow::Result<GraphStore> {
    let data_dir = path.join(DATA_DIR);
    if !GraphStore::exists(&data_dir) {
        anyhow::bail!(
            "No index found at {}. Run `gitgrapher analyze` first.",
            data_dir.display()
        );
    }
    GraphStore::load(&data_dir).map_err(|e| anyhow::anyhow!(e))
}

fn cmd_query(path: &Path, query: &str, limit: usize) -> anyhow::Result<()> {
    let store = load_graph(path)?;

    // Use Tantivy BM25 search
    let engine = gg_search::SearchEngine::build(&store)
        .map_err(|e| anyhow::anyhow!("Search index error: {e}"))?;
    let results = engine
        .search(query, limit)
        .map_err(|e| anyhow::anyhow!("Search error: {e}"))?;

    if results.is_empty() {
        // Fall back to substring search
        let fallback = store.search_nodes(query, limit);
        if fallback.is_empty() {
            println!("No results for \"{}\"", query);
            return Ok(());
        }
        println!();
        println!(
            "  Results for \"{}\" ({} found, substring match)",
            query,
            fallback.len()
        );
        println!();
        for node in &fallback {
            let exported = if node.properties.is_exported {
                " [exported]"
            } else {
                ""
            };
            println!(
                "  {:>12}  {}{}",
                format!("{}", node.label),
                node.properties.name,
                exported
            );
            println!(
                "               {}:{}",
                node.properties.file_path,
                node.properties.start_line.unwrap_or(0)
            );
        }
        println!();
        return Ok(());
    }

    println!();
    println!("  Results for \"{}\" ({} found)", query, results.len());
    println!();

    for r in &results {
        let exported = if r.is_exported { " [exported]" } else { "" };
        println!(
            "  {:>12}  {}{} (score: {:.2})",
            r.label, r.name, exported, r.score,
        );
        println!("               {}:{}", r.file_path, r.line);
    }
    println!();

    Ok(())
}

fn cmd_context(path: &Path, name: &str) -> anyhow::Result<()> {
    let store = load_graph(path)?;
    let matches = store.nodes_by_name(name);

    if matches.is_empty() {
        println!("No symbol named \"{}\" found.", name);
        return Ok(());
    }

    for node in &matches {
        println!();
        println!("  {} {} ({})", node.label, node.properties.name, node.id);
        println!(
            "  File: {}:{}",
            node.properties.file_path,
            node.properties.start_line.unwrap_or(0)
        );
        if node.properties.is_exported {
            println!("  Exported: yes");
        }
        if let Some(ref rt) = node.properties.return_type {
            println!("  Returns: {}", rt);
        }

        // Incoming edges (who calls/uses this)
        let incoming = store.incoming_edges(&node.id, None);
        if !incoming.is_empty() {
            println!();
            println!("  Incoming ({}):", incoming.len());
            for edge in incoming.iter().take(15) {
                let source_name = store
                    .get_node(&edge.source_id)
                    .map(|n| n.properties.name.as_str())
                    .unwrap_or("?");
                println!(
                    "    <- {} {} (confidence: {:.2})",
                    edge.rel_type, source_name, edge.confidence
                );
            }
            if incoming.len() > 15 {
                println!("    ... and {} more", incoming.len() - 15);
            }
        }

        // Outgoing edges (what this calls/uses)
        let outgoing = store.outgoing_edges(&node.id, None);
        if !outgoing.is_empty() {
            println!();
            println!("  Outgoing ({}):", outgoing.len());
            for edge in outgoing.iter().take(15) {
                let target_name = store
                    .get_node(&edge.target_id)
                    .map(|n| n.properties.name.as_str())
                    .unwrap_or("?");
                println!(
                    "    -> {} {} (confidence: {:.2})",
                    edge.rel_type, target_name, edge.confidence
                );
            }
            if outgoing.len() > 15 {
                println!("    ... and {} more", outgoing.len() - 15);
            }
        }
    }

    println!();
    Ok(())
}

fn cmd_impact(path: &Path, name: &str, direction: &str, depth: usize) -> anyhow::Result<()> {
    let store = load_graph(path)?;
    let matches = store.nodes_by_name(name);

    if matches.is_empty() {
        println!("No symbol named \"{}\" found.", name);
        return Ok(());
    }

    let dir = match direction {
        "up" | "upstream" => Direction::Incoming,
        "down" | "downstream" => Direction::Outgoing,
        "both" => Direction::Both,
        _ => {
            println!("Direction must be 'up', 'down', or 'both'");
            return Ok(());
        }
    };

    let dir_label = match dir {
        Direction::Incoming => "depends on this (upstream)",
        Direction::Outgoing => "this depends on (downstream)",
        Direction::Both => "all connections",
    };

    for node in &matches {
        if !node.label.is_symbol() {
            continue;
        }

        println!();
        println!(
            "  Impact: {} {} — {}",
            node.label, node.properties.name, dir_label
        );
        println!("  Depth: {}", depth);
        println!();

        let hits = store.bfs(
            &node.id,
            dir,
            depth,
            Some(&|edge: &gg_core::types::GraphEdge| {
                // Only follow meaningful edges for impact
                matches!(
                    edge.rel_type,
                    RelationType::Calls
                        | RelationType::Imports
                        | RelationType::Extends
                        | RelationType::Implements
                        | RelationType::Uses
                )
            }),
        );

        if hits.is_empty() {
            println!("  No impact found within depth {}.", depth);
        } else {
            // Group by depth
            let max_depth = hits.iter().map(|h| h.depth).max().unwrap_or(0);
            for d in 1..=max_depth {
                let at_depth: Vec<_> = hits.iter().filter(|h| h.depth == d).collect();
                if at_depth.is_empty() {
                    continue;
                }
                println!("  Depth {}:", d);
                for hit in at_depth.iter().take(20) {
                    if let Some(n) = store.get_node(&hit.node_id) {
                        if n.label.is_symbol() {
                            println!(
                                "    {} {} ({}:{})",
                                n.label,
                                n.properties.name,
                                n.properties.file_path,
                                n.properties.start_line.unwrap_or(0)
                            );
                        }
                    }
                }
                if at_depth.len() > 20 {
                    println!("    ... and {} more", at_depth.len() - 20);
                }
            }
            println!();
            println!("  Total: {} symbols affected", hits.len());
        }
    }

    println!();
    Ok(())
}

fn cmd_status(path: &Path) -> anyhow::Result<()> {
    let data_dir = path.join(DATA_DIR);

    if !GraphStore::exists(&data_dir) {
        println!();
        println!("  Not indexed.");
        println!("  Run: gitgrapher analyze {}", path.display());
        println!();
        return Ok(());
    }

    let store = GraphStore::load(&data_dir).map_err(|e| anyhow::anyhow!(e))?;

    // Count by label
    let functions = store.nodes_by_label(NodeLabel::Function).len();
    let classes = store.nodes_by_label(NodeLabel::Class).len();
    let methods = store.nodes_by_label(NodeLabel::Method).len();
    let interfaces = store.nodes_by_label(NodeLabel::Interface).len();
    let files = store.nodes_by_label(NodeLabel::File).len();

    let graph_path = data_dir.join("graph.json");
    let size = std::fs::metadata(&graph_path).map(|m| m.len()).unwrap_or(0);

    println!();
    println!("  GitGrapher Index: {}", path.display());
    println!();
    println!(
        "  {} nodes | {} edges",
        store.node_count(),
        store.edge_count()
    );
    println!(
        "  {} files | {} functions | {} classes | {} methods | {} interfaces",
        files, functions, classes, methods, interfaces
    );
    println!("  Index size: {:.1} MB", size as f64 / 1_048_576.0);
    println!();

    Ok(())
}

fn cmd_export(
    path: &Path,
    format: &str,
    output: Option<&Path>,
    limit: Option<usize>,
) -> anyhow::Result<()> {
    let store = load_graph(path)?;

    let default_output = match format {
        "json" => PathBuf::from("gitgrapher-graph.json"),
        "dot" => PathBuf::from("gitgrapher-graph.dot"),
        _ => PathBuf::from("gitgrapher-graph.html"),
    };
    let out_path = output.unwrap_or(&default_output);

    match format {
        "json" => export_json(&store, out_path, limit)?,
        "dot" => export_dot(&store, out_path, limit)?,
        "html" => export_html(&store, out_path, limit)?,
        _ => anyhow::bail!("Unknown format: {}. Use html, json, or dot.", format),
    }

    println!("  Exported to {}", out_path.display());
    if format == "html" {
        println!();
        println!("  NOTE: The 3D graph loads Three.js from a CDN.");
        println!("  Browsers block CDN scripts on file:// URLs.");
        println!("  Use the serve command instead:");
        println!();
        println!("    gitgrapher serve -p {}", path.display());
    }

    Ok(())
}

fn export_json(store: &GraphStore, out: &Path, limit: Option<usize>) -> anyhow::Result<()> {
    let nodes: Vec<_> = store
        .nodes()
        .filter(|n| n.label.is_symbol() || matches!(n.label, NodeLabel::File))
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    let node_ids: std::collections::HashSet<_> = nodes.iter().map(|n| &n.id).collect();
    let edges: Vec<_> = store
        .edges()
        .filter(|e| node_ids.contains(&e.source_id) && node_ids.contains(&e.target_id))
        .collect();

    let json = serde_json::json!({
        "nodes": nodes.iter().map(|n| serde_json::json!({
            "id": n.id,
            "label": format!("{}", n.label),
            "name": n.properties.name,
            "file": n.properties.file_path,
            "line": n.properties.start_line,
            "exported": n.properties.is_exported,
        })).collect::<Vec<_>>(),
        "edges": edges.iter().map(|e| serde_json::json!({
            "source": e.source_id,
            "target": e.target_id,
            "type": format!("{}", e.rel_type),
            "confidence": e.confidence,
        })).collect::<Vec<_>>(),
    });

    std::fs::write(out, serde_json::to_string_pretty(&json)?)?;
    Ok(())
}

fn export_dot(store: &GraphStore, out: &Path, limit: Option<usize>) -> anyhow::Result<()> {
    let mut dot =
        String::from("digraph gitgrapher {\n  rankdir=LR;\n  node [shape=box, style=filled];\n\n");

    let nodes: Vec<_> = store
        .nodes()
        .filter(|n| n.label.is_symbol())
        .take(limit.unwrap_or(500))
        .collect();
    let node_ids: std::collections::HashSet<_> = nodes.iter().map(|n| &n.id).collect();

    for n in &nodes {
        let color = match n.label {
            NodeLabel::Class => "#4FC3F7",
            NodeLabel::Interface => "#81C784",
            NodeLabel::Function => "#FFB74D",
            NodeLabel::Method => "#FF8A65",
            NodeLabel::Enum => "#CE93D8",
            _ => "#E0E0E0",
        };
        let safe_id =
            n.id.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
        dot.push_str(&format!(
            "  {} [label=\"{}\\n({})\", fillcolor=\"{}\"];\n",
            safe_id, n.properties.name, n.label, color
        ));
    }

    dot.push('\n');

    for e in store.edges() {
        if !node_ids.contains(&e.source_id) || !node_ids.contains(&e.target_id) {
            continue;
        }
        let src = e
            .source_id
            .replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
        let tgt = e
            .target_id
            .replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
        let color = match e.rel_type {
            RelationType::Calls => "red",
            RelationType::Imports => "blue",
            RelationType::Extends | RelationType::Implements => "green",
            RelationType::Contains => "gray",
            _ => "black",
        };
        dot.push_str(&format!(
            "  {} -> {} [color={}, label=\"{}\"];\n",
            src, tgt, color, e.rel_type
        ));
    }

    dot.push_str("}\n");
    std::fs::write(out, dot)?;
    Ok(())
}

fn export_html(store: &GraphStore, out: &Path, limit: Option<usize>) -> anyhow::Result<()> {
    let max_nodes = limit.unwrap_or(3000);

    let mut node_list: Vec<_> = store.nodes().filter(|n| n.label.is_symbol()).collect();
    node_list.sort_by_key(|node| std::cmp::Reverse(store.degree(&node.id)));
    let nodes: Vec<_> = node_list.into_iter().take(max_nodes).collect();
    let node_ids: std::collections::HashSet<_> = nodes.iter().map(|n| &n.id).collect();
    let edges: Vec<_> = store
        .edges()
        .filter(|e| node_ids.contains(&e.source_id) && node_ids.contains(&e.target_id))
        .filter(|e| e.rel_type != RelationType::Contains)
        .collect();

    // Build JSON data
    let nodes_json: Vec<serde_json::Value> = nodes
        .iter()
        .map(|n| {
            let group = match n.label {
                NodeLabel::Class => 1,
                NodeLabel::Interface => 2,
                NodeLabel::Function => 3,
                NodeLabel::Method => 4,
                NodeLabel::Enum => 5,
                NodeLabel::Property => 6,
                NodeLabel::TypeAlias => 7,
                _ => 8,
            };
            let file_name = Path::new(n.properties.file_path.as_str())
                .file_name()
                .and_then(|f| f.to_str())
                .unwrap_or("");
            let folder = Path::new(n.properties.file_path.as_str())
                .parent()
                .and_then(|p| p.to_str())
                .unwrap_or("");
            serde_json::json!({
                "id": n.id,
                "name": n.properties.name,
                "label": format!("{}", n.label),
                "file": n.properties.file_path,
                "fileName": file_name,
                "folder": folder,
                "line": n.properties.start_line.unwrap_or(0),
                "exported": n.properties.is_exported,
                "group": group,
                "degree": store.degree(&n.id),
                "inDeg": store.in_degree(&n.id),
                "outDeg": store.out_degree(&n.id),
                "returnType": n.properties.return_type.as_deref().unwrap_or(""),
                "isAsync": n.properties.is_async,
            })
        })
        .collect();

    let links_json: Vec<serde_json::Value> = edges
        .iter()
        .map(|e| {
            serde_json::json!({
                "source": e.source_id,
                "target": e.target_id,
                "type": format!("{}", e.rel_type),
                "confidence": e.confidence,
            })
        })
        .collect();

    let data = serde_json::json!({
        "nodes": nodes_json,
        "links": links_json,
    });
    let data_str = serde_json::to_string(&data)?;

    let stats = format!(
        "{} symbols | {} connections (of {} total nodes)",
        nodes.len(),
        edges.len(),
        store.node_count()
    );

    // Load template and inline the 3D library (no CDN = no CORS issues)
    let template = include_str!("../template.html");
    let lib_js = include_str!("../3d-force-graph.min.js");
    let html = template
        .replace("/*3D_FORCE_GRAPH_LIB*/", lib_js)
        .replace("/*DATA_JSON*/null", &data_str)
        .replace("<!--STATS-->", &stats);

    std::fs::write(out, html)?;
    Ok(())
}

fn cmd_serve(path: &Path, port: u16) -> anyhow::Result<()> {
    let abs_path = std::fs::canonicalize(path)?;
    let data_dir = abs_path.join(DATA_DIR);
    if !GraphStore::exists(&data_dir) {
        anyhow::bail!("No index found. Run `gitgrapher analyze` first.");
    }

    // Export to a temp HTML file and serve it
    let tmp_dir = std::env::temp_dir().join("gitgrapher-serve");
    std::fs::create_dir_all(&tmp_dir)?;
    let html_path = tmp_dir.join("index.html");

    let store = GraphStore::load(&data_dir).map_err(|e| anyhow::anyhow!(e))?;
    export_html(&store, &html_path, Some(2000))?;

    // Also write graph.json for API access
    let json_path = tmp_dir.join("graph.json");
    export_json(&store, &json_path, None)?;

    let url = format!("http://127.0.0.1:{}", port);
    println!();
    println!("  GitGrapher Server");
    println!(
        "  Graph: {} nodes, {} edges",
        store.node_count(),
        store.edge_count()
    );
    println!();
    println!("  Open: {}", url);
    println!("  API:  {}/graph.json", url);
    println!();
    println!("  Press Ctrl+C to stop");
    println!();

    // Simple static file server
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind(format!("127.0.0.1:{}", port))?;

    // Auto-open browser
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/c", "start", &url])
        .spawn();

    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(_) => continue,
        };

        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);

        let (file_path, content_type) = if request.contains("GET /graph.json") {
            (&json_path, "application/json")
        } else {
            (&html_path, "text/html")
        };

        let body = std::fs::read(file_path).unwrap_or_default();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
            content_type, body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(&body);
    }

    Ok(())
}

fn cmd_symbols(path: &Path, label_filter: Option<&str>, limit: usize) -> anyhow::Result<()> {
    let store = load_graph(path)?;

    let label = label_filter.and_then(|l| match l.to_lowercase().as_str() {
        "function" | "fn" => Some(NodeLabel::Function),
        "class" => Some(NodeLabel::Class),
        "method" => Some(NodeLabel::Method),
        "interface" => Some(NodeLabel::Interface),
        "enum" => Some(NodeLabel::Enum),
        "type" | "typealias" => Some(NodeLabel::TypeAlias),
        "variable" | "var" => Some(NodeLabel::Variable),
        "property" | "prop" => Some(NodeLabel::Property),
        "struct" => Some(NodeLabel::Struct),
        "trait" => Some(NodeLabel::Trait),
        _ => None,
    });

    let nodes: Vec<_> = if let Some(l) = label {
        store.nodes_by_label(l).into_iter().take(limit).collect()
    } else {
        store
            .nodes()
            .filter(|n| n.label.is_symbol())
            .take(limit)
            .collect()
    };

    if nodes.is_empty() {
        println!("No symbols found.");
        return Ok(());
    }

    println!();
    println!("  Symbols ({}):", nodes.len());
    println!();

    for node in &nodes {
        let exported = if node.properties.is_exported {
            " [exported]"
        } else {
            ""
        };
        println!(
            "  {:>12}  {}{} — {}:{}",
            format!("{}", node.label),
            node.properties.name,
            exported,
            node.properties.file_path,
            node.properties.start_line.unwrap_or(0),
        );
    }
    println!();

    Ok(())
}

fn cmd_setup() -> anyhow::Result<()> {
    println!();
    println!("  GitGrapher AI-agent setup");
    println!();
    println!("  MCP support is available over stdio:");
    println!("    gitgrapher mcp");
    println!();
    println!("  Use that command in Claude Code, Cursor, or any MCP client");
    println!("  that can launch a local stdio server. This command does not");
    println!("  write editor configuration automatically.");
    println!();
    println!("  Today, use the local CLI commands:");
    println!("    gitgrapher analyze /path/to/repo");
    println!("    gitgrapher query <term> -p /path/to/repo");
    println!("    gitgrapher context <symbol> -p /path/to/repo");
    println!("    gitgrapher impact <symbol> -p /path/to/repo");

    let registry = Registry::load();
    if !registry.repos.is_empty() {
        println!();
        println!("  Indexed repositories:");
        for (name, entry) in &registry.repos {
            println!("    {} — {}", name, entry.path);
        }
    }
    println!();

    Ok(())
}

fn cmd_list() -> anyhow::Result<()> {
    let registry = Registry::load();

    if registry.repos.is_empty() {
        println!();
        println!("  No indexed repositories.");
        println!("  Run: gitgrapher analyze /path/to/repo");
        println!();
        return Ok(());
    }

    println!();
    println!("  Indexed repositories ({}):", registry.repos.len());
    println!();

    for (name, entry) in &registry.repos {
        let stale = if GraphStore::exists(&Path::new(&entry.path).join(DATA_DIR)) {
            ""
        } else {
            " [stale - index missing]"
        };
        println!(
            "  {:>20}  {} nodes | {} edges | {} files{}",
            name, entry.nodes, entry.edges, entry.files, stale
        );
        println!("                       {}", entry.path);
        if !entry.indexed_at.is_empty() {
            println!("                       indexed: {}", entry.indexed_at);
        }
    }
    println!();

    Ok(())
}

fn cmd_clean(path: &Path) -> anyhow::Result<()> {
    let abs_path = std::fs::canonicalize(path)?;
    let data_dir = abs_path.join(DATA_DIR);

    if !data_dir.exists() {
        println!("  No index found at {}", data_dir.display());
        return Ok(());
    }

    std::fs::remove_dir_all(&data_dir)?;
    println!("  Removed index at {}", data_dir.display());

    // Remove from registry
    let repo_name = abs_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let mut registry = Registry::load();
    if registry.repos.remove(&repo_name).is_some() {
        registry.save();
        println!("  Removed \"{}\" from global registry", repo_name);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Global registry (~/.gitgrapher/registry.json)
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, Default)]
struct Registry {
    repos: HashMap<String, RepoEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepoEntry {
    path: String,
    nodes: usize,
    edges: usize,
    files: usize,
    indexed_at: String,
}

impl Registry {
    fn registry_path() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(REGISTRY_DIR).join(REGISTRY_FILE)
    }

    fn load() -> Self {
        let path = Self::registry_path();
        if path.exists() {
            let data = std::fs::read_to_string(&path).unwrap_or_default();
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    fn save(&self) {
        let path = Self::registry_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }
}

fn chrono_now() -> String {
    // Simple timestamp without chrono dependency
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    // Rough ISO format: days since epoch * readable
    format!("unix:{}", secs)
}
