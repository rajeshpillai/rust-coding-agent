//! OpenAI Chat Completions. Ollama (`/v1`) and Gemini's OpenAI-compatible endpoint use it too.

use super::Request;
use crate::chat::{Message, Reply, Usage};
use crate::sse;
use serde_json::{Value, json};
use std::io::{self, BufRead};

pub struct OpenAi {
    pub model: String,
    pub key: Option<String>,
    /// `/v1/chat/completions` for OpenAI and Ollama, `/v1beta/openai/chat/completions` for Gemini.
    pub path: String,
}

impl OpenAi {
    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn request(&self, messages: &[Message]) -> Request {
        let messages: Vec<Value> = messages
            .iter()
            .map(|m| match m {
                Message::System(t) => json!({ "role": "system", "content": t }),
                Message::User(t) => json!({ "role": "user", "content": t }),
                Message::Assistant { text } => json!({ "role": "assistant", "content": text }),
            })
            .collect();
        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
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

    pub fn parse(&self, body: &mut dyn BufRead, on_text: &mut dyn FnMut(&str)) -> io::Result<Reply> {
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
            Ok(())
        })?;
        Ok(reply)
    }
}
