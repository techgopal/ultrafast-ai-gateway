//! A tiny MCP server that speaks JSON-RPC over stdio, for the tests of local
//! servers. Built as the test-support binary `mcp-stdio-mock`.
//!
//! Flags (first arguments):
//!   --crash            exit with status 1 at once, before any reply
//!   --stderr N         write N bytes to stderr (ending with the text END) first
//!   --child            start a `sleep 300` child and report its pid in the `pids` tool
//!   --hang             read requests and never answer
//!   --exit-after-init  exit when `notifications/initialized` arrives
//!
//! Tools: `echo` (arguments as text), `env` (the sorted names of its
//! environment, or the value of the one named by `name`), `cwd`, `pids` (its own pid and its child's), `big` (a
//! result of the number of bytes in `n`), `die` (exits without a reply).

use std::io::{BufRead, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    if has("--crash") {
        eprintln!("mcp-stdio-mock: told to crash");
        std::process::exit(1);
    }
    if let Some(at) = args.iter().position(|a| a == "--stderr") {
        let n: usize = args
            .get(at + 1)
            .and_then(|v| v.parse().ok())
            .unwrap_or(1024);
        let block = vec![b'e'; 8192];
        let mut err = std::io::stderr().lock();
        let mut left = n;
        while left > 0 {
            let k = left.min(block.len());
            let _ = err.write_all(&block[..k]);
            left -= k;
        }
        let _ = err.write_all(b"END\n");
        let _ = err.flush();
    }
    let child = if has("--child") {
        std::process::Command::new("sleep").arg("300").spawn().ok()
    } else {
        None
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if has("--hang") {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let method = msg["method"].as_str().unwrap_or("");
        if method == "notifications/initialized" && has("--exit-after-init") {
            std::process::exit(0);
        }
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let result = match method {
            "initialize" => serde_json::json!({
                "protocolVersion": "2025-06-18",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "mcp-stdio-mock", "version": "0"},
            }),
            "ping" => serde_json::json!({}),
            "tools/list" => serde_json::json!({"tools": [
                tool("echo", "echoes its arguments"),
                tool("env", "the names of its environment"),
                tool("cwd", "its working directory"),
                tool("pids", "its pid and its child's"),
                tool("big", "a result of n bytes"),
                tool("die", "exits"),
            ]}),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or("");
                let text = match name {
                    "echo" => msg["params"]["arguments"].to_string(),
                    "env" if msg["params"]["arguments"]["name"].is_string() => {
                        let name = msg["params"]["arguments"]["name"].as_str().unwrap_or("");
                        std::env::var(name).unwrap_or_default()
                    }
                    "env" => {
                        let mut names: Vec<String> = std::env::vars().map(|(k, _)| k).collect();
                        names.sort();
                        names.join(",")
                    }
                    "cwd" => std::env::current_dir()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    "pids" => format!(
                        "{} {}",
                        std::process::id(),
                        child.as_ref().map_or(0, std::process::Child::id)
                    ),
                    "big" => {
                        let n = msg["params"]["arguments"]["n"].as_u64().unwrap_or(10) as usize;
                        "b".repeat(n)
                    }
                    "die" => std::process::exit(3),
                    _ => {
                        let _ = writeln!(
                            out,
                            "{}",
                            serde_json::json!({"jsonrpc": "2.0", "id": id,
                                "error": {"code": -32602, "message": "unknown tool"}})
                        );
                        let _ = out.flush();
                        continue;
                    }
                };
                serde_json::json!({"content": [{"type": "text", "text": text}], "isError": false})
            }
            "resources/list" => serde_json::json!({"resources": []}),
            _ => {
                let _ = writeln!(
                    out,
                    "{}",
                    serde_json::json!({"jsonrpc": "2.0", "id": id,
                        "error": {"code": -32601, "message": "method not found"}})
                );
                let _ = out.flush();
                continue;
            }
        };
        let _ = writeln!(
            out,
            "{}",
            serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result})
        );
        let _ = out.flush();
    }
}

fn tool(name: &str, description: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": {}},
    })
}
