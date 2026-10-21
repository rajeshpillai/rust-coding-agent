//! Anthropic Messages. A tool call is a `tool_use` content block, not a `tool_calls` list, and
//! tool results go back as `tool_result` blocks inside a user message.

use super::{Provider, Request};
use crate::chat::{Message, Reply, ToolCall, ToolSpec};
use crate::sse;
use serde_json::{Value, json};
use std::io::{self, BufRead};

pub struct Anthropic {
    pub model: String,
    pub key: String,
    pub max_tokens: u32,
}

impl Provider for Anthropic {
    fn model(&self) -> &str {
        &self.model
    }

    fn request(&self, messages: &[Message], tools: &[ToolSpec]) -> Request {
        let mut system = String::new();
        let mut out: Vec<Value> = vec![];
        for m in messages {
            match m {
                Message::System(t) => system.push_str(t),
                Message::User(t) => out.push(json!({ "role": "user", "content": t })),
                Message::Assistant { text, calls } => {
                    let mut blocks = vec![];
                    if !text.is_empty() {
                        blocks.push(json!({ "type": "text", "text": text }));
                    }
                    for c in calls {
                        // Claude needs an object here; a model's broken JSON becomes `{}`.
                        let input: Value = serde_json::from_str(&c.args).unwrap_or(json!({}));
                        blocks
                            .push(json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": input }));
                    }
                    out.push(json!({ "role": "assistant", "content": blocks }));
                }
                Message::Tool { call_id, content, .. } => {
                    let block = json!({ "type": "tool_result", "tool_use_id": call_id, "content": content });
                    // Results of one turn's calls share one user message.
                    match out.last_mut() {
                        Some(last) if last["role"] == "user" && last["content"].is_array() => {
                            last["content"].as_array_mut().unwrap().push(block)
                        }
                        _ => out.push(json!({ "role": "user", "content": [block] })),
                    }
                }
            }
        }
        let mut body = json!({
            "model": self.model, "max_tokens": self.max_tokens, "stream": true, "messages": out,
        });
        if !system.is_empty() {
            body["system"] = json!(system);
        }
        if !tools.is_empty() {
            body["tools"] = tools
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.schema }))
                .collect();
        }
        Request {
            path: "/v1/messages".into(),
            headers: vec![
                ("x-api-key".into(), self.key.clone()),
                ("anthropic-version".into(), "2023-06-01".into()),
            ],
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    fn parse(&self, body: &mut dyn BufRead, on_text: &mut dyn FnMut(&str)) -> io::Result<Reply> {
        let mut reply = Reply::default();
        // Content blocks by index: Some(call number) for tool_use blocks.
        let mut block_call: Vec<Option<usize>> = vec![];
        sse::for_each(body, |data| {
            let v: Value = serde_json::from_str(data).map_err(io::Error::other)?;
            match v["type"].as_str().unwrap_or("") {
                "message_start" => {
                    let u = &v["message"]["usage"];
                    reply.usage.input = u["input_tokens"].as_u64().unwrap_or(0)
                        + u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    reply.usage.cached = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                }
                "content_block_start" => {
                    let i = v["index"].as_u64().unwrap_or(0) as usize;
                    block_call.resize(block_call.len().max(i + 1), None);
                    let b = &v["content_block"];
                    if b["type"] == "tool_use" {
                        reply.calls.push(ToolCall {
                            id: b["id"].as_str().unwrap_or("").into(),
                            name: b["name"].as_str().unwrap_or("").into(),
                            args: String::new(),
                            extra: None,
                        });
                        block_call[i] = Some(reply.calls.len() - 1);
                    }
                }
                "content_block_delta" => {
                    let i = v["index"].as_u64().unwrap_or(0) as usize;
                    let d = &v["delta"];
                    match d["type"].as_str().unwrap_or("") {
                        "text_delta" => {
                            let t = d["text"].as_str().unwrap_or("");
                            on_text(t);
                            reply.text.push_str(t);
                        }
                        "input_json_delta" => {
                            if let Some(Some(c)) = block_call.get(i) {
                                reply.calls[*c]
                                    .args
                                    .push_str(d["partial_json"].as_str().unwrap_or(""));
                            }
                        }
                        _ => {}
                    }
                }
                "message_delta" => reply.usage.output = v["usage"]["output_tokens"].as_u64().unwrap_or(0),
                "error" => return Err(io::Error::other(format!("provider error: {}", v["error"]))),
                _ => {}
            }
            Ok(())
        })?;
        // A tool call with no arguments streams no JSON at all.
        for c in &mut reply.calls {
            if c.args.is_empty() {
                c.args = "{}".into();
            }
        }
        Ok(reply)
    }
}
