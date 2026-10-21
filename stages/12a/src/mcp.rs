//! The tools over MCP: newline-delimited JSON-RPC 2.0 on stdin/stdout.
//!
//! Protocol version 2026-07-28 is stateless: there is no handshake, and every request carries
//! its version and the client's capabilities in `_meta`. The server also answers the older
//! `initialize` handshake (2025-11-25), so clients from before July 2026 still work.

use crate::chat::{ToolCall, ToolSpec};
use crate::tools::{self, Tool, ToolHost, Workspace};
use serde_json::{Value, json};
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

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

/// The agent's side: starts a server process and calls its tools.
pub struct McpTools {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpTools {
    /// Starts `program args...` as an MCP server and checks that it speaks our version.
    pub fn spawn(program: &str, args: &[String]) -> io::Result<McpTools> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut m = McpTools {
            child,
            stdin,
            stdout,
            next_id: 1,
        };
        let found = m.request("server/discover", json!({}))?;
        let versions = found["supportedVersions"].as_array().cloned().unwrap_or_default();
        if !versions.iter().any(|v| v == VERSION) {
            return Err(io::Error::other(format!(
                "the MCP server does not support {VERSION}: {versions:?}"
            )));
        }
        Ok(m)
    }

    fn request(&mut self, method: &str, mut params: Value) -> io::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        params["_meta"] = json!({
            VERSION_KEY: VERSION,
            "io.modelcontextprotocol/clientInfo": info(),
            CAPS_KEY: {},
        });
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| io::Error::other("the MCP server is closed"))?;
        writeln!(stdin, "{msg}")?;
        stdin.flush()?;
        loop {
            let mut line = String::new();
            if self.stdout.read_line(&mut line)? == 0 {
                return Err(io::Error::other("the MCP server closed its output"));
            }
            let v: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
            if v["id"] != json!(id) {
                continue; // a notification, or a reply to something else
            }
            if let Some(e) = v.get("error") {
                return Err(io::Error::other(format!(
                    "MCP error {}: {}",
                    e["code"], e["message"]
                )));
            }
            return Ok(v["result"].clone());
        }
    }
}

impl ToolHost for McpTools {
    fn specs(&mut self) -> Vec<ToolSpec> {
        let listed = self.request("tools/list", json!({})).unwrap_or_default();
        listed["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|t| ToolSpec {
                name: t["name"].as_str().unwrap_or("").into(),
                description: t["description"].as_str().unwrap_or("").into(),
                schema: t["inputSchema"].clone(),
            })
            .collect()
    }

    fn call(&mut self, call: &ToolCall) -> (String, bool) {
        let args: Value = match serde_json::from_str(&call.args) {
            Ok(v) => v,
            Err(e) => return (format!("bad arguments for {}: {e}", call.name), true),
        };
        match self.request("tools/call", json!({ "name": call.name, "arguments": args })) {
            Ok(r) => {
                let text = r["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                (text, r["isError"].as_bool().unwrap_or(false))
            }
            Err(e) => (e.to_string(), true),
        }
    }
}

impl Drop for McpTools {
    fn drop(&mut self) {
        // The spec's shutdown: close the server's stdin, then wait for it to exit.
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}
