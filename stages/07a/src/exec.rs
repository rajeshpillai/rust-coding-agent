//! Runs one shell command for the agent: stdout and stderr merged in order, the output capped,
//! and a timeout.

use std::io::{self, Read};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Limits {
    pub timeout: Duration,
    pub max_bytes: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            timeout: Duration::from_secs(120),
            max_bytes: 16 * 1024,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Ended {
    Exited(i32),
    Signal(i32),
    TimedOut,
}

#[derive(Debug)]
pub struct Output {
    /// At most `max_bytes` of the merged output.
    pub text: String,
    /// How many bytes the command wrote in all.
    pub total: usize,
    pub ended: Ended,
}

impl Output {
    /// The text the model sees.
    pub fn for_model(&self) -> String {
        let mut s = self.text.clone();
        if self.total > self.text.len() {
            s.push_str(&format!(
                "\n[output cut: showed {} of {} bytes]",
                self.text.len(),
                self.total
            ));
        }
        s.push_str(&match self.ended {
            Ended::Exited(c) => format!("\n[exit code {c}]"),
            Ended::Signal(n) => format!("\n[killed by signal {n}]"),
            Ended::TimedOut => "\n[timed out; the command was killed]".into(),
        });
        s
    }
}

pub fn run(command: &str, dir: &Path, limits: &Limits) -> io::Result<Output> {
    let (mut reader, writer) = io::pipe()?;
    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(command).current_dir(dir);
    cmd.stdin(Stdio::null())
        .stdout(writer.try_clone()?)
        .stderr(writer);
    let mut child = cmd.spawn()?;
    // `cmd` holds the pipe's write ends. Drop it, or the reader never sees the end of the output.
    drop(cmd);

    let max = limits.max_bytes;
    let collector = thread::spawn(move || {
        let (mut kept, mut total, mut buf) = (Vec::new(), 0, [0u8; 8192]);
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            total += n;
            let room = max.saturating_sub(kept.len());
            kept.extend_from_slice(&buf[..n.min(room)]);
        }
        (kept, total)
    });

    let start = Instant::now();
    let ended = loop {
        if let Some(status) = child.try_wait()? {
            break match status.code() {
                Some(c) => Ended::Exited(c),
                None => Ended::Signal(status.signal().unwrap_or(0)),
            };
        }
        if start.elapsed() > limits.timeout {
            break Ended::TimedOut;
        }
        thread::sleep(Duration::from_millis(10));
    };
    // On a timeout, kill the command.
    if ended == Ended::TimedOut {
        child.kill()?;
    }
    let _ = child.wait();
    let (kept, total) = collector
        .join()
        .map_err(|_| io::Error::other("output thread panicked"))?;
    Ok(Output {
        text: String::from_utf8_lossy(&kept).into_owned(),
        total,
        ended,
    })
}
