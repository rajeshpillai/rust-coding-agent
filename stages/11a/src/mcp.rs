//! The tools over MCP: newline-delimited JSON-RPC 2.0 on stdin/stdout.
//!
//! Protocol version 2026-07-28 is stateless: there is no handshake, and every request carries
//! its version and the client's capabilities in `_meta`. The server also answers the older
//! `initialize` handshake (2025-11-25), so clients from before July 2026 still work.

use crate::tools::{self, Tool, Workspace};
use serde_json::{Value, json};
use std::io::{self, BufRead, Write};

pub const VERSION: &str = "2026-07-28";
pub const LEGACY_VERSION: &str = "2025-11-25";
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";
const CAPS_KEY: &str = "io.modelcontextprotocol/clientCapabilities";

fn info() -> Value {
    json!({ "name": "kaam", "version": env!("CARGO_PKG_VERSION") })
}

/// Serves the workspace's tools until stdin closes.
pub fn serve(ws: &mut Workspace) -> io::Result<()> {
    let stdin = io::stdin();
    let mut out = io::stdout().lock();
    let mut legacy = false;
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"), None)),
            Ok(msg) => handle(ws, &msg, &mut legacy),
        };
        if let Some(r) = reply {
            writeln!(out, "{r}")?;
            out.flush()?;
        }
    }
    Ok(())
}

fn error(id: Value, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut e = json!({ "code": code, "message": message });
    if let Some(d) = data {
        e["data"] = d;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": e })
}

fn result(id: Value, mut r: Value) -> Value {
    r["resultType"] = json!("complete");
    r["_meta"] = json!({ "io.modelcontextprotocol/serverInfo": info() });
    json!({ "jsonrpc": "2.0", "id": id, "result": r })
}

fn handle(ws: &mut Workspace, msg: &Value, legacy: &mut bool) -> Option<Value> {
    let id = msg.get("id")?.clone(); // no id: a notification, which gets no reply
    let method = msg["method"].as_str().unwrap_or("");
    let params = &msg["params"];

    if method == "initialize" {
        *legacy = true;
        return Some(json!({ "jsonrpc": "2.0", "id": id, "result": {
            "protocolVersion": LEGACY_VERSION, "capabilities": { "tools": {} }, "serverInfo": info(),
        }}));
    }
    if !*legacy {
        let meta = &params["_meta"];
        match meta[VERSION_KEY].as_str() {
            None => return Some(error(id, -32602, &format!("missing _meta {VERSION_KEY}"), None)),
            Some(v) if v != VERSION => {
                return Some(error(
                    id,
                    -32022,
                    "Unsupported protocol version",
                    Some(json!({ "supported": [VERSION, LEGACY_VERSION], "requested": v })),
                ));
            }
            Some(_) if meta.get(CAPS_KEY).is_none() => {
                return Some(error(id, -32602, &format!("missing _meta {CAPS_KEY}"), None));
            }
            Some(_) => {}
        }
    }
    Some(match method {
        "server/discover" => result(
            id,
            json!({
                "supportedVersions": [VERSION, LEGACY_VERSION], "capabilities": { "tools": {} },
            }),
        ),
        "tools/list" => result(
            id,
            json!({ "tools": tools::specs().iter().map(|t| json!({
            "name": t.name, "description": t.description, "inputSchema": t.schema,
        })).collect::<Vec<_>>() }),
        ),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match Tool::parse(name, &args.to_string()) {
                // An unknown tool is a protocol error; bad arguments are a tool error the model can fix.
                Err(e) if e.starts_with("unknown tool") => error(id, -32602, &e, None),
                parsed => {
                    let (text, is_error) = match parsed.and_then(|t| ws.run(t)) {
                        Ok(s) => (s, false),
                        Err(e) => (e, true),
                    };
                    result(
                        id,
                        json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }),
                    )
                }
            }
        }
        _ => error(id, -32601, &format!("method not found: {method}"), None),
    })
}
