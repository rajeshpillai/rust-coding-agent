//! The loop: ask the model, run the tools it asks for, send back the results, until it stops.

use crate::chat::{Message, ToolCall};
use crate::client::Client;
use crate::signal;
use crate::tools::{Tool, ToolHost};
use std::io::{self, BufRead, Write};

pub enum Approver {
    /// Ask on the terminal.
    Ask,
    /// Approve everything, and say so on screen.
    Yes,
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Done { steps: usize },
    StepBudget,
    Cancelled,
}

pub struct Agent {
    pub client: Client,
    pub host: Box<dyn ToolHost>,
    pub approver: Approver,
    pub max_steps: usize,
    /// When a request uses more input tokens than this, old tool output is dropped.
    pub context_budget: u64,
    pub history: Vec<Message>,
}

pub const SYSTEM: &str = "You are kaam, a coding agent working in a project folder. \
Use the tools to look at files, change them, and run commands. \
Make the smallest change that does the task, and check it by running the project's tests. \
When the task is done, reply with a short summary and no tool call.";

impl Agent {
    pub fn new(client: Client, host: Box<dyn ToolHost>) -> Agent {
        Agent {
            client,
            host,
            approver: Approver::Ask,
            max_steps: 20,
            context_budget: 24_000,
            history: vec![Message::System(SYSTEM.into())],
        }
    }

    pub fn run(&mut self, task: &str) -> io::Result<Outcome> {
        self.history.push(Message::User(task.into()));
        let specs = self.host.specs();
        let mut last_input = 0;
        for step in 1..=self.max_steps {
            if last_input > self.context_budget {
                self.drop_old_output();
            }
            let mut out = io::stdout();
            let reply = self.client.complete(&self.history, &specs, &mut |t| {
                print!("{t}");
                let _ = out.flush();
            })?;
            if !reply.text.is_empty() {
                println!();
            }
            let u = reply.usage;
            println!(
                "  · tokens: {} in ({} cached), {} out",
                u.input, u.cached, u.output
            );
            last_input = u.input;
            if signal::interrupted() {
                signal::clear();
                return Ok(Outcome::Cancelled);
            }
            let calls = reply.calls.clone();
            self.history.push(Message::Assistant {
                text: reply.text,
                calls: reply.calls,
            });
            if calls.is_empty() {
                return Ok(Outcome::Done { steps: step });
            }
            for call in &calls {
                let (content, is_error) = self.one_call(call)?;
                println!("  {} {}", if is_error { "✗" } else { "←" }, summary(&content));
                self.history.push(Message::Tool {
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    content,
                });
            }
        }
        Ok(Outcome::StepBudget)
    }

    fn one_call(&mut self, call: &ToolCall) -> io::Result<(String, bool)> {
        println!("→ {} {}", call.name, short(&call.args, 96));
        if Tool::needs_approval(&call.name) {
            let yes = match self.approver {
                Approver::Yes => {
                    println!("  (approved: --yes)");
                    true
                }
                Approver::Ask => ask("  run it? [y/N] ")?,
            };
            if !yes {
                return Ok(("the user said no to this tool call".into(), true));
            }
        }
        Ok(self.host.call(call))
    }

    /// Keeps the last two tool results whole and replaces older long ones with a note.
    fn drop_old_output(&mut self) {
        let tool_msgs: Vec<usize> = (0..self.history.len())
            .filter(|&i| matches!(self.history[i], Message::Tool { .. }))
            .collect();
        let mut dropped = 0;
        for &i in tool_msgs.iter().rev().skip(2) {
            if let Message::Tool { name, content, .. } = &mut self.history[i]
                && content.len() > 200
            {
                dropped += content.len();
                *content = format!(
                    "[dropped to save context: {} bytes of {name} output]",
                    content.len()
                );
            }
        }
        if dropped > 0 {
            println!("  · context budget: dropped {dropped} bytes of old tool output");
        }
    }
}

/// The first line of a result, and its last line when there is more than one.
fn summary(content: &str) -> String {
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    match lines.as_slice() {
        [] => String::new(),
        [one] => short(one, 100),
        [first, .., last] => format!("{} … {}", short(first, 60), short(last, 60)),
    }
}

fn short(s: &str, max: usize) -> String {
    let one = s.replace('\n', " ");
    if one.chars().count() <= max {
        one
    } else {
        one.chars().take(max).collect::<String>() + "…"
    }
}

/// Reads y or n from the terminal itself, so piped stdin cannot answer for the user.
fn ask(prompt: &str) -> io::Result<bool> {
    print!("{prompt}");
    io::stdout().flush()?;
    let tty = std::fs::File::open("/dev/tty")?;
    let mut line = String::new();
    io::BufReader::new(tty).read_line(&mut line)?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes"))
}
