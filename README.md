# kaam — a terminal coding agent in Rust

`kaam` is a small coding agent that runs in your terminal. You give it a task. It asks a model what to
do, runs tools in your project, shows you each step, and stops when the task is done.

```text
$ kaam --root examples/textkit "cargo test fails in this crate. Fix it."
kaam · openai · gpt-5.4-mini-2026-03-17 · examples/textkit
sandbox: Landlock ABI 8, writes only in examples/textkit, /tmp, /dev/null
→ run_command {"command": "cargo test"}
  run it? [y/N] y
  ← running 3 tests … [exit code 101]
→ read_file {"path":"src/lib.rs"}
→ edit_file {"path":"src/lib.rs", ...}
  run it? [y/N] y
  ← edited src/lib.rs
→ run_command {"command":"cargo test"}
  ← running 3 tests … [exit code 0]
done in 5 turns
```

The code is the companion to a step-by-step video course. It is small on purpose: every line is
written on screen.

## What it does

| Part | How |
|---|---|
| Models | OpenAI by default. Claude, Gemini and a local model (Ollama) use the same `Provider` trait. |
| HTTP | HTTP/1.1 written by hand over a TCP or TLS stream, with chunked bodies and server-sent events. |
| Tools | `run_command`, `read_file`, `edit_file`, `list_files`. Arguments are checked strictly. |
| Commands | Each command runs in its own process group. Output is merged in order and capped. A timeout or Ctrl+C kills the whole group. |
| Approval | Every command and every edit waits for your yes. |
| Sandbox | Commands run under Linux Landlock. A write outside the project fails with `Permission denied`. |
| MCP | `kaam mcp` serves the tools over MCP (stdio). `kaam --mcp` uses them through that server. |
| Record and replay | `--record DIR` saves each request body and response body. `--replay DIR` plays a session again with no network and no key. |

Crates: `serde`, `serde_json`, `rustls` and `webpki-roots`. Everything else is the standard library.

## Run it

You need Linux and Rust 1.95 (the repo pins it in `rust-toolchain.toml`).

Replay the recorded session. This needs no key and no network. The model's replies come from the
recording; the tools run for real:

```bash
cp -r examples/textkit /tmp/textkit
cargo run --release -- --root /tmp/textkit --yes --replay examples/textkit-session "cargo test fails in this crate. Fix it."
```

Run it live with OpenAI:

```bash
export OPENAI_API_KEY=...
cargo run --release -- --root /tmp/textkit "cargo test fails in this crate. Fix it."
```

Other providers:

```bash
kaam --provider anthropic --model <model-id> "..."   # ANTHROPIC_API_KEY
kaam --provider gemini "..."                          # GEMINI_API_KEY
kaam --provider ollama --model qwen3:1.7b "..."       # a local Ollama on localhost:11434
```

`kaam --help` lists every option.

## What was tested, and what was not

- Tested on Linux 7.0 (Landlock ABI 8) with rustc 1.95.0: the unit and integration tests, live runs
  with OpenAI `gpt-5.4-mini-2026-03-17`, replay, and the MCP server with the official MCP Inspector
  2.11.0.
- The Anthropic and Gemini adapters follow each provider's API documentation. They have **not been run
  yet**.
- The local model path works with Ollama's OpenAI-compatible endpoint. On a laptop CPU an agent turn
  takes minutes, so it was not used for the recordings.
- Linux only. The sandbox is Landlock, and process control uses Linux system calls.
- The sandbox limits writes, not reads, and not the network.

## MCP

`kaam mcp` speaks MCP protocol version **2026-07-28**: every request carries its version in `_meta`,
and there is no handshake. It also answers the older `initialize` handshake (2025-11-25), because
clients from before July 2026 still use it. The official Inspector 2.11.0 is one of them.

## Licence

MIT. See `LICENSE`.
