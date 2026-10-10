//! The client toward an upstream MCP server.
// stub: replaced by Task 3

use std::fmt;

use crate::store::{DiscoveredTool, McpServerRow};

/// Why an upstream could not be used. The text is ours, never the upstream's.
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
            Self::Connect => "could not connect to the server",
            Self::Auth => "the server refused the credentials",
            Self::Protocol => "the server did not answer as an MCP server",
            Self::TooMany => "the server lists too many tools",
            Self::Timeout => "the server did not answer in time",
        })
    }
}

impl std::error::Error for UpstreamError {}

/// What a discovery found.
#[derive(Debug, Clone, Default)]
pub struct Discovered {
    pub tools: Vec<DiscoveredTool>,
    /// Tools the upstream listed that were left out (unusable names, shapes).
    pub skipped: usize,
    pub ttl_ms: Option<u64>,
}

/// The tools the server offers.
pub async fn discover_tools(
    _http: &reqwest::Client,
    _server: &McpServerRow,
    _secret: Option<&str>,
) -> Result<Discovered, UpstreamError> {
    Err(UpstreamError::Connect)
}
