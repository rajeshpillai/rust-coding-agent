//! The agent's own types. Every provider is translated to and from these.

#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    System(String),
    User(String),
    Assistant { text: String },
}
