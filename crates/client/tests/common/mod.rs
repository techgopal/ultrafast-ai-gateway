//! A scripted HTTP server on a free port that records what it was sent.
#![allow(dead_code)]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone)]
pub struct Script {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Body parts, each sent as its own chunk with a pause after it.
    pub parts: Vec<Vec<u8>>,
    /// False: the connection is cut in the middle of the last chunk.
    pub clean_end: bool,
    /// Never answer at all.
    pub hang: bool,
}

impl Script {
    pub fn json(status: u16, body: &str) -> Self {
        Script {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            parts: vec![body.as_bytes().to_vec()],
            clean_end: true,
            hang: false,
        }
    }

    pub fn sse(parts: Vec<Vec<u8>>) -> Self {
        Script {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            parts,
            clean_end: true,
            hang: false,
        }
    }

    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }

    pub fn cut(mut self) -> Self {
        self.clean_end = false;
        self
    }
}

pub struct Server {
    pub url: String,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    /// Every request received so far, as head + body text, lower-cased heads.
    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }

    pub fn only(&self) -> String {
        let r = self.requests();
        assert_eq!(r.len(), 1, "expected exactly one request: {r:?}");
        r[0].clone()
    }
}

pub async fn serve(script: Script) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let _ = sock.set_nodelay(true);
            let script = script.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut tmp = [0u8; 4096];
                loop {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(end) = find(&buf, b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&buf[..end]).to_string();
                        let len = head
                            .to_lowercase()
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:").map(str::to_string))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if buf.len() >= end + 4 + len {
                            let body = String::from_utf8_lossy(&buf[end + 4..end + 4 + len]);
                            log.lock()
                                .unwrap()
                                .push(format!("{}\r\n\r\n{body}", head.to_lowercase()));
                            break;
                        }
                    }
                }
                if script.hang {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    return;
                }
                let mut out = format!("HTTP/1.1 {} X\r\n", script.status);
                for (k, v) in &script.headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("transfer-encoding: chunked\r\nconnection: close\r\n\r\n");
                let _ = sock.write_all(out.as_bytes()).await;
                for (i, part) in script.parts.iter().enumerate() {
                    let cut_here = !script.clean_end && i + 1 == script.parts.len();
                    let declared = part.len() + if cut_here { 50 } else { 0 };
                    let _ = sock.write_all(format!("{declared:x}\r\n").as_bytes()).await;
                    let _ = sock.write_all(part).await;
                    if !cut_here {
                        let _ = sock.write_all(b"\r\n").await;
                    }
                    let _ = sock.flush().await;
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                if script.clean_end {
                    let _ = sock.write_all(b"0\r\n\r\n").await;
                }
                let _ = sock.shutdown().await;
            });
        }
    });
    Server { url, requests }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// A URL where nothing listens.
pub async fn dead_url() -> String {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    drop(l);
    url
}

pub const OPENAI_CHAT: &str = r#"{"id":"c1","model":"gpt-4o","choices":[{"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2}}"#;
pub const ANTHROPIC_CHAT: &str = r#"{"id":"m1","model":"claude-sonnet-5","content":[{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":2}}"#;
pub const GEMINI_CHAT: &str = r#"{"candidates":[{"content":{"parts":[{"text":"hello"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}"#;
