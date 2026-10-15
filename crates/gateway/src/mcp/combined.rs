//! `POST /mcp`: one endpoint for every tool a key may call, each named
//! `server__tool`.
//!
//! The separator is `__`, not `/`, which MCP clients mangle in tool names. A
//! server name may not contain `_` (see `validate_server_name`), so the name
//! is split at the first `__` and the server is unambiguous, whatever the
//! tool is called: tool `b__c` of server `a` is `a__b__c`, which splits into
//! `a` and `b__c`. Telemetry is unchanged: a call is still
//! `mcp:<server>/<tool>` in the logs.

use serde_json::Value;

use super::server::tool_json;
use crate::access;
use crate::snapshot::{SnapKey, SnapMcpServer, Snapshot};

/// The separator between a server and a tool in the combined endpoint.
pub const SEPARATOR: &str = "__";

/// The name a tool has in the combined endpoint.
pub fn exposed_name(server: &str, tool: &str) -> String {
    format!("{server}{SEPARATOR}{tool}")
}

/// `(server, tool)` of a combined name: split at the first separator, the
/// server must exist and the tool must be non-empty.
pub fn split(name: &str) -> Option<(&str, &str)> {
    let (server, tool) = name.split_once(SEPARATOR)?;
    (!server.is_empty() && !tool.is_empty()).then_some((server, tool))
}

/// The server and the name of the tool a combined name refers to, if they
/// are in the snapshot. Whether the key may call it is decided by the call.
pub fn resolve<'a>(snapshot: &'a Snapshot, name: &'a str) -> Option<(&'a SnapMcpServer, &'a str)> {
    let (server, tool) = split(name)?;
    let server = snapshot
        .mcp_server(server)
        .filter(|s| s.enabled && s.usable)?;
    Some((server, tool))
}

/// Every tool the key may call, by server and tool name.
pub fn list(snapshot: &Snapshot, key: &SnapKey) -> Vec<Value> {
    let mut all = access::callable_tools_of_key(snapshot, key);
    all.sort_by(|a, b| (&a.0.name, &a.1.name).cmp(&(&b.0.name, &b.1.name)));
    all.into_iter()
        .map(|(server, tool)| tool_json(&server.name, tool, exposed_name(&server.name, &tool.name)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_splits_at_the_first_separator() {
        assert_eq!(split("a__b"), Some(("a", "b")));
        assert_eq!(split("a__b__c"), Some(("a", "b__c")));
        assert_eq!(split("a-b__c"), Some(("a-b", "c")));
        assert_eq!(split("ab"), None);
        assert_eq!(split("__b"), None);
        assert_eq!(split("a__"), None);
        assert_eq!(exposed_name("a", "b__c"), "a__b__c");
    }

    #[test]
    fn servers_that_could_collide_cannot_exist() {
        // `a` with a tool `b__c` and `a-b` with a tool `c`: different names.
        assert_ne!(exposed_name("a", "b__c"), exposed_name("a-b", "c"));
        // A server name with an underscore would be ambiguous: it is refused.
        assert!(crate::mcp::validate_server_name("a_b").is_err());
        assert!(crate::mcp::validate_server_name("a__b").is_err());
    }
}
