use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn run_mcp(messages: &[Value]) -> Vec<Value> {
    let expected_responses = messages
        .iter()
        .filter(|message| message.get("id").is_some())
        .count();
    let exe = env!("CARGO_BIN_EXE_gitgrapher");
    let mut child = Command::new(exe)
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn gitgrapher mcp");

    let stdout = child.stdout.take().expect("child stdout");
    let mut stdin = child.stdin.take().expect("child stdin");
    for message in messages {
        let line = serde_json::to_string(message).unwrap();
        writeln!(stdin, "{line}").unwrap();
    }
    drop(stdin);

    let mut reader = BufReader::new(stdout);
    let mut responses = Vec::new();
    for _ in 0..expected_responses {
        let mut line = String::new();
        let bytes = reader
            .read_line(&mut line)
            .expect("failed to read MCP response");
        assert!(bytes > 0, "MCP server closed stdout before responding");
        responses.push(serde_json::from_str(&line).expect("stdout line should be JSON-RPC"));
    }

    let _ = child.kill();
    let _ = child.wait();
    responses
}

#[test]
fn stdio_initialize_and_tools_list() {
    let responses = run_mcp(&[
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "integration-test", "version": "0.0.0" }
            }
        }),
        json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }),
        json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/list"
        }),
    ]);

    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-06-18");

    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(names.contains(&"query"));
    assert!(names.contains(&"context"));
    assert!(names.contains(&"impact"));
    assert!(names.contains(&"list_repos"));
}

#[test]
fn stdio_tools_call_query_reads_indexed_repo() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(
        dir.path().join("src/app.ts"),
        "export function handleLogin() { return true; }\n",
    )
    .unwrap();
    gg_pipeline::analyze(dir.path().to_str().unwrap()).unwrap();

    let responses = run_mcp(&[json!({
        "jsonrpc": "2.0",
        "id": "call",
        "method": "tools/call",
        "params": {
            "name": "query",
            "arguments": {
                "path": dir.path(),
                "query": "handleLogin"
            }
        }
    })]);

    assert_eq!(responses.len(), 1);
    assert!(responses[0].get("error").is_none(), "{responses:#?}");
    let results = responses[0]["result"]["structuredContent"]["results"]
        .as_array()
        .unwrap();
    assert!(
        results
            .iter()
            .any(|result| result["name"].as_str() == Some("handleLogin")),
        "{responses:#?}"
    );
}
