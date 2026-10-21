use kaam::chat::Message;
use kaam::http::{self, Endpoint};
use kaam::provider::openai::OpenAi;
use kaam::tools::{self, Tool};
use std::io::{self, Write};

fn main() {
    let task = std::env::args().nth(1).expect("give a task");
    let provider = OpenAi {
        model: "gpt-5.4-mini-2026-03-17".into(),
        key: Some(std::env::var("OPENAI_API_KEY").expect("set OPENAI_API_KEY")),
        path: "/v1/chat/completions".into(),
        strict: true,
    };
    let ep = Endpoint::parse("https://api.openai.com").unwrap();
    let req = provider.request(&[Message::User(task)], &tools::specs());
    let mut res = http::post(&ep, &req.path, &req.headers, &req.body).unwrap();
    let reply = provider
        .parse(&mut res.body, &mut |t| {
            print!("{t}");
            let _ = io::stdout().flush();
        })
        .unwrap();
    for call in &reply.calls {
        println!("→ {} {}", call.name, call.args);
        match Tool::parse(&call.name, &call.args).and_then(tools::run) {
            Ok(out) => print!("{out}"),
            Err(e) => println!("error: {e}"),
        }
    }
}
