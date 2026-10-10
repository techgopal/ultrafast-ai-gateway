//! Talking to an upstream MCP server: tool discovery.

use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

use reqwest::header::{HeaderName, HeaderValue, AUTHORIZATION};
use rmcp::model::PaginatedRequestParams;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::ServiceExt;

use crate::store::{DiscoveredTool, McpServerRow};

pub const MAX_TOOLS: usize = 2000;
pub const MAX_PAGES: usize = 20;
pub const MAX_NAME_BYTES: usize = 128;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpstreamError {
    Connect,
    Auth,
    Protocol,
    TooMany,
    Timeout,
}

impl fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Connect => "could not connect to the MCP server",
            Self::Auth => "the MCP server refused the credentials",
            Self::Protocol => "the MCP server sent an unexpected response",
            Self::TooMany => "the MCP server lists too many tools",
            Self::Timeout => "the MCP server did not answer in time",
        })
    }
}

impl std::error::Error for UpstreamError {}

/// What one discovery found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Discovered {
    pub tools: Vec<DiscoveredTool>,
    /// Tools skipped because their name is longer than `MAX_NAME_BYTES`.
    pub skipped: usize,
    /// `ttlMs` of the first list page, when the server sent one.
    pub ttl_ms: Option<u64>,
}

pub async fn discover_tools(
    http: &reqwest::Client,
    server: &McpServerRow,
    secret: Option<&str>,
) -> Result<Discovered, UpstreamError> {
    discover_tools_with(http, server, secret, REQUEST_TIMEOUT).await
}

/// As `discover_tools` with a chosen per-request timeout (tests use a short one).
pub async fn discover_tools_with(
    http: &reqwest::Client,
    server: &McpServerRow,
    secret: Option<&str>,
    timeout: Duration,
) -> Result<Discovered, UpstreamError> {
    let mut headers = HashMap::new();
    if let Some(secret) = secret {
        let (name, value) = match server.auth_kind.as_str() {
            "bearer" => (AUTHORIZATION, format!("Bearer {secret}")),
            "header" => {
                let name = server
                    .auth_header
                    .as_deref()
                    .and_then(|n| HeaderName::from_bytes(n.as_bytes()).ok())
                    .ok_or(UpstreamError::Protocol)?;
                (name, secret.to_string())
            }
            _ => (AUTHORIZATION, String::new()),
        };
        if !value.is_empty() {
            let mut value = HeaderValue::from_str(&value).map_err(|_| UpstreamError::Auth)?;
            value.set_sensitive(true);
            headers.insert(name, value);
        }
    }
    let config =
        StreamableHttpClientTransportConfig::with_uri(server.url.as_str()).custom_headers(headers);
    let transport = StreamableHttpClientTransport::with_client(http.clone(), config);
    // The client handshake is part of the first request.
    let client = tokio::time::timeout(timeout, ().serve(transport))
        .await
        .map_err(|_| UpstreamError::Timeout)?
        .map_err(|e| classify(&e.to_string()))?;
    let out = list_all(&client, timeout).await;
    let _ = client.cancel().await;
    out
}

async fn list_all(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    timeout: Duration,
) -> Result<Discovered, UpstreamError> {
    let mut out = Discovered::default();
    let mut cursor: Option<String> = None;
    for page in 0.. {
        if page == MAX_PAGES {
            return Err(UpstreamError::TooMany);
        }
        let params = PaginatedRequestParams::default().with_cursor(cursor.take());
        let result = tokio::time::timeout(timeout, client.list_tools(Some(params)))
            .await
            .map_err(|_| UpstreamError::Timeout)?
            .map_err(|e| classify(&e.to_string()))?;
        if page == 0 {
            out.ttl_ms = result.ttl_ms;
        }
        for t in result.tools {
            if t.name.len() > MAX_NAME_BYTES {
                out.skipped += 1;
                continue;
            }
            if out.tools.len() >= MAX_TOOLS {
                return Err(UpstreamError::TooMany);
            }
            out.tools.push(DiscoveredTool {
                name: t.name.to_string(),
                title: t.title,
                description: t.description.map(|d| d.to_string()).unwrap_or_default(),
                input_schema: json(&*t.input_schema)?,
                output_schema: t.output_schema.map(|s| json(&*s)).transpose()?,
                annotations: t.annotations.map(|a| json(&a)).transpose()?,
            });
        }
        cursor = result.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(out)
}

fn json<T: serde::Serialize>(v: &T) -> Result<String, UpstreamError> {
    serde_json::to_string(v).map_err(|_| UpstreamError::Protocol)
}

/// Sorts an rmcp error into one of our fixed kinds. The text is read, never kept.
fn classify(text: &str) -> UpstreamError {
    let t = text.to_ascii_lowercase();
    if t.contains("401")
        || t.contains("403")
        || t.contains("unauthorized")
        || t.contains("forbidden")
    {
        UpstreamError::Auth
    } else if t.contains("timeout") || t.contains("timed out") {
        UpstreamError::Timeout
    } else if t.contains("connect")
        || t.contains("error sending request")
        || t.contains("dns")
        || t.contains("refused")
    {
        UpstreamError::Connect
    } else {
        UpstreamError::Protocol
    }
}
