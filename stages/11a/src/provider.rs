//! A provider turns our messages into one wire format, and a streamed reply back into ours.

use crate::chat::{Message, Reply, ToolSpec};
use std::io::{self, BufRead};

pub mod anthropic;
pub mod openai;

/// One HTTP request, before it is sent. Headers may hold a key, so they are never recorded.
pub struct Request {
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub trait Provider {
    /// The model id, for the screen and the recording.
    fn model(&self) -> &str;
    fn request(&self, messages: &[Message], tools: &[ToolSpec]) -> Request;
    /// Reads a streamed reply. `on_text` gets each piece of text as it arrives.
    fn parse(&self, body: &mut dyn BufRead, on_text: &mut dyn FnMut(&str)) -> io::Result<Reply>;
}
