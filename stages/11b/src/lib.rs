//! kaam: a small terminal coding agent.

#[cfg(not(target_os = "linux"))]
compile_error!("kaam runs on Linux: its sandbox is Landlock and its process control is Linux system calls");

pub mod agent;
pub mod chat;
pub mod client;
pub mod exec;
pub mod http;
pub mod mcp;
pub mod provider;
pub mod sandbox;
pub mod signal;
pub mod sse;
pub mod sys;
pub mod tools;
