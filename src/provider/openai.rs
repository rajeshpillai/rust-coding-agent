//! OpenAI Chat Completions. Ollama (`/v1`) and Gemini's OpenAI-compatible endpoint use it too.

use super::{Provider, Request};
use crate::chat::{Message, Reply, ToolCall, ToolSpec, Usage};
use crate::sse;
use serde_json::{Value, json};
use std::io::{self, BufRead};

pub struct OpenAi {
    pub model: String,
    pub key: Option<String>,
    /// `/v1/chat/completions` for OpenAI and Ollama, `/v1beta/openai/chat/completions` for Gemini.
    pub path: String,
    /// Strict mode: the model may only send arguments the schema allows.
    pub strict: bool,
}

impl Provider for OpenAi {
    fn model(&self) -> &str {
        &self.model
    }

    fn request(&self, messages: &[Message], tools: &[ToolSpec]) -> Request {
        let messages: Vec<Value> = messages
            .iter()
            .map(|m| match m {
                Message::System(t) => json!({ "role": "system", "content": t }),
                Message::User(t) => json!({ "role": "user", "content": t }),
                Message::Assistant { text, calls } if calls.is_empty() => {
                    json!({ "role": "assistant", "content": text })
                }
                Message::Assistant { text, calls } => json!({
                    "role": "assistant",
                    "content": if text.is_empty() { Value::Null } else { json!(text) },
                    "tool_calls": calls.iter().map(|c| json!({
                        "id": c.id, "type": "function",
                        "function": { "name": c.name, "arguments": c.args },
                    })).collect::<Vec<_>>(),
                }),
                Message::Tool { call_id, content, .. } => {
                    json!({ "role": "tool", "tool_call_id": call_id, "content": content })
                }
            })
            .collect();
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if !tools.is_empty() {
            body["tools"] = tools
                .iter()
                .map(|t| {
                    json!({ "type": "function", "function": {
                        "name": t.name, "description": t.description,
                        "parameters": t.schema, "strict": self.strict,
                    }})
                })
                .collect();
        }
        let mut headers = vec![];
        if let Some(k) = &self.key {
            headers.push(("Authorization".into(), format!("Bearer {k}")));
        }
        Request {
            path: self.path.clone(),
            headers,
            body: serde_json::to_vec(&body).unwrap(),
        }
    }

    fn parse(&self, body: &mut dyn BufRead, on_text: &mut dyn FnMut(&str)) -> io::Result<Reply> {
        let mut reply = Reply::default();
        sse::for_each(body, |data| {
            if data == "[DONE]" {
                return Ok(());
            }
            let v: Value = serde_json::from_str(data).map_err(io::Error::other)?;
            if let Some(e) = v.get("error") {
                return Err(io::Error::other(format!("provider error: {e}")));
            }
            if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
                reply.usage = Usage {
                    input: u["prompt_tokens"].as_u64().unwrap_or(0),
                    cached: u["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0),
                    output: u["completion_tokens"].as_u64().unwrap_or(0),
                };
            }
            let delta = &v["choices"][0]["delta"];
            if let Some(t) = delta["content"].as_str() {
                on_text(t);
                reply.text.push_str(t);
            }
            // A tool call arrives in pieces: the first piece has the id and name, later ones add
            // to the arguments. `index` says which call a piece belongs to.
            for piece in delta["tool_calls"].as_array().into_iter().flatten() {
                let i = piece["index"].as_u64().unwrap_or(0) as usize;
                while reply.calls.len() <= i {
                    reply.calls.push(ToolCall {
                        id: String::new(),
                        name: String::new(),
                        args: String::new(),
                    });
                }
                let call = &mut reply.calls[i];
                if let Some(id) = piece["id"].as_str() {
                    call.id = id.to_string();
                }
                if let Some(n) = piece["function"]["name"].as_str() {
                    call.name.push_str(n);
                }
                if let Some(a) = piece["function"]["arguments"].as_str() {
                    call.args.push_str(a);
                }
            }
            Ok(())
        })?;
        Ok(reply)
    }
}
