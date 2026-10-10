//! What a call's guardrail checks found, as the request log keeps it.
//!
//! Ids, names, actions, directions and counts only: never the text that
//! matched. SQLite hands out the ids of deleted rows again, so the log keeps
//! the guardrail's name next to its id.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Direction, Outcome};

/// What the checks of a call did, worst first: a block, else a redaction,
/// else a flag. The order is the order of severity.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    utoipa::ToSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LoggedAction {
    Flagged,
    Redacted,
    Blocked,
}

impl LoggedAction {
    pub fn as_str(self) -> &'static str {
        match self {
            LoggedAction::Flagged => "flagged",
            LoggedAction::Redacted => "redacted",
            LoggedAction::Blocked => "blocked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "flagged" => Some(LoggedAction::Flagged),
            "redacted" => Some(LoggedAction::Redacted),
            "blocked" => Some(LoggedAction::Blocked),
            _ => None,
        }
    }
}

/// A guardrail by id and name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GuardrailRef {
    pub id: i64,
    pub name: String,
}

/// A flag rule that matched.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct FlagLog {
    pub guardrail_id: i64,
    pub rule_id: String,
}

/// The checks of one direction (the input of a call, or its output).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct SideLog {
    /// The worst thing that happened in this direction.
    pub action: LoggedAction,
    /// The guardrails this direction was checked with.
    pub checked_with: Vec<GuardrailRef>,
    /// The guardrail that blocked, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<GuardrailRef>,
    /// Replacements made, by PII type or rule id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub redactions: BTreeMap<String, u32>,
    /// Flag rules that matched.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<FlagLog>,
}

impl SideLog {
    /// The side a check came to, or `None` when it found nothing.
    pub fn of(checked_with: &[GuardrailRef], outcome: &Outcome) -> Option<Self> {
        let action = if outcome.blocked_by.is_some() {
            LoggedAction::Blocked
        } else if !outcome.redactions.is_empty() {
            LoggedAction::Redacted
        } else if !outcome.flags.is_empty() || outcome.external_failed() {
            LoggedAction::Flagged
        } else {
            return None;
        };
        Some(SideLog {
            action,
            checked_with: checked_with.to_vec(),
            blocked_by: outcome.blocked_by.as_ref().map(|(id, name)| GuardrailRef {
                id: *id,
                name: name.clone(),
            }),
            redactions: outcome.redactions.clone(),
            flags: outcome
                .all_flags()
                .into_iter()
                .map(|(g, r)| FlagLog {
                    guardrail_id: g,
                    rule_id: r,
                })
                .collect(),
        })
    }
}

/// The guardrail record of a call: `None` for a direction that found nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct GuardrailLog {
    /// The worst action of either direction.
    pub action: LoggedAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<SideLog>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<SideLog>,
}

impl GuardrailLog {
    /// Puts a direction's result in, or leaves things as they are for `None`.
    pub fn with(log: Option<GuardrailLog>, dir: Direction, side: Option<SideLog>) -> Option<Self> {
        let Some(side) = side else {
            return log;
        };
        let mut log = log.unwrap_or(GuardrailLog {
            action: side.action,
            input: None,
            output: None,
        });
        log.action = log.action.max(side.action);
        match dir {
            Direction::Input => log.input = Some(side),
            Direction::Output => log.output = Some(side),
        }
        Some(log)
    }

    /// What someone who is not an admin may see: the action of the call and
    /// of each direction, and nothing about which guardrail, rule or type
    /// did it (a flag is the silent action, and the people it watches should
    /// not be able to read its rules from their own log).
    pub fn action_only(&self) -> Self {
        let strip = |side: &SideLog| SideLog {
            action: side.action,
            checked_with: Vec::new(),
            blocked_by: None,
            redactions: BTreeMap::new(),
            flags: Vec::new(),
        };
        GuardrailLog {
            action: self.action,
            input: self.input.as_ref().map(strip),
            output: self.output.as_ref().map(strip),
        }
    }

    /// The stored form.
    pub fn to_stored(&self) -> String {
        serde_json::to_string(self).expect("a plain struct serializes")
    }

    /// Reads the stored form; a row nobody can read reads as none.
    pub fn from_stored(text: &str) -> Option<Self> {
        serde_json::from_str(text).ok()
    }
}
