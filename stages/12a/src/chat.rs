//! The agent's own types. Every provider is translated to and from these.

use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    System(String),
    User(String),
    Assistant {
        text: String,
        calls: Vec<ToolCall>,
    },
    Tool {
        call_id: String,
        name: String,
        content: String,
    },
}

/// One tool call from the model. `args` is the JSON text exactly as the model sent it.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub args: String,
    /// Provider data that must go back with the call, untouched (Gemini's thought signature).
    pub extra: Option<Value>,
}

/// What we tell the model about one tool.
#[derive(Clone, Debug)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub usage: Usage,
}
