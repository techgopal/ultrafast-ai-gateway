//! What a coding agent says about itself on a request (Claude Code, Codex).

/// Who made a call, as far as the agent's own headers tell. Each value is
/// cut to 128 printable ASCII characters; anything else reads as absent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentInfo {
    pub session: Option<String>,
    pub agent: Option<String>,
    pub prompt: Option<String>,
    pub class: Option<String>,
    /// `claude-code` or `codex`.
    pub client: Option<&'static str>,
}

use axum::http::HeaderMap;

/// A header value cut to 128 characters; `None` when it holds anything but
/// printable ASCII or is empty.
fn clean(headers: &HeaderMap, name: &str) -> Option<String> {
    let v = headers.get(name)?.to_str().ok()?;
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_graphic() || b == b' ') {
        return None;
    }
    Some(v.chars().take(128).collect())
}

/// Reads what Claude Code (`x-claude-code-*`) or Codex (`session-id`,
/// `originator`) say about themselves. Nothing here is a credential.
pub fn agent_info(headers: &HeaderMap) -> AgentInfo {
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let claude = headers
        .keys()
        .any(|k| k.as_str().starts_with("x-claude-code-"))
        || user_agent.starts_with("claude-cli");
    let codex = headers
        .get("originator")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.to_ascii_lowercase().contains("codex"))
        || user_agent.contains("codex");
    AgentInfo {
        session: clean(headers, "x-claude-code-session-id")
            .or_else(|| clean(headers, "session-id")),
        agent: clean(headers, "x-claude-code-agent-id"),
        prompt: clean(headers, "x-claude-code-prompt-id"),
        class: clean(headers, "x-claude-code-request-class"),
        client: if claude {
            Some("claude-code")
        } else if codex {
            Some("codex")
        } else {
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                v.parse().unwrap(),
            );
        }
        h
    }

    #[test]
    fn claude_code_headers() {
        let i = agent_info(&map(&[
            ("x-claude-code-session-id", "s1"),
            ("x-claude-code-agent-id", "a1"),
            ("x-claude-code-prompt-id", "p1"),
            ("x-claude-code-request-class", "main"),
        ]));
        assert_eq!(
            i,
            AgentInfo {
                session: Some("s1".into()),
                agent: Some("a1".into()),
                prompt: Some("p1".into()),
                class: Some("main".into()),
                client: Some("claude-code"),
            }
        );
        assert_eq!(
            agent_info(&map(&[("user-agent", "claude-cli/2.1.306 (external)")])).client,
            Some("claude-code")
        );
    }

    #[test]
    fn codex_headers() {
        let i = agent_info(&map(&[("session-id", "c1"), ("originator", "codex_exec")]));
        assert_eq!(i.session.as_deref(), Some("c1"));
        assert_eq!(i.client, Some("codex"));
        assert_eq!(
            agent_info(&map(&[("user-agent", "codex_cli_rs/0.160.1")])).client,
            Some("codex")
        );
        assert_eq!(
            agent_info(&map(&[("user-agent", "curl/8")])),
            AgentInfo::default()
        );
    }

    #[test]
    fn values_are_cut_and_cleaned() {
        let long = "a".repeat(300);
        let i = agent_info(&map(&[("x-claude-code-session-id", &long)]));
        assert_eq!(i.session.unwrap().len(), 128);
        let mut h = HeaderMap::new();
        h.insert(
            "x-claude-code-session-id",
            axum::http::HeaderValue::from_bytes("caf\u{e9}".as_bytes()).unwrap(),
        );
        assert_eq!(agent_info(&h).session, None);
        // Claude Code's own session wins over a Codex one.
        let i = agent_info(&map(&[
            ("x-claude-code-session-id", "x"),
            ("session-id", "y"),
        ]));
        assert_eq!(i.session.as_deref(), Some("x"));
    }
}
