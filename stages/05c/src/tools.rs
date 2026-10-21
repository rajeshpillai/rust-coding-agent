//! The four tools, and the project folder they work in.

use crate::chat::{ToolCall, ToolSpec};
use serde::Deserialize;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommand {
    pub command: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadFile {
    pub path: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditFile {
    pub path: String,
    pub old: String,
    pub new: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListFiles {
    pub dir: String,
}

#[derive(Debug)]
pub enum Tool {
    Run(RunCommand),
    Read(ReadFile),
    Edit(EditFile),
    List(ListFiles),
}

impl Tool {
    /// The model's call, checked. The error text goes back to the model as the tool's result.
    pub fn parse(name: &str, args: &str) -> Result<Tool, String> {
        let bad = |e: serde_json::Error| format!("bad arguments for {name}: {e}");
        match name {
            "run_command" => serde_json::from_str(args).map(Tool::Run).map_err(bad),
            "read_file" => serde_json::from_str(args).map(Tool::Read).map_err(bad),
            "edit_file" => serde_json::from_str(args).map(Tool::Edit).map_err(bad),
            "list_files" => serde_json::from_str(args).map(Tool::List).map_err(bad),
            _ => Err(format!(
                "unknown tool `{name}`. The tools are: run_command, read_file, edit_file, list_files"
            )),
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
    vec![
        spec(
            "run_command",
            "Run a shell command in the project folder. Returns its output and exit code.",
            json!({ "command": s("The command, run with sh -c") }),
            &["command"],
        ),
        spec(
            "read_file",
            "Read a text file in the project.",
            json!({ "path": s("Path relative to the project folder") }),
            &["path"],
        ),
        spec(
            "edit_file",
            "Replace one exact piece of text in a file. `old` must appear exactly once. \
              To create a new file, use an empty `old` and a path that does not exist.",
            json!({ "path": s("Path relative to the project folder"), "old": s("The exact text to replace"),
                     "new": s("The text to put in its place") }),
            &["path", "old", "new"],
        ),
        spec(
            "list_files",
            "List the files below a folder of the project.",
            json!({ "dir": s("Folder relative to the project, or .") }),
            &["dir"],
        ),
    ]
}

pub struct Workspace {
    pub root: PathBuf,
}

impl Workspace {
    pub fn new(root: &Path) -> std::io::Result<Workspace> {
        Ok(Workspace {
            root: root.canonicalize()?,
        })
    }

    pub fn run(&self, tool: Tool) -> Result<String, String> {
        match tool {
            Tool::Run(t) => {
                let out = Command::new("sh")
                    .arg("-c")
                    .arg(&t.command)
                    .current_dir(&self.root)
                    .output()
                    .map_err(|e| format!("could not run the command: {e}"))?;
                Ok(format!(
                    "{}{}\n[exit code {}]",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr),
                    out.status.code().unwrap_or(-1)
                ))
            }
            Tool::Read(t) => {
                let p = self.root.join(&t.path);
                let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", t.path))?;
                Ok(text)
            }
            Tool::Edit(t) => {
                let p = self.root.join(&t.path);
                if t.old.is_empty() {
                    if p.exists() {
                        return Err(format!(
                            "{}: already exists; give the exact text to replace",
                            t.path
                        ));
                    }
                    if let Some(dir) = p.parent() {
                        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    }
                    fs::write(&p, &t.new).map_err(|e| e.to_string())?;
                    return Ok(format!("created {}", t.path));
                }
                let text = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", t.path))?;
                match text.matches(&t.old).count() {
                    0 => Err(format!(
                        "{}: `old` was not found. Read the file and copy the text exactly.",
                        t.path
                    )),
                    1 => {
                        fs::write(&p, text.replacen(&t.old, &t.new, 1)).map_err(|e| e.to_string())?;
                        Ok(format!("edited {}", t.path))
                    }
                    n => Err(format!(
                        "{}: `old` appears {n} times. Include more lines so it appears once.",
                        t.path
                    )),
                }
            }
            Tool::List(t) => {
                let dir = self.root.join(&t.dir);
                let mut out = vec![];
                list(&dir, &self.root, &mut out);
                out.sort();
                Ok(out.join("\n"))
            }
        }
    }

    /// The text for the model, and whether it is an error.
    pub fn call(&self, call: &ToolCall) -> (String, bool) {
        match Tool::parse(&call.name, &call.args).and_then(|t| self.run(t)) {
            Ok(s) => (s, false),
            Err(e) => (e, true),
        }
    }
}

fn list(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        if name == "target" || name == ".git" || out.len() >= 500 {
            continue;
        }
        let path = e.path();
        if path.is_dir() {
            list(&path, root, out);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.display().to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_arguments() {
        let e = Tool::parse("read_file", r#"{"path":"a.rs","start_line":240}"#).unwrap_err();
        assert!(e.contains("unknown field `start_line`"), "{e}");
    }

    #[test]
    fn rejects_unknown_tools() {
        assert!(
            Tool::parse("run_shell", "{}")
                .unwrap_err()
                .starts_with("unknown tool `run_shell`")
        );
    }
}
