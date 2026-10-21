//! Talks to `kaam mcp` with raw JSON-RPC lines, as any MCP client would.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn session(lines: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_kaam"))
        .args(["mcp", "--root", ".", "--no-sandbox"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for l in lines {
        writeln!(stdin, "{l}").unwrap();
    }
    drop(stdin); // the spec's shutdown signal
    let out: Vec<Value> = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect();
    assert!(child.wait().unwrap().success());
    out
}

fn meta(version: &str) -> Value {
    json!({ "io.modelcontextprotocol/protocolVersion": version, "io.modelcontextprotocol/clientCapabilities": {} })
}

#[test]
fn modern_requests() {
    let r = session(&[
        json!({ "jsonrpc": "2.0", "id": 1, "method": "server/discover", "params": { "_meta": meta("2026-07-28") } }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": { "_meta": meta("2026-07-28") } }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": { "_meta": meta("2026-07-28"),
                "name": "read_file", "arguments": { "path": "Cargo.toml" } } }),
        json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": { "_meta": meta("2026-07-28"),
                "name": "read_file", "arguments": { "path": "Cargo.toml", "start_line": 1 } } }),
        json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": { "_meta": meta("2026-07-28"),
                "name": "run_shell", "arguments": {} } }),
    ]);
    assert_eq!(r.len(), 5);
    assert_eq!(r[0]["result"]["supportedVersions"][0], "2026-07-28");
    assert_eq!(r[0]["result"]["resultType"], "complete");
    assert_eq!(r[1]["result"]["tools"].as_array().unwrap().len(), 4);
    assert!(
        r[2]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("name = \"kaam\"")
    );
    assert_eq!(r[2]["result"]["isError"], false);
    // Bad arguments: a tool error, which the model can read and fix.
    assert_eq!(r[3]["result"]["isError"], true);
    // An unknown tool: a protocol error.
    assert_eq!(r[4]["error"]["code"], -32602);
}

#[test]
fn rejects_missing_meta_and_unknown_versions() {
    let r = session(&[
        json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {} }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": { "_meta": meta("1900-01-01") } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": { "requestId": 9 } }),
    ]);
    assert_eq!(r.len(), 2, "a notification gets no reply");
    assert_eq!(r[0]["error"]["code"], -32602);
    assert_eq!(r[1]["error"]["code"], -32022);
    assert_eq!(r[1]["error"]["data"]["requested"], "1900-01-01");
}

#[test]
fn answers_the_legacy_handshake() {
    let r = session(&[
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "old", "version": "1" } } }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
    ]);
    assert_eq!(r[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(r[1]["result"]["tools"].as_array().unwrap().len(), 4);
}
