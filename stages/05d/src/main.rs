use kaam::agent::{Agent, Outcome};
use kaam::client::{Client, Transport};
use kaam::http::Endpoint;
use kaam::provider::openai::OpenAi;
use kaam::tools::Workspace;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
usage: kaam [options] \"task\"

options:
  --model M        the model id (default: per provider, or $KAAM_MODEL)
  --root DIR       the project folder (default: .)
  --record DIR     write every request body and response body to DIR (never headers)
  --replay DIR     replay a recorded session: no network, no key
  --max-steps N    stop after N model turns (default 20)";

struct Options {
    model: Option<String>,
    root: PathBuf,
    record: Option<PathBuf>,
    replay: Option<PathBuf>,
    max_steps: usize,
    task: Option<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        model: std::env::var("KAAM_MODEL").ok(),
        root: ".".into(),
        record: None,
        replay: None,
        max_steps: 20,
        task: None,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut value = || it.next().cloned().ok_or(format!("{a} needs a value"));
        match a.as_str() {
            "--model" => o.model = Some(value()?),
            "--root" => o.root = value()?.into(),
            "--record" => o.record = Some(value()?.into()),
            "--replay" => o.replay = Some(value()?.into()),
            "--max-steps" => o.max_steps = value()?.parse().map_err(|e| format!("--max-steps: {e}"))?,
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
fn workspace(o: &Options) -> Result<Workspace, String> {
    let ws = Workspace::new(&o.root).map_err(|e| format!("{}: {e}", o.root.display()))?;
    Ok(ws)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let o = match parse(&args) {
        Ok(o) if o.task.is_some() => o,
        Ok(_) => {
            eprintln!("kaam: no task given\n\n{USAGE}");
            return ExitCode::from(2);
        }
        Err(e) if e.is_empty() => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("kaam: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(o) {
        Ok(code) => code,
        // a failure while running (the network, the API) is not a usage mistake: no help text
        Err(e) => {
            eprintln!("kaam: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(o: Options) -> Result<ExitCode, String> {
    let task = o.task.clone().unwrap_or_default();
    let provider = OpenAi {
        model: o.model.clone().unwrap_or("gpt-5.4-mini-2026-03-17".into()),
        key: if o.replay.is_none() {
            Some(key("OPENAI_API_KEY")?)
        } else {
            None
        },
        path: "/v1/chat/completions".into(),
        strict: true,
    };
    let transport = match &o.replay {
        Some(dir) => Client::replay_from(dir).map_err(|e| e.to_string())?,
        None => Transport::Live(Endpoint::parse("https://api.openai.com").map_err(|e| e.to_string())?),
    };
    let model = provider.model().to_string();
    let mut client = Client::new(provider, transport);
    client.record = o.record.clone();

    let ws = workspace(&o)?;
    println!("kaam · {} · {}", model, ws.root.display());

    let mut agent = Agent::new(client, ws);
    agent.max_steps = o.max_steps;
    match agent.run(&task).map_err(|e| e.to_string())? {
        Outcome::Done { steps } => {
            println!("done in {steps} turns");
            Ok(ExitCode::SUCCESS)
        }
        Outcome::StepBudget => {
            println!("stopped: the step budget ({}) ran out", o.max_steps);
            Ok(ExitCode::from(3))
        }
    }
}
