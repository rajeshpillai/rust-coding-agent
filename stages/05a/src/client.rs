//! Sends requests to the model and reads the streamed reply.

use crate::chat::{Message, Reply, ToolSpec};
use crate::http::{self, Endpoint};
use crate::provider::openai::OpenAi;
use std::io::{self, BufRead, Read};

pub enum Transport {
    Live(Endpoint),
}

pub struct Client {
    pub provider: OpenAi,
    pub transport: Transport,
}

impl Client {
    pub fn new(provider: OpenAi, transport: Transport) -> Client {
        Client { provider, transport }
    }

    pub fn complete(
        &mut self,
        messages: &[Message],
        tools: &[ToolSpec],
        on_text: &mut dyn FnMut(&str),
    ) -> io::Result<Reply> {
        let req = self.provider.request(messages, tools);
        let mut body: Box<dyn BufRead> = match &self.transport {
            Transport::Live(ep) => {
                let res = http::post(ep, &req.path, &req.headers, &req.body)?;
                if res.status != 200 {
                    let mut text = String::new();
                    let mut b = res.body;
                    b.read_to_string(&mut text)?;
                    return Err(io::Error::other(format!("HTTP {}: {}", res.status, text.trim())));
                }
                res.body
            }
        };
        self.provider.parse(&mut body, on_text)
    }
}
