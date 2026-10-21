//! Sends requests live, or replays a recorded session. Can record what it sends and receives.
//!
//! A recording is a folder of numbered pairs: `001-request.json` (the body we sent) and
//! `001-response.txt` (the body we received, as it arrived). Headers are never written, so a
//! recording never holds a key.

use crate::chat::{Message, Reply, ToolSpec};
use crate::http::{self, Endpoint};
use crate::provider::openai::OpenAi;
use std::fs;
use std::io::{self, BufRead, Read};
use std::path::PathBuf;

pub enum Transport {
    Live(Endpoint),
    /// Response bodies, in order. From a recording folder, or from a test.
    Replay {
        responses: Vec<Vec<u8>>,
        requests: Vec<Vec<u8>>,
    },
}

pub struct Client {
    pub provider: OpenAi,
    pub transport: Transport,
    pub record: Option<PathBuf>,
    turn: usize,
}

impl Client {
    pub fn new(provider: OpenAi, transport: Transport) -> Client {
        Client {
            provider,
            transport,
            record: None,
            turn: 0,
        }
    }

    /// Loads every `NNN-response.txt` in `dir`, in order.
    pub fn replay_from(dir: &std::path::Path) -> io::Result<Transport> {
        let (mut responses, mut requests) = (vec![], vec![]);
        for n in 1.. {
            let res = dir.join(format!("{n:03}-response.txt"));
            if !res.exists() {
                break;
            }
            responses.push(fs::read(res)?);
            requests.push(fs::read(dir.join(format!("{n:03}-request.json"))).unwrap_or_default());
        }
        if responses.is_empty() {
            return Err(io::Error::other(format!(
                "no recorded responses in {}",
                dir.display()
            )));
        }
        Ok(Transport::Replay { responses, requests })
    }

    pub fn complete(
        &mut self,
        messages: &[Message],
        tools: &[ToolSpec],
        on_text: &mut dyn FnMut(&str),
    ) -> io::Result<Reply> {
        let req = self.provider.request(messages, tools);
        let n = self.turn;
        self.turn += 1;
        if let Some(dir) = &self.record {
            fs::create_dir_all(dir)?;
            fs::write(dir.join(format!("{:03}-request.json", n + 1)), pretty(&req.body))?;
        }
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
            Transport::Replay { responses, requests } => {
                let bytes = responses
                    .get(n)
                    .ok_or_else(|| io::Error::other(format!("the recording has no turn {}", n + 1)))?;
                if let Some(old) = requests.get(n).filter(|r| !r.is_empty())
                    && pretty(old) != pretty(&req.body)
                {
                    eprintln!("(replay: request {} differs from the recording)", n + 1);
                }
                Box::new(io::Cursor::new(bytes.clone()))
            }
        };
        let mut tee = Tee {
            inner: &mut *body,
            copy: vec![],
        };
        let reply = self.provider.parse(&mut tee, on_text);
        if let Some(dir) = &self.record {
            fs::write(dir.join(format!("{:03}-response.txt", n + 1)), &tee.copy)?;
        }
        reply
    }
}

/// Pretty JSON when the bytes are JSON, so recordings diff well. Otherwise the bytes as they are.
fn pretty(body: &[u8]) -> Vec<u8> {
    match serde_json::from_slice::<serde_json::Value>(body) {
        Ok(v) => serde_json::to_vec_pretty(&v).unwrap(),
        Err(_) => body.to_vec(),
    }
}

/// Keeps a copy of every byte the parser consumes.
struct Tee<'a> {
    inner: &'a mut dyn BufRead,
    copy: Vec<u8>,
}

impl Read for Tee<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.copy.extend_from_slice(&buf[..n]);
        Ok(n)
    }
}

impl BufRead for Tee<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.inner.fill_buf()
    }
    fn consume(&mut self, amt: usize) {
        if let Ok(buf) = self.inner.fill_buf() {
            self.copy.extend_from_slice(&buf[..amt]);
        }
        self.inner.consume(amt);
    }
}
