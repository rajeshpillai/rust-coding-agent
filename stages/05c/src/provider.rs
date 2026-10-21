//! A provider turns our messages into one wire format, and a streamed reply back into ours.

pub mod openai;

/// One HTTP request, before it is sent. Headers may hold a key, so they are never recorded.
pub struct Request {
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
