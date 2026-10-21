use kaam::http::{self, Endpoint};
use serde_json::json;
use std::io::Read;

fn main() {
    let key = std::env::var("OPENAI_API_KEY").expect("set OPENAI_API_KEY");
    let ep = Endpoint::parse("https://api.openai.com").unwrap();
    let body = json!({
        "model": "gpt-5.4-mini-2026-03-17",
        "messages": [{ "role": "user", "content": "Say hi in three words." }],
    });
    let headers = [("Authorization".to_string(), format!("Bearer {key}"))];
    let mut res = http::post(&ep, "/v1/chat/completions", &headers, body.to_string().as_bytes()).unwrap();
    let mut text = String::new();
    res.body.read_to_string(&mut text).unwrap();
    println!("HTTP {}\n{text}", res.status);
}
