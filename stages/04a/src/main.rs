use kaam::chat::Message;
use kaam::http::{self, Endpoint};
use kaam::provider::openai::OpenAi;
use std::io::{self, BufRead, Write};

fn main() {
    let provider = OpenAi {
        model: "gpt-5.4-mini-2026-03-17".into(),
        key: Some(std::env::var("OPENAI_API_KEY").expect("set OPENAI_API_KEY")),
        path: "/v1/chat/completions".into(),
    };
    let ep = Endpoint::parse("https://api.openai.com").unwrap();
    let mut history = vec![];
    for line in io::stdin().lock().lines() {
        history.push(Message::User(line.unwrap()));
        let req = provider.request(&history);
        let mut res = http::post(&ep, &req.path, &req.headers, &req.body).unwrap();
        print!("> ");
        let reply = provider
            .parse(&mut res.body, &mut |t| {
                print!("{t}");
                let _ = io::stdout().flush();
            })
            .unwrap();
        println!(
            "\n  · tokens: {} in, {} out",
            reply.usage.input, reply.usage.output
        );
        history.push(Message::Assistant { text: reply.text });
    }
}
