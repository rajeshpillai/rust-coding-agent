//! OpenAI Chat Completions. Ollama (`/v1`) and Gemini's OpenAI-compatible endpoint use it too.

use super::Request;
use crate::chat::Message;
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

    pub fn parse(&self, body: &mut dyn BufRead) -> io::Result<String> {
        let v: Value = serde_json::from_reader(body).map_err(io::Error::other)?;
        if let Some(e) = v.get("error") {
            return Err(io::Error::other(format!("provider error: {e}")));
        }
        Ok(v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string())
    }
}
