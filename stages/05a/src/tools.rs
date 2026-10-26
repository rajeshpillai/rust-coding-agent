//! The tools the model can call.

use crate::chat::ToolSpec;
use serde::Deserialize;
use serde_json::json;
use std::process::Command;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommand {
    pub command: String,
}

#[derive(Debug)]
pub enum Tool {
    Run(RunCommand),
}

impl Tool {
    /// The model's call, checked. The error text goes back to the model as the tool's result.
    pub fn parse(name: &str, args: &str) -> Result<Tool, String> {
        let bad = |e: serde_json::Error| format!("bad arguments for {name}: {e}");
        match name {
            "run_command" => serde_json::from_str(args).map(Tool::Run).map_err(bad),
            _ => Err(format!("unknown tool `{name}`. The tools are: run_command")),
        }
    }
}

pub fn specs() -> Vec<ToolSpec> {
    let spec = |name: &str, description: &str, props: serde_json::Value, required: &[&str]| ToolSpec {
        name: name.into(),
        description: description.into(),
        schema: json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false }),
    };
    let s = |d: &str| json!({ "type": "string", "description": d });
    vec![spec(
        "run_command",
        "Run a shell command in the project folder. Returns its output.",
        json!({ "command": s("The command, run with sh -c") }),
        &["command"],
    )]
}

/// Runs one tool call.
pub fn run(tool: Tool) -> Result<String, String> {
    match tool {
        Tool::Run(t) => {
            let out = Command::new("sh")
                .arg("-c")
                .arg(&t.command)
                .output()
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_tools() {
        assert!(
            Tool::parse("run_shell", "{}")
                .unwrap_err()
                .starts_with("unknown tool `run_shell`")
        );
    }
}
