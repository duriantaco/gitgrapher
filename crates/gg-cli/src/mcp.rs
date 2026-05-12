use gg_core::types::{GraphEdge, GraphNode, RelationType};
use gg_graph::store::{Direction, GraphStore};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

const DATA_DIR: &str = ".gitgrapher";
const REGISTRY_DIR: &str = ".gitgrapher";
const REGISTRY_FILE: &str = "registry.json";
const SUPPORTED_PROTOCOL_VERSION: &str = "2025-06-18";
const COMPATIBLE_PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-03-26", "2024-11-05"];

pub fn run_stdio() -> anyhow::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle_message(message),
            Err(err) => Some(error_response(
                Value::Null,
                -32700,
                format!("Parse error: {err}"),
            )),
        };

        if let Some(response) = response {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }

    Ok(())
}

fn handle_message(message: Value) -> Option<Value> {
    let id = message.get("id").cloned();
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();

    if id.is_none() {
        return handle_notification(method);
    }

    let id = id.unwrap();
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match method {
        "initialize" => Ok(initialize_result(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => handle_tool_call(&params),
        "resources/list" => Ok(json!({ "resources": resources_list() })),
        "resources/read" => handle_resource_read(&params),
        _ => Err(McpError::method_not_found(method)),
    };

    Some(match result {
        Ok(result) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }),
        Err(err) => error_response(id, err.code, err.message),
    })
}

fn handle_notification(method: &str) -> Option<Value> {
    match method {
        "notifications/initialized" | "notifications/cancelled" | "$/cancelRequest" => None,
        _ => None,
    }
}

fn initialize_result(params: &Value) -> Value {
    let requested = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or(SUPPORTED_PROTOCOL_VERSION);
    let protocol_version = if requested == SUPPORTED_PROTOCOL_VERSION
        || COMPATIBLE_PROTOCOL_VERSIONS.contains(&requested)
    {
        requested
    } else {
        SUPPORTED_PROTOCOL_VERSION
    };

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {
            "tools": {
                "listChanged": false
            },
            "resources": {
                "subscribe": false,
                "listChanged": false
            }
        },
        "serverInfo": {
            "name": "gitgrapher",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "query",
            "title": "Search Symbols",
            "description": "Search indexed symbols by name, file path, or keyword.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query." },
                    "path": { "type": "string", "description": "Repository path. Defaults to the current directory." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Maximum results. Defaults to 10." }
                },
                "required": ["query"]
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "context",
            "title": "Symbol Context",
            "description": "Show callers, callees, and nearby graph relationships for a symbol.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Exact symbol name." },
                    "path": { "type": "string", "description": "Repository path. Defaults to the current directory." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100, "description": "Maximum incoming/outgoing relationships per symbol. Defaults to 15." }
                },
                "required": ["name"]
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "impact",
            "title": "Impact Analysis",
            "description": "Traverse upstream or downstream graph impact from a symbol.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Exact symbol name." },
                    "path": { "type": "string", "description": "Repository path. Defaults to the current directory." },
                    "direction": { "type": "string", "enum": ["up", "upstream", "down", "downstream", "both"], "description": "Traversal direction. Defaults to up." },
                    "depth": { "type": "integer", "minimum": 1, "maximum": 20, "description": "Maximum traversal depth. Defaults to 3." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500, "description": "Maximum returned nodes. Defaults to 100." }
                },
                "required": ["name"]
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "list_repos",
            "title": "List Indexed Repositories",
            "description": "List repositories recorded in GitGrapher's local registry.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
    ]
}

fn handle_tool_call(params: &Value) -> Result<Value, McpError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::invalid_params("tools/call requires params.name"))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let structured = match name {
        "query" => tool_query(&args)?,
        "context" => tool_context(&args)?,
        "impact" => tool_impact(&args)?,
        "list_repos" => tool_list_repos(),
        _ => return Err(McpError::method_not_found(name)),
    };

    Ok(json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string_pretty(&structured).unwrap_or_else(|_| "{}".to_string())
            }
        ],
        "structuredContent": structured
    }))
}

fn tool_query(args: &Value) -> Result<Value, McpError> {
    let query = required_str(args, "query")?;
    let path = path_arg(args);
    let limit = usize_arg(args, "limit", 10, 1, 100);
    let store = load_graph(&path)?;

    let mut results = Vec::new();
    match gg_search::SearchEngine::build(&store).and_then(|engine| engine.search(query, limit)) {
        Ok(hits) if !hits.is_empty() => {
            results.extend(hits.into_iter().map(|hit| {
                json!({
                    "node_id": hit.node_id,
                    "name": hit.name,
                    "label": hit.label,
                    "file": hit.file_path,
                    "line": hit.line,
                    "score": hit.score,
                    "exported": hit.is_exported
                })
            }));
        }
        _ => {
            results.extend(store.search_nodes(query, limit).into_iter().map(node_json));
        }
    }

    Ok(json!({
        "query": query,
        "path": path.to_string_lossy(),
        "count": results.len(),
        "results": results
    }))
}

fn tool_context(args: &Value) -> Result<Value, McpError> {
    let name = required_str(args, "name")?;
    let path = path_arg(args);
    let limit = usize_arg(args, "limit", 15, 1, 100);
    let store = load_graph(&path)?;
    let matches = store.nodes_by_name(name);

    let symbols: Vec<_> = matches
        .into_iter()
        .map(|node| {
            let incoming = store
                .incoming_edges(&node.id, None)
                .into_iter()
                .take(limit)
                .map(|edge| edge_json(edge, &store))
                .collect::<Vec<_>>();
            let outgoing = store
                .outgoing_edges(&node.id, None)
                .into_iter()
                .take(limit)
                .map(|edge| edge_json(edge, &store))
                .collect::<Vec<_>>();

            json!({
                "symbol": node_json(node),
                "incoming": incoming,
                "outgoing": outgoing
            })
        })
        .collect();

    Ok(json!({
        "name": name,
        "path": path.to_string_lossy(),
        "count": symbols.len(),
        "symbols": symbols
    }))
}

fn tool_impact(args: &Value) -> Result<Value, McpError> {
    let name = required_str(args, "name")?;
    let path = path_arg(args);
    let depth = usize_arg(args, "depth", 3, 1, 20);
    let limit = usize_arg(args, "limit", 100, 1, 500);
    let direction = str_arg(args, "direction", "up");
    let dir = match direction {
        "up" | "upstream" => Direction::Incoming,
        "down" | "downstream" => Direction::Outgoing,
        "both" => Direction::Both,
        _ => {
            return Err(McpError::invalid_params(
                "direction must be up, down, or both",
            ))
        }
    };

    let store = load_graph(&path)?;
    let matches = store.nodes_by_name(name);
    let mut results = Vec::new();

    for node in matches {
        if !node.label.is_symbol() {
            continue;
        }

        let hits = store.bfs(
            &node.id,
            dir,
            depth,
            Some(&|edge: &GraphEdge| {
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

        for hit in hits.into_iter().take(limit) {
            if let Some(hit_node) = store.get_node(&hit.node_id) {
                if hit_node.label.is_symbol() {
                    results.push(json!({
                        "depth": hit.depth,
                        "path": hit.path.iter().map(|id| id.to_string()).collect::<Vec<_>>(),
                        "symbol": node_json(hit_node)
                    }));
                }
            }
        }
    }

    Ok(json!({
        "name": name,
        "path": path.to_string_lossy(),
        "direction": direction,
        "depth": depth,
        "count": results.len(),
        "results": results
    }))
}

fn tool_list_repos() -> Value {
    let registry = Registry::load();
    let repos = registry
        .repos
        .into_iter()
        .map(|(name, entry)| {
            let indexed = GraphStore::exists(&Path::new(&entry.path).join(DATA_DIR));
            json!({
                "name": name,
                "path": entry.path,
                "nodes": entry.nodes,
                "edges": entry.edges,
                "files": entry.files,
                "indexed_at": entry.indexed_at,
                "indexed": indexed
            })
        })
        .collect::<Vec<_>>();

    json!({
        "count": repos.len(),
        "repos": repos
    })
}

fn resources_list() -> Vec<Value> {
    let registry = Registry::load();
    let mut resources = vec![json!({
        "uri": "gitgrapher://repos",
        "name": "Indexed repositories",
        "description": "GitGrapher's local registry of indexed repositories.",
        "mimeType": "application/json"
    })];

    for (name, entry) in registry.repos {
        if GraphStore::exists(&Path::new(&entry.path).join(DATA_DIR)) {
            resources.push(json!({
                "uri": format!("gitgrapher://repo/{}/graph", percent_encode(&name)),
                "name": format!("{name} graph"),
                "description": format!("Knowledge graph for {name}."),
                "mimeType": "application/json"
            }));
        }
    }

    resources
}

fn handle_resource_read(params: &Value) -> Result<Value, McpError> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::invalid_params("resources/read requires params.uri"))?;

    let value = if uri == "gitgrapher://repos" {
        tool_list_repos()
    } else if let Some(encoded_name) = uri
        .strip_prefix("gitgrapher://repo/")
        .and_then(|rest| rest.strip_suffix("/graph"))
    {
        let repo_name = percent_decode(encoded_name)?;
        let registry = Registry::load();
        let entry = registry
            .repos
            .get(&repo_name)
            .ok_or_else(|| McpError::invalid_params(format!("unknown repository: {repo_name}")))?;
        let store = load_graph(Path::new(&entry.path))?;
        graph_json(&store)
    } else {
        return Err(McpError::invalid_params(format!(
            "unknown resource URI: {uri}"
        )));
    };

    Ok(json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
            }
        ]
    }))
}

fn graph_json(store: &GraphStore) -> Value {
    let nodes = store.nodes().map(node_json).collect::<Vec<_>>();
    let edges = store
        .edges()
        .map(|edge| edge_json(edge, store))
        .collect::<Vec<_>>();

    json!({
        "nodes": nodes,
        "edges": edges,
        "node_count": store.node_count(),
        "edge_count": store.edge_count()
    })
}

fn load_graph(path: &Path) -> Result<GraphStore, McpError> {
    let data_dir = path.join(DATA_DIR);
    if !GraphStore::exists(&data_dir) {
        return Err(McpError::invalid_params(format!(
            "No index found at {}. Run `gitgrapher analyze` first.",
            data_dir.display()
        )));
    }

    GraphStore::load(&data_dir)
        .map_err(|err| McpError::internal(format!("Graph load failed: {err}")))
}

fn node_json(node: &GraphNode) -> Value {
    json!({
        "id": node.id,
        "name": node.properties.name,
        "label": node.label.as_str(),
        "file": node.properties.file_path,
        "line": node.properties.start_line,
        "end_line": node.properties.end_line,
        "exported": node.properties.is_exported,
        "language": node.properties.language.map(|lang| lang.as_str()),
        "return_type": node.properties.return_type,
        "async": node.properties.is_async
    })
}

fn edge_json(edge: &GraphEdge, store: &GraphStore) -> Value {
    let source = store.get_node(&edge.source_id);
    let target = store.get_node(&edge.target_id);

    json!({
        "id": edge.id,
        "source": edge.source_id,
        "source_name": source.map(|node| node.properties.name.as_str()),
        "target": edge.target_id,
        "target_name": target.map(|node| node.properties.name.as_str()),
        "type": edge.rel_type.as_str(),
        "confidence": edge.confidence,
        "reason": edge.reason
    })
}

fn path_arg(args: &Value) -> PathBuf {
    args.get("path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, McpError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| McpError::invalid_params(format!("missing required string argument: {key}")))
}

fn str_arg<'a>(args: &'a Value, key: &str, default: &'a str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or(default)
}

fn usize_arg(args: &Value, key: &str, default: usize, min: usize, max: usize) -> usize {
    args.get(key)
        .and_then(Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(default)
        .clamp(min, max)
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn percent_decode(value: &str) -> Result<String, McpError> {
    let mut bytes = Vec::new();
    let input = value.as_bytes();
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' {
            if i + 2 >= input.len() {
                return Err(McpError::invalid_params("invalid percent encoding"));
            }
            let hex = std::str::from_utf8(&input[i + 1..i + 3])
                .map_err(|_| McpError::invalid_params("invalid percent encoding"))?;
            let byte = u8::from_str_radix(hex, 16)
                .map_err(|_| McpError::invalid_params("invalid percent encoding"))?;
            bytes.push(byte);
            i += 3;
        } else {
            bytes.push(input[i]);
            i += 1;
        }
    }

    String::from_utf8(bytes).map_err(|_| McpError::invalid_params("invalid UTF-8 in URI"))
}

fn error_response(id: Value, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

#[derive(Debug)]
struct McpError {
    code: i64,
    message: String,
}

impl McpError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }

    fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {method}"),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
        }
    }
}

#[derive(Debug, serde::Deserialize, Default)]
struct Registry {
    repos: HashMap<String, RepoEntry>,
}

#[derive(Debug, serde::Deserialize)]
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_advertises_tools_and_resources() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0.0.0" }
            }
        }))
        .expect("request should produce response");

        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert!(response["result"]["capabilities"]["tools"].is_object());
        assert!(response["result"]["capabilities"]["resources"].is_object());
    }

    #[test]
    fn tools_list_includes_core_tools() {
        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": "tools",
            "method": "tools/list"
        }))
        .expect("request should produce response");

        let names = response["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>();

        assert!(names.contains(&"query"));
        assert!(names.contains(&"context"));
        assert!(names.contains(&"impact"));
        assert!(names.contains(&"list_repos"));
    }

    #[test]
    fn tools_call_query_reads_indexed_repo() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/app.ts"),
            "export function handleLogin() { return true; }\n",
        )
        .unwrap();
        gg_pipeline::analyze(dir.path().to_str().unwrap()).unwrap();

        let response = handle_message(json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": "query",
                "arguments": {
                    "path": dir.path(),
                    "query": "handleLogin"
                }
            }
        }))
        .expect("request should produce response");

        assert!(response.get("error").is_none(), "{response:#?}");
        let results = response["result"]["structuredContent"]["results"]
            .as_array()
            .unwrap();
        assert!(
            results
                .iter()
                .any(|result| result["name"].as_str() == Some("handleLogin")),
            "{response:#?}"
        );
    }
}
