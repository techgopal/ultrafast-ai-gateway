//! `POST /mcp`: one endpoint for every tool a key may call, each named
//! `server__tool`.
//!
//! The separator is `__`, not `/`, which MCP clients mangle in tool names. A
//! server name may not contain `_` (see `validate_server_name`), so the name
//! is split at the first `__` and the server is unambiguous, whatever the
//! tool is called: tool `b__c` of server `a` is `a__b__c`, which splits into
//! `a` and `b__c`. Telemetry is unchanged: a call is still
//! `mcp:<server>/<tool>` in the logs.

use serde_json::{json, Value};

use super::discover;
use super::server::tool_json;
use crate::access;
use crate::snapshot::{SnapKey, SnapMcpServer, SnapMcpTool, Snapshot};

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

/// What `tools/list` gives the key: every tool it may call, or, when that
/// is more than `inline_max`, the search tools and the tools pinned to the
/// key that it may still call.
pub fn list(snapshot: &Snapshot, key: &SnapKey, inline_max: usize) -> Vec<Value> {
    let mut all = access::callable_tools_of_key(snapshot, key);
    all.sort_by(|a, b| (&a.0.name, &a.1.name).cmp(&(&b.0.name, &b.1.name)));
    let shown = |server: &SnapMcpServer, tool: &SnapMcpTool| {
        tool_json(&server.name, tool, exposed_name(&server.name, &tool.name))
    };
    if all.len() <= inline_max {
        return all.into_iter().map(|(s, t)| shown(s, t)).collect();
    }
    let mut out = discover::meta_tools();
    for entry in &key.pinned {
        let Some((server, tool)) = crate::mcp::parse_allow_entry(entry) else {
            continue;
        };
        if let Some((s, t)) = all.iter().find(|(s, t)| s.name == server && t.name == tool) {
            out.push(shown(s, t));
        }
    }
    out
}

fn may_call(snapshot: &Snapshot, key: &SnapKey, server: &str, tool: &str) -> bool {
    snapshot
        .mcp_server(server)
        .zip(snapshot.mcp_tool(server, tool))
        .is_some_and(|(s, t)| {
            s.enabled && s.usable && access::key_may_call_tool(snapshot, key, s, t)
        })
}

/// A result of a gateway tool: the data as text and as structured content.
fn answer(data: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": data.to_string()}],
        "structuredContent": data,
        "isError": false,
    })
}

/// `find_tools`: the best matches among the tools the key may call.
pub fn find(snapshot: &Snapshot, key: &SnapKey, arguments: &Value) -> Result<Value, &'static str> {
    let query = arguments["query"]
        .as_str()
        .filter(|q| !q.trim().is_empty())
        .ok_or("query is required.")?;
    let limit = match arguments.get("limit") {
        None | Some(Value::Null) => 5,
        Some(v) => v
            .as_u64()
            .filter(|n| (1..=discover::MAX_RESULTS as u64).contains(n))
            .ok_or("limit must be a whole number from 1 to 20.")? as usize,
    };
    let hits = snapshot
        .tool_index()
        .search(query, limit, &|s, t| may_call(snapshot, key, s, t));
    let tools: Vec<Value> = hits
        .iter()
        .filter_map(|h| {
            let tool = snapshot.mcp_tool(h.server, h.tool)?;
            Some(json!({
                "name": exposed_name(h.server, h.tool),
                "server": h.server,
                "title": tool.title,
                "description": tool.description,
            }))
        })
        .collect();
    Ok(answer(json!({ "tools": tools })))
}

/// `describe_tools`: the schemas of tools, by the names `find_tools` gave. A
/// name the key may not use is left out as a name that does not exist is.
pub fn describe(
    snapshot: &Snapshot,
    key: &SnapKey,
    arguments: &Value,
) -> Result<Value, &'static str> {
    let names = arguments["names"]
        .as_array()
        .filter(|n| !n.is_empty() && n.len() <= discover::MAX_DESCRIBE)
        .ok_or("names must list from 1 to 20 tool names.")?;
    let mut tools = Vec::new();
    for name in names {
        let name = name.as_str().ok_or("names must be text.")?;
        let Some((server, tool)) = split(name) else {
            continue;
        };
        if !may_call(snapshot, key, server, tool) {
            continue;
        }
        if let Some(t) = snapshot.mcp_tool(server, tool) {
            tools.push(tool_json(server, t, exposed_name(server, tool)));
        }
    }
    Ok(answer(json!({ "tools": tools })))
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
