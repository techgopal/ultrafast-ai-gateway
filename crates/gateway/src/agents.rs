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
