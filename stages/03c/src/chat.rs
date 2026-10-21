//! The agent's own types. Every provider is translated to and from these.

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    System(String),
    User(String),
    Assistant { text: String },
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
    pub usage: Usage,
}
