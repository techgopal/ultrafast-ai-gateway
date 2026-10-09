//! Guardrail engine: matchers, redaction and a streaming scanner.
//!
//! A pure module: no storage, no API, no proxy wiring. A guardrail is a list of
//! rules (keywords, a regular expression, built-in PII detectors), each with an
//! action (block, redact, flag) and the directions it applies to. Rules are
//! compiled once into a [`Compiled`] and shared.
//!
//! Semantics (the same for whole texts and for streams):
//! - every rule of every guardrail in the set is matched against the ORIGINAL
//!   text, never against another rule's output;
//! - any matching `block` rule wins: the texts are left untouched and the
//!   first blocking guardrail (in set order) is reported;
//! - otherwise `redact` matches are replaced in one pass, leftmost-longest
//!   first (ties: the earlier rule), never twice over the same characters;
//! - `flag` matches are recorded once per rule.
//!
//! Text handling:
//! - JSON string escapes (`\n`, `\t`, `\"`, `\uXXXX`, ...) count as a boundary:
//!   detectors and whole-word keywords see a space there, so tool-call
//!   arguments (JSON text) match like plain text. Positions are unchanged.
//! - Whole-word keywords that contain a character of a script written without
//!   spaces (Han, Hiragana, Katakana, Thai, Lao, Khmer, Myanmar) match as
//!   substrings, because Unicode word boundaries never fall between such letters.
//! - No Unicode normalization or full case folding is done: `café` written
//!   with a combining accent does not match `café` written precomposed, and
//!   zero-width characters inside a word defeat a keyword.
//! - Regular expressions may not use anchors (`^ $ \A \z`): a stream only
//!   sees part of the text.
//! - A private key block (`-----BEGIN ... PRIVATE KEY-----` to the matching
//!   END line, PGP blocks included) is redacted whole; a stream swallows
//!   everything after a BEGIN line until the END line or the end of the stream.
//!
//! Matched text is never logged, stored or returned except as the redacted
//! output itself.

mod keywords;
pub mod log;
mod pii;
pub mod run;
mod scan;
mod stream;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub use pii::PiiType;
pub use stream::{FinalRelease, Release, StreamScanner};

/// Most keywords one rule may hold.
pub const MAX_KEYWORDS: usize = 1000;
/// Longest keyword, in characters (keeps a keyword match inside the hold-back).
pub const MAX_KEYWORD_CHARS: usize = 256;
/// Most rules one guardrail may hold.
pub const MAX_RULES: usize = 50;
/// Compiled-size limit of a regex rule, in bytes.
pub const REGEX_SIZE_LIMIT: usize = 1 << 20;
/// Stream hold-back, in characters: the longest match the scanner is
/// guaranteed to catch across chunk boundaries. Regex rules matching more than
/// this may be missed in streams.
pub const HOLD_BACK_CHARS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Input,
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Block,
    Redact,
    Flag,
}

/// The directions a rule applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum Directions {
    Input,
    Output,
    Both,
}

impl Directions {
    /// `input`, `output` or `both`: the form it is stored in.
    pub fn as_str(self) -> &'static str {
        match self {
            Directions::Input => "input",
            Directions::Output => "output",
            Directions::Both => "both",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "input" => Some(Directions::Input),
            "output" => Some(Directions::Output),
            "both" => Some(Directions::Both),
            _ => None,
        }
    }

    pub fn covers(self, dir: Direction) -> bool {
        matches!(
            (self, dir),
            (Directions::Both, _)
                | (Directions::Input, Direction::Input)
                | (Directions::Output, Direction::Output)
        )
    }
}

fn whole_word_default() -> bool {
    true
}

/// What a rule looks for. Written as `{"keywords": {"words": [...],
/// "whole_word": true}}`, `{"regex": "..."}` or `{"pii": ["EMAIL", ...]}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Matcher {
    Keywords {
        /// Up to 1 000, each 1 to 256 characters. Case-insensitive.
        words: Vec<String>,
        /// Match whole words only (the default); `false` matches substrings.
        #[serde(default = "whole_word_default")]
        whole_word: bool,
    },
    Regex(String),
    Pii(Vec<PiiType>),
}

/// One rule of a guardrail.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RuleSpec {
    /// Stable within a guardrail; the label of keyword/regex redaction counts.
    pub id: String,
    pub matcher: Matcher,
    pub action: Action,
    /// The directions the rule applies to.
    pub directions: Directions,
}

/// Why a guardrail does not compile. Messages never contain scanned text.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardrailError {
    #[error("a guardrail holds at most {MAX_RULES} rules")]
    TooManyRules,
    #[error("rule id '{0}' is empty or used twice")]
    BadRuleId(String),
    #[error("rule '{0}': a keyword rule needs at least one keyword")]
    NoKeywords(String),
    #[error("rule '{0}': at most {MAX_KEYWORDS} keywords")]
    TooManyKeywords(String),
    #[error("rule '{0}': a keyword is empty")]
    EmptyKeyword(String),
    #[error("rule '{0}': a keyword is longer than {MAX_KEYWORD_CHARS} characters")]
    KeywordTooLong(String),
    #[error("rule '{0}': the keyword list is too large to compile")]
    KeywordsTooLarge(String),
    #[error("rule '{0}': invalid regular expression: {1}")]
    InvalidRegex(String, String),
    #[error("rule '{0}': the regular expression is too large (limit 1 MiB compiled)")]
    RegexTooLarge(String),
    #[error("rule '{0}': the regular expression matches the empty string")]
    RegexMatchesEmpty(String),
    #[error("rule '{0}': the regular expression uses an anchor (^, $, \\A, \\z); anchors mean the start or end of the whole text, which a stream never sees, so they are not allowed")]
    RegexAnchor(String),
    #[error("rule '{0}': pick at least one PII type")]
    NoPiiTypes(String),
}

/// The compiled rules of one guardrail.
pub struct Compiled {
    id: i64,
    name: String,
    rules: Vec<scan::CompiledRule>,
}

impl std::fmt::Debug for Compiled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compiled")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("rules", &self.rules.len())
            .finish()
    }
}

impl Compiled {
    pub fn compile(
        guardrail_id: i64,
        name: &str,
        rules: &[RuleSpec],
    ) -> Result<Self, GuardrailError> {
        Ok(Compiled {
            id: guardrail_id,
            name: name.to_string(),
            rules: scan::compile_rules(rules)?,
        })
    }

    pub fn id(&self) -> i64 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether some rule applies to `dir`.
    pub fn applies(&self, dir: Direction) -> bool {
        self.rules.iter().any(|r| r.directions.covers(dir))
    }
}

/// What a check found. Holds counts and ids only, never matched text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// `(guardrail id, guardrail name)` of the first blocking guardrail.
    pub blocked_by: Option<(i64, String)>,
    /// Replacements made, keyed by PII type name or by rule id.
    pub redactions: BTreeMap<String, u32>,
    /// `(guardrail id, rule id)` of every flag rule that matched, once each.
    pub flags: Vec<(i64, String)>,
}

impl Outcome {
    pub(crate) fn add_flag(&mut self, guardrail: i64, rule: &str) {
        if !self.flags.iter().any(|(g, r)| *g == guardrail && r == rule) {
            self.flags.push((guardrail, rule.to_string()));
        }
    }

    pub(crate) fn add_redaction(&mut self, label: &str) {
        *self.redactions.entry(label.to_string()).or_insert(0) += 1;
    }
}

/// Checks `texts` against `set` for `dir`, redacting in place. When a rule
/// blocks, no text is changed.
pub fn check_texts(set: &[Arc<Compiled>], dir: Direction, texts: &mut [String]) -> Outcome {
    scan::check_texts(set, dir, texts)
}
