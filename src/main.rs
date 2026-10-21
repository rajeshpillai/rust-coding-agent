use kaam::agent::{Agent, Approver, Outcome};
use kaam::client::{Client, Transport};
use kaam::http::Endpoint;
use kaam::mcp::{self, McpTools};
use kaam::provider::{Provider, anthropic::Anthropic, openai::OpenAi};
use kaam::sandbox::Sandbox;
use kaam::signal;
use kaam::tools::{ToolHost, Workspace};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "\
usage: kaam [options] \"task\"
       kaam mcp [--root DIR] [--no-sandbox]    serve the tools over MCP (stdio)

options:
  --provider P     openai (default), anthropic, gemini, ollama
  --model M        the model id (default: per provider, or $KAAM_MODEL)
  --root DIR       the project folder (default: .)
  --yes            approve every command and edit
  --no-sandbox     run commands without Landlock
  --mcp            run the tools through `kaam mcp`, not in this process
  --record DIR     write every request body and response body to DIR (never headers)
  --replay DIR     replay a recorded session: no network, no key
  --max-steps N    stop after N model turns (default 20)
  --timeout S      kill a command after S seconds (default 120)";

struct Options {
    provider: String,
    model: Option<String>,
    root: PathBuf,
    yes: bool,
    sandbox: bool,
    mcp: bool,
    record: Option<PathBuf>,
    replay: Option<PathBuf>,
    max_steps: usize,
    timeout: u64,
    task: Option<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        provider: "openai".into(),
        model: std::env::var("KAAM_MODEL").ok(),
        root: ".".into(),
        yes: false,
        sandbox: true,
        mcp: false,
        record: None,
        replay: None,
        max_steps: 20,
        timeout: 120,
        task: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--provider" => o.provider = value()?,
            "--model" => o.model = Some(value()?),
            "--root" => o.root = value()?.into(),
            "--yes" => o.yes = true,
            "--no-sandbox" => o.sandbox = false,
            "--mcp" => o.mcp = true,
            "--record" => o.record = Some(value()?.into()),
            "--replay" => o.replay = Some(value()?.into()),
            "--max-steps" => o.max_steps = value()?.parse().map_err(|e| format!("--max-steps: {e}"))?,
            "--timeout" => o.timeout = value()?.parse().map_err(|e| format!("--timeout: {e}"))?,
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s => o.task = Some(s.to_string()),
        }
    }
    Ok(o)
}

fn key(var: &str) -> Result<String, String> {
    std::env::var(var)
        .ok()
        .filter(|k| !k.is_empty())
        .ok_or(format!("set {var}, or use --replay"))
}

fn provider(o: &Options, need_key: bool) -> Result<(Box<dyn Provider>, String), String> {
    let model = |default: &str| o.model.clone().unwrap_or(default.to_string());
    let k = |var: &str| if need_key { key(var) } else { Ok(String::new()) };
    Ok(match o.provider.as_str() {
        "openai" => (
            Box::new(OpenAi {
                model: model("gpt-5.4-mini-2026-03-17"),
                key: Some(k("OPENAI_API_KEY")?),
                path: "/v1/chat/completions".into(),
                strict: true,
            }),
            "https://api.openai.com".into(),
        ),
        "gemini" => (
            Box::new(OpenAi {
                model: model("gemini-3.8-flash"),
                key: Some(k("GEMINI_API_KEY")?),
                path: "/v1beta/openai/chat/completions".into(),
                strict: false,
            }),
            "https://generativelanguage.googleapis.com".into(),
        ),
        "ollama" => (
            Box::new(OpenAi {
                model: model("qwen3:1.7b"),
                key: None,
                path: "/v1/chat/completions".into(),
                strict: false,
            }),
            std::env::var("OLLAMA_HOST").unwrap_or("http://localhost:11434".into()),
        ),
        "anthropic" => (
            Box::new(Anthropic {
                model: o.model.clone().ok_or("--model is required for anthropic")?,
                key: k("ANTHROPIC_API_KEY")?,
                max_tokens: 4096,
            }),
            "https://api.anthropic.com".into(),
        ),
        p => return Err(format!("unknown provider {p}")),
    })
}

fn workspace(o: &Options) -> Result<Workspace, String> {
    let mut ws = Workspace::new(&o.root).map_err(|e| format!("{}: {e}", o.root.display()))?;
    ws.limits.timeout = Duration::from_secs(o.timeout);
    if o.sandbox {
        let writable = vec![ws.root.clone(), "/tmp".into(), "/dev/null".into()];
        ws.sandbox = Some(
            Sandbox::new(&writable)
                .map_err(|e| format!("Landlock is not available ({e}); use --no-sandbox"))?,
        );
    }
    Ok(ws)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("mcp") {
        let r = parse(&args[1..])
            .and_then(|o| workspace(&o))
            .map_err(|e| e.to_string())
            .and_then(|mut ws| mcp::serve(&mut ws).map_err(|e| e.to_string()));
        return match r {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("kaam mcp: {e}");
                ExitCode::FAILURE
            }
        };
    }
    match run(&args) {
        Ok(code) => code,
        Err(e) if e.is_empty() => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("kaam: {e}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let o = parse(args)?;
    let task = o.task.clone().ok_or("no task given")?;
    let (provider, base) = provider(&o, o.replay.is_none())?;
    let transport = match &o.replay {
        Some(dir) => Client::replay_from(dir).map_err(|e| e.to_string())?,
        None => Transport::Live(Endpoint::parse(&base).map_err(|e| e.to_string())?),
    };
    let model = provider.model().to_string();
    let mut client = Client::new(provider, transport);
    client.record = o.record.clone();

    let ws = workspace(&o)?;
    println!("kaam · {} · {} · {}", o.provider, model, ws.root.display());
    match &ws.sandbox {
        Some(s) => println!(
            "sandbox: Landlock ABI {}, writes only in {}",
            s.abi,
            s.writable
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        None => println!("sandbox: off"),
    }
    let host: Box<dyn ToolHost> = if o.mcp {
        let me = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut a = vec!["mcp".to_string(), "--root".into(), ws.root.display().to_string()];
        if !o.sandbox {
            a.push("--no-sandbox".into());
        }
        println!("tools: MCP server `kaam mcp`, protocol {}", mcp::VERSION);
        Box::new(McpTools::spawn(&me.display().to_string(), &a).map_err(|e| e.to_string())?)
    } else {
        Box::new(ws)
    };

    signal::install();
    let mut agent = Agent::new(client, host);
    agent.max_steps = o.max_steps;
    if o.yes {
        agent.approver = Approver::Yes;
    }
    match agent.run(&task).map_err(|e| e.to_string())? {
        Outcome::Done { steps } => {
            println!("done in {steps} turns");
            Ok(ExitCode::SUCCESS)
        }
        Outcome::StepBudget => {
            println!("stopped: the step budget ({}) ran out", o.max_steps);
            Ok(ExitCode::from(3))
        }
        Outcome::Cancelled => {
            println!("cancelled");
            Ok(ExitCode::from(130))
        }
    }
}
